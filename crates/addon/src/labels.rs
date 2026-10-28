//! The user's own stream labels: templates with `{variables}`, where a part in
//! `[brackets]` is left out when a variable inside it is empty.

use std::collections::HashMap;

use crate::{
    debrid::Service,
    torrent::{format_size, release_group, resolution_label, Quality, Torrent},
};

/// Every variable, with an example, for the settings window.
pub const VARIABLES: &[(&str, &str)] = &[
    ("service", "RD or TB"),
    ("serviceName", "Real-Debrid or TorBox"),
    ("cached", "⚡ when cached"),
    ("resolution", "4K, 1080p…"),
    ("quality", "Remux, BluRay, WEB…"),
    ("hdr", "DV HDR10+"),
    ("codec", "HEVC"),
    ("audio", "Atmos TrueHD 7.1"),
    ("languages", "🇬🇧 🇩🇪"),
    ("size", "18.4 GB"),
    ("seeders", "120"),
    ("origin", "Tracker or site"),
    ("group", "Release group"),
    ("name", "Release name"),
    ("filename", "File, if it differs from the name"),
];

pub const DEFAULT_NAME: &str = "Matzio {service}{cached}\n{resolution}";
pub const DEFAULT_TITLE: &str = "{name}\n[{filename}]\n[💾 {size}] [👤 {seeders}] [⚙️ {origin}]";

fn join(tags: &[&str], label: fn(&str) -> &str) -> String {
    tags.iter().map(|t| label(t)).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ")
}

fn hdr_label(tag: &str) -> &str {
    match tag {
        "dv" => "DV",
        "hdr10plus" => "HDR10+",
        "hdr" => "HDR",
        _ => "",
    }
}

fn codec_label(tag: &str) -> &str {
    match tag {
        "av1" => "AV1",
        "hevc" => "HEVC",
        "avc" => "AVC",
        "xvid" => "XviD",
        _ => "",
    }
}

fn audio_label(tag: &str) -> &str {
    match tag {
        "atmos" => "Atmos",
        "truehd" => "TrueHD",
        "dts-x" => "DTS:X",
        "dts-hd" => "DTS-HD",
        "dts" => "DTS",
        "ddp" => "DD+",
        "dd" => "DD",
        "aac" => "AAC",
        "flac" => "FLAC",
        other => other,
    }
}

fn flag(code: &str) -> &str {
    match code {
        "en" => "🇬🇧", "de" => "🇩🇪", "fr" => "🇫🇷", "es" => "🇪🇸", "it" => "🇮🇹", "pt" => "🇵🇹",
        "nl" => "🇳🇱", "pl" => "🇵🇱", "ru" => "🇷🇺", "uk" => "🇺🇦", "cs" => "🇨🇿", "hu" => "🇭🇺",
        "tr" => "🇹🇷", "el" => "🇬🇷", "sv" => "🇸🇪", "da" => "🇩🇰", "no" => "🇳🇴", "fi" => "🇫🇮",
        "ar" => "🇸🇦", "he" => "🇮🇱", "hi" | "ta" | "te" => "🇮🇳", "ja" => "🇯🇵", "ko" => "🇰🇷",
        "zh" => "🇨🇳", "th" => "🇹🇭", "vi" => "🇻🇳", "multi" => "🌐",
        _ => "",
    }
}

