use crate::error::{ModelayError, Result};
use crate::paths;
use chrono::Local;
use rusqlite::{backup::Backup, params, Connection, TransactionBehavior};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug)]
pub struct RebindReport {
    pub changed_count: usize,
    pub backup_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct SessionBackup {
    database_path: PathBuf,
    backup_path: PathBuf,
}

pub fn detect_official_provider() -> String {
    detect_official_provider_at(&paths::state_db_path().unwrap_or_default())
        .unwrap_or_else(|_| "openai_http".into())
}

fn detect_official_provider_at(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok("openai_http".into());
    }
    let connection = Connection::open(path)?;
    let has_thread_source = has_column(&connection, "threads", "thread_source")?;
    let ordering = if has_column(&connection, "threads", "updated_at_ms")? {
        "updated_at_ms"
    } else if has_column(&connection, "threads", "updated_at")? {
        "updated_at * 1000"
    } else {
        "rowid"
    };
    let sql = if has_thread_source {
        format!("SELECT model_provider FROM threads WHERE thread_source='user' AND COALESCE(preview,'')<>'' AND COALESCE(model,'') NOT LIKE '%auto-review%' AND COALESCE(source,'') NOT LIKE '%subagent%' AND model_provider LIKE 'openai%' ORDER BY {ordering} DESC LIMIT 1")
    } else {
        format!("SELECT model_provider FROM threads WHERE model_provider LIKE 'openai%' AND COALESCE(model,'') NOT LIKE '%auto-review%' AND COALESCE(source,'') NOT LIKE '%subagent%' ORDER BY {ordering} DESC LIMIT 1")
    };
    Ok(connection
        .query_row(&sql, [], |row| row.get::<_, String>(0))
        .unwrap_or_else(|_| "openai_http".into()))
}

pub fn backup() -> Result<Option<SessionBackup>> {
    backup_at(&paths::state_db_path()?, &paths::backup_dir()?)
}

pub fn rebind_prepared(
    backup: Option<&SessionBackup>,
    provider: &str,
    model: &str,
    reasoning_effort: &str,
) -> Result<Option<RebindReport>> {
    rebind_prepared_with_timeout(
        backup,
        provider,
        model,
        reasoning_effort,
        Duration::from_secs(10),
    )
}

#[cfg(test)]
fn rebind_at_with_timeout(
    database_path: &Path,
    backup_directory: &Path,
    provider: &str,
    model: &str,
    reasoning_effort: &str,
    busy_timeout: Duration,
) -> Result<Option<RebindReport>> {
    let backup = backup_at(database_path, backup_directory)?;
    rebind_prepared_with_timeout(
        backup.as_ref(),
        provider,
        model,
        reasoning_effort,
        busy_timeout,
    )
}

fn backup_at(database_path: &Path, backup_directory: &Path) -> Result<Option<SessionBackup>> {
    if !database_path.exists() {
        return Ok(None);
    }
    fs::create_dir_all(backup_directory)?;
    let backup_path = backup_directory.join(format!(
        "state-{}.sqlite",
        Local::now().format("%Y%m%d-%H%M%S-%3f")
    ));
    {
        let source = Connection::open(database_path)?;
        let mut destination = Connection::open(&backup_path)?;
        let backup = Backup::new(&source, &mut destination)?;
        backup.run_to_completion(16, Duration::from_millis(20), None)?;
    }
    Ok(Some(SessionBackup {
        database_path: database_path.to_owned(),
        backup_path,
    }))
}

fn rebind_prepared_with_timeout(
    backup: Option<&SessionBackup>,
    provider: &str,
    model: &str,
    reasoning_effort: &str,
    busy_timeout: Duration,
) -> Result<Option<RebindReport>> {
    let Some(backup) = backup else {
        return Ok(None);
    };
    let mut connection = Connection::open(&backup.database_path)?;
    connection.busy_timeout(busy_timeout)?;
    validate_schema(&connection)?;
    let has_thread_source = has_column(&connection, "threads", "thread_source")?;
    let has_reasoning_effort = has_column(&connection, "threads", "reasoning_effort")?;
    let selector = if has_thread_source {
        "thread_source = 'user' AND COALESCE(preview, '') <> '' AND COALESCE(model, '') NOT LIKE '%auto-review%' AND COALESCE(source, '') NOT LIKE '%subagent%'"
    } else {
        "preview <> '' AND COALESCE(model, '') NOT LIKE '%auto-review%' AND COALESCE(source, '') NOT LIKE '%subagent%'"
    };
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (changed_count, remaining) = if has_reasoning_effort {
        let sql = format!("UPDATE threads SET model_provider=?1, model=?2, reasoning_effort=?3 WHERE (model_provider LIKE 'openai%' OR model_provider='custom' OR model_provider LIKE 'custom_%') AND {selector}");
        let changed_count =
            transaction.execute(&sql, params![provider, model, reasoning_effort])?;
        let verify_sql = format!("SELECT COUNT(*) FROM threads WHERE (model_provider LIKE 'openai%' OR model_provider='custom' OR model_provider LIKE 'custom_%') AND {selector} AND (model_provider<>?1 OR model<>?2 OR COALESCE(reasoning_effort,'')<>?3)");
        let remaining = transaction.query_row(
            &verify_sql,
            params![provider, model, reasoning_effort],
            |row| row.get::<_, i64>(0),
        )?;
        (changed_count, remaining)
    } else {
        let sql = format!("UPDATE threads SET model_provider=?1, model=?2 WHERE (model_provider LIKE 'openai%' OR model_provider='custom' OR model_provider LIKE 'custom_%') AND {selector}");
        let changed_count = transaction.execute(&sql, params![provider, model])?;
        let verify_sql = format!("SELECT COUNT(*) FROM threads WHERE (model_provider LIKE 'openai%' OR model_provider='custom' OR model_provider LIKE 'custom_%') AND {selector} AND (model_provider<>?1 OR model<>?2)");
        let remaining = transaction.query_row(&verify_sql, params![provider, model], |row| {
            row.get::<_, i64>(0)
        })?;
        (changed_count, remaining)
    };
    if remaining != 0 {
        return Err(ModelayError::Message(
            "旧任务渠道设置验证失败，数据库事务已回滚。".into(),
        ));
    }
    transaction.commit()?;
    Ok(Some(RebindReport {
        changed_count,
        backup_path: backup.backup_path.clone(),
    }))
}

