use crate::error::{ModelayError, Result};
use crate::models::ThreadSummary;
use crate::paths;
use chrono::Local;
use rusqlite::{backup::Backup, named_params, params, Connection, TransactionBehavior};
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug)]
pub struct RebindReport {
    pub changed_count: usize,
    pub backup_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RebindScope {
    None,
    Recent(usize),
    All,
    Single(String),
}

impl RebindScope {
    pub fn label(&self) -> String {
        match self {
            Self::None => "不修改旧任务".into(),
            Self::Recent(limit) => format!("最近活动的 {limit} 个任务"),
            Self::All => "全部旧任务".into(),
            Self::Single(thread_id) => format!("指定任务 {thread_id}"),
        }
    }
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

pub fn list_user_threads(limit: usize) -> Result<Vec<ThreadSummary>> {
    list_user_threads_at(&paths::state_db_path()?, limit)
}

fn list_user_threads_at(path: &Path, limit: usize) -> Result<Vec<ThreadSummary>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let connection = Connection::open(path)?;
    validate_schema(&connection)?;
    let has_thread_source = has_column(&connection, "threads", "thread_source")?;
    let title = if has_column(&connection, "threads", "title")? {
        "COALESCE(NULLIF(title,''), preview, '')"
    } else {
        "COALESCE(preview, '')"
    };
    let cwd = if has_column(&connection, "threads", "cwd")? {
        "COALESCE(cwd, '')"
    } else {
        "''"
    };
    let rollout_path = if has_column(&connection, "threads", "rollout_path")? {
        "COALESCE(rollout_path, '')"
    } else {
        "''"
    };
    let ordering = if has_column(&connection, "threads", "updated_at_ms")? {
        "COALESCE(updated_at_ms, 0)"
    } else if has_column(&connection, "threads", "updated_at")? {
        "COALESCE(updated_at * 1000, 0)"
    } else {
        "rowid"
    };
    let user_filter = if has_thread_source {
        "COALESCE(thread_source,'user')='user' AND"
    } else {
        ""
    };
    let sql = format!(
        "SELECT id, {title}, {cwd}, COALESCE(model_provider,''), COALESCE(model,''), {ordering}, {rollout_path} FROM threads WHERE {user_filter} COALESCE(preview,'')<>'' AND COALESCE(model,'') NOT LIKE '%auto-review%' AND COALESCE(source,'') NOT LIKE '%subagent%' ORDER BY {ordering} DESC LIMIT ?1"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![limit.clamp(1, 200) as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (thread_id, title, cwd, provider_id, model, updated_at_ms, rollout_path) = row?;
        result.push(ThreadSummary {
            thread_id,
            title,
            cwd,
            original_provider_id: original_provider_from_rollout(&rollout_path),
            provider_id,
            model,
            updated_at_ms,
            issue: None,
        });
    }
    Ok(result)
}

fn original_provider_from_rollout(path: &str) -> Option<String> {
    let file = File::open(path).ok()?;
    for line in BufReader::new(file).lines().map_while(std::result::Result::ok).take(20) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }
        return value
            .pointer("/payload/model_provider")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    None
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
    scope: &RebindScope,
) -> Result<Option<RebindReport>> {
    rebind_prepared_with_timeout(
        backup,
        provider,
        model,
        reasoning_effort,
        scope,
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
    scope: &RebindScope,
    busy_timeout: Duration,
) -> Result<Option<RebindReport>> {
    let backup = backup_at(database_path, backup_directory)?;
    rebind_prepared_with_timeout(
        backup.as_ref(),
        provider,
        model,
        reasoning_effort,
        scope,
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
    scope: &RebindScope,
    busy_timeout: Duration,
) -> Result<Option<RebindReport>> {
    let Some(backup) = backup else {
        return Ok(None);
    };
    if matches!(scope, RebindScope::None) {
        return Ok(None);
    }
    let mut connection = Connection::open(&backup.database_path)?;
    connection.busy_timeout(busy_timeout)?;
    validate_schema(&connection)?;
    let has_thread_source = has_column(&connection, "threads", "thread_source")?;
    let has_reasoning_effort = has_column(&connection, "threads", "reasoning_effort")?;
    let ordering = if has_column(&connection, "threads", "updated_at_ms")? {
        "updated_at_ms"
    } else if has_column(&connection, "threads", "updated_at")? {
        "updated_at * 1000"
    } else {
        "rowid"
    };
    let selector = if has_thread_source {
        "thread_source = 'user' AND COALESCE(preview, '') <> '' AND COALESCE(model, '') NOT LIKE '%auto-review%' AND COALESCE(source, '') NOT LIKE '%subagent%'"
    } else {
        "preview <> '' AND COALESCE(model, '') NOT LIKE '%auto-review%' AND COALESCE(source, '') NOT LIKE '%subagent%'"
    };
    let candidates = format!(
        "(model_provider LIKE 'openai%' OR model_provider='custom' OR model_provider LIKE 'custom_%') AND {selector}"
    );
    let scoped_selector = match scope {
        RebindScope::All => candidates.clone(),
        RebindScope::Recent(limit) => {
            if *limit == 0 || *limit > 100 {
                return Err("最近任务数量必须在 1 到 100 之间。".into());
            }
            format!(
                "id IN (SELECT id FROM threads WHERE {candidates} ORDER BY {ordering} DESC LIMIT {limit})"
            )
        }
        RebindScope::Single(_) => format!("id=:thread_id AND {candidates}"),
        RebindScope::None => "0".into(),
    };
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (changed_count, remaining) = if has_reasoning_effort {
        let sql = format!("UPDATE threads SET model_provider=:provider, model=:model, reasoning_effort=:effort WHERE {scoped_selector}");
        let verify_sql = format!("SELECT COUNT(*) FROM threads WHERE {scoped_selector} AND (model_provider<>:provider OR model<>:model OR COALESCE(reasoning_effort,'')<>:effort)");
        let (changed_count, remaining) = match scope {
            RebindScope::Single(thread_id) => {
                let values = named_params! {":provider": provider, ":model": model, ":effort": reasoning_effort, ":thread_id": thread_id};
                let changed = transaction.execute(&sql, values)?;
                let remaining =
                    transaction.query_row(&verify_sql, values, |row| row.get::<_, i64>(0))?;
                (changed, remaining)
            }
            _ => {
                let values = named_params! {":provider": provider, ":model": model, ":effort": reasoning_effort};
                let changed = transaction.execute(&sql, values)?;
                let remaining =
                    transaction.query_row(&verify_sql, values, |row| row.get::<_, i64>(0))?;
                (changed, remaining)
            }
        };
        if matches!(scope, RebindScope::Single(_)) && changed_count == 0 {
            return Err("指定的会话 ID 不存在，或该任务不是可覆盖的用户任务。".into());
        }
        (changed_count, remaining)
    } else {
        let sql = format!(
            "UPDATE threads SET model_provider=:provider, model=:model WHERE {scoped_selector}"
        );
        let verify_sql = format!("SELECT COUNT(*) FROM threads WHERE {scoped_selector} AND (model_provider<>:provider OR model<>:model)");
        let (changed_count, remaining) = match scope {
            RebindScope::Single(thread_id) => {
                let values =
                    named_params! {":provider": provider, ":model": model, ":thread_id": thread_id};
                let changed = transaction.execute(&sql, values)?;
                let remaining =
                    transaction.query_row(&verify_sql, values, |row| row.get::<_, i64>(0))?;
                (changed, remaining)
            }
            _ => {
                let values = named_params! {":provider": provider, ":model": model};
                let changed = transaction.execute(&sql, values)?;
                let remaining =
                    transaction.query_row(&verify_sql, values, |row| row.get::<_, i64>(0))?;
                (changed, remaining)
            }
        };
        if matches!(scope, RebindScope::Single(_)) && changed_count == 0 {
            return Err("指定的会话 ID 不存在，或该任务不是可覆盖的用户任务。".into());
        }
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
    for column in ["id", "model_provider", "model", "preview", "source"] {
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
        scope: &RebindScope,
    ) -> Result<Option<RebindReport>> {
        let backup = backup_at(database_path, backup_directory)?;
        rebind_prepared_with_timeout(
            backup.as_ref(),
            provider,
            model,
            reasoning_effort,
            scope,
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
            &RebindScope::All,
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
        let report = rebind_at(
            &database,
            directory.path(),
            "openai_http",
            "gpt-5.5",
            "low",
            &RebindScope::All,
        )
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
    fn rebinds_only_the_five_most_recent_user_threads() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, model_provider TEXT NOT NULL, model TEXT, reasoning_effort TEXT, preview TEXT NOT NULL, source TEXT NOT NULL, thread_source TEXT, updated_at_ms INTEGER); INSERT INTO threads VALUES ('u1','openai_http','old','high','visible','vscode','user',1000),('u2','openai_http','old','high','visible','vscode','user',2000),('u3','openai_http','old','high','visible','vscode','user',3000),('u4','openai_http','old','high','visible','vscode','user',4000),('u5','openai_http','old','high','visible','vscode','user',5000),('u6','openai_http','old','high','visible','vscode','user',6000),('review','openai_http','codex-auto-review','high','visible','subagent','subagent',7000);").unwrap();
        drop(connection);
        let report = rebind_at(
            &database,
            directory.path(),
            "custom",
            "new-model",
            "medium",
            &RebindScope::Recent(5),
        )
        .unwrap()
        .unwrap();
        assert_eq!(report.changed_count, 5);
        let connection = Connection::open(database).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT model_provider FROM threads WHERE id='u1'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "openai_http"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT model_provider FROM threads WHERE id='u6'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "custom"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT model_provider FROM threads WHERE id='review'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "openai_http"
        );
    }

    #[test]
    fn rebinds_one_requested_user_thread_and_rejects_unknown_ids() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, model_provider TEXT NOT NULL, model TEXT, reasoning_effort TEXT, preview TEXT NOT NULL, source TEXT NOT NULL, thread_source TEXT, updated_at_ms INTEGER); INSERT INTO threads VALUES ('target-id','openai_http','old','high','visible','vscode','user',1000),('other-id','custom','old','high','visible','vscode','user',2000);").unwrap();
        drop(connection);
        let report = rebind_at(
            &database,
            directory.path(),
            "custom_proxy",
            "new-model",
            "low",
            &RebindScope::Single("target-id".into()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(report.changed_count, 1);
        let connection = Connection::open(&database).unwrap();
        assert_eq!(connection.query_row("SELECT model_provider || ':' || reasoning_effort FROM threads WHERE id='target-id'", [], |row| row.get::<_, String>(0)).unwrap(), "custom_proxy:low");
        assert_eq!(
            connection
                .query_row(
                    "SELECT model_provider FROM threads WHERE id='other-id'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "custom"
        );
        drop(connection);
        let error = rebind_at(
            &database,
            directory.path(),
            "custom_proxy",
            "new-model",
            "low",
            &RebindScope::Single("missing-id".into()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("不存在"));
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
            &RebindScope::All,
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

    #[test]
    fn lists_user_threads_and_detects_provider_rewrites_from_rollout() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.sqlite");
        let rollout = directory.path().join("session.jsonl");
        fs::write(
            &rollout,
            r#"{"type":"session_meta","payload":{"model_provider":"custom_old"}}
"#,
        )
        .unwrap();
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch(&format!(
            "CREATE TABLE threads(id TEXT PRIMARY KEY, title TEXT, cwd TEXT, model_provider TEXT NOT NULL, model TEXT, preview TEXT NOT NULL, source TEXT NOT NULL, thread_source TEXT, updated_at_ms INTEGER, rollout_path TEXT); INSERT INTO threads VALUES ('user','旧任务','/tmp/project','custom_new','gpt-test','visible','vscode','user',2000,'{}'),('review','review','/tmp','openai_http','codex-auto-review','visible','subagent','subagent',3000,'');",
            rollout.display()
        )).unwrap();
        drop(connection);
        let items = list_user_threads_at(&database, 20).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].thread_id, "user");
        assert_eq!(items[0].provider_id, "custom_new");
        assert_eq!(items[0].original_provider_id.as_deref(), Some("custom_old"));
    }
}