/// The values for one stream.
pub fn variables(service: Service, cached: bool, t: &Torrent) -> HashMap<&'static str, String> {
    let dts_any = |tags: &mut Vec<&'static str>| {
        // "dts" also marks DTS-HD and DTS:X; show only the most specific one.
        if tags.iter().any(|t| *t == "dts-hd" || *t == "dts-x") {
            tags.retain(|t| *t != "dts");
        }
    };
    let mut audio = t.audio_tags();
    dts_any(&mut audio);
    let quality = match t.quality() {
        Quality::Remux => "Remux",
        Quality::BluRay => "BluRay",
        Quality::Web => "WEB",
        Quality::Tv => "HDTV",
        Quality::Screener => "SCR",
        Quality::Cam => "CAM",
        Quality::Unknown => "",
    };
    let filename = t.filename.clone().filter(|f| *f != t.name).unwrap_or_default();
    HashMap::from([
        ("service", service.short_name().to_owned()),
        ("serviceName", match service {
            Service::RealDebrid => "Real-Debrid".to_owned(),
            Service::TorBox => "TorBox".to_owned(),
        }),
        ("cached", if cached { "⚡".to_owned() } else { String::new() }),
        ("resolution", resolution_label(t.resolution()).to_owned()),
        ("quality", quality.to_owned()),
        ("hdr", join(&t.hdr_tags(), hdr_label)),
        ("codec", join(&t.codec_tags(), codec_label)),
        ("audio", join(&audio, audio_label)),
        ("languages", join(&t.languages(), flag)),
        ("size", t.size_bytes.map(format_size).unwrap_or_default()),
        ("seeders", t.seeders.map(|s| s.to_string()).unwrap_or_default()),
        ("origin", t.origin.clone().unwrap_or_default()),
        ("group", release_group(t.filename.as_deref().unwrap_or(&t.name)).or_else(|| release_group(&t.name)).unwrap_or_default().to_owned()),
        ("name", t.name.clone()),
        ("filename", filename),
    ])
}

/// Fills in `{variables}`. A `[part]` is dropped when any variable in it is empty; an unknown
/// `{word}` is kept as typed, so a typo shows up in the label. Empty lines are dropped.
pub fn render(template: &str, vars: &HashMap<&'static str, String>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('[') {
        out.push_str(&fill(&rest[..start], vars).0);
        let after = &rest[start + 1..];
        match after.find(']') {
            Some(end) => {
                let (text, all_set) = fill(&after[..end], vars);
                if all_set {
                    out.push_str(&text);
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(&fill(rest, vars).0);

    out.lines()
        .map(|line| line.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replaces variables in a piece of text; also says whether none of them was empty.
fn fill(text: &str, vars: &HashMap<&'static str, String>) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let mut all_set = true;
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}').and_then(|end| Some((end, vars.get(&after[..end])?))) {
            Some((end, value)) => {
                all_set &= !value.is_empty();
                out.push_str(value);
                rest = &after[end + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    (out, all_set)
}

/// A made-up stream to preview labels with.
pub fn sample() -> Torrent {
    Torrent {
        info_hash: String::new(),
        name: "Dune.Part.Two.2024.2160p.BluRay.REMUX.DV.HDR10.HEVC.TrueHD.Atmos.7.1-FraMeSToR".into(),
        filename: Some("Dune.Part.Two.2024.2160p.BluRay.REMUX.DV.HDR10.HEVC.TrueHD.Atmos.7.1-FraMeSToR.mkv".into()),
        size_bytes: Some(65_400_000_000),
        seeders: Some(120),
        origin: Some("TorrentGalaxy".into()),
        notes: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_variables_and_drops_empty_parts() {
        let mut t = sample();
        t.seeders = None;
        t.filename = Some(t.name.clone());
        let vars = variables(Service::RealDebrid, true, &t);
        assert_eq!(render(DEFAULT_NAME, &vars), "Matzio RD⚡\n4K");
        assert_eq!(
            render(DEFAULT_TITLE, &vars),
            "Dune.Part.Two.2024.2160p.BluRay.REMUX.DV.HDR10.HEVC.TrueHD.Atmos.7.1-FraMeSToR\n💾 60.91 GB ⚙️ TorrentGalaxy"
        );
        assert_eq!(
            render("{resolution} {hdr} [{quality}] · {codec} · {audio} {languages} [by {group}] {oops}", &vars),
            "4K DV HDR Remux · HEVC · Atmos TrueHD 7.1 🇬🇧 by FraMeSToR {oops}"
        );
        let uncached = variables(Service::TorBox, false, &t);
        assert_eq!(render("{serviceName}[ {cached}] [x] [unclosed", &uncached), "TorBox x [unclosed");
    }
}
