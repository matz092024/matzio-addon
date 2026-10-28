//! SubDL API. Searching needs the user's free API key; the files themselves are public
//! zip archives, unpacked here because Stremio can't read zips.

use std::io::{Cursor, Read};

use serde::Deserialize;
use serde_json::Value;

use super::{content_type, valid_file, File, Lang, Request, Subtitle, SubtitleError, LANGUAGES};
use crate::torrent::{matches_episode, Episode};

/// Largest unpacked subtitle accepted, so a broken or hostile archive can't fill memory.
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    status: bool,
    error: Option<String>,
    #[serde(default)]
    subtitles: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    release_name: Option<String>,
    name: Option<String>,
    language: Option<String>,
    url: Option<String>,
    #[serde(default)]
    hi: bool,
    #[serde(default)]
    download_count: Option<Value>,
    /// Every release the subtitle fits, when asked for with `releases=1`.
    #[serde(default)]
    releases: Vec<Value>,
}

fn search_query(key: &str, request: &Request<'_>) -> Vec<(&'static str, String)> {
    let codes: Vec<&str> = request.languages.iter().map(|l| l.subdl).collect();
    let mut query = vec![
        ("api_key", key.to_owned()),
        ("imdb_id", request.imdb_id.to_owned()),
        ("languages", codes.join(",")),
        ("subs_per_page", "30".to_owned()),
        ("hi", "1".to_owned()),
        ("releases", "1".to_owned()),
    ];
    match request.episode {
        Some(ep) => {
            query.push(("type", "tv".to_owned()));
            query.push(("season_number", ep.season.to_string()));
            query.push(("episode_number", ep.episode.to_string()));
        }
        None => query.push(("type", "movie".to_owned())),
    }
    query
}

pub async fn search(
    http: &reqwest::Client,
    base: &str,
    key: &str,
    request: &Request<'_>,
) -> Result<Vec<Subtitle>, SubtitleError> {
    let body: SearchResponse = http
        .get(format!("{}/subtitles", base.trim_end_matches('/')))
        .query(&search_query(key, request))
        .send()
        .await?
        .json()
        .await?;
    if !body.status {
        let error = body.error.unwrap_or_default();
        // "Not found" is just an empty result.
        if error.to_ascii_lowercase().contains("not found") {
            return Ok(Vec::new());
        }
        return Err(SubtitleError::Api(format!("SubDL: {}", if error.is_empty() { "search failed" } else { &error })));
    }
    Ok(body.subtitles.into_iter().filter_map(parse).collect())
}

fn lang(code: &str) -> Option<&'static Lang> {
    LANGUAGES.iter().find(|l| l.subdl.eq_ignore_ascii_case(code))
}

/// "/subtitle/3197651-3213944.zip" becomes "3197651-3213944".
fn file_id(url: &str) -> Option<String> {
    let id = url.trim().rsplit('/').next()?.strip_suffix(".zip")?;
    valid_file(id).then(|| id.to_owned())
}

fn parse(item: Item) -> Option<Subtitle> {
    // Releases come as names, or as objects holding one.
    let also = item.releases.iter().filter_map(|r| r.as_str().or(r["release_name"].as_str()).or(r["name"].as_str())).map(str::to_owned);
    let releases: Vec<String> = item.release_name.into_iter().chain(item.name).chain(also).filter(|r| !r.trim().is_empty()).collect();
    Some(Subtitle {
        provider: super::Provider::Subdl,
        file: file_id(item.url.as_deref()?)?,
        lang: lang(item.language.as_deref()?)?,
        releases,
        hearing_impaired: item.hi,
        machine_translated: false,
        downloads: item.download_count.as_ref().and_then(crate::sources::number).unwrap_or(0),
        hash_match: false,
    })
}

pub async fn download(http: &reqwest::Client, base: &str, file: &str, episode: Option<Episode>) -> Result<File, SubtitleError> {
    if !valid_file(file) {
        return Err(SubtitleError::NotFound);
    }
    let url = format!("{}/subtitle/{file}.zip", base.trim_end_matches('/'));
    let response = http.get(url).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(SubtitleError::NotFound);
    }
    let archive = response.error_for_status()?.bytes().await?;
    unpack(&archive, episode)
}

