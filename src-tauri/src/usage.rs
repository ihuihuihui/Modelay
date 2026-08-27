use crate::codex;
use crate::error::{ModelayError, Result};
use crate::models::{ChannelProfile, UsageSnapshot, UsageWindow};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use std::time::Instant;

const CACHE_TTL: Duration = Duration::from_secs(8);

#[derive(Clone)]
struct CacheEntry {
    stored_at: Instant,
    snapshot: UsageSnapshot,
}

fn usage_cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn cached(
    channel_id: &str,
    fetch: impl FnOnce() -> Result<UsageSnapshot>,
) -> Result<UsageSnapshot> {
    let mut cache = usage_cache()
        .lock()
        .map_err(|_| ModelayError::Message("额度缓存状态异常。".into()))?;
    if let Some(entry) = cache.get(channel_id) {
        if entry.stored_at.elapsed() < CACHE_TTL {
            return Ok(entry.snapshot.clone());
        }
    }
    let snapshot = fetch()?;
    cache.insert(
        channel_id.to_owned(),
        CacheEntry {
            stored_at: Instant::now(),
            snapshot: snapshot.clone(),
        },
    );
    Ok(snapshot)
}

pub fn clear_cache() {
    if let Ok(mut cache) = usage_cache().lock() {
        cache.clear();
    }
}

pub fn official() -> Result<UsageSnapshot> {
    let result = codex::rpc(
        "account/rateLimits/read",
        json!({}),
        Duration::from_secs(20),
    )?;
    parse_official(&result)
}

pub fn channel(channel: &ChannelProfile, secret: &str) -> Result<UsageSnapshot> {
    if channel.usage_path.trim().is_empty() {
        return Err(ModelayError::Message(format!(
            "{} 不支持余额查询：未配置余额接口。",
            channel.name
        )));
    }
    let endpoint = channel
        .endpoint(&channel.usage_path)
        .ok_or_else(|| ModelayError::Message(format!("{} 余额地址无效。", channel.name)))?;
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?
        .get(endpoint)
        .bearer_auth(secret)
        .header("Accept", "application/json")
        .send()?;
    let status = response.status();
    if matches!(status.as_u16(), 404 | 405) {
        return Err(ModelayError::Message(format!(
            "{} 不支持余额查询（HTTP {}）。",
            channel.name,
            status.as_u16()
        )));
    }
    let data: Value = response
        .json()
        .map_err(|_| ModelayError::Message(format!("{} 余额响应不是有效 JSON。", channel.name)))?;
    if !status.is_success() {
        let detail = data
            .get("message")
            .or_else(|| data.pointer("/error/message"))
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        let detail = detail.replace(secret, "<已隐藏>");
        return Err(ModelayError::Message(format!(
            "{} 余额查询失败（HTTP {}）：{}",
            channel.name,
            status.as_u16(),
            detail
        )));
    }
    parse_channel(&channel.id, &data)
}

