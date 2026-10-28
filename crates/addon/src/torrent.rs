//! Torrent results and the helpers for ranking them and picking the right file.

use std::sync::LazyLock;

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

    pub fn magnet(&self) -> String {
        format!("magnet:?xt=urn:btih:{}", self.info_hash)
    }
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
