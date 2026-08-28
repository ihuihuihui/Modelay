use crate::error::{ModelayError, Result};
use crate::models::ChannelProfile;
use crate::{paths, storage};
use chrono::Local;
use std::fs;
use std::path::PathBuf;
use toml_edit::{value, DocumentMut, Item, Table};

pub struct ConfigDocument {
    pub existed: bool,
    pub original: String,
    pub document: DocumentMut,
}

pub fn read() -> Result<ConfigDocument> {
    let path = paths::config_path()?;
    let existed = path.exists();
    let original = if existed {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    let document = if original.trim().is_empty() {
        DocumentMut::new()
    } else {
        original.parse::<DocumentMut>()?
    };
    Ok(ConfigDocument {
        existed,
        original,
        document,
    })
}

pub fn active_provider(document: &DocumentMut) -> String {
    document
        .get("model_provider")
        .and_then(Item::as_str)
        .unwrap_or("openai_http")
        .to_owned()
}

pub fn active_model(document: &DocumentMut) -> String {
    document
        .get("model")
        .and_then(Item::as_str)
        .unwrap_or("")
        .to_owned()
}

pub fn active_reasoning_effort(document: &DocumentMut) -> String {
    document
        .get("model_reasoning_effort")
        .and_then(Item::as_str)
        .filter(|value| crate::models::valid_reasoning_effort(value))
        .unwrap_or("medium")
        .to_owned()
}

pub fn activate_official(document: &mut DocumentMut, model: &str, reasoning_effort: &str) {
    document["model"] = value(model);
    document["model_reasoning_effort"] = value(reasoning_effort);
    ensure_official_provider(document);
    document["model_provider"] = value("openai_http");
}

/// Keep the provider definition available even when the active channel is a
/// third-party one. Existing official tasks store `openai_http` in SQLite and
/// Codex must be able to resolve that provider when those tasks are opened.
pub fn ensure_official_provider(document: &mut DocumentMut) {
    if !document.contains_key("model_providers") {
        document["model_providers"] = Item::Table(Table::new());
    }
    let Some(providers) = document["model_providers"].as_table_mut() else {
        return;
    };
    if !providers.contains_key("openai_http") {
        providers["openai_http"] = Item::Table(Table::new());
    }
    if let Some(provider) = providers["openai_http"].as_table_mut() {
        provider["name"] = value("OpenAI");
        provider["requires_openai_auth"] = value(true);
        provider["wire_api"] = value("responses");
        provider.remove("env_key");
        provider.remove("base_url");
        provider.remove("experimental_bearer_token");
    }
}

pub fn activate_channel(
    document: &mut DocumentMut,
    channel: &ChannelProfile,
    reasoning_effort: &str,
) -> Result<()> {
    ensure_official_provider(document);
    let provider_id = channel.provider_id();
    document["model_provider"] = value(&provider_id);
    document["model"] = value(channel.model.trim());
    document["model_reasoning_effort"] = value(reasoning_effort);
    if !document.contains_key("model_providers") {
        document["model_providers"] = Item::Table(Table::new());
    }
    let providers = document["model_providers"].as_table_mut().ok_or_else(|| {
        ModelayError::Message("config.toml 中 model_providers 不是有效表格。".into())
    })?;
    if !providers.contains_key(&provider_id) {
        providers[&provider_id] = Item::Table(Table::new());
    }
    let provider = providers[&provider_id].as_table_mut().ok_or_else(|| {
        ModelayError::Message(format!("Provider {provider_id} 配置不是有效表格。"))
    })?;
    provider["name"] = value(channel.name.trim());
    provider["base_url"] = value(channel.normalized_base_url());
    provider["env_key"] = value(channel.environment_key());
    provider["wire_api"] = value("responses");
    provider["requires_openai_auth"] = value(false);
    provider["supports_websockets"] = value(false);
    provider.remove("experimental_bearer_token");
    Ok(())
}

pub fn is_channel_conformant(document: &DocumentMut, channel: &ChannelProfile) -> bool {
    let provider = document
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|table| table.get(&channel.provider_id()))
        .and_then(Item::as_table);
    let Some(provider) = provider else {
        return false;
    };
    provider.get("base_url").and_then(Item::as_str) == Some(channel.normalized_base_url().as_str())
        && provider.get("env_key").and_then(Item::as_str)
            == Some(channel.environment_key().as_str())
        && provider.get("wire_api").and_then(Item::as_str) == Some("responses")
        && provider.get("requires_openai_auth").and_then(Item::as_bool) == Some(false)
        && provider.get("supports_websockets").and_then(Item::as_bool) == Some(false)
        && provider.get("experimental_bearer_token").is_none()
}

pub fn backup(original: &str) -> Result<PathBuf> {
    let directory = paths::backup_dir()?;
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!(
        "config-{}.toml",
        Local::now().format("%Y%m%d-%H%M%S-%3f")
    ));
    fs::write(&path, original.as_bytes())?;
    Ok(path)
}

pub fn write(document: &DocumentMut) -> Result<()> {
    storage::atomic_write(&paths::config_path()?, document.to_string().as_bytes())
}

pub fn restore(original: &str, existed: bool) -> Result<()> {
    let path = paths::config_path()?;
    if existed {
        storage::atomic_write(&path, original.as_bytes())
    } else if path.exists() {
        fs::remove_file(path).map_err(ModelayError::from)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_unrelated_sections_and_removes_plaintext_secret() {
        let fixture = r#"model_provider = "custom"
model = "old"

[model_providers.custom]
name = "Old"
experimental_bearer_token = "secret"

[mcp_servers.node]
command = "node"

[plugins."browser@openai-bundled"]
enabled = true
"#;
        let mut document = fixture.parse::<DocumentMut>().unwrap();
        let mut channel = ChannelProfile::ailink();
        channel.model = "gpt-5.6-sol".into();
        activate_channel(&mut document, &channel, "medium").unwrap();
        let output = document.to_string();
        assert!(output.contains("[mcp_servers.node]"));
        assert!(output.contains("[plugins.\"browser@openai-bundled\"]"));
        assert!(!output.contains("experimental_bearer_token"));
        assert!(is_channel_conformant(&document, &channel));
        activate_official(&mut document, "gpt-5.6-sol", "low");
        assert_eq!(active_provider(&document), "openai_http");
        assert!(document
            .to_string()
            .contains("[model_providers.openai_http]"));
        assert_eq!(active_reasoning_effort(&document), "low");
        assert!(document.to_string().contains("[model_providers.custom]"));
    }
}
