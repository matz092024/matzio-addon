//! User settings, stored as JSON in the app's config folder
//! (%APPDATA%\com.matzio.addon\settings.json on Windows).

use std::{fs, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Download and install updates without asking.
    pub auto_update: bool,
    /// Debrid API keys, kept only in this file on the user's PC.
    pub real_debrid_key: Option<String>,
    pub torbox_key: Option<String>,
    /// The first-run setup was finished (or skipped); until then the window opens on it.
    pub setup_complete: bool,
    #[serde(flatten)]
    pub preferences: Preferences,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_update: true,
            real_debrid_key: None,
            torbox_key: None,
            setup_complete: false,
            preferences: Preferences::default(),
        }
    }
}

/// How streams are picked and shown. The settings window saves these as one unit.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    /// Sort keys, most important first: "cached", "resolution", "language", "size", "quality", "seeders".
    pub sort_order: Vec<String>,
    /// Measured or typed download speed in megabits per second.
    pub speed_mbps: Option<f64>,
    /// Hide streams whose bitrate (size / runtime) is more than the connection can play.
    pub filter_by_speed: bool,
    /// Resolutions or sources never to show, such as "480p" or "cam".
    pub exclude_resolutions: Vec<String>,
    /// How a stream is labelled in Stremio's picker: "compact" or "detailed".
    pub display_style: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            sort_order: ["cached", "resolution", "language", "size", "quality", "seeders"]
                .map(String::from)
                .to_vec(),
            speed_mbps: None,
            filter_by_speed: true,
            exclude_resolutions: vec!["cam".into()],
            display_style: "detailed".into(),
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
        Self { path, current: Mutex::new(current) }
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
        assert_eq!(s.preferences.sort_order, ["size"]);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["displayStyle"], "detailed");
        assert!(json.get("preferences").is_none());
    }
}
