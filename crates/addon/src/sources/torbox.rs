//! TorBox's own torrent search, available to TorBox users with their key.

use serde::Deserialize;
use serde_json::Value;

use crate::torrent::{Episode, Torrent};

#[derive(Deserialize)]
struct Response {
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    #[serde(default)]
    torrents: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    hash: Option<String>,
    raw_title: Option<String>,
    title: Option<String>,
    size: Option<Value>,
    seeders: Option<Value>,
    tracker: Option<String>,
}

pub async fn search(
    http: &reqwest::Client,
    base: &str,
    key: &str,
    imdb_id: &str,
    episode: Option<Episode>,
) -> Result<Vec<Torrent>, reqwest::Error> {
    let mut query = vec![
        ("metadata", "false".to_owned()),
        ("check_cache", "false".to_owned()),
        ("check_owned", "false".to_owned()),
        ("search_user_engines", "false".to_owned()),
    ];
    if let Some(ep) = episode {
        query.push(("season", ep.season.to_string()));
        query.push(("episode", ep.episode.to_string()));
    }
    let response: Response = http
        .get(format!("{base}/torrents/imdb:{imdb_id}"))
        .bearer_auth(key)
        .query(&query)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(response.data.map(|d| d.torrents).unwrap_or_default().into_iter().filter_map(parse).collect())
}

fn parse(hit: Hit) -> Option<Torrent> {
    Some(Torrent {
        info_hash: super::valid_hash(hit.hash?)?,
        name: hit.raw_title.or(hit.title)?,
        size_bytes: hit.size.as_ref().and_then(super::number),
        seeders: hit.seeders.as_ref().and_then(super::number).map(|n| n as u32),
        origin: hit.tracker.or_else(|| Some("TorBox".into())),
        ..Torrent::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hits() {
        let json = r#"{"success":true,"data":{"torrents":[
            {"hash":"ABCDEF0123456789ABCDEF0123456789ABCDEF01","raw_title":"Movie.2020.1080p.WEB","size":2147483648,"seeders":"12","tracker":"1337x"},
            {"hash":"short","raw_title":"bad"},
            {"raw_title":"no hash"}
        ]}}"#;
        let r: Response = serde_json::from_str(json).unwrap();
        let t: Vec<_> = r.data.unwrap().torrents.into_iter().filter_map(parse).collect();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].info_hash, "abcdef0123456789abcdef0123456789abcdef01");
        assert_eq!(t[0].size_bytes, Some(1 << 31));
        assert_eq!(t[0].seeders, Some(12));
        assert_eq!(t[0].origin.as_deref(), Some("1337x"));
    }
}
