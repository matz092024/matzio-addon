//! Runtime configuration shared between the tray app and the addon server.

use std::sync::{Arc, RwLock};

/// Debrid credentials. Changes from the settings window apply immediately.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub real_debrid_key: Option<String>,
    pub torbox_key: Option<String>,
}

impl Config {
    /// Treats empty or whitespace-only keys as not set.
    pub fn new(real_debrid_key: Option<String>, torbox_key: Option<String>) -> Self {
        let clean = |k: Option<String>| k.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty());
        Self { real_debrid_key: clean(real_debrid_key), torbox_key: clean(torbox_key) }
    }
}

pub type SharedConfig = Arc<RwLock<Config>>;

/// Base URLs of the services the addon talks to. Tests point these at local mocks.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub torrentio: String,
    pub torbox: String,
    pub real_debrid: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            torrentio: "https://torrentio.strem.fun".into(),
            torbox: "https://api.torbox.app/v1/api".into(),
            real_debrid: "https://api.real-debrid.com/rest/1.0".into(),
        }
    }
}
