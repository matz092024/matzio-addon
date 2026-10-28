//! End-to-end tests of the addon routes against local mocks of Torrentio,
//! Cinemeta, TorBox and Real-Debrid.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex, RwLock},
};

use axum::{
    body::Body,
    extract::{Path, Query},
    http::{header, Request, StatusCode},
    routing::{get, post},
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use super::*;

const CACHED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const UNCACHED: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
/// Only found by the optional sources.
const EXTRA: &str = "cccccccccccccccccccccccccccccccccccccccc";
const BASE: &str = "http://127.0.0.1:7777";

/// Starts a mock of the upstream services and returns its base URL and
/// a log of the calls that change state on a debrid account.
async fn mock_upstreams() -> (String, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let log = |calls: &Arc<Mutex<Vec<String>>>, what: &str| calls.lock().unwrap().push(what.to_owned());

    let torrentio = Router::new().route(
        "/torrentio/stream/{kind}/{id}",
        get(|| async {
            Json(json!({ "streams": [
                { "name": "Torrentio\n720p", "title": "Movie.2020.720p.WEB\n👤 900 💾 1.1 GB ⚙️ YTS", "infoHash": UNCACHED.to_uppercase() },
                { "name": "Torrentio\n2160p", "title": "Movie.2020.2160p.WEB\n👤 12 💾 15.2 GB ⚙️ RARBG", "infoHash": CACHED,
                  "behaviorHints": { "filename": "Movie.2020.2160p.WEB.mkv" } },
                { "name": "Torrentio\n2160p", "title": "dupe\n👤 1", "infoHash": CACHED }
            ]}))
        }),
    );

    let cinemeta = Router::new().route(
        "/cinemeta/meta/{kind}/{id}",
        get(|Path((kind, _id)): Path<(String, String)>| async move {
            let runtime = if kind == "series" { "45 min" } else { "120 min" };
            Json(json!({ "meta": { "runtime": runtime, "name": "Movie", "year": "2020" } }))
        }),
    );

    let c3 = calls.clone();
    // Comet repeats a Torrentio result (with its file size in behaviorHints), MediaFusion is down.
    let others = Router::new()
        .route(
            "/comet/stream/{kind}/{id}",
            get(|| async {
                Json(json!({ "streams": [
                    { "name": "[TORRENT] Comet 2160p", "description": "📄 Movie.2020.2160p.WEB\n💾 15.2 GB 👤 12 🔎 RARBG",
                      "infoHash": CACHED, "behaviorHints": { "videoSize": 16_320_875_724u64 } }
                ]}))
            }),
        )
        .route("/mediafusion/stream/{kind}/{id}", get(|| async { StatusCode::INTERNAL_SERVER_ERROR }))
        .route(
            "/tbsearch/torrents/{id}",
            get(move |Path(id): Path<String>, headers: axum::http::HeaderMap| async move {
                log(&c3, &format!("tbsearch:{id}:{}", headers[header::AUTHORIZATION].to_str().unwrap()));
                Json(json!({ "success": true, "data": { "torrents": [
                    { "hash": EXTRA, "raw_title": "Movie.2020.1080p.BluRay", "size": 4_000_000_000u64, "seeders": 5 }
                ]}}))
            }),
        )
        .route(
            "/zilean/dmm/filtered",
            get(|| async { Json(json!([{ "info_hash": EXTRA, "raw_title": "Movie.2020.1080p.BluRay", "size": "4000000000" }])) }),
        )
        .route(
            "/knaben",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["query"], "Movie 2020");
                Json(json!({ "hits": [{ "hash": UNCACHED, "title": "Movie.2020.720p.WEB", "bytes": 1, "seeders": 1 }] }))
            }),
        );

    let c = calls.clone();
    let c2 = calls.clone();
    let torbox = Router::new()
        .route(
            "/torbox/torrents/checkcached",
            get(|Query(q): Query<std::collections::HashMap<String, String>>| async move {
                let data: serde_json::Map<String, Value> = q["hash"]
                    .split(',')
                    .filter(|h| *h == CACHED)
                    .map(|h| (h.to_owned(), json!({ "name": "Movie", "size": 1, "hash": h })))
                    .collect();
                Json(json!({ "success": true, "detail": "ok", "data": data }))
            }),
        )
        .route(
            "/torbox/torrents/createtorrent",
            post(move || async move {
                log(&c, "torbox:create");
                Json(json!({ "success": true, "data": { "torrent_id": 42, "hash": CACHED } }))
            }),
        )
        .route(
            "/torbox/torrents/mylist",
            get(|| async {
                Json(json!({ "success": true, "data": { "id": 42, "download_present": true, "files": [
                    { "id": 0, "name": "Movie/sample.mkv", "size": 10 },
                    { "id": 1, "name": "Movie/Movie.2020.2160p.WEB.mkv", "size": 15_000 }
                ]}}))
            }),
        )
        .route(
            "/torbox/user/me",
            get(|headers: axum::http::HeaderMap| async move {
                if headers[header::AUTHORIZATION] != "Bearer good" {
                    return (StatusCode::FORBIDDEN, Json(json!({ "success": false, "detail": "Invalid token" })));
                }
                (StatusCode::OK, Json(json!({ "success": true, "data": { "email": "m@x.test", "plan": 0, "premium_expires_at": null } })))
            }),
        )
        .route(
            "/torbox/torrents/requestdl",
            get(move |Query(q): Query<std::collections::HashMap<String, String>>| async move {
                log(&c2, &format!("torbox:dl:{}:{}", q["torrent_id"], q["file_id"]));
                Json(json!({ "success": true, "data": "https://cdn.torbox.test/movie.mkv" }))
            }),
        );

    // Which RD torrents have had a file selected.
    let selected: Arc<Mutex<HashSet<String>>> = Default::default();
    let (sel_info, sel_select) = (selected.clone(), selected.clone());
    let c = calls.clone();
    let rd = Router::new()
        .route(
            "/rd/torrents/addMagnet",
            post(move |body: String| async move {
                log(&c, "rd:add");
                let id = if body.contains(CACHED) { "cached" } else { "fresh" };
                Json(json!({ "id": id, "uri": "x" }))
            }),
        )
        .route(
            "/rd/torrents/info/{id}",
            get(move |Path(id): Path<String>| async move {
                let files = json!([
                    { "id": 1, "path": "/Movie/Movie.2160p.mkv", "bytes": 15_000, "selected": 0 },
                    { "id": 2, "path": "/Movie/extras.mkv", "bytes": 100, "selected": 0 }
                ]);
                // After selecting, a cached torrent is downloaded; the other keeps downloading.
                let was_selected = sel_info.lock().unwrap().contains(&id);
                Json(match (id.as_str(), was_selected) {
                    (_, false) => json!({ "status": "waiting_files_selection", "files": files, "links": [] }),
                    ("cached", true) => json!({ "status": "downloaded", "files": files, "links": ["https://rd.test/d/1"] }),
                    _ => json!({ "status": "downloading", "files": files, "links": [] }),
                })
            }),
        )
        .route(
            "/rd/torrents/selectFiles/{id}",
            post(move |Path(id): Path<String>, body: String| async move {
                assert_eq!(body, "files=1");
                sel_select.lock().unwrap().insert(id);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/rd/user",
            get(|headers: axum::http::HeaderMap| async move {
                if headers[header::AUTHORIZATION] != "Bearer good" {
                    return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "bad_token", "error_code": 8 })));
                }
                (StatusCode::OK, Json(json!({ "username": "matz", "type": "premium", "expiration": "2027-01-01T00:00:00.000Z" })))
            }),
        )
        .route(
            "/rd/unrestrict/link",
            post(|| async { Json(json!({ "download": "https://download.rd.test/movie.mkv" })) }),
        );

    let c = calls.clone();
    let c2 = calls.clone();
    let subtitles = Router::new()
        .route(
            "/os/subtitles",
            get(|headers: axum::http::HeaderMap, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                assert_eq!(headers["api-key"], "os-key");
                assert!(headers[header::USER_AGENT].to_str().unwrap().starts_with("Matzio v"));
                assert!(q["languages"] == "de,en" || q["languages"] == "en", "{}", q["languages"]);
                assert_eq!(q["imdb_id"], "111161");
                Json(json!({ "data": [
                    { "attributes": { "language": "en", "download_count": 5000, "release": "Movie.2020.720p.BluRay.x264-SPARKS",
                      "files": [{ "file_id": 11, "file_name": "a.srt" }] } },
                    { "attributes": { "language": "en", "download_count": 10, "release": "Movie.2020.1080p.WEB-DL.x264-NTb",
                      "files": [{ "file_id": 12, "file_name": "b.srt" }] } },
                    { "attributes": { "language": "en", "download_count": 99999, "machine_translated": true, "release": "x",
                      "files": [{ "file_id": 13, "file_name": "c.srt" }] } }
                ]}))
            }),
        )
        .route(
            "/os/login",
            post(|Json(body): Json<Value>| async move {
                if body["password"] != "pw" {
                    return (StatusCode::UNAUTHORIZED, Json(json!({ "message": "Error, invalid username/password" })));
                }
                (StatusCode::OK, Json(json!({ "token": "jwt", "base_url": "api.opensubtitles.com", "status": 200 })))
            }),
        )
        .route(
            "/os/download",
            post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| async move {
                let auth = headers.get(header::AUTHORIZATION).map(|v| v.to_str().unwrap().to_owned()).unwrap_or_default();
                log(&c, &format!("os:download:{}:{auth}", body["file_id"]));
                let host = headers[header::HOST].to_str().unwrap();
                Json(json!({ "link": format!("http://{host}/osfiles/{}.srt", body["file_id"]), "file_name": "b.srt", "remaining": 19 }))
            }),
        )
        .route("/osfiles/{file}", get(|Path(file): Path<String>| async move { format!("1\n00:00:01,000 --> 00:00:02,000\nfile {file}\n") }))
        .route(
            "/subdl/subtitles",
            get(|Query(q): Query<std::collections::HashMap<String, String>>| async move {
                if q["api_key"] != "subdl-key" {
                    return Json(json!({ "status": false, "error": "Invalid api key" }));
                }
                assert!(q["languages"] == "DE,EN" || q["languages"] == "EN", "{}", q["languages"]);
                Json(json!({ "status": true, "subtitles": [
                    { "release_name": "Movie.2020.German.BluRay", "language": "DE", "url": "/subtitle/100-200.zip" },
                    { "release_name": "Movie.2020.1080p.WEB-DL.x264-NTb", "language": "EN", "url": "/subtitle/300-400.zip", "hi": true }
                ]}))
            }),
        )
        .route(
            "/subdlfiles/subtitle/{file}",
            get(move |Path(file): Path<String>| async move {
                log(&c2, &format!("subdl:{file}"));
                subtitles::subdl::zip_of(&[("Movie.de.srt", "hallo")])
            }),
        );

    let app = torrentio.merge(cinemeta).merge(others).merge(torbox).merge(rd).merge(subtitles);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    (format!("http://{addr}"), calls)
}

