//! Knaben: a search engine over many public torrent sites. It searches by text,
//! so the title's name and year come from Cinemeta first.

use serde::Deserialize;
use serde_json::{json, Value};

use super::cinemeta::Title;
use crate::torrent::{Episode, Torrent};

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    hits: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    hash: Option<String>,
    title: Option<String>,
    bytes: Option<Value>,
    seeders: Option<Value>,
    tracker: Option<String>,
}

/// The text Knaben is asked for: "Name 2020" for movies, "Name S01E02" for episodes.
pub fn query(title: &Title, episode: Option<Episode>) -> String {
    match (episode, title.year) {
        (Some(ep), _) => format!("{} S{:02}E{:02}", title.name, ep.season, ep.episode),
        (None, Some(year)) => format!("{} {year}", title.name),
        (None, None) => title.name.clone(),
    }
}

pub async fn search(http: &reqwest::Client, base: &str, query: &str) -> Result<Vec<Torrent>, reqwest::Error> {
    let body = json!({
        "search_type": "100%",
        "search_field": "title",
        "query": query,
        "order_by": "seeders",
        "order_direction": "desc",
        "from": 0,
        "size": 50,
        "hide_unsafe": true,
        "hide_xxx": true,
    });
    let response: Response = http.post(base).json(&body).send().await?.error_for_status()?.json().await?;
    Ok(response.hits.into_iter().filter_map(parse).collect())
}

fn parse(hit: Hit) -> Option<Torrent> {
    Some(Torrent {
        info_hash: super::valid_hash(hit.hash?)?,
        name: hit.title?,
        size_bytes: hit.bytes.as_ref().and_then(super::number),
        seeders: hit.seeders.as_ref().and_then(super::number).map(|n| n as u32),
        origin: hit.tracker,
        ..Torrent::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_queries_and_parses_hits() {
        let t = Title { name: "Breaking Bad".into(), year: Some(2008) };
        assert_eq!(query(&t, Some(Episode { season: 1, episode: 2 })), "Breaking Bad S01E02");
        assert_eq!(query(&t, None), "Breaking Bad 2008");

        let r: Response = serde_json::from_str(
            r#"{"hits":[{"hash":"ABCDEF0123456789ABCDEF0123456789ABCDEF01","title":"Breaking.Bad.S01E02.720p","bytes":734003200,"seeders":40,"tracker":"EZTV"},
                        {"hash":null,"title":"magnet only"}]}"#,
        )
        .unwrap();
        let t: Vec<_> = r.hits.into_iter().filter_map(parse).collect();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].seeders, Some(40));
        assert_eq!(t[0].origin.as_deref(), Some("EZTV"));
    }
}
