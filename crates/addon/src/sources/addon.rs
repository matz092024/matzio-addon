//! Other Stremio addons used as torrent sources (Torrentio, Comet, MediaFusion),
//! queried without debrid options so they just list torrents.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use crate::torrent::{is_video_name, Torrent};

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    streams: Vec<Stream>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stream {
    info_hash: Option<String>,
    /// Older addons put the details here, newer ones in `description`.
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    behavior_hints: BehaviorHints,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BehaviorHints {
    filename: Option<String>,
    video_size: Option<u64>,
}

/// `base` is the addon's address, with or without its configuration part or `/manifest.json`.
pub async fn search(http: &reqwest::Client, base: &str, kind: &str, id: &str) -> Result<Vec<Torrent>, reqwest::Error> {
    let base = base.trim().trim_end_matches('/').trim_end_matches("/manifest.json").replacen("stremio://", "https://", 1);
    let url = format!("{base}/stream/{kind}/{id}.json");
    let response: Response = http.get(url).send().await?.error_for_status()?.json().await?;
    Ok(response.streams.into_iter().filter_map(parse).collect())
}

fn parse(stream: Stream) -> Option<Torrent> {
    static SEEDERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"👤\s*(\d+)").unwrap());
    static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"💾\s*([\d.]+)\s*(TB|GB|MB|KB)").unwrap());
    static ORIGIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:⚙️|🔎|🌐|🔗)\s*([^👤💾⚙🔎🌐🔗]+?)\s*$").unwrap());

    let info_hash = super::valid_hash(stream.info_hash?)?;
    // Torrentio: "<release name>\n[<file in pack>\n]👤 seeders 💾 size ⚙️ origin[\n<languages>]".
    // Others use the same emojis, sometimes with an icon before the name.
    let text = if stream.description.trim().is_empty() { &stream.title } else { &stream.description };
    let clean = |line: &str| line.trim().trim_start_matches(|c: char| !c.is_alphanumeric() && c != '[' && c != '(').trim().to_owned();
    let mut lines = text.lines();
    let mut name = clean(lines.next().unwrap_or_default());
    let stats = text.lines().find(|l| l.contains('👤') || l.contains('💾')).unwrap_or_default();
    if name.is_empty() || stats.contains(&name) {
        name = stream.behavior_hints.filename.clone().unwrap_or_else(|| stream.name.replace('\n', " "));
    }
    let seeders = SEEDERS.captures(stats).and_then(|c| c[1].parse().ok());
    let size_bytes = stream.behavior_hints.video_size.filter(|&s| s > 0).or_else(|| SIZE.captures(stats).and_then(|c| {
        let value: f64 = c[1].parse().ok()?;
        let unit = match &c[2] {
            "TB" => 1u64 << 40,
            "GB" => 1 << 30,
            "MB" => 1 << 20,
            _ => 1 << 10,
        };
        Some((value * unit as f64) as u64)
    }));
    let origin = ORIGIN.captures(stats).map(|c| c[1].to_owned());
    // In season packs the line between the name and the stats is the episode's file.
    let filename = stream.behavior_hints.filename.or_else(|| {
        let file = lines.next()?;
        let file = (!file.contains('👤') && !file.contains('💾')).then(|| clean(file))?;
        (!file.is_empty() && is_video_name(&file)).then_some(file)
    });
    // Every other line, such as Torrentio's "Multi Audio / 🇬🇧 / 🇮🇹" or Comet's "📹 HEVC • DV".
    let notes = text
        .lines()
        .skip(1)
        .filter(|l| *l != stats && filename.as_deref() != Some(clean(l).as_str()) && !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let notes = (!notes.is_empty()).then_some(notes);
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

    #[test]
    fn parses_description_style_addons() {
        let json = r#"{"streams":[
            {"name":"[TORRENT] Comet 2160p","description":"📄 Movie.2020.2160p.WEB.DV\n📹 HEVC • DV\n💾 15.2 GB 👤 12 🔎 RARBG",
             "infoHash":"ABCDEF0123456789ABCDEF0123456789ABCDEF01","behaviorHints":{"videoSize":16320875724}}
        ]}"#;
        let response: Response = serde_json::from_str(json).unwrap();
        let t: Vec<_> = response.streams.into_iter().filter_map(parse).collect();
        assert_eq!(t[0].name, "Movie.2020.2160p.WEB.DV");
        assert_eq!(t[0].filename, None);
        assert_eq!(t[0].hdr_tags(), ["dv", "dv-only"]);
        assert_eq!(t[0].size_bytes, Some(16_320_875_724));
        assert_eq!(t[0].seeders, Some(12));
        assert_eq!(t[0].origin.as_deref(), Some("RARBG"));
    }
}
