//! OpenSubtitles.com REST API. Searching only needs the user's API key; downloading counts
//! against a daily limit, which is higher when the user also signs in with their account.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{json, Value};

use super::{content_type, languages, File, Lang, Request, Subtitle, SubtitleError, LANGUAGES};
use crate::config::SubtitleKeys;

/// OpenSubtitles wants every app to name itself and its version.
const USER_AGENT: &str = concat!("Matzio v", env!("CARGO_PKG_VERSION"));

/// Sign-in tokens last 24 hours; renew a little before.
const TOKEN_TTL: Duration = Duration::from_secs(23 * 60 * 60);

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    attributes: Attributes,
}

#[derive(Deserialize)]
struct Attributes {
    language: Option<String>,
    #[serde(default)]
    download_count: Option<Value>,
    #[serde(default)]
    hearing_impaired: bool,
    #[serde(default)]
    ai_translated: bool,
    #[serde(default)]
    machine_translated: bool,
    release: Option<String>,
    #[serde(default)]
    moviehash_match: bool,
    #[serde(default)]
    files: Vec<FileRef>,
}

#[derive(Deserialize)]
struct FileRef {
    file_id: Option<Value>,
    file_name: Option<String>,
}

/// The signed-in token, remembered for the account it belongs to.
#[derive(Default)]
pub struct Login(Mutex<Option<(Instant, String, String)>>);

impl Login {
    fn get(&self, username: &str) -> Option<String> {
        let saved = self.0.lock().unwrap();
        saved
            .as_ref()
            .filter(|(at, user, _)| at.elapsed() < TOKEN_TTL && user == username)
            .map(|(_, _, token)| token.clone())
    }

    fn set(&self, username: &str, token: Option<String>) {
        *self.0.lock().unwrap() = token.map(|t| (Instant::now(), username.to_owned(), t));
    }
}

fn request(http: &reqwest::Client, method: reqwest::Method, url: String, key: &str) -> reqwest::RequestBuilder {
    http.request(method, url).header("Api-Key", key).header(reqwest::header::USER_AGENT, USER_AGENT).header("Accept", "application/json")
}

/// OpenSubtitles redirects unless parameters are in alphabetical order and lowercase,
/// and the redirect would drop the API key.
fn search_query(request: &Request<'_>) -> Vec<(&'static str, String)> {
    let imdb = request.imdb_id.trim_start_matches("tt").trim_start_matches('0').to_owned();
    let mut codes: Vec<&str> = request.languages.iter().flat_map(|l| l.opensubtitles.iter().copied()).collect();
    codes.sort_unstable();
    codes.dedup();
    let mut query = vec![("ai_translated", "include".to_owned()), ("languages", codes.join(",")), ("machine_translated", "include".to_owned())];
    match request.episode {
        Some(ep) => {
            query.push(("episode_number", ep.episode.to_string()));
            query.push(("parent_imdb_id", imdb));
            query.push(("season_number", ep.season.to_string()));
        }
        None => query.push(("imdb_id", imdb)),
    }
    if let Some(hash) = request.video_hash.filter(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit())) {
        query.push(("moviehash", hash.to_ascii_lowercase()));
    }
    query.sort_by_key(|(k, _)| *k);
    query
}

pub async fn search(
    http: &reqwest::Client,
    base: &str,
    key: &str,
    request: &Request<'_>,
) -> Result<Vec<Subtitle>, SubtitleError> {
    let response = self::request(http, reqwest::Method::GET, format!("{}/subtitles", base.trim_end_matches('/')), key)
        .query(&search_query(request))
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(SubtitleError::Api(error_message(status, response).await));
    }
    let body: SearchResponse = response.json().await?;
    Ok(body.data.into_iter().filter_map(|item| parse(item.attributes)).collect())
}

fn lang(code: &str) -> Option<&'static Lang> {
    let code = code.to_ascii_lowercase();
    LANGUAGES.iter().find(|l| l.opensubtitles.contains(&code.as_str()))
}

fn parse(a: Attributes) -> Option<Subtitle> {
    // Subtitles split over several files (CD1, CD2) don't fit a single video.
    let [file] = <[FileRef; 1]>::try_from(a.files).ok()?;
    let file_id = crate::sources::number(file.file_id.as_ref()?)?.to_string();
    Some(Subtitle {
        provider: super::Provider::OpenSubtitles,
        file: file_id,
        lang: lang(a.language.as_deref()?)?,
        releases: a.release.into_iter().chain(file.file_name).filter(|r| !r.trim().is_empty()).collect(),
        hearing_impaired: a.hearing_impaired,
        machine_translated: a.ai_translated || a.machine_translated,
        downloads: a.download_count.as_ref().and_then(crate::sources::number).unwrap_or(0),
        hash_match: a.moviehash_match,
    })
}

async fn error_message(status: reqwest::StatusCode, response: reqwest::Response) -> String {
    let body: Value = response.json().await.unwrap_or(Value::Null);
    let message = body["message"].as_str().or(body["errors"][0].as_str()).map(str::to_owned);
    match (status.as_u16(), message) {
        (401 | 403, _) => "OpenSubtitles didn't accept the API key".into(),
        (_, Some(m)) => format!("OpenSubtitles: {m}"),
        (code, None) => format!("OpenSubtitles answered {code}"),
    }
}

