use crate::error::{ModelayError, Result};
use crate::models::ThreadHealth;
use crate::paths;
use chrono::{DateTime, Local};
use rusqlite::Connection;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const WARNING_TOKENS: i64 = 1_000_000;
const CRITICAL_TOKENS: i64 = 5_000_000;
const MIGRATION_WARNING_INPUT_TOKENS: i64 = 40_000;
const MIGRATION_CRITICAL_INPUT_TOKENS: i64 = 80_000;
const MAX_EXCERPT_CHARS: usize = 70_000;

#[derive(Clone, Debug)]
pub struct ThreadRecord {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub tokens_used: i64,
    pub updated_at_ms: i64,
    pub rollout_path: String,
}

#[derive(Default)]
pub struct TodayContent {
    pub messages: Vec<(String, String)>,
    pub bytes: u64,
    pub referenced_paths: Vec<String>,
    pub latest_input_tokens: i64,
}

pub fn inspect(thread_id: &str) -> Result<(ThreadRecord, TodayContent, ThreadHealth)> {
    validate_thread_id(thread_id)?;
    let record = read_thread(thread_id)?;
    let content = read_today_content(&record)?;
    let latest = content
        .messages
        .iter()
        .rev()
        .find(|(role, _)| role == "user")
        .map(|(_, text)| compact(text, 500));
    let mut reasons = Vec::new();
    let (level, label) = if content.latest_input_tokens >= MIGRATION_CRITICAL_INPUT_TOKENS {
        reasons.push(format!(
            "最近一轮输入约 {} tokens，跨渠道直接迁移通常会明显变慢。",
            content.latest_input_tokens
        ));
        ("critical", "建议智能续接")
    } else if content.latest_input_tokens >= MIGRATION_WARNING_INPUT_TOKENS {
        reasons.push(format!(
            "最近一轮输入约 {} tokens，第三方渠道首次恢复可能需要较长时间。",
            content.latest_input_tokens
        ));
        ("warning", "跨渠道迁移有延迟风险")
    } else if record.tokens_used >= CRITICAL_TOKENS {
        reasons.push("累计处理量已超过 500 万 tokens，恢复、压缩和重试成本很高。".into());
        ("critical", "建议立即续接")
    } else if record.tokens_used >= WARNING_TOKENS {
        reasons.push("累计处理量已超过 100 万 tokens，短问题也可能先经历历史恢复与压缩。".into());
        ("warning", "建议创建续接任务")
    } else if content.bytes >= 20 * 1024 * 1024 {
        reasons.push("当天会话记录已超过 20 MB，加载和同步可能明显变慢。".into());
        ("warning", "记录体积偏大")
    } else {
        ("healthy", "当前风险较低")
    };
    if content.messages.len() >= 80 {
        reasons.push("当天往返消息较多，工具输出和附件会进一步放大上下文。".into());
    }
    if reasons.is_empty() {
        reasons.push("尚未达到 Modelay 的风险阈值；网络或第三方服务仍可能造成延迟。".into());
    }
    let health = ThreadHealth {
        thread_id: record.id.clone(),
        title: record.title.clone(),
        cwd: record.cwd.clone(),
        provider_id: record.provider.clone(),
        model: record.model.clone(),
        reasoning_effort: record.effort.clone(),
        tokens_used: record.tokens_used,
        latest_input_tokens: content.latest_input_tokens,
        today_message_count: content.messages.len(),
        today_rollout_bytes: content.bytes,
        risk_level: level.into(),
        risk_label: label.into(),
        risk_reasons: reasons,
        latest_user_request: latest,
    };
    Ok((record, content, health))
}

