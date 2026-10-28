//! Torrent results and the helpers for ranking them and picking the right file.

use std::{sync::LazyLock, time::Duration};

use regex::Regex;

#[derive(Clone, Debug, PartialEq)]
pub struct Torrent {
    /// Lowercase hex info hash.
    pub info_hash: String,
    /// Release name as listed by the source.
    pub name: String,
    /// The file inside the torrent this result points at, if the source says.
    pub filename: Option<String>,
    pub size_bytes: Option<u64>,
    pub seeders: Option<u32>,
    /// Tracker or site the source found it on.
    pub origin: Option<String>,
}

impl Torrent {
    pub fn resolution(&self) -> u32 {
        resolution(&self.name).or_else(|| self.filename.as_deref().and_then(resolution)).unwrap_or(0)
    }

    pub fn quality(&self) -> Quality {
        quality(&self.name)
            .or_else(|| self.filename.as_deref().and_then(quality))
            .unwrap_or(Quality::Unknown)
    }

    pub fn language(&self) -> Language {
        match (language(&self.name), self.filename.as_deref().map(language)) {
            (Language::Foreign, _) | (_, Some(Language::Foreign)) => Language::Foreign,
            (Language::Multi, _) | (_, Some(Language::Multi)) => Language::Multi,
            _ => Language::English,
        }
    }

    pub fn is_excluded(&self, excludes: &[String]) -> bool {
        let res = self.resolution();
        let qual = self.quality();
        excludes.iter().any(|ex| match ex.to_ascii_lowercase().as_str() {
            "2160p" | "4k" => res == 2160,
            "1440p" => res == 1440,
            "1080p" => res == 1080,
            "720p" => res == 720,
            "480p" | "sd" => res <= 576,
            "cam" => qual == Quality::Cam,
            "scr" | "screener" => qual == Quality::Screener,
            _ => false,
        })
    }

    pub fn magnet(&self) -> String {
        format!("magnet:?xt=urn:btih:{}", self.info_hash)
    }
}

/// Release source tier, ordered from lowest to highest quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quality {
    Cam,
    Screener,
    Unknown,
    Tv,
    Web,
    BluRay,
    Remux,
}

/// Language tier for sorting streams (English preferred over multi-audio over foreign dubs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Language {
    Foreign,
    Multi,
    English,
}

/// Which season and episode a series request is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Episode {
    pub season: u32,
    pub episode: u32,
}

/// Splits a Stremio id: `tt123` for movies, `tt123:1:2` for an episode.
pub fn parse_id(id: &str) -> Option<(&str, Option<Episode>)> {
    let mut parts = id.split(':');
    let imdb = parts.next().filter(|p| p.starts_with("tt") && p.len() > 2)?;
    match (parts.next(), parts.next(), parts.next()) {
        (None, ..) => Some((imdb, None)),
        (Some(s), Some(e), None) => Some((imdb, Some(Episode { season: s.parse().ok()?, episode: e.parse().ok()? }))),
        _ => None,
    }
}

pub fn resolution(text: &str) -> Option<u32> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b(2160p|4k|uhd|1440p|1080p|720p|576p|480p)\b").unwrap());
    let m = RE.find(text)?.as_str().to_ascii_lowercase();
    Some(match m.as_str() {
        "2160p" | "4k" | "uhd" => 2160,
        other => other.trim_end_matches('p').parse().ok()?,
    })
}

pub fn resolution_label(res: u32) -> &'static str {
    match res {
        2160 => "4K",
        1440 => "1440p",
        1080 => "1080p",
        720 => "720p",
        576 => "576p",
        480 => "480p",
        _ => "SD",
    }
}

pub fn quality(text: &str) -> Option<Quality> {
    static REMUX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)(?:^|[^a-z0-9])(?:remux|bdremux|uhd[ ._-]?remux)(?:$|[^a-z0-9])").unwrap());
    static CAM: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)(?:^|[^a-z0-9])(?:cam|camrip|hdcam|telesync|hdts|pdvd|telecine|hdtc|workprint)(?:$|[^a-z0-9])",
        )
        .unwrap()
    });
    static SCR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:^|[^a-z0-9])(?:scr|screener|dvdscr|bdscr|hdscr)(?:$|[^a-z0-9])").unwrap()
    });
    static BLURAY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:^|[^a-z0-9])(?:bluray|blu[ ._-]?ray|bdrip|brrip|bdr)(?:$|[^a-z0-9])").unwrap()
    });
    static WEB: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)(?:^|[^a-z0-9])(?:web[ ._-]?dl|webrip|web)(?:$|[^a-z0-9])").unwrap());
    static TV: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)(?:^|[^a-z0-9])(?:hdtv|pdtv|dsr|dvdrip|dvd)(?:$|[^a-z0-9])").unwrap());

    if REMUX.is_match(text) {
        Some(Quality::Remux)
    } else if CAM.is_match(text) {
        Some(Quality::Cam)
    } else if SCR.is_match(text) {
        Some(Quality::Screener)
    } else if BLURAY.is_match(text) {
        Some(Quality::BluRay)
    } else if WEB.is_match(text) {
        Some(Quality::Web)
    } else if TV.is_match(text) {
        Some(Quality::Tv)
    } else {
        None
    }
}