/// Picks the subtitle out of an archive: for an episode the file naming it (season packs hold
/// many), otherwise the first one, preferring SRT.
fn unpack(archive: &[u8], episode: Option<Episode>) -> Result<File, SubtitleError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(|e| SubtitleError::Api(format!("SubDL sent a broken archive: {e}")))?;
    let names: Vec<String> = zip
        .file_names()
        .filter_map(|name| name.ok().map(|n| n.into_owned()))
        .filter(|name| {
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            matches!(ext.as_str(), "srt" | "vtt" | "ass" | "ssa")
        })
        .collect();
    let srt_first = |a: &&String, b: &&String| (!a.to_ascii_lowercase().ends_with(".srt")).cmp(&!b.to_ascii_lowercase().ends_with(".srt"));
    let mut candidates: Vec<&String> = names.iter().collect();
    if let Some(ep) = episode {
        let matching: Vec<&String> = names.iter().filter(|n| matches_episode(n, ep)).collect();
        // A pack without this episode has nothing to offer; a single unnamed file is likely it.
        if !matching.is_empty() || names.len() > 1 {
            candidates = matching;
        }
    }
    candidates.sort_by(srt_first);
    let name = candidates.first().ok_or(SubtitleError::NotFound)?.to_string();
    let entry = zip.by_name(&name).map_err(|_| SubtitleError::NotFound)?;
    if entry.size() > MAX_FILE_BYTES {
        return Err(SubtitleError::Api("SubDL subtitle is too large".into()));
    }
    let mut bytes = Vec::new();
    entry.take(MAX_FILE_BYTES).read_to_end(&mut bytes).map_err(|e| SubtitleError::Api(format!("couldn't unpack the SubDL subtitle: {e}")))?;
    Ok(File { bytes, content_type: content_type(&name) })
}

/// Checks the key with a small search.
pub async fn check(http: &reqwest::Client, base: &str, key: &str) -> Result<String, SubtitleError> {
    let langs = super::languages(&["en".to_owned()]);
    let probe = Request { imdb_id: "tt0111161", episode: None, languages: &langs, video_hash: None };
    match search(http, base, key, &probe).await {
        Ok(_) => Ok("Key works".into()),
        Err(SubtitleError::Api(m)) if m.to_ascii_lowercase().contains("key") => Err(SubtitleError::Api("SubDL didn't accept the API key".into())),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
pub(crate) fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write;
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in files {
        out.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(text.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_results() {
        let body: SearchResponse = serde_json::from_value(json!({ "status": true, "subtitles": [
            { "release_name": "Movie.2020.1080p.WEB", "language": "EN", "url": "/subtitle/3197651-3213944.zip", "hi": true },
            { "release_name": "Movie.2020.BluRay", "language": "BR_PT", "url": "/subtitle/1-2.zip",
              "releases": ["Movie.2020.1080p.BluRay.x264-SPARKS", { "release_name": "Movie.2020.720p.BluRay" }] },
            { "release_name": "bad", "language": "EN", "url": "/subtitle/a b.zip" },
            { "release_name": "unknown language", "language": "QQ", "url": "/subtitle/3-4.zip" }
        ]}))
        .unwrap();
        let subs: Vec<_> = body.subtitles.into_iter().filter_map(parse).collect();
        assert_eq!(subs.len(), 2);
        assert_eq!((subs[0].file.as_str(), subs[0].lang.code, subs[0].hearing_impaired), ("3197651-3213944", "en", true));
        assert_eq!(subs[1].lang.code, "pt-br");
        assert_eq!(subs[1].releases, ["Movie.2020.BluRay", "Movie.2020.1080p.BluRay.x264-SPARKS", "Movie.2020.720p.BluRay"]);
    }

    #[test]
    fn picks_the_episode_from_a_season_pack() {
        let pack = zip_of(&[
            ("Show.S01E01.srt", "one"),
            ("Show.S01E02.ass", "two ass"),
            ("Show.S01E02.srt", "two"),
            ("readme.txt", "hi"),
        ]);
        let file = unpack(&pack, Some(Episode { season: 1, episode: 2 })).unwrap();
        assert_eq!(file.bytes, b"two");
        assert_eq!(file.content_type, "application/x-subrip");
        assert!(matches!(unpack(&pack, Some(Episode { season: 1, episode: 9 })), Err(SubtitleError::NotFound)));

        let single = zip_of(&[("Movie.vtt", "WEBVTT")]);
        let file = unpack(&single, Some(Episode { season: 1, episode: 9 })).unwrap();
        assert_eq!(file.bytes, b"WEBVTT");
        assert!(file.content_type.starts_with("text/vtt"));
    }
}