fn validate_schema(connection: &Connection) -> Result<()> {
    for column in ["model_provider", "model", "preview", "source"] {
        if !has_column(connection, "threads", column)? {
            return Err(ModelayError::Message(format!(
                "当前 Codex 任务数据库缺少 {column} 字段，已停止修改。"
            )));
        }
    }
    Ok(())
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rebind_at(
        database_path: &Path,
        backup_directory: &Path,
        provider: &str,
        model: &str,
        reasoning_effort: &str,
    ) -> Result<Option<RebindReport>> {
        let backup = backup_at(database_path, backup_directory)?;
        rebind_prepared_with_timeout(
            backup.as_ref(),
            provider,
            model,
            reasoning_effort,
            Duration::from_secs(10),
        )
    }

    #[test]
    fn rebinds_only_user_threads() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, model_provider TEXT NOT NULL, model TEXT, reasoning_effort TEXT, preview TEXT NOT NULL, source TEXT NOT NULL, thread_source TEXT, updated_at INTEGER, updated_at_ms INTEGER); INSERT INTO threads VALUES ('official','openai_http','old','high','v','vscode','user',1,1000),('third','custom_proxy','old','high','v','vscode','user',2,2000),('review','openai_http','codex-auto-review','high','v','{\"subagent\":{}}','subagent',3,3000),('hidden','openai_http','old','high','','vscode','user',4,4000),('ollama','ollama','local','high','v','vscode','user',5,5000);").unwrap();
        drop(connection);
        let report = rebind_at(
            &database,
            directory.path(),
            "custom",
            "gpt-5.6-sol",
            "medium",
        )
        .unwrap()
        .unwrap();
        assert_eq!(report.changed_count, 2);
        assert!(report.backup_path.exists());
        let backup_connection = Connection::open(&report.backup_path).unwrap();
        assert_eq!(
            backup_connection
                .query_row(
                    "SELECT model_provider || ':' || model FROM threads WHERE id='official'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "openai_http:old"
        );
        let connection = Connection::open(database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT model_provider FROM threads WHERE id='official'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "custom"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT reasoning_effort FROM threads WHERE id='official'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "medium"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT model_provider FROM threads WHERE id='review'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "openai_http"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT model_provider FROM threads WHERE id='hidden'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "openai_http"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT model_provider FROM threads WHERE id='ollama'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "ollama"
        );
    }

    #[test]
    fn supports_legacy_schema_without_thread_source() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, model_provider TEXT NOT NULL, model TEXT, preview TEXT NOT NULL, source TEXT NOT NULL, updated_at INTEGER); INSERT INTO threads VALUES ('user','custom','old','visible','vscode',1),('hidden','openai_http','old','','vscode',2),('review','openai_http','codex-auto-review','visible','subagent',3);").unwrap();
        drop(connection);
        let report = rebind_at(&database, directory.path(), "openai_http", "gpt-5.5", "low")
            .unwrap()
            .unwrap();
        assert_eq!(report.changed_count, 1);
        let connection = Connection::open(database).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT model FROM threads WHERE id='user'", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "gpt-5.5"
        );
        assert_eq!(
            connection
                .query_row("SELECT model FROM threads WHERE id='hidden'", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            "old"
        );
        assert_eq!(
            connection
                .query_row("SELECT model FROM threads WHERE id='review'", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            "codex-auto-review"
        );
    }

    #[test]
    fn database_lock_leaves_threads_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.sqlite");
        let locker = Connection::open(&database).unwrap();
        locker.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, model_provider TEXT NOT NULL, model TEXT, preview TEXT NOT NULL, source TEXT NOT NULL, thread_source TEXT, updated_at INTEGER, updated_at_ms INTEGER); INSERT INTO threads VALUES ('user','openai_http','old','visible','vscode','user',1,1000); BEGIN IMMEDIATE;").unwrap();

        let result = rebind_at_with_timeout(
            &database,
            directory.path(),
            "custom",
            "gpt-5.6-sol",
            "medium",
            Duration::from_millis(30),
        );
        assert!(result.is_err());
        assert_eq!(
            locker
                .query_row(
                    "SELECT model_provider || ':' || model FROM threads WHERE id='user'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "openai_http:old"
        );
        locker.execute_batch("ROLLBACK").unwrap();
    }

    #[test]
    fn detects_latest_user_official_provider_only() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, model_provider TEXT NOT NULL, model TEXT, preview TEXT NOT NULL, source TEXT NOT NULL, thread_source TEXT, updated_at INTEGER, updated_at_ms INTEGER); INSERT INTO threads VALUES ('older','openai','gpt','visible','vscode','user',1,1000),('latest','openai_http','gpt','visible','vscode','user',2,2000),('review','openai_future','codex-auto-review','visible','subagent','subagent',3,3000);").unwrap();
        drop(connection);
        assert_eq!(
            detect_official_provider_at(&database).unwrap(),
            "openai_http"
        );
    }
}
