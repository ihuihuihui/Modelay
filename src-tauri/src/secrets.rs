use crate::error::{ModelayError, Result};
use crate::models::ChannelProfile;
#[cfg(target_os = "macos")]
use crate::platform;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[cfg(target_os = "macos")]
const SERVICE: &str = "app.modelay.desktop.v2";
#[cfg(not(target_os = "macos"))]
const SERVICE: &str = "app.modelay.desktop";

fn secret_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached(channel: &ChannelProfile) -> Option<String> {
    secret_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&account(channel)).cloned())
}

fn remember(channel: &ChannelProfile, secret: &str) {
    if let Ok(mut cache) = secret_cache().lock() {
        cache.insert(account(channel), secret.to_owned());
    }
}

fn forget(channel: &ChannelProfile) {
    if let Ok(mut cache) = secret_cache().lock() {
        cache.remove(&account(channel));
    }
}

fn account(channel: &ChannelProfile) -> String {
    if channel.id == "ailink" {
        "AiLink".into()
    } else {
        format!("Channel.{}", channel.id)
    }
}

fn entry(service: &str, channel: &ChannelProfile) -> Result<keyring::Entry> {
    keyring::Entry::new(service, &account(channel)).map_err(ModelayError::from)
}

#[cfg(not(target_os = "macos"))]
pub fn stored(channel: &ChannelProfile) -> Result<Option<String>> {
    match entry(SERVICE, channel)?.get_password() {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(target_os = "macos")]
pub fn stored(channel: &ChannelProfile) -> Result<Option<String>> {
    Ok(read_noninteractive(SERVICE, &account(channel)))
}

pub fn get(channel: &ChannelProfile) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    if let Some(value) = platform::get_user_environment(&channel.environment_key())? {
        remember(channel, &value);
        return Ok(Some(value));
    }
    if let Some(value) = cached(channel) {
        return Ok(Some(value));
    }
    let value = stored(channel)?;
    if let Some(secret) = value.as_deref() {
        remember(channel, secret);
    }
    Ok(value)
}

pub fn has(channel: &ChannelProfile) -> bool {
    get(channel).ok().flatten().is_some()
}

pub fn set(channel: &ChannelProfile, secret: &str) -> Result<()> {
    if secret.trim().is_empty() {
        return Err("API 密钥不能为空。".into());
    }
    let secret = secret.trim();
    entry(SERVICE, channel)?.set_password(secret)?;
    remember(channel, secret);
    Ok(())
}

pub fn delete(channel: &ChannelProfile) -> Result<()> {
    let result = match entry(SERVICE, channel)?.delete_credential() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
    };
    if result.is_ok() {
        forget(channel);
    }
    result
}

#[cfg(target_os = "macos")]
fn read_noninteractive(service: &str, account: &str) -> Option<String> {
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
    let mut options = ItemSearchOptions::new();
    options
        .class(ItemClass::generic_password())
        .service(service)
        .account(account)
        .load_data(true)
        .skip_authenticated_items(true);
    options
        .search()
        .ok()
        .and_then(|items| {
            items.into_iter().find_map(|item| match item {
                SearchResult::Data(data) => String::from_utf8(data).ok(),
                _ => None,
            })
        })
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_cache_survives_environment_changes_without_keychain_reads() {
        let channel = ChannelProfile::ailink();
        forget(&channel);
        remember(&channel, "cached-secret");
        assert_eq!(cached(&channel).as_deref(), Some("cached-secret"));
        forget(&channel);
        assert!(cached(&channel).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_the_stable_second_generation_keychain_service() {
        assert_eq!(SERVICE, "app.modelay.desktop.v2");
    }
}
