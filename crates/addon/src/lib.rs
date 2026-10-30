//! The Stremio addon protocol server.
//!
//! Stremio talks to an addon over plain HTTP: it fetches `/manifest.json` once
//! when the addon is installed, then `/stream/{type}/{id}.json` every time the
//! user opens a movie or episode. Picking a stream opens `/play/...`, which
//! resolves the torrent through the user's debrid service and redirects to the
//! direct link. Subtitles come from `/subtitles/...`, and each file from
//! `/subtitle/...` when the player opens it. Debrid and subtitle keys never leave
//! this PC except to their own service.
//! Everything here binds to localhost only, and every route sits behind a random
//! per-install token (`/<token>/manifest.json`), so a web page open in the user's
//! browser can't reach it: localhost is reachable from any site, the token isn't.

pub mod config;
pub mod debrid;
pub mod filters;
pub mod labels;
pub mod sources;
pub mod streams;
pub mod subtitles;
pub mod torrent;

use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, Query, Request, State},
    http::{header, Method, StatusCode, Uri},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tower::ServiceExt;
use tower_http::cors::{Any, CorsLayer};

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
        "description": "Instant debrid streams and subtitles from your own PC (Real-Debrid, TorBox, OpenSubtitles, SubDL).",
        "resources": ["stream", "subtitles"],
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
    origin: Arc<str>,
    token: Arc<RwLock<Arc<str>>>,
    links: Arc<Mutex<HashMap<(Service, PlayRequest), (Instant, String)>>>,
    runtimes: Arc<Mutex<HashMap<String, Duration>>>,
    subtitles: Arc<subtitles::Cache>,
}

