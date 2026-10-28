//! Runtime configuration shared between the tray app and the addon server.

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

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
#[serde(default, rename_all = "camelCase", from = "SavedPreferences")]
pub struct Preferences {
    /// Sort keys, most important first: see [`SORT_KEYS`].
    pub sort_order: Vec<String>,
    /// Measured or typed download speed in megabits per second.
    pub speed_mbps: Option<f64>,
    /// How a stream is labelled in Stremio's picker: "compact" or "detailed".
    pub display_style: String,
    pub filters: Filters,
    pub sources: Sources,
}

/// Where torrents are searched. Several can be on; results are merged and duplicates removed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Sources {
    pub torrentio: bool,
    pub comet: bool,
    pub mediafusion: bool,
    /// TorBox's own search; only used with a TorBox key.
    pub torbox: bool,
    pub zilean: bool,
    pub knaben: bool,
    /// Addresses replacing the public instance of "torrentio", "comet", "mediafusion" or
    /// "zilean", such as a self-hosted one. May hold a configured addon link, so it is
    /// never part of an exported settings file.
    pub urls: BTreeMap<String, String>,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            torrentio: true,
            comet: true,
            mediafusion: true,
            torbox: true,
            zilean: false,
            knaben: false,
            urls: BTreeMap::new(),
        }
    }
}

/// Every sort key, in the default order.
pub const SORT_KEYS: [&str; 10] =
    ["cached", "resolution", "language", "keywords", "hdr", "quality", "audio", "codec", "size", "seeders"];

impl Default for Preferences {
    fn default() -> Self {
        Self {
            sort_order: SORT_KEYS.map(String::from).to_vec(),
            speed_mbps: None,
            display_style: "detailed".into(),
            filters: Filters::default(),
            sources: Sources::default(),
        }
    }
}

/// What happens to a stream that fails a check.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    /// Leave it out of the list.
    #[default]
    Hide,
    /// Keep it, below every stream that passed.
    Demote,
}

/// The checks a stream goes through. Each one can be switched off on its own,
/// and a switched-off check has no effect at all, including on sorting.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Filters {
    /// Too big to stream over the user's connection in the title's runtime.
    pub speed: Check,
    pub size: SizeCheck,
    /// Resolutions and sources such as "480p", "cam" or "remux".
    pub quality: TagCheck,
    pub language: LanguageCheck,
    /// "dv", "dv-only", "hdr10plus", "hdr", "sdr".
    pub hdr: TagCheck,
    /// "av1", "hevc", "avc", "xvid".
    pub codec: TagCheck,
    /// "atmos", "truehd", "dts-x", "dts-hd", "dts", "ddp", "dd", "aac", "flac", "7.1", "5.1", "2.0".
    pub audio: TagCheck,
    /// Words or release groups; `/.../` entries are regular expressions.
    pub keywords: TagCheck,
    pub limits: Limits,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            speed: Check::default(),
            size: SizeCheck::default(),
            quality: TagCheck { avoid: vec!["cam".into()], ..TagCheck::default() },
            language: LanguageCheck::default(),
            hdr: TagCheck::default(),
            codec: TagCheck::default(),
            audio: TagCheck::default(),
            keywords: TagCheck::default(),
            limits: Limits::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Check {
    pub enabled: bool,
    pub action: Action,
}

impl Default for Check {
    fn default() -> Self {
        Self { enabled: true, action: Action::Hide }
    }
}

/// Plain size limits in GB, independent of the connection speed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SizeCheck {
    pub enabled: bool,
    pub action: Action,
    pub min_gb: Option<f64>,
    pub max_gb: Option<f64>,
    /// Overrides `max_gb` for "2160p", "1440p", "1080p", "720p" or "sd".
    pub max_gb_by_resolution: BTreeMap<String, f64>,
}

impl Default for SizeCheck {
    fn default() -> Self {
        Self { enabled: true, action: Action::Hide, min_gb: None, max_gb: None, max_gb_by_resolution: BTreeMap::new() }
    }
}

/// Streams with an `avoid` tag fail the check; `prefer` tags rank higher, earliest first.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct TagCheck {
    pub enabled: bool,
    pub action: Action,
    pub avoid: Vec<String>,
    pub prefer: Vec<String>,
}

