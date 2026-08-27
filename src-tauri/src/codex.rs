use crate::error::{ModelayError, Result};
use crate::models::ModelInfo;
use crate::platform;
use regex::Regex;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;

pub struct CommandOutput {
    pub success: bool,
    pub text: String,
}

pub fn run(
    arguments: &[&str],
    environment: Option<(&str, &str)>,
    timeout: Duration,
) -> Result<CommandOutput> {
    let overrides = environment
        .map(|(key, value)| vec![(key, Some(value))])
        .unwrap_or_default();
    run_with_environment(arguments, &overrides, timeout)
}

pub fn run_with_environment(
    arguments: &[&str],
    environment: &[(&str, Option<&str>)],
    timeout: Duration,
) -> Result<CommandOutput> {
    let mut command = Command::new(platform::codex_executable()?);
    command
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    platform::clear_provider_environment(&mut command);
    for (key, value) in environment {
        command.env_remove(key);
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    let mut child = command.spawn()?;
    let stdout_reader = child.stdout.take().map(read_pipe);
    let stderr_reader = child.stderr.take().map(read_pipe);
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_pipe(stdout_reader);
            let _ = join_pipe(stderr_reader);
            return Err(ModelayError::Message(format!(
                "Codex 命令执行超时：{}",
                arguments.join(" ")
            )));
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_pipe(stdout_reader);
            let _ = join_pipe(stderr_reader);
            return Err(error.into());
        }
    };
    let mut bytes = join_pipe(stdout_reader)?;
    bytes.extend(join_pipe(stderr_reader)?);
    let raw = String::from_utf8_lossy(&bytes);
    let text = redact_with_secrets(&raw, environment.iter().filter_map(|(_, value)| *value));
    Ok(CommandOutput {
        success: status.success(),
        text,
    })
}

fn read_pipe<T>(mut pipe: T) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>>
where
    T: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

fn join_pipe(reader: Option<std::thread::JoinHandle<std::io::Result<Vec<u8>>>>) -> Result<Vec<u8>> {
    match reader {
        Some(reader) => reader
            .join()
            .map_err(|_| ModelayError::Message("读取 Codex 命令输出的线程异常结束。".into()))?
            .map_err(Into::into),
        None => Ok(Vec::new()),
    }
}