impl AppState {
    /// `origin` is how Stremio reaches this server, e.g. `http://127.0.0.1:7777`, and `token`
    /// the secret path segment every request must start with: see [`new_token`].
    pub fn new(config: SharedConfig, endpoints: Endpoints, origin: &str, token: &str) -> Self {
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
            origin: origin.trim_end_matches('/').into(),
            token: Arc::new(RwLock::new(token.into())),
            links: Default::default(),
            runtimes: Default::default(),
            subtitles: Default::default(),
        }
    }

    /// Where every addon URL starts, token included, e.g. `http://127.0.0.1:7777/<token>`.
    pub fn base_url(&self) -> String {
        format!("{}/{}", self.origin, self.token.read().unwrap())
    }

    /// The URL users paste into Stremio, or open as a `stremio://` link.
    pub fn manifest_url(&self) -> String {
        format!("{}/manifest.json", self.base_url())
    }

    /// Replaces the token. Links handed out with the old one stop working at once, so the
    /// addon has to be installed in Stremio again.
    pub fn set_token(&self, token: &str) {
        *self.token.write().unwrap() = token.into();
    }

    fn token_matches(&self, given: &str) -> bool {
        let token = self.token.read().unwrap();
        // Compare every byte, so the time taken doesn't hint at how much of a guess was right.
        given.len() == token.len()
            && given.bytes().zip(token.bytes()).fold(0, |diff, (a, b)| diff | (a ^ b)) == 0
    }

    /// The debrid link most recently opened for this file, if it is still fresh.
    fn playing_link(&self, filename: &str, episode: Option<torrent::Episode>) -> Option<String> {
        let base = |name: &str| name.rsplit(['/', '\\']).next().unwrap_or(name).to_owned();
        let wanted = base(filename);
        self.links
            .lock()
            .unwrap()
            .iter()
            .filter(|((_, request), (at, _))| {
                at.elapsed() < LINK_TTL
                    && request.filename.as_deref().is_some_and(|f| base(f).eq_ignore_ascii_case(&wanted))
                    && (episode.is_none() || request.episode() == episode)
            })
            .max_by_key(|(_, (at, _))| *at)
            .map(|(_, (_, link))| link.clone())
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

/// How long one source may take before its results are left out.
const SOURCE_TIMEOUT: Duration = Duration::from_secs(10);

/// Runs one source's search if it is switched on; a failing or slow source just adds nothing.
async fn gather<E: std::fmt::Display>(
    name: &str,
    on: bool,
    search: impl std::future::Future<Output = Result<Vec<torrent::Torrent>, E>>,
) -> Vec<torrent::Torrent> {
    if !on {
        return Vec::new();
    }
    match tokio::time::timeout(SOURCE_TIMEOUT, search).await {
        Ok(Ok(found)) => found,
        Ok(Err(e)) => {
            eprintln!("{name} search failed: {e}");
            Vec::new()
        }
        Err(_) => {
            eprintln!("{name} took longer than {}s", SOURCE_TIMEOUT.as_secs());
            Vec::new()
        }
    }
}

impl AppState {
    /// Searches every switched-on source at once and merges the results. A torrent found by
    /// several sources is listed once, with whatever details any of them knew.
    async fn search(
        &self,
        config: &Config,
        kind: &str,
        id: &str,
        imdb_id: &str,
        episode: Option<torrent::Episode>,
    ) -> Vec<torrent::Torrent> {
        let on = &config.preferences.sources;
        let (http, e) = (&self.http, &*self.endpoints);
        let url = |name: &str, default: &str| {
            on.urls.get(name).map(|u| u.trim()).filter(|u| !u.is_empty()).unwrap_or(default).to_owned()
        };
        let (torrentio, comet, mediafusion, zilean) = (
            url("torrentio", &e.torrentio),
            url("comet", &e.comet),
            url("mediafusion", &e.mediafusion),
            url("zilean", &e.zilean),
        );
        let torbox_key = config.torbox_key.as_deref().unwrap_or_default();

        let (from_addons, from_apis) = tokio::join!(
            async {
                tokio::join!(
                    gather("Torrentio", on.torrentio, sources::addon::search(http, &torrentio, kind, id)),
                    gather("Comet", on.comet, sources::addon::search(http, &comet, kind, id)),
                    gather("MediaFusion", on.mediafusion, sources::addon::search(http, &mediafusion, kind, id)),
                )
            },
            async {
                tokio::join!(
                    gather(
                        "TorBox search",
                        on.torbox && !torbox_key.is_empty(),
                        sources::torbox::search(http, &e.torbox_search, torbox_key, imdb_id, episode),
                    ),
                    gather("Zilean", on.zilean, sources::zilean::search(http, &zilean, imdb_id, episode)),
                    gather("Knaben", on.knaben, async {
                        match sources::cinemeta::title(http, &e.cinemeta, kind, imdb_id).await {
                            Some(title) => sources::knaben::search(http, &e.knaben, &sources::knaben::query(&title, episode)).await,
                            None => Ok(Vec::new()),
                        }
                    }),
                )
            }
        );

        // Addons list the episode's own file; the others list whole torrents, which for a
        // series can be season packs (whose size says nothing about the episode) or other episodes.
        let (a, b, c) = from_addons;
        let (d, f, g) = from_apis;
        let whole_torrents = [d, f, g].into_iter().flatten().filter_map(|mut t| {
            if let Some(ep) = episode {
                if t.is_other_episode(ep) {
                    return None;
                }
                if !torrent::matches_episode(&t.name, ep) {
                    t.size_bytes = None;
                }
            }
            Some(t)
        });

        let mut merged: Vec<torrent::Torrent> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        for t in [a, b, c].into_iter().flatten().chain(whole_torrents) {
            match index.get(&t.info_hash) {
                Some(&i) => {
                    let known = &mut merged[i];
                    known.filename = known.filename.take().or(t.filename);
                    known.size_bytes = known.size_bytes.or(t.size_bytes);
                    known.seeders = known.seeders.or(t.seeders);
                    known.notes = known.notes.take().or(t.notes);
                }
                None => {
                    index.insert(t.info_hash.clone(), merged.len());
                    merged.push(t);
                }
            }
        }
        merged
    }

    /// Checks a debrid API key (not necessarily the saved one).
    pub async fn check_key(&self, service: Service, key: &str) -> Result<debrid::Account, DebridError> {
        debrid::account(&self.http, &self.endpoints, service, key.trim()).await
    }

    /// Checks a subtitle provider's key (not necessarily the saved one).
    pub async fn check_subtitle_key(
        &self,
        provider: subtitles::Provider,
        keys: config::SubtitleKeys,
    ) -> Result<String, subtitles::SubtitleError> {
        subtitles::check(&self.http, &self.endpoints, provider, &keys.cleaned()).await
    }
}

/// A new random token for the addon's URLs: 128 bits, as 32 hex characters.
pub fn new_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether a saved token looks like one [`new_token`] made (a hand-edited or damaged one
/// is replaced rather than trusted).
pub fn valid_token(token: &str) -> bool {
    token.len() >= 32 && token.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Builds the HTTP router. The addon itself only answers under `/<token>/`; outside it there
/// is just the "Matzio is running" page and, for an install from before tokens, a hint to
/// install again.
///
/// Stremio's web client fetches the manifest, streams and subtitles from another origin, so
/// CORS stays open to any origin for reads. That is safe because a page can only call a URL
/// it knows, and it can't learn the token: cross-origin reads of anything else get nothing.
pub fn router(state: AppState) -> Router {
    let unlocked = addon_routes().with_state(state.clone());
    Router::new()
        .route("/", get(home))
        .route("/stream/{kind}/{file}", get(reinstall_hint))
        .fallback(move |State(state): State<AppState>, request: Request| unlock(state, unlocked.clone(), request))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::HEAD])
                .allow_headers(Any),
        )
        .with_state(state)
}