/// Signs in with the user's account, if they gave one. A failed sign-in falls back to
/// downloading with just the key.
async fn token(http: &reqwest::Client, base: &str, login: &Login, key: &str, keys: &SubtitleKeys) -> Result<Option<String>, SubtitleError> {
    let (Some(username), Some(password)) = (&keys.open_subtitles_username, &keys.open_subtitles_password) else {
        return Ok(None);
    };
    if let Some(token) = login.get(username) {
        return Ok(Some(token));
    }
    let response = request(http, reqwest::Method::POST, format!("{}/login", base.trim_end_matches('/')), key)
        .json(&json!({ "username": username, "password": password }))
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let message = error_message(status, response).await;
        return Err(SubtitleError::Api(if status.as_u16() == 401 { "OpenSubtitles didn't accept the username or password".into() } else { message }));
    }
    let body: Value = response.json().await?;
    let token = body["token"].as_str().map(str::to_owned);
    login.set(username, token.clone());
    Ok(token)
}

pub async fn download(
    http: &reqwest::Client,
    base: &str,
    login: &Login,
    key: &str,
    keys: &SubtitleKeys,
    file_id: &str,
) -> Result<File, SubtitleError> {
    let id: u64 = file_id.parse().map_err(|_| SubtitleError::NotFound)?;
    let token = match token(http, base, login, key, keys).await {
        Ok(token) => token,
        Err(e) => {
            eprintln!("OpenSubtitles sign-in failed, downloading without it: {e}");
            None
        }
    };
    let mut req = request(http, reqwest::Method::POST, format!("{}/download", base.trim_end_matches('/')), key)
        .json(&json!({ "file_id": id, "sub_format": "srt" }));
    if let Some(token) = &token {
        req = req.bearer_auth(token);
    }
    let response = req.send().await?;
    let status = response.status();
    if status.as_u16() == 401 && token.is_some() {
        // The token ran out early; sign in again next time.
        login.set(keys.open_subtitles_username.as_deref().unwrap_or_default(), None);
    }
    if !status.is_success() {
        return Err(SubtitleError::Api(error_message(status, response).await));
    }
    let body: Value = response.json().await?;
    let link = body["link"].as_str().ok_or_else(|| SubtitleError::Api(format!("OpenSubtitles: {}", body["message"].as_str().unwrap_or("no download link"))))?;
    let name = body["file_name"].as_str().unwrap_or("subtitle.srt").to_owned();
    let bytes = http.get(link).send().await?.error_for_status()?.bytes().await?;
    Ok(File { bytes: bytes.to_vec(), content_type: content_type(&name) })
}

/// Checks the key, and the account if one is given.
pub async fn check(http: &reqwest::Client, base: &str, keys: &SubtitleKeys) -> Result<String, SubtitleError> {
    let key = keys.open_subtitles_key.as_deref().ok_or(SubtitleError::Api("paste an API key first".into()))?;
    if let (Some(username), Some(_)) = (&keys.open_subtitles_username, &keys.open_subtitles_password) {
        // Always a fresh sign-in, so a changed password is noticed.
        let login = Login::default();
        token(http, base, &login, key, keys).await?;
        return Ok(format!("Signed in as {username}"));
    }
    // A small search shows whether the key works.
    let langs = languages(&["en".to_owned()]);
    let probe = Request { imdb_id: "tt0111161", episode: None, languages: &langs, video_hash: None };
    search(http, base, key, &probe).await?;
    Ok("Key works · add your account for more downloads a day".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::torrent::Episode;

    #[test]
    fn builds_sorted_lowercase_queries() {
        let langs = languages(&["pt-br".into(), "en".into(), "zh".into()]);
        let req = Request { imdb_id: "tt0903747", episode: Some(Episode { season: 1, episode: 2 }), languages: &langs, video_hash: Some("8E245D9679D31E12") };
        let query = search_query(&req);
        let keys: Vec<_> = query.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["ai_translated", "episode_number", "languages", "machine_translated", "moviehash", "parent_imdb_id", "season_number"]);
        let get = |k: &str| query.iter().find(|(key, _)| *key == k).unwrap().1.clone();
        assert_eq!(get("languages"), "en,pt-br,zh-cn,zh-tw");
        assert_eq!(get("parent_imdb_id"), "903747");
        assert_eq!(get("moviehash"), "8e245d9679d31e12");
    }

    #[test]
    fn parses_results() {
        let body: SearchResponse = serde_json::from_value(json!({ "data": [
            { "attributes": { "language": "en", "download_count": 1200, "hearing_impaired": true, "release": "Movie.2020.1080p.WEB",
              "files": [{ "file_id": 99, "file_name": "Movie.srt" }] } },
            { "attributes": { "language": "pt-BR", "download_count": "5", "ai_translated": true, "files": [{ "file_id": 7, "file_name": "x.srt" }] } },
            { "attributes": { "language": "en", "files": [{ "file_id": 1 }, { "file_id": 2 }] } },
            { "attributes": { "language": "xx", "files": [{ "file_id": 3 }] } }
        ]}))
        .unwrap();
        let subs: Vec<_> = body.data.into_iter().filter_map(|i| parse(i.attributes)).collect();
        assert_eq!(subs.len(), 2);
        assert_eq!((subs[0].file.as_str(), subs[0].lang.code, subs[0].downloads, subs[0].hearing_impaired), ("99", "en", 1200, true));
        assert_eq!((subs[1].lang.code, subs[1].releases[0].as_str(), subs[1].machine_translated), ("pt-br", "x.srt", true));
    }
}