pub fn language(text: &str) -> Language {
    static MULTI: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)(?:^|[^a-z0-9])(?:multi|dual(?:[ ._-]?audio)?)(?:$|[^a-z0-9])").unwrap());
    static ENGLISH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)(?:🇬🇧|🇺🇸|(?:^|[^a-z0-9])(?:eng|english)(?:$|[^a-z0-9]))").unwrap());
    static FOREIGN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)(?:🇫🇷|🇩🇪|🇪🇸|🇮🇹|🇷🇺|🇮🇳|🇵🇱|🇵🇹|🇧🇷|🇨🇿|🇹🇷|(?:^|[^a-z0-9])(?:french|fra|fre|vff|vfq|truefrench|german|ger|deu|spanish|spa|esp|latino|castellano|italian|ita|russian|rus|dubbed|hindi|hin|tamil|telugu|polish|pol|pldub|portuguese|por|czech|ces|turkish|tur|korean|kor|japanese|jpn)(?:$|[^a-z0-9]))",
        )
        .unwrap()
    });

    let has_multi = MULTI.is_match(text);
    let has_eng = ENGLISH.is_match(text);
    let has_foreign = FOREIGN.is_match(text);

    if has_multi || (has_eng && has_foreign) {
        Language::Multi
    } else if has_foreign {
        Language::Foreign
    } else {
        Language::English
    }
}

/// Whether a torrent of `size_bytes` can stream over `speed_mbps` within `runtime`.
pub fn fits_speed(size_bytes: Option<u64>, runtime: Duration, speed_mbps: f64) -> bool {
    let Some(bytes) = size_bytes else { return true };
    let secs = runtime.as_secs_f64();
    if secs <= 0.0 || speed_mbps <= 0.0 {
        return true;
    }
    let needed_mbps = (bytes as f64 * 8.0) / (secs * 1_000_000.0);
    needed_mbps <= speed_mbps
}

pub fn format_size(bytes: u64) -> String {
    let gb = bytes as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.2} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}

const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "avi", "m4v", "mov", "webm", "ts", "m2ts", "wmv", "mpg", "mpeg"];