fn parse_official(result: &Value) -> Result<UsageSnapshot> {
    let by_limit_id = result.get("rateLimitsByLimitId").and_then(Value::as_object);
    let preferred_bucket = by_limit_id
        .and_then(|items| items.get("codex"))
        .or_else(|| result.get("rateLimits"));
    if preferred_bucket.is_none() && by_limit_id.is_none_or(serde_json::Map::is_empty) {
        return Err(ModelayError::Message(
            "OpenAI 额度响应缺少 rateLimits。".into(),
        ));
    }

    // Multi-limit accounts can include unrelated buckets such as
    // `base_model_inference`. The UI is specifically reporting Codex usage, so
    // never let a different bucket with the same window duration win merely
    // because its map key sorts first.
    let mut windows = preferred_bucket
        .map(windows_from_bucket)
        .unwrap_or_default();
    if windows.is_empty() {
        windows = by_limit_id
            .into_iter()
            .flat_map(|items| items.values())
            .flat_map(windows_from_bucket)
            .collect();
    }
    windows.sort_by_key(|window| window.duration_minutes.unwrap_or(i64::MAX));
    windows.dedup_by(|left, right| {
        left.duration_minutes == right.duration_minutes
            && left.resets_at == right.resets_at
            && (left.remaining_percent - right.remaining_percent).abs() < f64::EPSILON
    });
    let five_hour = closest_window(&windows, 300, |minutes| minutes <= 600)
        .or_else(|| windows.first().cloned());
    let weekly = closest_window(&windows, 10_080, |minutes| minutes >= 1_000)
        .or_else(|| windows.get(1).cloned());

    let metadata_bucket = by_limit_id
        .and_then(|items| items.get("codex").or_else(|| items.values().next()))
        .or_else(|| result.get("rateLimits"));
    let credits_balance = metadata_bucket
        .and_then(|bucket| bucket.pointer("/credits/balance"))
        .or_else(|| result.pointer("/credits/balance"))
        .and_then(string_or_number);
    Ok(UsageSnapshot {
        kind: "official".into(),
        channel_id: "official".into(),
        plan_name: metadata_bucket
            .and_then(|bucket| bucket.get("planType"))
            .or_else(|| result.get("planType"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        five_hour,
        weekly,
        remaining_balance: None,
        balance_label: None,
        credits_balance,
        updated_at: Utc::now().timestamp(),
    })
}

fn windows_from_bucket(bucket: &Value) -> Vec<UsageWindow> {
    let mut windows = Vec::new();
    windows.extend(bucket.get("primary").and_then(parse_window));
    windows.extend(bucket.get("secondary").and_then(parse_window));
    if bucket.get("usedPercent").is_some() {
        windows.extend(parse_window(bucket));
    }
    windows
}

fn closest_window(
    windows: &[UsageWindow],
    target_minutes: i64,
    predicate: impl Fn(i64) -> bool,
) -> Option<UsageWindow> {
    windows
        .iter()
        .filter_map(|window| {
            let minutes = window.duration_minutes?;
            predicate(minutes).then_some((minutes.abs_diff(target_minutes), window))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, window)| window.clone())
}

fn parse_window(value: &Value) -> Option<UsageWindow> {
    let used = value.get("usedPercent")?.as_f64()?;
    Some(UsageWindow {
        remaining_percent: (100.0 - used).clamp(0.0, 100.0),
        duration_minutes: value.get("windowDurationMins").and_then(Value::as_i64),
        resets_at: value.get("resetsAt").and_then(Value::as_i64),
    })
}

fn parse_channel(channel_id: &str, value: &Value) -> Result<UsageSnapshot> {
    let (remaining, label) = if let Some(value) =
        number(value.get("remaining")).or_else(|| number(value.get("balance")))
    {
        (value, value_label(value, "可用余额"))
    } else if let Some(value) = number(value.pointer("/quota/remaining")) {
        (value, "剩余配额".into())
    } else {
        return Err(ModelayError::Message(
            "余额响应中没有 remaining、balance 或 quota.remaining。".into(),
        ));
    };
    Ok(UsageSnapshot {
        kind: "channel".into(),
        channel_id: channel_id.into(),
        plan_name: value
            .get("planName")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| Some(label.clone())),
        five_hour: None,
        weekly: None,
        remaining_balance: Some(remaining),
        balance_label: Some(label),
        credits_balance: None,
        updated_at: Utc::now().timestamp(),
    })
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn string_or_number(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
        .or_else(|| value.as_f64().map(|value| value.to_string()))
}
fn value_label(_value: f64, fallback: &str) -> String {
    fallback.into()
}

pub fn list_channel_models(
    channel: &ChannelProfile,
    secret: &str,
) -> Result<Vec<crate::models::ModelInfo>> {
    let endpoint = channel
        .endpoint(&channel.models_path)
        .ok_or_else(|| ModelayError::Message(format!("{} 模型列表地址无效。", channel.name)))?;
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?
        .get(endpoint)
        .bearer_auth(secret)
        .send()?;
    if !response.status().is_success() {
        return Err(ModelayError::Message(format!(
            "无法读取 {} 模型列表（HTTP {}）。",
            channel.name,
            response.status().as_u16()
        )));
    }
    let value: Value = response.json()?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| ModelayError::Message(format!("{} 模型列表格式无法识别。", channel.name)))?;
    let mut models = data
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(|id| crate::models::ModelInfo {
            id: id.into(),
            display_name: id.into(),
            description: String::new(),
            is_default: false,
            supported_reasoning_efforts: Vec::new(),
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(models)
}

pub fn endpoint_status(channel: &ChannelProfile, secret: &str) -> Result<String> {
    let endpoint = channel
        .endpoint(&channel.models_path)
        .unwrap_or_else(|| channel.normalized_base_url());
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()?
        .get(endpoint)
        .bearer_auth(secret)
        .send()?;
    let status = response.status();
    if matches!(status.as_u16(), 401 | 403) {
        return Err(ModelayError::Message(format!(
            "{} 拒绝了当前 API 密钥（HTTP {}）。",
            channel.name,
            status.as_u16()
        )));
    }
    if status.as_u16() == 429 {
        return Err(ModelayError::Message(format!(
            "{} 当前受到限流（HTTP 429），已停止切换。",
            channel.name
        )));
    }
    if status.is_server_error() {
        return Err(ModelayError::Message(format!(
            "{} 服务返回 HTTP {}。",
            channel.name,
            status.as_u16()
        )));
    }
    Ok(format!("服务可达（HTTP {}）", status.as_u16()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[test]
    fn parses_official_single_and_multi_bucket() {
        let value = json!({"rateLimits":{"primary":{"usedPercent":11,"windowDurationMins":300,"resetsAt":1787748781},"secondary":{"usedPercent":38,"windowDurationMins":10080,"resetsAt":1788280440},"planType":"plus","credits":{"balance":"12.5"}}});
        let snapshot = parse_official(&value).unwrap();
        assert_eq!(snapshot.five_hour.unwrap().remaining_percent, 89.0);
        assert_eq!(snapshot.weekly.unwrap().remaining_percent, 62.0);
        assert_eq!(snapshot.credits_balance.as_deref(), Some("12.5"));
        let numeric_credits = json!({"rateLimits":{"primary":{"usedPercent":11,"windowDurationMins":300},"credits":{"balance":8.75}}});
        assert_eq!(
            parse_official(&numeric_credits)
                .unwrap()
                .credits_balance
                .as_deref(),
            Some("8.75")
        );
        let multi = json!({"rateLimits":{},"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":20,"windowDurationMins":300},"secondary":{"usedPercent":30,"windowDurationMins":10080}}}});
        assert_eq!(
            parse_official(&multi)
                .unwrap()
                .weekly
                .unwrap()
                .remaining_percent,
            70.0
        );

        let split_buckets = json!({
            "rateLimitsByLimitId": {
                "short": {"usedPercent": 25, "windowDurationMins": 300, "resetsAt": 10},
                "long": {"usedPercent": 45, "windowDurationMins": 10080, "resetsAt": 20}
            },
            "credits": {"balance": 3.5},
            "planType": "plus"
        });
        let snapshot = parse_official(&split_buckets).unwrap();
        assert_eq!(snapshot.five_hour.unwrap().remaining_percent, 75.0);
        assert_eq!(snapshot.weekly.unwrap().remaining_percent, 55.0);
        assert_eq!(snapshot.credits_balance.as_deref(), Some("3.5"));
        assert_eq!(snapshot.plan_name.as_deref(), Some("plus"));

        let unrelated_bucket_first = json!({
            "rateLimits": {},
            "rateLimitsByLimitId": {
                "base_model_inference": {
                    "primary": {"usedPercent": 91, "windowDurationMins": 10080}
                },
                "codex": {
                    "primary": {"usedPercent": 12, "windowDurationMins": 300},
                    "secondary": {"usedPercent": 34, "windowDurationMins": 10080}
                }
            }
        });
        let snapshot = parse_official(&unrelated_bucket_first).unwrap();
        assert_eq!(snapshot.five_hour.unwrap().remaining_percent, 88.0);
        assert_eq!(snapshot.weekly.unwrap().remaining_percent, 66.0);
    }
    #[test]
    fn parses_channel_wallet_and_quota() {
        assert_eq!(
            parse_channel("ailink", &json!({"remaining":280.25,"planName":"钱包余额"}))
                .unwrap()
                .remaining_balance,
            Some(280.25)
        );
        assert_eq!(
            parse_channel("ailink", &json!({"quota":{"remaining":27.5}}))
                .unwrap()
                .balance_label
                .as_deref(),
            Some("剩余配额")
        );
    }

    #[test]
    fn endpoint_status_rejects_auth_rate_limit_and_server_failures() {
        for status in [401, 403, 429, 500] {
            let (channel, server) = local_status_channel(status);
            let result = endpoint_status(&channel, "test-secret");
            server.join().unwrap();
            assert!(result.is_err(), "HTTP {status} should block switching");
        }
        let (channel, server) = local_status_channel(404);
        assert!(endpoint_status(&channel, "test-secret")
            .unwrap()
            .contains("HTTP 404"));
        server.join().unwrap();
    }

    #[test]
    fn channel_error_never_exposes_the_full_secret() {
        let secret = "private-test-secret-123";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let echoed = secret.to_owned();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            let body = serde_json::json!({"message": format!("rejected {echoed}")}).to_string();
            write!(
                stream,
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let mut channel_profile = ChannelProfile::ailink();
        channel_profile.base_url = format!("http://{address}");
        let error = channel(&channel_profile, secret).unwrap_err().to_string();
        server.join().unwrap();
        assert!(!error.contains(secret));
        assert!(error.contains("<已隐藏>"));
    }

    #[test]
    fn missing_usage_endpoint_is_reported_as_unsupported() {
        let (channel_profile, server) = local_status_channel(404);
        let error = channel(&channel_profile, "test-secret")
            .unwrap_err()
            .to_string();
        server.join().unwrap();
        assert!(error.contains("不支持余额查询"));
        assert!(error.contains("HTTP 404"));
    }

    #[test]
    fn shared_cache_coalesces_simultaneous_window_refreshes() {
        static FETCHES: AtomicUsize = AtomicUsize::new(0);
        let key = format!("cache-test-{:?}", std::thread::current().id());
        let fetch = || {
            FETCHES.fetch_add(1, Ordering::SeqCst);
            Ok(UsageSnapshot {
                kind: "official".into(),
                channel_id: "official".into(),
                plan_name: None,
                five_hour: None,
                weekly: None,
                remaining_balance: None,
                balance_label: None,
                credits_balance: None,
                updated_at: Utc::now().timestamp(),
            })
        };
        let before = FETCHES.load(Ordering::SeqCst);
        let first = cached(&key, fetch).unwrap();
        let second = cached(&key, fetch).unwrap();
        assert_eq!(first.updated_at, second.updated_at);
        assert_eq!(FETCHES.load(Ordering::SeqCst) - before, 1);
    }

    fn local_status_channel(status: u16) -> (ChannelProfile, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            let reason = match status {
                401 => "Unauthorized",
                403 => "Forbidden",
                404 => "Not Found",
                429 => "Too Many Requests",
                _ => "Internal Server Error",
            };
            write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });
        let mut channel = ChannelProfile::ailink();
        channel.base_url = format!("http://{address}");
        (channel, server)
    }
}
