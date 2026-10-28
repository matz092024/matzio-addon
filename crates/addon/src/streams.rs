//! Turns torrent results into the stream list Stremio shows in its picker.

use serde_json::{json, Value};

use crate::{
    debrid::Service,
    torrent::{format_size, resolution_label, Episode, Torrent},
};

/// Most results listed per debrid service.
pub const MAX_PER_SERVICE: usize = 25;

/// Best first: higher resolution, then more seeders.
pub fn rank(torrents: &mut [Torrent]) {
    torrents.sort_by(|a, b| b.resolution().cmp(&a.resolution()).then(b.seeders.cmp(&a.seeders)));
}

pub fn stream(base_url: &str, service: Service, cached: bool, torrent: &Torrent, episode: Option<Episode>) -> Value {
    let res = resolution_label(torrent.resolution());
    let badge = if cached { "⚡" } else { "" };

    let mut url = format!("{base_url}/play/{}/{}", service.slug(), torrent.info_hash);
    let mut query = Vec::new();
    if let Some(file) = &torrent.filename {
        query.push(format!("file={}", encode(file)));
    }
    if let Some(ep) = episode {
        query.push(format!("s={}&e={}", ep.season, ep.episode));
    }
    if !query.is_empty() {
        url = format!("{url}?{}", query.join("&"));
    }

    let mut details = Vec::new();
    if let Some(size) = torrent.size_bytes {
        details.push(format!("💾 {}", format_size(size)));
    }
    if let Some(seeders) = torrent.seeders {
        details.push(format!("👤 {seeders}"));
    }
    if let Some(origin) = &torrent.origin {
        details.push(format!("⚙️ {origin}"));
    }
    let mut title = torrent.name.clone();
    if let Some(file) = torrent.filename.as_ref().filter(|f| **f != torrent.name) {
        title.push('\n');
        title.push_str(file);
    }
    if !details.is_empty() {
        title.push('\n');
        title.push_str(&details.join("  "));
    }

    let mut hints = json!({ "bingeGroup": format!("matzio-{}-{res}", service.slug()) });
    if let Some(file) = &torrent.filename {
        hints["filename"] = json!(file);
    }
    json!({
        "name": format!("Matzio {}{badge}\n{res}", service.short_name()),
        "title": title,
        "url": url,
        "behaviorHints": hints,
    })
}

/// Shown when no debrid key is set yet, so the picker isn't silently empty.
pub fn setup_hint(base_url: &str) -> Value {
    json!({
        "name": "Matzio",
        "title": "⚙️ Add your Real-Debrid or TorBox key\nOpen Matzio from the tray to finish setup",
        "externalUrl": format!("{base_url}/"),
    })
}

/// Percent-encodes a query value.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torrent(name: &str, seeders: u32) -> Torrent {
        Torrent {
            info_hash: "abc".into(),
            name: name.into(),
            filename: None,
            size_bytes: Some(1 << 30),
            seeders: Some(seeders),
            origin: Some("YTS".into()),
        }
    }

    #[test]
    fn ranks_by_resolution_then_seeders() {
        let mut list = vec![torrent("A 720p", 500), torrent("B 2160p", 5), torrent("C 1080p", 50), torrent("D 1080p", 90)];
        rank(&mut list);
        let names: Vec<_> = list.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["B 2160p", "D 1080p", "C 1080p", "A 720p"]);
    }

    #[test]
    fn builds_play_url_with_file_and_episode() {
        let mut t = torrent("Show.S01.1080p", 10);
        t.filename = Some("Show S01E02 [1080p].mkv".into());
        let s = stream("http://127.0.0.1:7777", Service::TorBox, true, &t, Some(Episode { season: 1, episode: 2 }));
        assert_eq!(s["url"], "http://127.0.0.1:7777/play/torbox/abc?file=Show%20S01E02%20%5B1080p%5D.mkv&s=1&e=2");
        assert_eq!(s["name"], "Matzio TB⚡\n1080p");
        assert_eq!(s["title"], "Show.S01.1080p\nShow S01E02 [1080p].mkv\n💾 1.00 GB  👤 10  ⚙️ YTS");
        assert_eq!(s["behaviorHints"]["filename"], "Show S01E02 [1080p].mkv");
    }
}
