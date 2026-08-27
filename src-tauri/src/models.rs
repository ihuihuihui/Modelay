use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChannelProfile {
    pub id: String,
    pub name: String,
    #[serde(alias = "baseURL")]
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_models_path", alias = "modelsPath")]
    pub models_path: String,
    #[serde(default = "default_usage_path", alias = "usagePath")]
    pub usage_path: String,
    #[serde(default = "default_true", alias = "validatesModelList")]
    pub validates_model_list: bool,
    #[serde(default, alias = "isBuiltIn")]
    pub is_built_in: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_secret: Option<bool>,
}

fn default_models_path() -> String {
    "/v1/models".into()
}
fn default_usage_path() -> String {
    "/v1/usage".into()
}
fn default_true() -> bool {
    true
}

impl ChannelProfile {
    #[cfg(test)]
    pub fn ailink() -> Self {
        Self {
            id: "ailink".into(),
            name: "AiLink".into(),
            base_url: "https://ai.ailink1.com".into(),
            model: "gpt-5.6-sol".into(),
            models_path: default_models_path(),
            usage_path: default_usage_path(),
            validates_model_list: true,
            is_built_in: true,
            has_secret: None,
        }
    }

    pub fn normalized_base_url(&self) -> String {
        self.base_url.trim().trim_end_matches('/').to_owned()
    }
    pub fn has_valid_id(&self) -> bool {
        let id = self.id.as_bytes();
        !id.is_empty()
            && id.len() <= 64
            && id[0].is_ascii_alphanumeric()
            && id
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            && self.id != "official"
    }
    pub fn provider_id(&self) -> String {
        if self.id == "ailink" {
            "custom".into()
        } else {
            format!("custom_{}", sanitize_identifier(&self.id).to_lowercase())
        }
    }
    pub fn environment_key(&self) -> String {
        if self.id == "ailink" {
            "AILINK_API_KEY".into()
        } else {
            format!(
                "CODEX_{}_API_KEY",
                sanitize_identifier(&self.id).to_uppercase()
            )
        }
    }
    pub fn endpoint(&self, path: &str) -> Option<String> {
        let base = self.normalized_base_url();
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        let suffix = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };
        if base.ends_with("/v1") && suffix.starts_with("/v1/") {
            Some(format!("{}{}", base, &suffix[3..]))
        } else {
            Some(format!("{base}{suffix}"))
        }
    }
}

fn sanitize_identifier(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default = "default_channels")]
    pub channels: Vec<ChannelProfile>,
    #[serde(default = "default_official_model", alias = "officialModel")]
    pub official_model: String,
    #[serde(default, alias = "lastChannelID")]
    pub last_channel_id: Option<String>,
    #[serde(default = "default_dock_mode")]
    pub dock_mode: String,
    #[serde(default)]
    pub widget_position: Option<WidgetPosition>,
}

fn default_channels() -> Vec<ChannelProfile> {
    Vec::new()
}
fn default_official_model() -> String {
    "gpt-5.6-sol".into()
}
fn default_dock_mode() -> String {
    "free".into()
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            channels: default_channels(),
            official_model: default_official_model(),
            last_channel_id: None,
            dock_mode: default_dock_mode(),
            widget_position: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveChannelRequest {
    pub channel: ChannelProfile,
    #[serde(default)]
    pub secret: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchRequest {
    pub channel_id: String,
    pub model: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub title: String,
    pub detail: String,
    pub state: CheckState,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckState {
    Passed,
    Warning,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchReport {
    pub channel_id: String,
    pub provider_id: String,
    pub model: String,
    pub image_skill: String,
    pub backup_path: String,
    pub needs_restart: bool,
    pub checks: Vec<CheckResult>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub platform: String,
    pub current_mode: String,
    pub current_channel_id: Option<String>,
    pub current_provider_id: String,
    pub current_model: String,
    pub official_logged_in: bool,
    pub config_exists: bool,
    pub config_conformant: bool,
    pub image_skill: String,
    pub channels: Vec<ChannelProfile>,
    pub official_model: String,
    pub backup_directory: String,
    pub dock_mode: String,
    pub widget_position: Option<WidgetPosition>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetState {
    pub current_mode: String,
    pub current_channel_id: Option<String>,
    pub current_provider_id: String,
    pub dock_mode: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub is_default: bool,
    pub supported_reasoning_efforts: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub remaining_percent: f64,
    pub duration_minutes: Option<i64>,
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub kind: String,
    pub channel_id: String,
    pub plan_name: Option<String>,
    pub five_hour: Option<UsageWindow>,
    pub weekly: Option<UsageWindow>,
    pub remaining_balance: Option<f64>,
    pub balance_label: Option<String>,
    pub credits_balance: Option<String>,
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_and_endpoint_normalization() {
        let channel = ChannelProfile {
            id: "channel-test".into(),
            name: "Test".into(),
            base_url: "https://proxy.example/v1/".into(),
            model: "m".into(),
            models_path: "/v1/models".into(),
            usage_path: "/v1/usage".into(),
            validates_model_list: true,
            is_built_in: false,
            has_secret: None,
        };
        assert_eq!(channel.provider_id(), "custom_channel_test");
        assert_eq!(channel.environment_key(), "CODEX_CHANNEL_TEST_API_KEY");
        assert_eq!(
            channel.endpoint(&channel.models_path).unwrap(),
            "https://proxy.example/v1/models"
        );
        assert!(channel.has_valid_id());
        assert!(!ChannelProfile {
            id: "bad id".into(),
            ..channel.clone()
        }
        .has_valid_id());
        assert!(!ChannelProfile {
            id: "-bad".into(),
            ..channel.clone()
        }
        .has_valid_id());
        assert_eq!(
            ChannelProfile {
                id: "same-id".into(),
                ..channel.clone()
            }
            .provider_id(),
            ChannelProfile {
                id: "same_id".into(),
                ..channel
            }
            .provider_id()
        );
    }
}