/// The routes behind the token, as Stremio sees them once the token is taken off the path.
fn addon_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(home))
        .route("/manifest.json", get(|| async { Json(manifest()) }))
        .route("/stream/{kind}/{file}", get(stream))
        .route("/play/{service}/{hash}", get(play))
        .route("/subtitles/{kind}/{file}", get(subtitles_plain))
        .route("/subtitles/{kind}/{id}/{extra}", get(subtitles_with_extra))
        .route("/subtitle/{provider}/{file}", get(subtitle_file))
}

/// Checks the token at the start of the path and passes the rest on to the addon's routes.
/// A missing or wrong token gets the same plain 404 as a page that doesn't exist.
async fn unlock(state: AppState, unlocked: Router, mut request: Request) -> Response {
    let path = request.uri().path();
    let Some((token, rest)) = path.strip_prefix('/').and_then(|p| p.split_once('/')) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !state.token_matches(token) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let rest = match request.uri().query() {
        Some(query) => format!("/{rest}?{query}"),
        None => format!("/{rest}"),
    };
    let Ok(uri) = rest.parse::<Uri>() else { return StatusCode::BAD_REQUEST.into_response() };
    *request.uri_mut() = uri;
    match unlocked.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

/// Stremio still asks an addon installed from the old, token-less link for streams; answer
/// with one entry saying how to fix it, and nothing else.
async fn reinstall_hint() -> Json<Value> {
    Json(json!({ "streams": [{
        "name": "Matzio",
        "title": "🔒 Matzio's link has changed\nOpen Matzio from the tray and click Install in Stremio",
    }] }))
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
        return Json(json!({ "streams": [streams::setup_hint(&state.base_url())] })).into_response();
    }

    let prefs = &config.preferences;
    let speed_limit = prefs.speed_mbps.filter(|&mbps| prefs.filters.speed.enabled && mbps > 0.0);

    let (torrents, runtime) = tokio::join!(state.search(&config, &kind, id, imdb_id, episode), async {
        let _ = speed_limit?;
        state.runtime(&kind, id, imdb_id, episode).await
    });

    let cx = filters::Context { speed_mbps: speed_limit, runtime };
    // Each kept torrent with whether it failed a "show at the bottom" check.
    let mut kept: Vec<(torrent::Torrent, bool)> = torrents
        .into_iter()
        .filter_map(|t| {
            let verdict = filters::judge(&t, &prefs.filters, cx);
            (!verdict.hidden).then_some((t, verdict.demoted))
        })
        .collect();
    kept.sort_by(|(a, a_down), (b, b_down)| a_down.cmp(b_down).then_with(|| streams::compare(false, a, false, b, prefs)));

    let mut candidates: Vec<(Service, bool, bool, &torrent::Torrent)> = Vec::new();
    if let Some(key) = &config.torbox_key {
        let hashes: Vec<_> = kept.iter().map(|(t, _)| t.info_hash.clone()).collect();
        match debrid::torbox::cached(&state.http, &state.endpoints.torbox, key, &hashes).await {
            Ok(cached) => candidates.extend(
                kept.iter()
                    .filter(|(t, _)| cached.contains(&t.info_hash))
                    .take(streams::MAX_PER_SERVICE)
                    .map(|(t, down)| (Service::TorBox, true, *down, t)),
            ),
            Err(e) => eprintln!("TorBox cache check failed: {e}"),
        }
    }
    if config.real_debrid_key.is_some() {
        candidates.extend(
            kept.iter()
                .take(streams::MAX_PER_SERVICE)
                .map(|(t, down)| (Service::RealDebrid, false, *down, t)),
        );
    }
    candidates.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| streams::compare(a.1, a.3, b.1, b.3, prefs)));
    let candidates = filters::limit(candidates, &prefs.filters, |c| c.3.resolution());

    let base_url = state.base_url();
    let list: Vec<_> = candidates
        .into_iter()
        .map(|(service, cached, _, t)| {
            streams::stream_for(&base_url, service, cached, t, episode, prefs)
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

async fn subtitles_plain(State(state): State<AppState>, Path((kind, file)): Path<(String, String)>) -> Response {
    let Some(id) = file.strip_suffix(".json") else { return StatusCode::NOT_FOUND.into_response() };
    list_subtitles(state, &kind, id, Default::default()).await
}

async fn subtitles_with_extra(
    State(state): State<AppState>,
    Path((kind, id, _)): Path<(String, String, String)>,
    uri: Uri,
) -> Response {
    // Read the extras from the raw path, so encoded "&" and "+" in file names survive.
    let raw = uri.path().rsplit('/').next().unwrap_or_default();
    list_subtitles(state, &kind, &id, subtitles::parse_extra(raw)).await
}

async fn list_subtitles(state: AppState, kind: &str, id: &str, extra: HashMap<String, String>) -> Response {
    let Some((imdb_id, episode)) = parse_id(id) else { return StatusCode::NOT_FOUND.into_response() };
    if !matches!((kind, episode.is_some()), ("movie", false) | ("series", true)) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let config = state.config.read().unwrap().clone();
    let prefs = &config.preferences.subtitles;
    if !prefs.enabled {
        return Json(json!({ "subtitles": [] })).into_response();
    }
    let languages = subtitles::languages(&prefs.languages);
    let filename = extra.get("filename").map(String::as_str);
    // Most players don't send the video's hash; work it out from the link being played,
    // so OpenSubtitles can point at the subtitle made for this exact file.
    let video_hash = match extra.get("videoHash") {
        Some(hash) => Some(hash.clone()),
        None => match filename.and_then(|f| state.playing_link(f, episode)) {
            Some(link) => subtitles::video_hash(&state.http, &state.subtitles, &link).await,
            None => None,
        },
    };
    let request = subtitles::Request {
        imdb_id,
        episode,
        languages: &languages,
        video_hash: video_hash.as_deref(),
    };
    let found = subtitles::search(&state.http, &state.endpoints, &state.subtitles, &config.subtitle_keys, prefs, &request).await;
    let ranked = subtitles::rank(found, prefs, &languages, filename);
    let base_url = state.base_url();
    let list: Vec<_> = ranked.iter().map(|s| subtitles::to_stremio(&base_url, s, episode)).collect();
    Json(json!({ "subtitles": list })).into_response()
}

async fn subtitle_file(
    State(state): State<AppState>,
    Path((provider, file)): Path<(String, String)>,
    Query(query): Query<PlayQuery>,
) -> Response {
    let Some(provider) = subtitles::Provider::from_slug(&provider) else { return StatusCode::NOT_FOUND.into_response() };
    let file = file.strip_suffix(".srt").unwrap_or(&file);
    if !subtitles::valid_file(file) {
        return (StatusCode::BAD_REQUEST, "invalid subtitle id").into_response();
    }
    let episode = query.s.zip(query.e).map(|(season, episode)| torrent::Episode { season, episode });
    let keys = state.config.read().unwrap().subtitle_keys.clone();
    match subtitles::download(&state.http, &state.endpoints, &state.subtitles, &keys, provider, file, episode).await {
        Ok(found) => ([(header::CONTENT_TYPE, found.content_type)], found.bytes).into_response(),
        Err(e) => {
            eprintln!("could not get {} subtitle {file}: {e}", provider.slug());
            let status = match e {
                subtitles::SubtitleError::NotFound => StatusCode::NOT_FOUND,
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

/// Where the server is reached, without the token: pass it to [`AppState::new`].
pub fn origin(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

#[cfg(test)]
mod tests;
