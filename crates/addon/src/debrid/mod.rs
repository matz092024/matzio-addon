//! Debrid services: checking what is cached and turning a torrent into a
//! direct link Stremio can play.

pub mod realdebrid;
pub mod torbox;

use std::fmt;

use crate::torrent::Episode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    RealDebrid,
    TorBox,
}

impl Service {
    pub fn slug(self) -> &'static str {
        match self {
            Service::RealDebrid => "realdebrid",
            Service::TorBox => "torbox",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Service::RealDebrid => "RD",
            Service::TorBox => "TB",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "realdebrid" => Some(Service::RealDebrid),
            "torbox" => Some(Service::TorBox),
            _ => None,
        }
    }
}

/// Result of checking an API key, shown in the settings window.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Account {
    pub username: Option<String>,
    pub premium: bool,
    pub expires: Option<String>,
}

/// Checks that `key` works and returns the account it belongs to.
pub async fn account(
    http: &reqwest::Client,
    endpoints: &crate::Endpoints,
    service: Service,
    key: &str,
) -> Result<Account, DebridError> {
    match service {
        Service::RealDebrid => realdebrid::account(http, &endpoints.real_debrid, key).await,
        Service::TorBox => torbox::account(http, &endpoints.torbox, key).await,
    }
}

/// What the player asked for: a torrent plus a hint about which file in it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlayRequest {
    pub info_hash: String,
    pub filename: Option<String>,
    pub episode: Option<(u32, u32)>,
}

impl PlayRequest {
    pub fn episode(&self) -> Option<Episode> {
        self.episode.map(|(season, episode)| Episode { season, episode })
    }
}

#[derive(Debug)]
pub enum DebridError {
    /// The service doesn't have it ready; it may have started downloading it.
    NotCached,
    /// No playable file in the torrent.
    NoFile,
    /// The service rejected the request, e.g. a wrong API key.
    Api(String),
    Http(reqwest::Error),
}

impl fmt::Display for DebridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DebridError::NotCached => write!(f, "not cached yet; the debrid service is downloading it"),
            DebridError::NoFile => write!(f, "no playable file in this torrent"),
            DebridError::Api(msg) => write!(f, "debrid service error: {msg}"),
            DebridError::Http(e) => write!(f, "could not reach the debrid service: {e}"),
        }
    }
}

impl From<reqwest::Error> for DebridError {
    fn from(e: reqwest::Error) -> Self {
        DebridError::Http(e)
    }
}
