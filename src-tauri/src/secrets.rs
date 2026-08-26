use crate::error::{ModelayError, Result};
use crate::models::ChannelProfile;
#[cfg(target_os = "macos")]
use crate::platform;

const SERVICE: &str = "app.modelay.desktop";
#[cfg(target_os = "macos")]
const LEGACY_SERVICE: &str = "com.local.CodexSwitch";

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
        return Ok(Some(value));
    }
    stored(channel)
}

pub fn has(channel: &ChannelProfile) -> bool {
    get(channel).ok().flatten().is_some()
}

pub fn set(channel: &ChannelProfile, secret: &str) -> Result<()> {
    if secret.trim().is_empty() {
        return Err("API 密钥不能为空。".into());
    }
    entry(SERVICE, channel)?.set_password(secret.trim())?;
    Ok(())
}

pub fn delete(channel: &ChannelProfile) -> Result<()> {
    match entry(SERVICE, channel)?.delete_credential() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(target_os = "macos")]
pub fn migrate_legacy_noninteractive(channel: &ChannelProfile) -> Result<bool> {
    if has(channel) {
        return Ok(false);
    }
    if let Some(value) = read_noninteractive(LEGACY_SERVICE, &account(channel)) {
        set(channel, &value)?;
        Ok(true)
    } else {
        Ok(false)
    }
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

#[cfg(not(target_os = "macos"))]
pub fn migrate_legacy_noninteractive(_channel: &ChannelProfile) -> Result<bool> {
    Ok(false)
}
