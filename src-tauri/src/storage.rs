use crate::error::Result;
use crate::models::{ChannelProfile, Preferences};
use crate::{paths, platform, secrets};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct FileSnapshot {
    path: PathBuf,
    data: Option<Vec<u8>>,
}

impl FileSnapshot {
    pub fn restore(&self) -> Result<()> {
        match &self.data {
            Some(data) => atomic_write(&self.path, data),
            None if self.path.exists() => fs::remove_file(&self.path).map_err(Into::into),
            None => Ok(()),
        }
    }
}

pub fn initialize() -> Result<Preferences> {
    fs::create_dir_all(paths::support_dir()?)?;
    fs::create_dir_all(paths::backup_dir()?)?;
    let preferences_path = paths::preferences_path()?;
    let first_launch = !preferences_path.exists();
    let mut preferences = if !first_launch {
        serde_json::from_slice::<Preferences>(&fs::read(&preferences_path)?)?
    } else {
        migrate_legacy_preferences()?.unwrap_or_default()
    };
    normalize(&mut preferences);
    save_preferences(&preferences)?;
    migrate_image_routing()?;
    Ok(preferences)
}

pub fn migrate_secrets_nonblocking() {
    let Ok(preferences) = load_preferences() else {
        return;
    };
    for channel in &preferences.channels {
        let key = channel.environment_key();
        if let Ok(Some(secret)) = platform::get_user_environment(&key) {
            let _ = secrets::set(channel, &secret);
        } else {
            let _ = secrets::migrate_legacy_noninteractive(channel);
        }
    }
}

fn migrate_legacy_preferences() -> Result<Option<Preferences>> {
    let directory = paths::legacy_support_dir()?;
    let preferences_path = directory.join("preferences.json");
    let settings_path = directory.join("ailink.json");
    let preferences_value = preferences_path
        .exists()
        .then(|| fs::read(&preferences_path))
        .transpose()?
        .map(|data| serde_json::from_slice::<serde_json::Value>(&data))
        .transpose()?;
    let settings_value = settings_path
        .exists()
        .then(|| fs::read(&settings_path))
        .transpose()?
        .map(|data| serde_json::from_slice::<serde_json::Value>(&data))
        .transpose()?;
    let Some(mut preferences) = parse_legacy_preferences(preferences_value, settings_value)? else {
        return Ok(None);
    };
    normalize(&mut preferences);
    Ok(Some(preferences))
}