pub fn build_prompt(record: &ThreadRecord, content: &TodayContent) -> String {
    let mut user_requests = Vec::new();
    let mut progress = Vec::new();
    let mut used = 0usize;
    for (role, text) in content.messages.iter().rev() {
        if used >= MAX_EXCERPT_CHARS {
            break;
        }
        let excerpt = compact(text, if role == "user" { 5000 } else { 7000 });
        used += excerpt.chars().count();
        if role == "user" && user_requests.len() < 16 {
            user_requests.push(excerpt);
        } else if role == "assistant" && progress.len() < 12 {
            progress.push(excerpt);
        }
    }
    user_requests.reverse();
    progress.reverse();
    let paths = if content.referenced_paths.is_empty() {
        "- 未从当天消息中识别到额外路径；请先检查工作目录。".into()
    } else {
        content
            .referenced_paths
            .iter()
            .map(|path| format!("- {path}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        r#"这是 Modelay 从旧任务自动生成的续接任务。不要加载或复述旧任务的完整历史，只依据当前工作区和下面的精简交接继续。

## 来源
- 旧任务 ID：{}
- 旧任务标题：{}
- 项目工作目录：{}
- 旧任务累计处理量：{} tokens
- 最近一轮输入：{} tokens

## 项目资料与引用路径
{}

## 当天用户需求（按时间顺序）
{}

## 当天助手进度与结论（按时间顺序）
{}

## 续接要求
1. 先只读检查工作目录、git status 和上面引用的关键文件，以文件事实确认真实进度。
2. 严禁 reset、checkout、clean、删除或覆盖旧任务及用户已有改动。
3. 先用简洁清单整理：项目目标、最新需求、已完成、进行中、待办、风险与下一步。
4. 以最后一条用户需求为当前优先事项；若它已完成，则从真实待办继续。
5. 避免重新读取无关的大型历史、重复生成已有成果或无依据猜测。
6. 完成整理后继续执行任务，并把关键产物路径清楚汇报给用户。"#,
        record.id,
        record.title,
        record.cwd,
        record.tokens_used,
        content.latest_input_tokens,
        paths,
        numbered(&user_requests),
        numbered(&progress),
    )
}

fn read_thread(thread_id: &str) -> Result<ThreadRecord> {
    let connection = Connection::open(paths::state_db_path()?)?;
    let updated_at = if has_column(&connection, "threads", "updated_at_ms")? {
        "COALESCE(updated_at_ms, 0)"
    } else if has_column(&connection, "threads", "updated_at")? {
        "COALESCE(updated_at * 1000, 0)"
    } else {
        "0"
    };
    let user_filter = if has_column(&connection, "threads", "thread_source")? {
        " AND COALESCE(thread_source, 'user') = 'user'"
    } else {
        ""
    };
    let query = format!(
        "SELECT id, COALESCE(title,''), COALESCE(cwd,''), COALESCE(model_provider,''), COALESCE(model,''), COALESCE(reasoning_effort,'medium'), COALESCE(tokens_used,0), {updated_at}, COALESCE(rollout_path,'') FROM threads WHERE id=?1{user_filter}"
    );
    connection
        .query_row(&query, [thread_id], |row| {
            Ok(ThreadRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                cwd: row.get(2)?,
                provider: row.get(3)?,
                model: row.get(4)?,
                effort: row.get(5)?,
                tokens_used: row.get(6)?,
                updated_at_ms: row.get(7)?,
                rollout_path: row.get(8)?,
            })
        })
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                ModelayError::Message("没有找到该用户任务，请检查会话 ID。".into())
            }
            other => other.into(),
        })
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        if row.get::<_, String>(1)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_today_content(record: &ThreadRecord) -> Result<TodayContent> {
    let mut paths_found = BTreeSet::new();
    let mut files = rollout_files(record)?;
    files.sort();
    let today = DateTime::from_timestamp_millis(record.updated_at_ms)
        .map(|timestamp| timestamp.with_timezone(&Local).date_naive())
        .unwrap_or_else(|| Local::now().date_naive());
    let mut result = TodayContent::default();
    for path in files {
        result.bytes += fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        let reader = BufReader::new(File::open(path)?);
        for line in reader.lines().map_while(std::result::Result::ok) {
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) else {
                continue;
            };
            let Ok(parsed) = DateTime::parse_from_rfc3339(timestamp) else {
                continue;
            };
            if parsed.with_timezone(&Local).date_naive() != today {
                continue;
            }
            if let Some(tokens) = input_tokens(&value) {
                result.latest_input_tokens = tokens;
            }
            let Some((role, text)) = message_text(&value) else {
                continue;
            };
            collect_paths(&text, &mut paths_found);
            result.messages.push((role, text));
        }
    }
    result.referenced_paths = paths_found.into_iter().take(40).collect();
    Ok(result)
}

fn input_tokens(value: &Value) -> Option<i64> {
    if value.get("type").and_then(Value::as_str) != Some("event_msg")
        || value.pointer("/payload/type").and_then(Value::as_str) != Some("token_count")
    {
        return None;
    }
    value
        .pointer("/payload/info/last_token_usage/input_tokens")
        .and_then(Value::as_i64)
}

