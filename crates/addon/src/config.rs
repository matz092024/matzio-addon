//! Runtime configuration shared between the tray app and the addon server.

use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

/// Debrid credentials and stream preferences. Changes from the settings window apply immediately.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub real_debrid_key: Option<String>,
    pub torbox_key: Option<String>,
    pub preferences: Preferences,
}

impl Config {
    /// Treats empty or whitespace-only keys as not set.
    pub fn new(real_debrid_key: Option<String>, torbox_key: Option<String>) -> Self {
        let clean = |k: Option<String>| k.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty());
        Self {
            real_debrid_key: clean(real_debrid_key),
            torbox_key: clean(torbox_key),
            preferences: Preferences::default(),
        }
    }

    pub fn with_preferences(mut self, preferences: Preferences) -> Self {
        self.preferences = preferences;
        self
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

pub type SharedConfig = Arc<RwLock<Config>>;

/// Base URLs of the services the addon talks to. Tests point these at local mocks.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub torrentio: String,
    pub cinemeta: String,
    pub torbox: String,
    pub real_debrid: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            torrentio: "https://torrentio.strem.fun".into(),
            cinemeta: "https://v3-cinemeta.strem.io".into(),
            torbox: "https://api.torbox.app/v1/api".into(),
            real_debrid: "https://api.real-debrid.com/rest/1.0".into(),
        }
    }
}