fn redact_full(text: &str) -> String {
    let text = Regex::new(r"sk-[A-Za-z0-9_-]{8,}")
        .unwrap()
        .replace_all(text, "<已隐藏>")
        .into_owned();
    let text = Regex::new(r#"(?i)(Bearer\s+)[^\s\"']+"#)
        .unwrap()
        .replace_all(&text, "$1<已隐藏>")
        .into_owned();
    Regex::new(r#"(?i)(\"(?:api[_-]?key|access[_-]?token|refresh[_-]?token)\"\s*:\s*\")[^\"]+"#)
        .unwrap()
        .replace_all(&text, "$1<已隐藏>")
        .into_owned()
}

pub fn redact(text: &str) -> String {
    redact_full(text).chars().take(4000).collect()
}

fn redact_with_secrets<'a>(text: &str, secrets: impl IntoIterator<Item = &'a str>) -> String {
    let mut sanitized = text.to_owned();
    for secret in secrets {
        if !secret.is_empty() {
            sanitized = sanitized.replace(secret, "<已隐藏>");
        }
    }
    redact_full(&sanitized)
}

pub fn login_status() -> bool {
    run(&["login", "status"], None, Duration::from_secs(12))
        .map(|output| output.success && output.text.to_lowercase().contains("chatgpt"))
        .unwrap_or(false)
}

pub fn login() -> Result<()> {
    let output = run(&["login"], None, Duration::from_secs(600))?;
    if !output.success {
        return Err(ModelayError::Message(format!(
            "OpenAI 登录未完成：{}",
            summary(&output.text)
        )));
    }
    if login_status() {
        Ok(())
    } else {
        Err("登录结果不是 ChatGPT 官方账号。".into())
    }
}

pub fn doctor(environment: &[(&str, Option<&str>)]) -> Result<String> {
    let output = run_with_environment(&["doctor", "--json"], environment, Duration::from_secs(30))?;
    parse_doctor(&output.text, output.success)
}

fn parse_doctor(text: &str, process_success: bool) -> Result<String> {
    let report = parse_json_output(text).ok_or_else(|| {
        ModelayError::Message(format!("Codex Doctor 未返回有效 JSON：{}", summary(text)))
    })?;
    let checks = report
        .get("checks")
        .and_then(Value::as_object)
        .ok_or_else(|| ModelayError::Message("Codex Doctor 响应缺少 checks。".into()))?;
    let required = ["config.load", "auth.credentials"];
    let mut failures = Vec::new();
    for id in required {
        let Some(check) = checks.get(id) else {
            failures.push(format!("{id} 缺失"));
            continue;
        };
        let status = check
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if !matches!(status, "ok" | "warning" | "passed") {
            let detail = check
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or(status);
            failures.push(format!("{id}: {detail}"));
        }
    }
    if !failures.is_empty() {
        return Err(ModelayError::Message(format!(
            "Codex Doctor 关键检查未通过：{}",
            failures.join("；")
        )));
    }
    let overall = report
        .get("overallStatus")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if process_success && matches!(overall, "ok" | "passed") {
        Ok("配置与认证诊断通过".into())
    } else {
        Ok(format!(
            "配置与认证通过；Doctor 总状态为 {overall}，其余为非阻塞环境检查"
        ))
    }
}

fn parse_json_output(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    serde_json::from_str(trimmed)
        .ok()
        .or_else(|| {
            trimmed
                .lines()
                .rev()
                .find_map(|line| serde_json::from_str(line.trim()).ok())
        })
        .or_else(|| {
            let start = trimmed.find('{')?;
            let end = trimmed.rfind('}')?;
            serde_json::from_str(&trimmed[start..=end]).ok()
        })
}

pub fn summary(text: &str) -> String {
    let rows: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    rows.iter()
        .rev()
        .take(3)
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ")
        .chars()
        .take(300)
        .collect()
}

struct RpcProcess {
    child: Child,
    input: ChildStdin,
    receiver: mpsc::Receiver<Value>,
    output_reader: Option<std::thread::JoinHandle<()>>,
    next_id: i64,
}

impl RpcProcess {
    fn spawn() -> Result<Self> {
        Self::spawn_with_environment(&[])
    }

    fn spawn_with_environment(environment: &[(&str, Option<&str>)]) -> Result<Self> {
        let mut command = Command::new(platform::codex_executable()?);
        command
            .args(["app-server", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        platform::clear_provider_environment(&mut command);
        for (key, value) in environment {
            command.env_remove(key);
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        let mut child = command.spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| ModelayError::Message("无法连接 Codex app-server 输入。".into()))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| ModelayError::Message("无法连接 Codex app-server 输出。".into()))?;
        let (sender, receiver) = mpsc::channel();
        let output_reader = std::thread::spawn(move || {
            for line in BufReader::new(output)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    let _ = sender.send(value);
                }
            }
        });
        let mut process = Self {
            child,
            input,
            receiver,
            output_reader: Some(output_reader),
            next_id: 2,
        };
        process.write(&json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"modelay","title":"Modelay","version":env!("CARGO_PKG_VERSION"),"experimentalApi":true},"capabilities":null}}))?;
        process.wait_for(1, Duration::from_secs(20), "initialize")?;
        process.write(&json!({"method":"initialized"}))?;
        Ok(process)
    }

    fn write(&mut self, request: &Value) -> Result<()> {
        writeln!(self.input, "{}", serde_json::to_string(request)?)?;
        self.input.flush()?;
        Ok(())
    }

    fn wait_for(&mut self, id: i64, timeout: Duration, method: &str) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ModelayError::Message(format!("读取 Codex {method} 超时。")));
            }
            let value = self
                .receiver
                .recv_timeout(remaining)
                .map_err(|_| ModelayError::Message(format!("读取 Codex {method} 超时。")))?;
            if value.get("id").and_then(Value::as_i64) == Some(id) {
                return Ok(value);
            }
        }
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&json!({"id":id,"method":method,"params":params}))?;
        let response = self.wait_for(id, timeout, method)?;
        if let Some(error) = response.get("error") {
            return Err(ModelayError::Message(format!(
                "Codex {method} 返回错误：{}",
                redact(&error.to_string())
            )));
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| ModelayError::Message(format!("Codex {method} 未返回结果。")))
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.output_reader.take() {
            let _ = reader.join();
        }
    }
}

pub fn create_handoff_thread(
    prompt: String,
    cwd: &str,
    provider: &str,
    model: &str,
    environment: &[(&str, Option<&str>)],
) -> Result<String> {
    let mut process = RpcProcess::spawn_with_environment(environment)?;
    let started = process.request(
        "thread/start",
        json!({
            "cwd": cwd,
            "model": model,
            "modelProvider": provider,
            "approvalPolicy": "never",
            "sandbox": "danger-full-access"
        }),
        Duration::from_secs(30),
    )?;
    let thread_id = started
        .pointer("/thread/id")
        .and_then(Value::as_str)
        .ok_or_else(|| ModelayError::Message("Codex 未返回新任务 ID。".into()))?
        .to_owned();
    // A user message is persisted by starting a real turn. Interrupt it as soon
    // as Codex accepts the input so no model work keeps the new thread owned by
    // Modelay's short-lived app-server process.
    let turn = match process.request(
        "turn/start",
        handoff_turn_params(&thread_id, prompt),
        Duration::from_secs(30),
    ) {
        Ok(turn) => turn,
        Err(error) => {
            let _ = process.request(
                "thread/archive",
                json!({"threadId": thread_id}),
                Duration::from_secs(10),
            );
            return Err(error);
        }
    };
    let turn_id = turn
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .ok_or_else(|| ModelayError::Message("Codex 未返回续接轮次 ID。".into()))?;
    // Once turn/start returns, Codex has accepted and persisted the user
    // message. The turn may already have stopped before the interrupt arrives,
    // so an interrupt error must not turn a valid handoff into a false failure.
    let _ = process.request(
        "turn/interrupt",
        json!({"threadId": thread_id, "turnId": turn_id}),
        Duration::from_secs(10),
    );
    Ok(thread_id)
}

