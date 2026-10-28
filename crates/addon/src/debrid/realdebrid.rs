//! Real-Debrid: https://api.real-debrid.com/rest/1.0
//!
//! Real-Debrid no longer offers a cache check, so results are listed without
//! one and checked when played: a cached torrent is "downloaded" right after
//! its file is selected. Anything else is left downloading on the account.

use serde::Deserialize;

use super::{Account, DebridError, PlayRequest};
use crate::torrent::{pick_file, DebridFile};

#[derive(Deserialize)]
struct Added {
    id: String,
}

#[derive(Deserialize)]
struct Info {
    status: String,
    #[serde(default)]
    files: Vec<File>,
    #[serde(default)]
    links: Vec<String>,
}

#[derive(Deserialize)]
struct File {
    id: u64,
    path: String,
    #[serde(default)]
    bytes: u64,
}

#[derive(Deserialize)]
struct Unrestricted {
    download: String,
}

#[derive(Deserialize)]
struct ApiError {
    error: Option<String>,
}

async fn check(response: reqwest::Response) -> Result<reqwest::Response, DebridError> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let message = response.json::<ApiError>().await.ok().and_then(|e| e.error).unwrap_or_else(|| status.to_string());
    Err(DebridError::Api(message))
}

async fn info(http: &reqwest::Client, base: &str, key: &str, id: &str) -> Result<Info, DebridError> {
    let response = http.get(format!("{base}/torrents/info/{id}")).bearer_auth(key).send().await?;
    Ok(check(response).await?.json().await?)
}

pub async fn resolve(http: &reqwest::Client, base: &str, key: &str, req: &PlayRequest) -> Result<String, DebridError> {
    let magnet = format!("magnet:?xt=urn:btih:{}", req.info_hash);
    let response =
        http.post(format!("{base}/torrents/addMagnet")).bearer_auth(key).form(&[("magnet", magnet)]).send().await?;
    let Added { id } = check(response).await?.json().await?;

    let mut torrent = info(http, base, key, &id).await?;
    if torrent.status == "waiting_files_selection" {
        let files: Vec<_> =
            torrent.files.iter().map(|f| DebridFile { id: f.id, path: f.path.clone(), size: f.bytes }).collect();
        let file = pick_file(&files, req.filename.as_deref(), req.episode()).ok_or(DebridError::NoFile)?;
        let response = http
            .post(format!("{base}/torrents/selectFiles/{id}"))
            .bearer_auth(key)
            .form(&[("files", file.id.to_string())])
            .send()
            .await?;
        check(response).await?;
        torrent = info(http, base, key, &id).await?;
    }

    // With one file selected, a cached torrent is immediately "downloaded" with one link.
    let link = match (torrent.status.as_str(), torrent.links.first()) {
        ("downloaded", Some(link)) => link.clone(),
        _ => return Err(DebridError::NotCached),
    };
    let response =
        http.post(format!("{base}/unrestrict/link")).bearer_auth(key).form(&[("link", link)]).send().await?;
    let Unrestricted { download } = check(response).await?.json().await?;
    Ok(download)
}

#[derive(Deserialize)]
struct User {
    username: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    expiration: Option<String>,
}

pub async fn account(http: &reqwest::Client, base: &str, key: &str) -> Result<Account, DebridError> {
    let response = http.get(format!("{base}/user")).bearer_auth(key).send().await?;
    let user: User = check(response).await?.json().await?;
    let premium = user.kind.as_deref() == Some("premium");
    Ok(Account { username: user.username, premium, expires: user.expiration.filter(|_| premium) })
}
