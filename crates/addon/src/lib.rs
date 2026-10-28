//! The Stremio addon protocol server.
//!
//! Stremio talks to an addon over plain HTTP: it fetches `/manifest.json` once
//! when the addon is installed, then `/stream/{type}/{id}.json` every time the
//! user opens a movie or episode. Picking a stream opens `/play/...`, which
//! resolves the torrent through the user's debrid service and redirects to the
//! direct link. Debrid keys never leave this PC except to the debrid service.
//! Everything here binds to localhost only.

pub mod config;
pub mod debrid;
pub mod sources;
pub mod streams;
pub mod torrent;

use std::{
    collections::{HashMap, HashSet},
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

pub use config::{Config, Endpoints, Preferences, SharedConfig};
use debrid::{DebridError, PlayRequest, Service};
use torrent::parse_id;

/// Default port the addon listens on.
pub const DEFAULT_PORT: u16 = 7777;

const ADDON_ID: &str = "community.matzio.addon";

/// How long a resolved link is reused, so retries and seeks don't re-add the torrent.
const LINK_TTL: Duration = Duration::from_secs(30 * 60);

/// The addon manifest Stremio reads when installing.
pub fn manifest() -> Value {
    json!({
        "id": ADDON_ID,
        "version": env!("CARGO_PKG_VERSION"),
        "name": "Matzio",
        "description": "Instant debrid streams from your own PC (Real-Debrid, TorBox).",
        "resources": ["stream"],
        "types": ["movie", "series"],
        "idPrefixes": ["tt"],
        "catalogs": [],
        "behaviorHints": { "configurable": false, "configurationRequired": false }
    })
}

#[derive(Clone)]
pub struct AppState {
    config: SharedConfig,
    endpoints: Arc<Endpoints>,
    http: reqwest::Client,
    base_url: Arc<str>,
    links: Arc<Mutex<HashMap<(Service, PlayRequest), (Instant, String)>>>,
    runtimes: Arc<Mutex<HashMap<String, Duration>>>,
}

impl AppState {
    /// `base_url` is how Stremio reaches this server, e.g. `http://127.0.0.1:7777`.
    pub fn new(config: SharedConfig, endpoints: Endpoints, base_url: &str) -> Self {
        // reqwest is built without a bundled TLS provider; use ring, which the app already links.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .user_agent(concat!("Matzio/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("HTTP client builds with static settings");
        Self {
            config,
            endpoints: Arc::new(endpoints),
            http,
            base_url: base_url.trim_end_matches('/').into(),
            links: Default::default(),
            runtimes: Default::default(),
        }
    }

    async fn runtime(&self, kind: &str, id: &str, imdb_id: &str, episode: Option<torrent::Episode>) -> Option<Duration> {
        if let Some(d) = self.runtimes.lock().unwrap().get(id).copied() {
            return Some(d);
        }
        let d = sources::cinemeta::runtime(&self.http, &self.endpoints.cinemeta, kind, imdb_id, episode).await?;
        self.runtimes.lock().unwrap().insert(id.to_owned(), d);
        Some(d)
    }
}

impl AppState {
    /// Checks a debrid API key (not necessarily the saved one).
    pub async fn check_key(&self, service: Service, key: &str) -> Result<debrid::Account, DebridError> {
        debrid::account(&self.http, &self.endpoints, service, key.trim()).await
    }
}

/// Builds the HTTP router. Stremio's web and desktop clients need CORS.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/manifest.json", get(|| async { Json(manifest()) }))
        .route("/stream/{kind}/{file}", get(stream))
        .route("/play/{service}/{hash}", get(play))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn home() -> Html<&'static str> {
    Html(
        "<!doctype html><meta charset=utf-8><title>Matzio</title>\
         <body style=\"font-family:system-ui;background:#0d0d10;color:#eee;padding:40px\">\
         <h1>Matzio is running</h1><p>Open Matzio from the system tray to add your debrid keys and settings.</p>",
    )
}

