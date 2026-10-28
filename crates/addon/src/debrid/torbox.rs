//! TorBox: https://api.torbox.app/v1/api
//!
//! Supports a bulk cache check, so only cached results are shown.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use serde_json::Value;

use super::{Account, DebridError, PlayRequest};
use crate::torrent::{pick_file, DebridFile};

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    success: bool,
    detail: Option<String>,
    data: Option<T>,
}

impl<T> Envelope<T> {
    fn into_data(self) -> Result<T, DebridError> {
        match (self.success, self.data) {
            (true, Some(data)) => Ok(data),
            _ => Err(DebridError::Api(self.detail.unwrap_or_else(|| "unknown TorBox error".into()))),
        }
    }
}

/// Returns the lowercase hashes TorBox has cached. Checks in batches of 100.
pub async fn cached(
    http: &reqwest::Client,
    base: &str,
    key: &str,
    hashes: &[String],
) -> Result<HashSet<String>, DebridError> {
    let mut found = HashSet::new();
    for chunk in hashes.chunks(100) {
        let response: Envelope<Value> = http
            .get(format!("{base}/torrents/checkcached"))
            .bearer_auth(key)
            .query(&[("hash", chunk.join(",").as_str()), ("format", "object"), ("list_files", "false")])
            .send()
            .await?
            .json()
            .await?;
        if !response.success {
            return Err(DebridError::Api(response.detail.unwrap_or_else(|| "cache check failed".into())));
        }
        // `data` is an object keyed by hash, or null/empty when nothing is cached.
        if let Some(Value::Object(map)) = response.data {
            found.extend(map.keys().map(|h| h.to_ascii_lowercase()));
        }
    }
    Ok(found)
}

#[derive(Deserialize)]
struct Created {
    torrent_id: u64,
}

#[derive(Deserialize)]
struct TorrentInfo {
    #[serde(default)]
    download_present: bool,
    #[serde(default)]
    files: Vec<TorrentFile>,
}

#[derive(Deserialize)]
struct TorrentFile {
    id: u64,
    name: String,
    #[serde(default)]
    size: u64,
}

/// Adds the torrent (instant when cached) and returns a direct download link.
pub async fn resolve(http: &reqwest::Client, base: &str, key: &str, req: &PlayRequest) -> Result<String, DebridError> {
    let magnet = format!("magnet:?xt=urn:btih:{}", req.info_hash);
    let form = reqwest::multipart::Form::new().text("magnet", magnet).text("allow_zip", "false");
    let created: Envelope<Created> =
        http.post(format!("{base}/torrents/createtorrent")).bearer_auth(key).multipart(form).send().await?.json().await?;
    let torrent_id = created.into_data()?.torrent_id;

    let info: Envelope<TorrentInfo> = http
        .get(format!("{base}/torrents/mylist"))
        .bearer_auth(key)
        .query(&[("id", torrent_id.to_string()), ("bypass_cache", "true".into())])
        .send()
        .await?
        .json()
        .await?;
    let info = info.into_data()?;
    if !info.download_present {
        return Err(DebridError::NotCached);
    }

    let files: Vec<_> = info.files.into_iter().map(|f| DebridFile { id: f.id, path: f.name, size: f.size }).collect();
    let file = pick_file(&files, req.filename.as_deref(), req.episode()).ok_or(DebridError::NoFile)?;

    let link: Envelope<String> = http
        .get(format!("{base}/torrents/requestdl"))
        .query(&HashMap::from([
            ("token", key.to_owned()),
            ("torrent_id", torrent_id.to_string()),
            ("file_id", file.id.to_string()),
        ]))
        .send()
        .await?
        .json()
        .await?;
    link.into_data()
}

#[derive(Deserialize)]
struct User {
    email: Option<String>,
    #[serde(default)]
    plan: u32,
    premium_expires_at: Option<String>,
}

pub async fn account(http: &reqwest::Client, base: &str, key: &str) -> Result<Account, DebridError> {
    let response = http.get(format!("{base}/user/me")).bearer_auth(key).send().await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED || response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(DebridError::Api("invalid API key".into()));
    }
    let user = response.json::<Envelope<User>>().await?.into_data()?;
    // Plan 0 is the free tier.
    let premium = user.plan > 0;
    Ok(Account { username: user.email, premium, expires: user.premium_expires_at.filter(|_| premium) })
}
