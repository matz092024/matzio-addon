//! Torrentio, queried without any debrid options so it just lists torrents.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use crate::torrent::Torrent;

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    streams: Vec<Stream>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stream {
    info_hash: Option<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    behavior_hints: BehaviorHints,
}

#[derive(Default, Deserialize)]
struct BehaviorHints {
    filename: Option<String>,
}

pub async fn search(http: &reqwest::Client, base: &str, kind: &str, id: &str) -> Result<Vec<Torrent>, reqwest::Error> {
    let url = format!("{base}/stream/{kind}/{id}.json");
    let response: Response = http.get(url).send().await?.error_for_status()?.json().await?;
    Ok(response.streams.into_iter().filter_map(parse).collect())
}

fn parse(stream: Stream) -> Option<Torrent> {
    static SEEDERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"👤\s*(\d+)").unwrap());
    static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"💾\s*([\d.]+)\s*(TB|GB|MB|KB)").unwrap());
    static ORIGIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"⚙️\s*(.+?)\s*$").unwrap());

    let info_hash = stream.info_hash?.to_ascii_lowercase();
    // The title is "<release name>\n[<file in pack>\n]👤 seeders 💾 size ⚙️ origin".
    let mut lines = stream.title.lines();
    let mut name = lines.next().unwrap_or_default().trim().to_owned();
    let stats = stream.title.lines().find(|l| l.contains('👤') || l.contains('💾')).unwrap_or_default();
    if name.is_empty() || name == stats {
        name = stream.behavior_hints.filename.clone().unwrap_or_else(|| stream.name.replace('\n', " "));
    }
    let seeders = SEEDERS.captures(stats).and_then(|c| c[1].parse().ok());
    let size_bytes = SIZE.captures(stats).and_then(|c| {
        let value: f64 = c[1].parse().ok()?;
        let unit = match &c[2] {
            "TB" => 1u64 << 40,
            "GB" => 1 << 30,
            "MB" => 1 << 20,
            _ => 1 << 10,
        };
        Some((value * unit as f64) as u64)
    });
    let origin = ORIGIN.captures(stats).map(|c| c[1].to_owned());
    // In season packs the line between the name and the stats is the episode's file.
    let filename = stream.behavior_hints.filename.or_else(|| {
        let file = lines.next()?.trim();
        (!file.is_empty() && file != stats).then(|| file.to_owned())
    });
    // Lines after the stats, such as "Multi Audio / 🇬🇧 / 🇮🇹".
    let notes = stream.title.lines().skip_while(|l| *l != stats).skip(1).collect::<Vec<_>>().join("\n");
    let notes = (!stats.is_empty() && !notes.trim().is_empty()).then_some(notes);
    Some(Torrent { info_hash, name, filename, size_bytes, seeders, origin, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_torrentio_streams() {
        let json = r#"{"streams":[
            {"name":"Torrentio\n1080p","title":"The.Movie.2019.1080p.BluRay.x264\n👤 152 💾 2.1 GB ⚙️ YTS\nMulti Audio / 🇬🇧 / 🇮🇹",
             "infoHash":"ABCDEF0123456789ABCDEF0123456789ABCDEF01","fileIdx":0,
             "behaviorHints":{"bingeGroup":"x","filename":"The.Movie.2019.1080p.BluRay.x264.mp4"}},
            {"name":"Torrentio\n720p","title":"Show.S01.720p.WEB\nShow.S01E02.720p.WEB.mkv\n👤 9 💾 700 MB ⚙️ EZTV",
             "infoHash":"1111111111111111111111111111111111111111","fileIdx":1},
            {"name":"Torrentio","title":"no hash here"}
        ]}"#;
        let response: Response = serde_json::from_str(json).unwrap();
        let torrents: Vec<_> = response.streams.into_iter().filter_map(parse).collect();
        assert_eq!(torrents.len(), 2);

        let movie = &torrents[0];
        assert_eq!(movie.info_hash, "abcdef0123456789abcdef0123456789abcdef01");
        assert_eq!(movie.name, "The.Movie.2019.1080p.BluRay.x264");
        assert_eq!(movie.seeders, Some(152));
        assert_eq!(movie.size_bytes, Some((2.1 * (1u64 << 30) as f64) as u64));
        assert_eq!(movie.origin.as_deref(), Some("YTS"));
        assert_eq!(movie.resolution(), 1080);
        assert_eq!(movie.notes.as_deref(), Some("Multi Audio / 🇬🇧 / 🇮🇹"));
        assert_eq!(movie.languages(), ["en", "it", "multi"]);

        let episode = &torrents[1];
        assert_eq!(episode.name, "Show.S01.720p.WEB");
        assert_eq!(episode.size_bytes, Some(700 << 20));
        assert_eq!(episode.origin.as_deref(), Some("EZTV"));
        assert_eq!(episode.filename.as_deref(), Some("Show.S01E02.720p.WEB.mkv"));
        assert_eq!(episode.notes, None);
    }
}