async fn stream(State(state): State<AppState>, Path((kind, file)): Path<(String, String)>) -> Response {
    // The id is "tt1234567" for movies or "tt1234567:season:episode" for series.
    let Some(id) = file.strip_suffix(".json") else { return StatusCode::NOT_FOUND.into_response() };
    let Some((imdb_id, episode)) = parse_id(id) else { return StatusCode::NOT_FOUND.into_response() };
    if !matches!((kind.as_str(), episode.is_some()), ("movie", false) | ("series", true)) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let config = state.config.read().unwrap().clone();
    if config.real_debrid_key.is_none() && config.torbox_key.is_none() {
        return Json(json!({ "streams": [streams::setup_hint(&state.base_url)] })).into_response();
    }

    let prefs = &config.preferences;
    let speed_limit = prefs.speed_mbps.filter(|&mbps| prefs.filter_by_speed && mbps > 0.0);

    let (search_res, runtime) = tokio::join!(
        sources::torrentio::search(&state.http, &state.endpoints.torrentio, &kind, id),
        async {
            let _ = speed_limit?;
            state.runtime(&kind, id, imdb_id, episode).await
        }
    );

    let mut torrents = match search_res {
        Ok(t) => t,
        Err(e) => {
            eprintln!("torrent search failed for {id}: {e}");
            Vec::new()
        }
    };
    let mut seen = HashSet::new();
    torrents.retain(|t| {
        seen.insert(t.info_hash.clone())
            && !t.is_excluded(&prefs.exclude_resolutions)
            && match (speed_limit, runtime) {
                (Some(mbps), Some(rt)) => torrent::fits_speed(t.size_bytes, rt, mbps),
                _ => true,
            }
    });
    streams::rank_by(&mut torrents, &prefs.sort_order);

    let mut candidates: Vec<(Service, bool, &torrent::Torrent)> = Vec::new();
    if let Some(key) = &config.torbox_key {
        let hashes: Vec<_> = torrents.iter().map(|t| t.info_hash.clone()).collect();
        match debrid::torbox::cached(&state.http, &state.endpoints.torbox, key, &hashes).await {
            Ok(cached) => candidates.extend(
                torrents
                    .iter()
                    .filter(|t| cached.contains(&t.info_hash))
                    .take(streams::MAX_PER_SERVICE)
                    .map(|t| (Service::TorBox, true, t)),
            ),
            Err(e) => eprintln!("TorBox cache check failed: {e}"),
        }
    }
    if config.real_debrid_key.is_some() {
        candidates.extend(
            torrents
                .iter()
                .take(streams::MAX_PER_SERVICE)
                .map(|t| (Service::RealDebrid, false, t)),
        );
    }
    candidates.sort_by(|a, b| streams::compare(a.1, a.2, b.1, b.2, &prefs.sort_order));

    let list: Vec<_> = candidates
        .into_iter()
        .map(|(service, cached, t)| {
            streams::stream_with_style(&state.base_url, service, cached, t, episode, &prefs.display_style)
        })
        .collect();
    Json(json!({ "streams": list })).into_response()
}

#[derive(Deserialize)]
struct PlayQuery {
    file: Option<String>,
    s: Option<u32>,
    e: Option<u32>,
}

async fn play(
    State(state): State<AppState>,
    Path((service, hash)): Path<(String, String)>,
    Query(query): Query<PlayQuery>,
) -> Response {
    let Some(service) = Service::from_slug(&service) else { return StatusCode::NOT_FOUND.into_response() };
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (StatusCode::BAD_REQUEST, "invalid info hash").into_response();
    }
    let request = PlayRequest {
        info_hash: hash.to_ascii_lowercase(),
        filename: query.file,
        episode: query.s.zip(query.e),
    };
    let cache_key = (service, request.clone());
    if let Some((at, link)) = state.links.lock().unwrap().get(&cache_key) {
        if at.elapsed() < LINK_TTL {
            return Redirect::temporary(link).into_response();
        }
    }

    let config = state.config.read().unwrap().clone();
    let result = match service {
        Service::TorBox => match &config.torbox_key {
            Some(key) => debrid::torbox::resolve(&state.http, &state.endpoints.torbox, key, &request).await,
            None => Err(DebridError::Api("no TorBox key set".into())),
        },
        Service::RealDebrid => match &config.real_debrid_key {
            Some(key) => debrid::realdebrid::resolve(&state.http, &state.endpoints.real_debrid, key, &request).await,
            None => Err(DebridError::Api("no Real-Debrid key set".into())),
        },
    };
    match result {
        Ok(link) => {
            let mut links = state.links.lock().unwrap();
            links.retain(|_, (at, _)| at.elapsed() < LINK_TTL);
            links.insert(cache_key, (Instant::now(), link.clone()));
            Redirect::temporary(&link).into_response()
        }
        Err(e) => {
            eprintln!("{} could not play {}: {e}", service.short_name(), request.info_hash);
            let status = match e {
                DebridError::NotCached | DebridError::NoFile => StatusCode::NOT_FOUND,
                _ => StatusCode::BAD_GATEWAY,
            };
            (status, e.to_string()).into_response()
        }
    }
}

/// Binds the addon to `127.0.0.1:port`. Serve it with [`serve`].
pub async fn bind(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await
}

/// Serves the addon on an already bound listener until the process exits.
pub async fn serve(listener: TcpListener, state: AppState) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}

/// The base URL Stremio uses to reach the addon.
pub fn base_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

/// The URL users paste into Stremio, or open as a `stremio://` link.
pub fn manifest_url(addr: SocketAddr) -> String {
    format!("{}/manifest.json", base_url(addr))
}

#[cfg(test)]
mod tests;
