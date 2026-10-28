//! Turns torrent results into the stream list Stremio shows in its picker.

use std::cmp::Ordering;

use serde_json::{json, Value};

use crate::{
    config::Preferences,
    debrid::Service,
    filters,
    torrent::{format_size, resolution_label, Episode, Torrent},
};

/// Most results listed per debrid service.
pub const MAX_PER_SERVICE: usize = 25;

/// Best first using the default preferences.
pub fn rank(torrents: &mut [Torrent]) {
    rank_by(torrents, &Preferences::default());
}

/// Ranks torrents according to the user's ordered sort keys.
pub fn rank_by(torrents: &mut [Torrent], prefs: &Preferences) {
    torrents.sort_by(|a, b| compare(false, a, false, b, prefs));
}

/// Compares two stream candidates using the user's ordered sort keys.
pub fn compare(a_cached: bool, a: &Torrent, b_cached: bool, b: &Torrent, prefs: &Preferences) -> Ordering {
    for key in &prefs.sort_order {
        let ord = match key.as_str() {
            "cached" => b_cached.cmp(&a_cached),
            "resolution" => b.resolution().cmp(&a.resolution()),
            // English, then multi-audio, then dubs, unless the user picked languages.
            "language" => filters::compare_preference(key, a, b, &prefs.filters)
                .unwrap_or_else(|| b.language().cmp(&a.language())),
            "hdr" | "codec" | "audio" | "keywords" => {
                filters::compare_preference(key, a, b, &prefs.filters).unwrap_or(Ordering::Equal)
            }
            "size" => b.size_bytes.unwrap_or(0).cmp(&a.size_bytes.unwrap_or(0)),
            "quality" => b.quality().cmp(&a.quality()),
            "seeders" => b.seeders.unwrap_or(0).cmp(&a.seeders.unwrap_or(0)),
            _ => Ordering::Equal,
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    b.resolution()
        .cmp(&a.resolution())
        .then_with(|| b.seeders.unwrap_or(0).cmp(&a.seeders.unwrap_or(0)))
}

pub fn stream(base_url: &str, service: Service, cached: bool, torrent: &Torrent, episode: Option<Episode>) -> Value {
    stream_with_style(base_url, service, cached, torrent, episode, "detailed")
}

pub fn stream_with_style(
    base_url: &str,
    service: Service,
    cached: bool,
    torrent: &Torrent,
    episode: Option<Episode>,
    display_style: &str,
) -> Value {
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

    let (name, title) = if display_style == "compact" {
        let tag = if cached {
            format!("{} ⚡", service.short_name())
        } else {
            service.short_name().to_owned()
        };
        let title = match torrent.size_bytes {
            Some(size) => format!("{} · {tag}", format_size(size)),
            None => tag,
        };
        (format!("Matzio\n{res}"), title)
    } else {
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
        (format!("Matzio {}{badge}\n{res}", service.short_name()), title)
    };

    let mut hints = json!({ "bingeGroup": format!("matzio-{}-{res}", service.slug()) });
    if let Some(file) = &torrent.filename {
        hints["filename"] = json!(file);
    }
    json!({
        "name": name,
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
            size_bytes: Some(1 << 30),
            seeders: Some(seeders),
            origin: Some("YTS".into()),
            ..Torrent::default()
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
    fn ranks_by_custom_sort_order() {
        let mut list = vec![
            torrent("French 2160p FRENCH BluRay", 500),
            torrent("Web 1080p WEB-DL", 100),
            torrent("Remux 1080p BluRay REMUX", 10),
        ];
        let prefs = Preferences {
            sort_order: vec!["language".into(), "quality".into(), "resolution".into()],
            ..Preferences::default()
        };
        rank_by(&mut list, &prefs);
        let names: Vec<_> = list.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Remux 1080p BluRay REMUX", "Web 1080p WEB-DL", "French 2160p FRENCH BluRay"]);
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

        let compact = stream_with_style("http://127.0.0.1:7777", Service::TorBox, true, &t, None, "compact");
        assert_eq!(compact["name"], "Matzio\n1080p");
        assert_eq!(compact["title"], "1.00 GB · TB ⚡");
    }
}