fn handoff_turn_params(thread_id: &str, prompt: String) -> Value {
    json!({
        "threadId": thread_id,
        "input": [{"type": "text", "text": prompt}]
    })
}

impl Drop for RpcProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn rpc_process() -> &'static Mutex<Option<RpcProcess>> {
    static PROCESS: OnceLock<Mutex<Option<RpcProcess>>> = OnceLock::new();
    PROCESS.get_or_init(|| Mutex::new(None))
}

pub fn reset_rpc() {
    if let Ok(mut process) = rpc_process().lock() {
        *process = None;
    }
}

pub fn rpc(method: &str, params: Value, timeout: Duration) -> Result<Value> {
    let mut process = rpc_process()
        .lock()
        .map_err(|_| ModelayError::Message("Codex app-server 状态锁异常。".into()))?;
    if process.is_none() {
        *process = Some(RpcProcess::spawn()?);
    }
    let result =
        process
            .as_mut()
            .expect("process initialized")
            .request(method, params.clone(), timeout);
    if result.is_ok() {
        return result;
    }
    *process = None;
    let mut replacement = RpcProcess::spawn()?;
    let retried = replacement.request(method, params, timeout);
    if retried.is_ok() {
        *process = Some(replacement);
    }
    retried
}

pub fn list_models() -> Result<Vec<ModelInfo>> {
    let mut result = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let value = rpc(
            "model/list",
            json!({"cursor":cursor,"includeHidden":false,"limit":100}),
            Duration::from_secs(20),
        )?;
        for item in value
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let id = item
                .get("model")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if id.is_empty() {
                continue;
            }
            let efforts = item
                .get("supportedReasoningEfforts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|effort| {
                    effort
                        .get("reasoningEffort")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .collect();
            result.push(ModelInfo {
                id: id.clone(),
                display_name: item
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_owned(),
                description: item
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                is_default: item
                    .get("isDefault")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                supported_reasoning_efforts: efforts,
            });
        }
        cursor = value
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result.dedup_by(|a, b| a.id == b.id);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_ignores_unrelated_terminal_failure() {
        let fixture = r#"{"overallStatus":"fail","checks":{"config.load":{"status":"ok"},"auth.credentials":{"status":"ok"},"terminal.env":{"status":"fail","summary":"TERM=dumb"}}}"#;
        assert!(parse_doctor(fixture, true).unwrap().contains("非阻塞"));
    }

    #[test]
    fn doctor_rejects_auth_failure() {
        let fixture = r#"{"overallStatus":"fail","checks":{"config.load":{"status":"ok"},"auth.credentials":{"status":"fail","summary":"missing credential"}}}"#;
        assert!(parse_doctor(fixture, true)
            .unwrap_err()
            .to_string()
            .contains("missing credential"));
    }

    #[test]
    fn doctor_accepts_json_surrounded_by_non_json_output() {
        let fixture = "warning before\n{\"overallStatus\":\"ok\",\"checks\":{\"config.load\":{\"status\":\"ok\"},\"auth.credentials\":{\"status\":\"ok\"}}}\nwarning after";
        assert_eq!(parse_doctor(fixture, true).unwrap(), "配置与认证诊断通过");
    }

    #[test]
    fn doctor_accepts_pretty_json_larger_than_the_display_limit() {
        let fixture = serde_json::to_string_pretty(&json!({
            "schemaVersion": 1,
            "overallStatus": "fail",
            "checks": {
                "config.load": {"status": "ok", "details": {"feature flags": "x".repeat(6000)}},
                "auth.credentials": {"status": "ok"},
                "network.provider_reachability": {"status": "warning"}
            }
        }))
        .unwrap();
        let sanitized = redact_with_secrets(&fixture, std::iter::empty::<&str>());
        assert!(sanitized.len() > 4000);
        assert!(parse_doctor(&sanitized, false).unwrap().contains("非阻塞"));
    }

    #[test]
    fn redaction_hides_explicit_and_common_credentials_before_truncation() {
        let explicit = "private-secret-value";
        let text = format!(
            "{}{} Bearer bearer-value {{\"api_key\":\"json-value\"}} sk-testCredential123",
            "x".repeat(3990),
            explicit
        );
        let redacted = redact_with_secrets(&text, [explicit]);
        assert!(!redacted.contains(explicit));
        assert!(!redacted.contains("bearer-value"));
        assert!(!redacted.contains("json-value"));
        assert!(!redacted.contains("sk-testCredential123"));
    }

    #[test]
    fn handoff_turn_uses_the_thread_model_without_repeating_overrides() {
        let params = handoff_turn_params("thread-id", "交接内容".into());
        assert_eq!(params["threadId"], "thread-id");
        assert_eq!(params["input"][0]["text"], "交接内容");
        assert!(params.get("model").is_none());
        assert!(params.get("effort").is_none());
    }
}
