use crate::error::{ModelayError, Result};
use std::path::PathBuf;

pub fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| ModelayError::Message("无法定位用户主目录。".into()))
}

pub fn codex_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(".codex"))
}
pub fn config_path() -> Result<PathBuf> {
    Ok(codex_dir()?.join("config.toml"))
}
pub fn state_db_path() -> Result<PathBuf> {
    Ok(codex_dir()?.join("state_5.sqlite"))
}

pub fn support_dir() -> Result<PathBuf> {
    dirs::data_dir()
        .map(|path| path.join("Modelay"))
        .ok_or_else(|| ModelayError::Message("无法定位应用数据目录。".into()))
}

pub fn preferences_path() -> Result<PathBuf> {
    Ok(support_dir()?.join("preferences.json"))
}
pub fn image_routing_path() -> Result<PathBuf> {
    Ok(support_dir()?.join("image-generation-routing.json"))
}
pub fn backup_dir() -> Result<PathBuf> {
    Ok(support_dir()?.join("Backups"))
}
