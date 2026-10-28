//! Runtime lookup from Stremio's Cinemeta metadata service.

use std::{sync::LazyLock, time::Duration};

use regex::Regex;
use serde::Deserialize;

use crate::torrent::Episode;

#[derive(Deserialize)]
struct Response {
    meta: Option<Meta>,
}

#[derive(Deserialize)]
struct Meta {
    runtime: Option<String>,
    #[serde(default)]
    videos: Vec<Video>,
}

#[derive(Deserialize)]
struct Video {
    season: Option<u32>,
    episode: Option<u32>,
    runtime: Option<String>,
}

/// Looks up the runtime of a movie or series episode from Cinemeta.
pub async fn runtime(
    http: &reqwest::Client,
    base: &str,
    kind: &str,
    imdb_id: &str,
    episode: Option<Episode>,
) -> Option<Duration> {
    let url = format!("{base}/meta/{kind}/{imdb_id}.json");
    let response: Response = http.get(url).send().await.ok()?.error_for_status().ok()?.json().await.ok()?;
    let meta = response.meta?;
    if let Some(ep) = episode {
        let ep_runtime = meta
            .videos
            .iter()
            .find(|v| v.season == Some(ep.season) && v.episode == Some(ep.episode))
            .and_then(|v| v.runtime.as_deref())
            .and_then(parse_runtime);
        if ep_runtime.is_some() {
            return ep_runtime;
        }
    }
    meta.runtime.as_deref().and_then(parse_runtime)
}

/// Parses a Cinemeta runtime string such as `"142 min"`, `"2h 22m"`, or `"45"`.
pub fn parse_runtime(text: &str) -> Option<Duration> {
    static HOURS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b(\d+)\s*(?:hours?|hrs?|h)(?:[^a-z]|$)").unwrap());
    static MINS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)(\d+)\s*(?:minutes?|mins?|m)(?:[^a-z]|$)").unwrap());
    static PLAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\d+)\s*$").unwrap());

    let hours: u64 = HOURS.captures(text).and_then(|c| c[1].parse().ok()).unwrap_or(0);
    let mins: u64 = MINS.captures(text).and_then(|c| c[1].parse().ok()).unwrap_or(0);
    let total_mins = if hours > 0 || mins > 0 {
        hours * 60 + mins
    } else {
        PLAIN.captures(text).and_then(|c| c[1].parse().ok()).unwrap_or(0)
    };
    (total_mins > 0).then(|| Duration::from_secs(total_mins * 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_formats() {
        assert_eq!(parse_runtime("142 min"), Some(Duration::from_secs(142 * 60)));
        assert_eq!(parse_runtime("2 h 22 min"), Some(Duration::from_secs(142 * 60)));
        assert_eq!(parse_runtime("2h22m"), Some(Duration::from_secs(142 * 60)));
        assert_eq!(parse_runtime("2 hr"), Some(Duration::from_secs(120 * 60)));
        assert_eq!(parse_runtime("45"), Some(Duration::from_secs(45 * 60)));
        assert_eq!(parse_runtime("N/A"), None);
        assert_eq!(parse_runtime("0 min"), None);
    }
}
