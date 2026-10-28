//! Zilean: an index of torrents known from Debrid Media Manager hash lists, so
//! its results are often already cached on debrid services.

use serde::Deserialize;
use serde_json::Value;

use crate::torrent::{Episode, Torrent};

#[derive(Deserialize)]
struct Hit {
    info_hash: Option<String>,
    raw_title: Option<String>,
    size: Option<Value>,
}

pub async fn search(
    http: &reqwest::Client,
    base: &str,
    imdb_id: &str,
    episode: Option<Episode>,
) -> Result<Vec<Torrent>, reqwest::Error> {
    let mut query = vec![("ImdbId", imdb_id.to_owned())];
    if let Some(ep) = episode {
        query.push(("Season", ep.season.to_string()));
        query.push(("Episode", ep.episode.to_string()));
    }
    let hits: Vec<Hit> = http
        .get(format!("{}/dmm/filtered", base.trim_end_matches('/')))
        .query(&query)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(hits.into_iter().filter_map(parse).collect())
}

fn parse(hit: Hit) -> Option<Torrent> {
    Some(Torrent {
        info_hash: super::valid_hash(hit.info_hash?)?,
        name: hit.raw_title?,
        size_bytes: hit.size.as_ref().and_then(super::number),
        origin: Some("DMM".into()),
        ..Torrent::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hits() {
        let json = r#"[{"info_hash":"abcdef0123456789abcdef0123456789abcdef01","raw_title":"Movie.2020.2160p.REMUX","size":"53687091200"},
                       {"info_hash":null,"raw_title":"x"}]"#;
        let hits: Vec<Hit> = serde_json::from_str(json).unwrap();
        let t: Vec<_> = hits.into_iter().filter_map(parse).collect();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].size_bytes, Some(50 << 30));
    }
}