impl Default for TagCheck {
    fn default() -> Self {
        Self { enabled: true, action: Action::Hide, avoid: Vec::new(), prefer: Vec::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct LanguageCheck {
    pub enabled: bool,
    pub action: Action,
    /// Language codes such as "en" or "de" (plus "multi"), most wanted first.
    pub preferred: Vec<String>,
    /// Fail streams in none of the preferred languages, instead of only ranking them lower.
    pub only_preferred: bool,
}

impl Default for LanguageCheck {
    fn default() -> Self {
        Self { enabled: true, action: Action::Hide, preferred: vec!["en".into()], only_preferred: false }
    }
}

/// Keeps the picker short.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Limits {
    pub enabled: bool,
    pub total: usize,
    pub per_resolution: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { enabled: false, total: 20, per_resolution: 5 }
    }
}

/// Preferences as saved by any version, including the flat filter fields of 0.3 and older.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SavedPreferences {
    sort_order: Option<Vec<String>>,
    speed_mbps: Option<f64>,
    display_style: Option<String>,
    filters: Option<Filters>,
    sources: Sources,
    filter_by_speed: Option<bool>,
    exclude_resolutions: Option<Vec<String>>,
}

impl From<SavedPreferences> for Preferences {
    fn from(saved: SavedPreferences) -> Self {
        let defaults = Preferences::default();
        let filters = saved.filters.unwrap_or_else(|| {
            let mut f = Filters::default();
            if let Some(on) = saved.filter_by_speed {
                f.speed.enabled = on;
            }
            if let Some(excludes) = saved.exclude_resolutions {
                f.quality.avoid = excludes;
            }
            f
        });
        let mut sort_order = saved.sort_order.unwrap_or(defaults.sort_order);
        // Keys added in later versions go in just above size, where they still matter.
        for key in SORT_KEYS {
            if !sort_order.iter().any(|k| k == key) {
                let at = sort_order.iter().position(|k| k == "size").unwrap_or(sort_order.len());
                sort_order.insert(at, key.to_owned());
            }
        }
        Self {
            sort_order,
            speed_mbps: saved.speed_mbps,
            display_style: saved.display_style.unwrap_or(defaults.display_style),
            filters,
            sources: saved.sources,
        }
    }
}

pub type SharedConfig = Arc<RwLock<Config>>;

/// Base URLs of the services the addon talks to. Tests point these at local mocks.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub torrentio: String,
    pub comet: String,
    pub mediafusion: String,
    pub torbox_search: String,
    pub zilean: String,
    pub knaben: String,
    pub cinemeta: String,
    pub torbox: String,
    pub real_debrid: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            torrentio: "https://torrentio.strem.fun".into(),
            comet: "https://comet.elfhosted.com".into(),
            mediafusion: "https://mediafusion.elfhosted.com".into(),
            torbox_search: "https://search-api.torbox.app".into(),
            zilean: "https://zilean.elfhosted.com".into(),
            knaben: "https://api.knaben.org/v1".into(),
            cinemeta: "https://v3-cinemeta.strem.io".into(),
            torbox: "https://api.torbox.app/v1/api".into(),
            real_debrid: "https://api.real-debrid.com/rest/1.0".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_settings_from_older_versions() {
        let old: Preferences = serde_json::from_str(
            r#"{"sortOrder":["size","cached","resolution","language","quality","seeders"],
                "filterBySpeed":false,"excludeResolutions":["480p","scr"],"displayStyle":"compact"}"#,
        )
        .unwrap();
        assert!(!old.filters.speed.enabled);
        assert_eq!(old.filters.quality.avoid, ["480p", "scr"]);
        assert_eq!(old.display_style, "compact");
        // New sort keys slot in just above size, the user's own order is kept.
        assert_eq!(
            old.sort_order,
            ["keywords", "hdr", "audio", "codec", "size", "cached", "resolution", "language", "quality", "seeders"]
        );

        let fresh: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(fresh, Preferences::default());
        assert_eq!(fresh.filters.quality.avoid, ["cam"]);
    }

    #[test]
    fn works_flattened_into_the_settings_file() {
        #[derive(Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        struct File {
            torbox_key: Option<String>,
            #[serde(flatten)]
            preferences: Preferences,
        }
        let f: File = serde_json::from_str(r#"{"torboxKey":"tb","filterBySpeed":false}"#).unwrap();
        assert_eq!(f.torbox_key.as_deref(), Some("tb"));
        assert!(!f.preferences.filters.speed.enabled);
        let json = serde_json::to_value(&f).unwrap();
        assert_eq!(json["torboxKey"], "tb");
        assert_eq!(json["filters"]["speed"]["enabled"], false);
    }

    #[test]
    fn round_trips() {
        let mut p = Preferences::default();
        p.filters.size.max_gb_by_resolution.insert("2160p".into(), 40.0);
        p.filters.hdr.action = Action::Demote;
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["filters"]["hdr"]["action"], "demote");
        assert!(json.get("filterBySpeed").is_none());
        assert_eq!(serde_json::from_value::<Preferences>(json).unwrap(), p);
    }
}