fn rollout_files(record: &ThreadRecord) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for root in [
        paths::codex_dir()?.join("sessions"),
        paths::codex_dir()?.join("archived_sessions"),
    ] {
        visit(&root, &record.id, &mut result)?;
    }
    let direct = PathBuf::from(&record.rollout_path);
    if direct.is_file() && !result.contains(&direct) {
        result.push(direct);
    }
    if result.is_empty() {
        return Err("没有找到该任务的本地会话记录。".into());
    }
    Ok(result)
}

fn visit(directory: &Path, needle: &str, result: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    for item in fs::read_dir(directory)? {
        let path = item?.path();
        if path.is_dir() {
            visit(&path, needle, result)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.contains(needle))
        {
            result.push(path);
        }
    }
    Ok(())
}

fn message_text(value: &Value) -> Option<(String, String)> {
    if value.get("type")?.as_str()? != "response_item" {
        return None;
    }
    let payload = value.get("payload")?;
    if payload.get("type")?.as_str()? != "message" {
        return None;
    }
    let role = payload.get("role")?.as_str()?;
    if !matches!(role, "user" | "assistant") {
        return None;
    }
    let text = payload
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|item| {
            item.get("text")
                .and_then(Value::as_str)
                .or_else(|| item.get("input_text").and_then(Value::as_str))
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then(|| (role.into(), text))
}

fn collect_paths(text: &str, result: &mut BTreeSet<String>) {
    for token in text.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            matches!(
                c,
                '`' | '"' | '\'' | ',' | '。' | '，' | ')' | '(' | '：' | ':'
            )
        });
        if (cleaned.starts_with('/')
            || (cleaned.len() > 3 && cleaned.as_bytes().get(1) == Some(&b':')))
            && cleaned.len() <= 500
        {
            result.insert(cleaned.into());
        }
    }
}

fn validate_thread_id(value: &str) -> Result<()> {
    let value = value.trim();
    if value.len() < 16
        || value.len() > 80
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("会话 ID 格式无效。".into());
    }
    Ok(())
}

fn compact(text: &str, limit: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= limit {
        normalized
    } else {
        format!("{}…", normalized.chars().take(limit).collect::<String>())
    }
}

fn numbered(items: &[String]) -> String {
    if items.is_empty() {
        return "1. 当天没有可提取的对应消息，请以工作区文件为准。".into();
    }
    items
        .iter()
        .enumerate()
        .map(|(index, item)| format!("{}. {}", index + 1, item))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_user_and_assistant_messages() {
        let user = serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"检查 /tmp/project/a.md"}]}});
        let developer = serde_json::json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"ignore"}]}});
        assert_eq!(message_text(&user).unwrap().0, "user");
        assert!(message_text(&developer).is_none());
    }

    #[test]
    fn handoff_prompt_is_bounded_and_structured() {
        let record = ThreadRecord {
            id: "thread-id".into(),
            title: "Project".into(),
            cwd: "/tmp/project".into(),
            provider: "custom".into(),
            model: "gpt".into(),
            effort: "medium".into(),
            tokens_used: 2_000_000,
            updated_at_ms: 0,
            rollout_path: String::new(),
        };
        let content = TodayContent {
            messages: vec![
                ("user".into(), "最新需求".repeat(30_000)),
                ("assistant".into(), "已完成 A".into()),
            ],
            bytes: 1,
            referenced_paths: vec!["/tmp/project/a.md".into()],
            latest_input_tokens: 12_345,
        };
        let prompt = build_prompt(&record, &content);
        assert!(prompt.contains("最新需求"));
        assert!(prompt.contains("/tmp/project/a.md"));
        assert!(prompt.contains("12345 tokens"));
        assert!(prompt.chars().count() < 80_000);
    }

    #[test]
    fn extracts_latest_input_tokens_from_token_events() {
        let value = serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {"last_token_usage": {"input_tokens": 82_000}}
            }
        });
        assert_eq!(input_tokens(&value), Some(82_000));
        assert_eq!(
            input_tokens(&serde_json::json!({"type": "event_msg"})),
            None
        );
    }

    #[test]
    fn detects_optional_legacy_thread_columns() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY, updated_at INTEGER);")
            .unwrap();
        assert!(has_column(&connection, "threads", "updated_at").unwrap());
        assert!(!has_column(&connection, "threads", "updated_at_ms").unwrap());
        assert!(!has_column(&connection, "threads", "thread_source").unwrap());
    }
}
