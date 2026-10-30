//! User settings, stored as JSON in the app's config folder
//! (%APPDATA%\com.matzio.addon\settings.json on Windows).

use std::{fs, path::PathBuf, sync::Mutex};

pub use matzio_addon::{config::SubtitleKeys, Preferences};
use matzio_addon::{new_token, valid_token};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Download and install updates without asking.
    pub auto_update: bool,
    /// Launch Matzio (to the tray) when the user signs in to Windows.
    pub start_with_windows: bool,
    /// Debrid API keys, kept only in this file on the user's PC.
    pub real_debrid_key: Option<String>,
    pub torbox_key: Option<String>,
    /// OpenSubtitles and SubDL keys, also only in this file.
    #[serde(flatten)]
    pub subtitle_keys: SubtitleKeys,
    /// The secret first part of the addon's URLs, so only Stremio (which was given the
    /// link) can use the addon, not any web page that tries localhost. Made on first run.
    pub addon_token: Option<String>,
    /// The first-run setup was finished (or skipped); until then the window opens on it.
    pub setup_complete: bool,
    #[serde(flatten)]
    pub preferences: Preferences,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_update: true,
            start_with_windows: true,
            real_debrid_key: None,
            torbox_key: None,
            subtitle_keys: SubtitleKeys::default(),
            addon_token: None,
            setup_complete: false,
            preferences: Preferences::default(),
        }
    }
}

/// Loaded settings plus where to save them.
pub struct SettingsStore {
    path: Option<PathBuf>,
    current: Mutex<Settings>,
}

impl SettingsStore {
    pub fn load(app: &AppHandle) -> Self {
        let path = app.path().app_config_dir().ok().map(|dir| dir.join("settings.json"));
        // A missing or unreadable file falls back to defaults rather than blocking startup.
        let current = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let store = Self { path, current: Mutex::new(current) };
        if !store.get().addon_token.as_deref().is_some_and(valid_token) {
            if let Err(e) = store.update(|s| s.addon_token = Some(new_token())) {
                // Still runs with the new token; it just changes again next start.
                eprintln!("could not save the addon token: {e}");
            }
        }
        store
    }

    /// The token the addon's URLs start with; see [`Settings::addon_token`].
    pub fn addon_token(&self) -> String {
        let mut current = self.current.lock().unwrap();
        current.addon_token.get_or_insert_with(new_token).clone()
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap().clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let mut current = self.current.lock().unwrap();
        change(&mut current);
        if let Some(path) = &self.path {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let json = serde_json::to_string_pretty(&*current).map_err(|e| e.to_string())?;
            fs::write(path, json).map_err(|e| e.to_string())?;
        }
        Ok(current.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_use_defaults() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, Settings::default());
        assert!(s.auto_update);
        let s: Settings = serde_json::from_str(r#"{"autoUpdate":false,"torboxKey":"tb"}"#).unwrap();
        assert!(!s.auto_update);
        assert_eq!(s.torbox_key.as_deref(), Some("tb"));
        assert_eq!(s.real_debrid_key, None);
    }

    #[test]
    fn preferences_sit_beside_other_settings() {
        let s: Settings = serde_json::from_str(r#"{"setupComplete":true,"speedMbps":120.5,"sortOrder":["size"]}"#).unwrap();
        assert!(s.setup_complete);
        assert_eq!(s.preferences.speed_mbps, Some(120.5));
        assert_eq!(s.preferences.sort_order.last().unwrap(), "size");
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["displayStyle"], "detailed");
        assert!(json.get("preferences").is_none());
    }

    #[test]
    fn subtitle_keys_sit_beside_the_debrid_keys() {
        let s: Settings = serde_json::from_str(r#"{"subdlKey":"sd","subtitles":{"languages":["de"]}}"#).unwrap();
        assert_eq!(s.subtitle_keys.subdl_key.as_deref(), Some("sd"));
        assert_eq!(s.preferences.subtitles.languages, ["de"]);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["subdlKey"], "sd");
        assert!(json["subtitles"].get("subdlKey").is_none());
    }

    #[test]
    fn keeps_filters_from_older_versions() {
        let s: Settings = serde_json::from_str(r#"{"torboxKey":"tb","filterBySpeed":false,"excludeResolutions":["480p"]}"#).unwrap();
        assert_eq!(s.torbox_key.as_deref(), Some("tb"));
        assert!(!s.preferences.filters.speed.enabled);
        assert_eq!(s.preferences.filters.quality.avoid, ["480p"]);
    }
}
