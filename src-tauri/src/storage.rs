use crate::error::Result;
use crate::models::Preferences;
use crate::paths;
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
    let mut preferences = if preferences_path.exists() {
        serde_json::from_slice::<Preferences>(&fs::read(&preferences_path)?)?
    } else {
        Preferences::default()
    };
    normalize(&mut preferences);
    save_preferences(&preferences)?;
    Ok(preferences)
}

fn normalize(preferences: &mut Preferences) {
    for channel in &mut preferences.channels {
        channel.has_secret = None;
        channel.is_built_in = false;
    }
    let mut seen = std::collections::HashSet::new();
    preferences
        .channels
        .retain(|channel| seen.insert(channel.id.clone()));
    if preferences.official_model.trim().is_empty() {
        preferences.official_model = "gpt-5.6-sol".into();
    }
    if !crate::models::valid_reasoning_effort(&preferences.official_reasoning_effort) {
        preferences.official_reasoning_effort = crate::models::default_reasoning_effort();
    }
    for channel in &mut preferences.channels {
        if !crate::models::valid_reasoning_effort(&channel.reasoning_effort) {
            channel.reasoning_effort = crate::models::default_reasoning_effort();
        }
    }
    if preferences
        .last_channel_id
        .as_ref()
        .is_some_and(|id| !preferences.channels.iter().any(|channel| &channel.id == id))
    {
        preferences.last_channel_id = None;
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
    use crate::models::ChannelProfile;

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
    fn default_preferences_start_with_official_only() {
        let preferences = Preferences::default();
        assert!(preferences.channels.is_empty());
        assert!(preferences.last_channel_id.is_none());
    }

    #[test]
    fn normalization_preserves_existing_ailink_as_a_user_channel() {
        let mut preferences = Preferences {
            channels: vec![ChannelProfile::ailink()],
            last_channel_id: Some("ailink".into()),
            ..Preferences::default()
        };
        normalize(&mut preferences);
        assert_eq!(preferences.channels.len(), 1);
        assert_eq!(preferences.channels[0].id, "ailink");
        assert!(!preferences.channels[0].is_built_in);
        assert_eq!(preferences.last_channel_id.as_deref(), Some("ailink"));
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
        assert!(!preferences.channels[0].is_built_in);
        assert!(!preferences.channels[1].is_built_in);
        assert!(preferences.last_channel_id.is_none());
        assert_eq!(preferences.dock_mode, "free");
    }
}