fn is_video(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn matches_episode(name: &str, ep: Episode) -> bool {
    let re = Regex::new(&format!(
        r"(?i)(?:s0*{s}[ ._-]*e0*{e}(?:\D|$)|\b0*{s}x0*{e}(?:\D|$))",
        s = ep.season,
        e = ep.episode
    ))
    .unwrap();
    re.is_match(basename(name))
}

/// A file inside a torrent on a debrid service.
#[derive(Clone, Debug)]
pub struct DebridFile<Id> {
    pub id: Id,
    pub path: String,
    pub size: u64,
}

/// Picks the file to play: the exact filename the source named, else the video
/// matching the episode, else the largest video, else the largest file.
pub fn pick_file<'a, Id>(
    files: &'a [DebridFile<Id>],
    filename: Option<&str>,
    episode: Option<Episode>,
) -> Option<&'a DebridFile<Id>> {
    if let Some(wanted) = filename.map(basename) {
        if let Some(f) = files.iter().find(|f| basename(&f.path).eq_ignore_ascii_case(wanted)) {
            return Some(f);
        }
    }
    let videos = || files.iter().filter(|f| is_video(&f.path));
    if let Some(ep) = episode {
        if let Some(f) = videos().filter(|f| matches_episode(&f.path, ep)).max_by_key(|f| f.size) {
            return Some(f);
        }
    }
    videos().max_by_key(|f| f.size).or_else(|| files.iter().max_by_key(|f| f.size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ids() {
        assert_eq!(parse_id("tt0111161"), Some(("tt0111161", None)));
        assert_eq!(parse_id("tt0903747:2:10"), Some(("tt0903747", Some(Episode { season: 2, episode: 10 }))));
        assert_eq!(parse_id("kitsu:1"), None);
        assert_eq!(parse_id("tt1:2"), None);
        assert_eq!(parse_id("tt1:a:b"), None);
    }

    #[test]
    fn reads_resolution() {
        assert_eq!(resolution("Movie.2019.2160p.UHD.BluRay"), Some(2160));
        assert_eq!(resolution("Movie 4K HDR"), Some(2160));
        assert_eq!(resolution("Show.S01E01.720p.WEB"), Some(720));
        assert_eq!(resolution("Movie.DVDRip"), None);
    }

    #[test]
    fn reads_quality_and_language() {
        assert_eq!(quality("Dune.Part.Two.2024.2160p.BluRay.REMUX"), Some(Quality::Remux));
        assert_eq!(quality("Movie.2024.1080p.BluRay.x264"), Some(Quality::BluRay));
        assert_eq!(quality("Show.S01E01.1080p.WEB-DL"), Some(Quality::Web));
        assert_eq!(quality("Movie.2024.1080p.HDCAM"), Some(Quality::Cam));
        assert_eq!(quality("Movie.2024.DVDSCR.XviD"), Some(Quality::Screener));
        assert!(Quality::Remux > Quality::BluRay && Quality::BluRay > Quality::Web && Quality::Unknown > Quality::Cam);

        assert_eq!(language("Movie.2024.1080p.WEB"), Language::English);
        assert_eq!(language("Movie.2024.MULTi.1080p.BluRay"), Language::Multi);
        assert_eq!(language("Movie.2024.ITA.ENG.1080p"), Language::Multi);
        assert_eq!(language("Movie.2024.FRENCH.1080p.WEB"), Language::Foreign);
        assert!(Language::English > Language::Multi && Language::Multi > Language::Foreign);
    }

    #[test]
    fn checks_excludes_and_speed_limit() {
        let cam = Torrent {
            info_hash: "a".into(),
            name: "Movie.2024.1080p.HDCAM".into(),
            filename: None,
            size_bytes: None,
            seeders: None,
            origin: None,
        };
        assert!(cam.is_excluded(&["cam".into()]));
        assert!(cam.is_excluded(&["1080p".into()]));
        assert!(!cam.is_excluded(&["2160p".into(), "scr".into()]));

        // A 4 GB file needs ~4.8 Mbps over a 2-hour movie, but ~12.7 Mbps over a 45-min episode.
        let four_gb = Some(4u64 << 30);
        assert!(fits_speed(four_gb, Duration::from_secs(120 * 60), 10.0));
        assert!(!fits_speed(four_gb, Duration::from_secs(45 * 60), 10.0));
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(format_size(2_254_857_830), "2.10 GB");
        assert_eq!(format_size(734_003_200), "700 MB");
    }

    fn file(id: u32, path: &str, size: u64) -> DebridFile<u32> {
        DebridFile { id, path: path.into(), size }
    }

    #[test]
    fn picks_named_file_first() {
        let files = [file(1, "Pack/Show.S01E01.mkv", 900), file(2, "Pack/Show.S01E02.mkv", 800)];
        assert_eq!(pick_file(&files, Some("Show.S01E02.mkv"), None).unwrap().id, 2);
    }

    #[test]
    fn picks_episode_from_pack() {
        let files = [
            file(1, "Show/Season 1/Show.S01E01.1080p.mkv", 900),
            file(2, "Show/Season 1/Show.S01E10.1080p.mkv", 800),
            file(3, "Show/Season 1/Show.S01E11.1080p.mkv", 950),
            file(4, "Show/sample.mkv", 10),
        ];
        let ep = |e| Some(Episode { season: 1, episode: e });
        assert_eq!(pick_file(&files, None, ep(10)).unwrap().id, 2);
        assert_eq!(pick_file(&files, None, ep(1)).unwrap().id, 1);
        let alt = [file(1, "show.1x05.mkv", 5), file(2, "show.1x15.mkv", 6)];
        assert_eq!(pick_file(&alt, None, ep(5)).unwrap().id, 1);
    }

    #[test]
    fn falls_back_to_largest_video() {
        let files = [file(1, "movie.nfo", 5_000), file(2, "movie.mkv", 4_000), file(3, "sample.mkv", 10)];
        assert_eq!(pick_file(&files, None, None).unwrap().id, 2);
        assert_eq!(pick_file(&files, Some("missing.mkv"), None).unwrap().id, 2);
    }
}