fn parse_legacy_preferences(
    preferences: Option<serde_json::Value>,
    settings: Option<serde_json::Value>,
) -> Result<Option<Preferences>> {
    if let Some(value) = preferences.as_ref() {
        if value
            .get("channels")
            .and_then(serde_json::Value::as_array)
            .is_some()
        {
            return Ok(Some(serde_json::from_value(value.clone())?));
        }
    }
    let source = preferences
        .as_ref()
        .and_then(|value| value.get("aiLink"))
        .or(settings.as_ref());
    let Some(source) = source else {
        return Ok(preferences.map(serde_json::from_value).transpose()?);
    };
    let mut channel = ChannelProfile::ailink();
    if let Some(base_url) = source
        .get("baseURL")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        channel.base_url = base_url.to_owned();
    }
    if let Some(model) = source
        .get("model")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        channel.model = model.to_owned();
    }
    let official_model = preferences
        .as_ref()
        .and_then(|value| value.get("officialModel"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("gpt-5.6-sol")
        .to_owned();
    Ok(Some(Preferences {
        channels: vec![channel],
        official_model,
        last_channel_id: Some("ailink".into()),
        ..Preferences::default()
    }))
}

fn migrate_image_routing() -> Result<()> {
    let target = paths::image_routing_path()?;
    if target.exists() {
        return Ok(());
    }
    let legacy = paths::legacy_support_dir()?.join("image-generation-routing.json");
    if legacy.exists() {
        fs::copy(legacy, target)?;
    }
    Ok(())
}

fn normalize(preferences: &mut Preferences) {
    if !preferences
        .channels
        .iter()
        .any(|channel| channel.id == "ailink")
    {
        preferences.channels.insert(0, ChannelProfile::ailink());
    }
    for channel in &mut preferences.channels {
        channel.has_secret = None;
        channel.is_built_in = channel.id == "ailink";
    }
    let mut seen = std::collections::HashSet::new();
    preferences
        .channels
        .retain(|channel| seen.insert(channel.id.clone()));
    if preferences.official_model.trim().is_empty() {
        preferences.official_model = "gpt-5.6-sol".into();
    }
    if !matches!(preferences.dock_mode.as_str(), "free" | "edge" | "off") {
        preferences.dock_mode = "free".into();
    }
}

pub fn load_preferences() -> Result<Preferences> {
    let path = paths::preferences_path()?;
    if !path.exists() {
        return initialize();
    }
    let mut preferences = serde_json::from_slice::<Preferences>(&fs::read(path)?)?;
    normalize(&mut preferences);
    Ok(preferences)
}

pub fn save_preferences(preferences: &Preferences) -> Result<()> {
    fs::create_dir_all(paths::support_dir()?)?;
    let mut safe = preferences.clone();
    for channel in &mut safe.channels {
        channel.has_secret = None;
    }
    let data = serde_json::to_vec_pretty(&safe)?;
    atomic_write(&paths::preferences_path()?, &data)
}

pub fn save_image_skill(skill: &str) -> Result<()> {
    fs::create_dir_all(paths::support_dir()?)?;
    atomic_write(&paths::image_routing_path()?, &serde_json::to_vec(skill)?)
}

pub fn snapshot_image_skill() -> Result<FileSnapshot> {
    snapshot_file(&paths::image_routing_path()?)
}

pub fn load_image_skill() -> String {
    paths::image_routing_path()
        .ok()
        .and_then(|path| fs::read(path).ok())
        .and_then(|data| serde_json::from_slice::<String>(&data).ok())
        .filter(|value| value == "imagegen" || value == "imagegen2")
        .unwrap_or_else(|| "imagegen".into())
}

pub fn atomic_write(path: &std::path::Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(
        path.parent().unwrap_or_else(|| std::path::Path::new(".")),
    )?;
    use std::io::Write;
    temporary.write_all(data)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn snapshot_file(path: &Path) -> Result<FileSnapshot> {
    Ok(FileSnapshot {
        path: path.to_owned(),
        data: path.exists().then(|| fs::read(path)).transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        fs::write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn file_snapshot_restores_existing_and_missing_files_exactly() {
        let directory = tempfile::tempdir().unwrap();
        let existing = directory.path().join("existing.json");
        fs::write(&existing, b"before\n").unwrap();
        let existing_snapshot = snapshot_file(&existing).unwrap();
        fs::write(&existing, b"after").unwrap();
        existing_snapshot.restore().unwrap();
        assert_eq!(fs::read(&existing).unwrap(), b"before\n");

        let missing = directory.path().join("missing.json");
        let missing_snapshot = snapshot_file(&missing).unwrap();
        fs::write(&missing, b"created").unwrap();
        missing_snapshot.restore().unwrap();
        assert!(!missing.exists());
    }

    #[test]
    fn migrates_current_channel_preferences() {
        let value = serde_json::json!({
            "channels": [{
                "id": "proxy-one", "name": "Proxy", "baseURL": "https://proxy.example",
                "model": "gpt-test", "modelsPath": "/v1/models", "usagePath": "",
                "validatesModelList": false, "isBuiltIn": false, "wireAPI": "responses"
            }],
            "officialModel": "gpt-official", "lastChannelID": "proxy-one"
        });
        let result = parse_legacy_preferences(Some(value), None)
            .unwrap()
            .unwrap();
        assert_eq!(result.channels[0].id, "proxy-one");
        assert_eq!(result.official_model, "gpt-official");
    }

    #[test]
    fn migrates_v2_ailink_preferences_and_settings_file() {
        let old_preferences = serde_json::json!({
            "aiLink": {"baseURL": "https://old.example/v1", "model": "old-model"},
            "officialModel": "official-model"
        });
        let result = parse_legacy_preferences(Some(old_preferences), None)
            .unwrap()
            .unwrap();
        assert_eq!(result.channels[0].base_url, "https://old.example/v1");
        assert_eq!(result.channels[0].model, "old-model");
        assert_eq!(result.official_model, "official-model");

        let settings =
            serde_json::json!({"baseURL": "https://settings.example", "model": "settings-model"});
        let result = parse_legacy_preferences(None, Some(settings))
            .unwrap()
            .unwrap();
        assert_eq!(result.channels[0].base_url, "https://settings.example");
        assert_eq!(result.channels[0].model, "settings-model");
    }

    #[test]
    fn normalization_repairs_untrusted_flags_modes_and_duplicate_channels() {
        let mut preferences = Preferences {
            channels: vec![
                ChannelProfile {
                    is_built_in: false,
                    ..ChannelProfile::ailink()
                },
                ChannelProfile::ailink(),
                ChannelProfile {
                    id: "proxy".into(),
                    is_built_in: true,
                    ..ChannelProfile::ailink()
                },
            ],
            dock_mode: "invalid".into(),
            ..Preferences::default()
        };
        normalize(&mut preferences);
        assert_eq!(preferences.channels.len(), 2);
        assert!(preferences.channels[0].is_built_in);
        assert!(!preferences.channels[1].is_built_in);
        assert_eq!(preferences.dock_mode, "free");
    }
}
