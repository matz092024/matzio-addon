//! The Stremio addon protocol server.
//!
//! Stremio talks to an addon over plain HTTP: it fetches `/manifest.json` once
//! when the addon is installed, then `/stream/{type}/{id}.json` every time the
//! user opens a movie or episode. Everything here binds to localhost only.

use std::net::{Ipv4Addr, SocketAddr};

use axum::{extract::Path, http::StatusCode, routing::get, Json, Router};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

/// Default port the addon listens on.
pub const DEFAULT_PORT: u16 = 7777;

const ADDON_ID: &str = "community.matzio.addon";

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

#[derive(Debug, Serialize, PartialEq)]
pub struct StreamResponse {
    pub streams: Vec<Value>,
}

/// Builds the HTTP router. Stremio's web and desktop clients need CORS.
pub fn router() -> Router {
    Router::new()
        .route("/", get(|| async { Json(json!({ "ok": true, "addon": ADDON_ID })) }))
        .route("/manifest.json", get(|| async { Json(manifest()) }))
        .route("/stream/{kind}/{file}", get(stream))
        .layer(CorsLayer::permissive())
}

async fn stream(Path((kind, file)): Path<(String, String)>) -> Result<Json<StreamResponse>, StatusCode> {
    // The id is "tt1234567" for movies or "tt1234567:season:episode" for series.
    let id = file.strip_suffix(".json").ok_or(StatusCode::NOT_FOUND)?;
    if !matches!(kind.as_str(), "movie" | "series") || !id.starts_with("tt") {
        return Err(StatusCode::NOT_FOUND);
    }
    // Stream lookup through debrid arrives in step 2; for now nothing is found.
    Ok(Json(StreamResponse { streams: Vec::new() }))
}

/// Binds the addon to `127.0.0.1:port`. Serve it with [`serve`].
pub async fn bind(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await
}

/// Serves the addon on an already bound listener until the process exits.
pub async fn serve(listener: TcpListener) -> std::io::Result<()> {
    axum::serve(listener, router()).await
}

/// The URL users paste into Stremio, or open as a `stremio://` link.
pub fn manifest_url(addr: SocketAddr) -> String {
    format!("http://{addr}/manifest.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get_json(uri: &str) -> (StatusCode, Option<Value>) {
        let res = router()
            .oneshot(Request::get(uri).header("origin", "https://web.stremio.com").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        if status == StatusCode::OK {
            assert_eq!(res.headers()["access-control-allow-origin"], "*");
        }
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).ok())
    }

    #[tokio::test]
    async fn serves_manifest() {
        let (status, body) = get_json("/manifest.json").await;
        assert_eq!(status, StatusCode::OK);
        let body = body.unwrap();
        assert_eq!(body["id"], ADDON_ID);
        assert_eq!(body["resources"], json!(["stream"]));
    }

    #[tokio::test]
    async fn movie_and_episode_streams_are_empty_for_now() {
        for uri in ["/stream/movie/tt0111161.json", "/stream/series/tt0903747:1:1.json"] {
            let (status, body) = get_json(uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert_eq!(body.unwrap(), json!({ "streams": [] }));
        }
    }

    #[tokio::test]
    async fn rejects_unknown_types_and_ids() {
        for uri in ["/stream/tv/tt0111161.json", "/stream/movie/kitsu:1.json", "/stream/movie/tt0111161"] {
            assert_eq!(get_json(uri).await.0, StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[tokio::test]
    async fn binds_and_serves_on_localhost() {
        let listener = bind(0).await.unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(addr.ip().is_loopback());
        assert_eq!(manifest_url(addr), format!("http://127.0.0.1:{}/manifest.json", addr.port()));
        tokio::spawn(serve(listener));
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stream.write_all(b"GET /manifest.json HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).await.unwrap();
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        assert!(out.contains(ADDON_ID));
    }
}