async fn state(rd: Option<&str>, torbox: Option<&str>) -> (AppState, Arc<Mutex<Vec<String>>>) {
    let (mock, calls) = mock_upstreams().await;
    let endpoints = Endpoints {
        torrentio: format!("{mock}/torrentio"),
        comet: format!("{mock}/comet"),
        mediafusion: format!("{mock}/mediafusion"),
        torbox_search: format!("{mock}/tbsearch"),
        zilean: format!("{mock}/zilean"),
        knaben: format!("{mock}/knaben"),
        cinemeta: format!("{mock}/cinemeta"),
        torbox: format!("{mock}/torbox"),
        real_debrid: format!("{mock}/rd"),
        open_subtitles: format!("{mock}/os"),
        subdl: format!("{mock}/subdl"),
        subdl_files: format!("{mock}/subdlfiles"),
    };
    let config = Arc::new(RwLock::new(Config::new(rd.map(Into::into), torbox.map(Into::into))));
    (AppState::new(config, endpoints, BASE), calls)
}

async fn app(rd: Option<&str>, torbox: Option<&str>) -> (Router, Arc<Mutex<Vec<String>>>) {
    let (state, calls) = state(rd, torbox).await;
    (router(state), calls)
}

async fn get_raw(app: &Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let res = app
        .clone()
        .oneshot(Request::get(uri).header("origin", "https://web.stremio.com").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = res.into_parts();
    (parts.status, parts.headers, body.collect().await.unwrap().to_bytes().to_vec())
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, Option<Value>) {
    let (status, headers, body) = get_raw(app, uri).await;
    if status == StatusCode::OK {
        assert_eq!(headers["access-control-allow-origin"], "*");
    }
    (status, serde_json::from_slice(&body).ok())
}

#[tokio::test]
async fn serves_manifest() {
    let (app, _) = app(None, None).await;
    let (status, body) = get_json(&app, "/manifest.json").await;
    assert_eq!(status, StatusCode::OK);
    let body = body.unwrap();
    assert_eq!(body["id"], ADDON_ID);
    assert_eq!(body["resources"], json!(["stream", "subtitles"]));
}

#[tokio::test]
async fn asks_for_setup_without_keys() {
    let (app, _) = app(None, None).await;
    let (status, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    assert_eq!(status, StatusCode::OK);
    let streams = &body.unwrap()["streams"];
    assert_eq!(streams.as_array().unwrap().len(), 1);
    assert!(streams[0]["title"].as_str().unwrap().contains("Real-Debrid or TorBox"));
}

#[tokio::test]
async fn rejects_unknown_types_and_ids() {
    let (app, _) = app(None, Some("tb")).await;
    for uri in [
        "/stream/tv/tt0111161.json",
        "/stream/movie/kitsu:1.json",
        "/stream/movie/tt0111161",
        "/stream/series/tt0111161.json",
        "/stream/movie/tt0903747:1:1.json",
    ] {
        assert_eq!(get_json(&app, uri).await.0, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn lists_only_torbox_cached_results() {
    let (app, _) = app(None, Some("tb-key")).await;
    let (status, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    assert_eq!(status, StatusCode::OK);
    let streams = body.unwrap()["streams"].as_array().unwrap().clone();
    assert_eq!(streams.len(), 1, "{streams:?}");
    assert_eq!(streams[0]["name"], "Matzio TB⚡\n4K");
    assert_eq!(streams[0]["url"], format!("{BASE}/play/torbox/{CACHED}?file=Movie.2020.2160p.WEB.mkv"));
}

#[tokio::test]
async fn lists_ranked_real_debrid_results_without_duplicates() {
    let (app, _) = app(Some("rd-key"), None).await;
    let (_, body) = get_json(&app, "/stream/series/tt0903747:1:2.json").await;
    let streams = body.unwrap()["streams"].as_array().unwrap().clone();
    let names: Vec<_> = streams.iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Matzio RD\n4K", "Matzio RD\n720p"]);
    assert!(streams[1]["url"].as_str().unwrap().ends_with(&format!("/play/realdebrid/{UNCACHED}?s=1&e=2")));
}

#[tokio::test]
async fn applies_preferences_for_sorting_filters_speed_and_display() {
    let (state, _) = state(Some("rd-key"), None).await;
    let app = router(state.clone());

    // Sort by seeders first, and use compact display style.
    state.config.write().unwrap().preferences = Preferences {
        sort_order: vec!["seeders".into()],
        display_style: "compact".into(),
        ..Preferences::default()
    };
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    let streams = body.unwrap()["streams"].as_array().unwrap().clone();
    assert_eq!(streams.len(), 2);
    assert_eq!(streams[0]["name"], "Matzio\n720p");
    assert_eq!(streams[0]["title"], "1.10 GB · RD");
    assert_eq!(streams[1]["name"], "Matzio\n4K");
    assert_eq!(streams[1]["title"], "15.20 GB · RD");

    // Exclude 4K (2160p).
    let mut prefs = Preferences::default();
    prefs.filters.quality.avoid = vec!["2160p".into()];
    state.config.write().unwrap().preferences = prefs.clone();
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    let streams = body.unwrap()["streams"].as_array().unwrap().clone();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0]["name"], "Matzio RD\n720p");

    // The same check set to "show at the bottom" keeps 4K, below 720p.
    prefs.filters.quality.action = config::Action::Demote;
    state.config.write().unwrap().preferences = prefs.clone();
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    let names: Vec<_> = body.unwrap()["streams"].as_array().unwrap().iter().map(|s| s["name"].clone()).collect();
    assert_eq!(names, ["Matzio RD\n720p", "Matzio RD\n4K"]);

    // Switched off, it does nothing.
    prefs.filters.quality.enabled = false;
    state.config.write().unwrap().preferences = prefs;
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    let names: Vec<_> = body.unwrap()["streams"].as_array().unwrap().iter().map(|s| s["name"].clone()).collect();
    assert_eq!(names, ["Matzio RD\n4K", "Matzio RD\n720p"]);

    // Speed filter with 10 Mbps on a 120-min movie (~9 GB cap) hides the 15.2 GB 4K stream.
    state.config.write().unwrap().preferences = Preferences { speed_mbps: Some(10.0), ..Preferences::default() };
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    let streams = body.unwrap()["streams"].as_array().unwrap().clone();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0]["name"], "Matzio RD\n720p");

    // A plain size cap does the same without a speed.
    let mut prefs = Preferences::default();
    prefs.filters.size.max_gb = Some(10.0);
    state.config.write().unwrap().preferences = prefs;
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    let streams = body.unwrap()["streams"].as_array().unwrap().clone();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0]["name"], "Matzio RD\n720p");

    // Result limits.
    let mut prefs = Preferences::default();
    prefs.filters.limits = config::Limits { enabled: true, total: 1, per_resolution: 5 };
    state.config.write().unwrap().preferences = prefs;
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    assert_eq!(body.unwrap()["streams"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn merges_sources_and_respects_their_switches() {
    let (state, _) = state(Some("rd-key"), None).await;
    let app = router(state.clone());
    let hashes = |body: Option<Value>| -> Vec<String> {
        body.unwrap()["streams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["url"].as_str().unwrap().split('/').nth(5).unwrap().split('?').next().unwrap().to_owned())
            .collect()
    };

    // Zilean and Knaben are off by default; Comet's copy of a Torrentio result is merged.
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    assert_eq!(hashes(body), [CACHED, UNCACHED]);

    let mut prefs = Preferences::default();
    prefs.sources.zilean = true;
    prefs.sources.knaben = true;
    state.config.write().unwrap().preferences = prefs.clone();
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    assert_eq!(hashes(body), [CACHED, EXTRA, UNCACHED]);

    // Only Zilean on.
    prefs.sources = config::Sources { torrentio: false, comet: false, mediafusion: false, knaben: false, ..prefs.sources };
    state.config.write().unwrap().preferences = prefs.clone();
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    assert_eq!(hashes(body), [EXTRA]);

    // A custom address replaces the built-in one.
    let base = state.endpoints.torrentio.clone();
    prefs.sources.torrentio = true;
    prefs.sources.zilean = false;
    prefs.sources.urls.insert("torrentio".into(), format!("{base}/manifest.json"));
    state.config.write().unwrap().preferences = prefs;
    let (_, body) = get_json(&app, "/stream/movie/tt0111161.json").await;
    assert_eq!(hashes(body), [CACHED, UNCACHED]);
}

#[tokio::test]
async fn uses_torbox_search_with_a_torbox_key() {
    let (app, calls) = app(None, Some("tb-key")).await;
    // The extra result isn't cached on TorBox, so it isn't listed, but the search ran.
    let (status, _) = get_json(&app, "/stream/movie/tt0111161.json").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(*calls.lock().unwrap(), ["tbsearch:imdb:tt0111161:Bearer tb-key"]);
}

#[tokio::test]
async fn plays_torbox_and_reuses_the_link() {
    let (app, calls) = app(None, Some("tb-key")).await;
    let uri = format!("/play/torbox/{CACHED}?file=Movie.2020.2160p.WEB.mkv");
    for _ in 0..2 {
        let (status, headers, _) = get_raw(&app, &uri).await;
        assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(headers[header::LOCATION], "https://cdn.torbox.test/movie.mkv");
    }
    // Picked the named file (id 1), and only resolved once.
    assert_eq!(*calls.lock().unwrap(), ["torbox:create", "torbox:dl:42:1"]);
}

#[tokio::test]
async fn plays_cached_real_debrid_and_reports_uncached() {
    let (app, _) = app(Some("rd-key"), None).await;
    let (status, headers, _) = get_raw(&app, &format!("/play/realdebrid/{CACHED}")).await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(headers[header::LOCATION], "https://download.rd.test/movie.mkv");

    let (status, _, body) = get_raw(&app, &format!("/play/realdebrid/{UNCACHED}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(String::from_utf8(body).unwrap().contains("not cached"));
}

#[tokio::test]
async fn play_rejects_bad_input() {
    let (app, _) = app(Some("rd"), None).await;
    assert_eq!(get_raw(&app, &format!("/play/premiumize/{CACHED}")).await.0, StatusCode::NOT_FOUND);
    assert_eq!(get_raw(&app, "/play/realdebrid/not-a-hash").await.0, StatusCode::BAD_REQUEST);
    // TorBox isn't configured here.
    assert_eq!(get_raw(&app, &format!("/play/torbox/{CACHED}")).await.0, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn checks_api_keys() {
    let (state, _) = state(None, None).await;
    let rd = state.check_key(Service::RealDebrid, " good ").await.unwrap();
    assert_eq!(rd.username.as_deref(), Some("matz"));
    assert!(rd.premium);
    assert_eq!(rd.expires.as_deref(), Some("2027-01-01T00:00:00.000Z"));
    assert!(matches!(state.check_key(Service::RealDebrid, "bad").await, Err(DebridError::Api(m)) if m == "bad_token"));

    let tb = state.check_key(Service::TorBox, "good").await.unwrap();
    assert_eq!(tb, debrid::Account { username: Some("m@x.test".into()), premium: false, expires: None });
    assert!(state.check_key(Service::TorBox, "bad").await.is_err());
}

fn subtitle_keys(password: &str) -> config::SubtitleKeys {
    config::SubtitleKeys {
        open_subtitles_key: Some("os-key".into()),
        open_subtitles_username: Some("matz".into()),
        open_subtitles_password: Some(password.into()),
        subdl_key: Some("subdl-key".into()),
    }
}

#[tokio::test]
async fn lists_subtitles_in_language_order_and_downloads_them_once() {
    let (state, calls) = state(None, None).await;
    let app = router(state.clone());
    {
        let mut config = state.config.write().unwrap();
        config.subtitle_keys = subtitle_keys("pw");
        config.preferences.subtitles.languages = vec!["de".into(), "en".into()];
    }

    let uri = "/subtitles/movie/tt0111161/filename=Movie.2020.1080p.WEB-DL.DDP5.1.x264-NTb.mkv&videoSize=1.json";
    let (status, body) = get_json(&app, uri).await;
    assert_eq!(status, StatusCode::OK);
    let subs = body.unwrap()["subtitles"].as_array().unwrap().clone();
    let urls: Vec<_> = subs.iter().map(|s| s["url"].as_str().unwrap().trim_start_matches(BASE).to_owned()).collect();
    // German first, then English: the release matching the file, the popular one, and the
    // hearing impaired one last. Machine translated is hidden by default.
    assert_eq!(urls, ["/subtitle/subdl/100-200.srt", "/subtitle/opensubtitles/12.srt", "/subtitle/opensubtitles/11.srt", "/subtitle/subdl/300-400.srt"]);
    let langs: Vec<_> = subs.iter().map(|s| s["lang"].as_str().unwrap()).collect();
    assert_eq!(langs, ["ger", "eng", "eng", "eng"]);

    let (status, _, body) = get_raw(&app, &urls[1]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(body).unwrap().contains("file 12"));
    // Opening it again uses the kept copy, not another of the day's downloads.
    get_raw(&app, &urls[1]).await;
    let (status, headers, body) = get_raw(&app, &urls[0]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/x-subrip");
    assert_eq!(body, b"hallo");
    assert_eq!(*calls.lock().unwrap(), ["os:download:12:Bearer jwt", "subdl:100-200.zip"]);

    // Switched off: nothing is searched.
    state.config.write().unwrap().preferences.subtitles.enabled = false;
    let (_, body) = get_json(&app, "/subtitles/movie/tt0111161.json").await;
    assert_eq!(body.unwrap()["subtitles"], json!([]));
}

#[tokio::test]
async fn subtitles_need_a_key_and_reject_bad_input() {
    let (app, _) = app(None, None).await;
    let (_, body) = get_json(&app, "/subtitles/movie/tt0111161.json").await;
    assert_eq!(body.unwrap()["subtitles"], json!([]));
    assert_eq!(get_raw(&app, "/subtitle/opensubtitles/..%2Fx.srt").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(get_raw(&app, "/subtitle/other/1.srt").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get_json(&app, "/subtitles/series/tt0111161.json").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn checks_subtitle_keys() {
    let (state, _) = state(None, None).await;
    let os = subtitles::Provider::OpenSubtitles;
    assert_eq!(state.check_subtitle_key(os, subtitle_keys("pw")).await.unwrap(), "Signed in as matz");
    let wrong = state.check_subtitle_key(os, subtitle_keys("nope")).await.unwrap_err();
    assert!(wrong.to_string().contains("username or password"), "{wrong}");
    let key_only = config::SubtitleKeys { open_subtitles_key: Some("os-key".into()), ..Default::default() };
    assert!(state.check_subtitle_key(os, key_only).await.unwrap().starts_with("Key works"));

    let subdl = subtitles::Provider::Subdl;
    assert_eq!(state.check_subtitle_key(subdl, subtitle_keys("pw")).await.unwrap(), "Key works");
    let bad = config::SubtitleKeys { subdl_key: Some("bad".into()), ..Default::default() };
    assert_eq!(state.check_subtitle_key(subdl, bad).await.unwrap_err().to_string(), "SubDL didn't accept the API key");
}

#[tokio::test]
async fn binds_and_serves_on_localhost() {
    let listener = bind(0).await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(addr.ip().is_loopback());
    assert_eq!(manifest_url(addr), format!("http://127.0.0.1:{}/manifest.json", addr.port()));
    let state = AppState::new(Default::default(), Endpoints::default(), &base_url(addr));
    tokio::spawn(serve(listener, state));
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream.write_all(b"GET /manifest.json HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut out = String::new();
    stream.read_to_string(&mut out).await.unwrap();
    assert!(out.starts_with("HTTP/1.1 200"), "{out}");
    assert!(out.contains(ADDON_ID));
}
