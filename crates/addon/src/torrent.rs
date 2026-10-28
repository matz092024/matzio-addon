//! Torrent results and the helpers for ranking them and picking the right file.

use std::{sync::LazyLock, time::Duration};

use regex::Regex;

#[derive(Clone, Debug, Default, PartialEq)]
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
    /// Anything else the source shows, such as Torrentio's language flags.
    pub notes: Option<String>,
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

    /// Name, file and notes together, for tag detection.
    fn text(&self) -> String {
        [Some(self.name.as_str()), self.filename.as_deref(), self.notes.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" \n ")
    }

    /// Resolution and source tags: "2160p", "1440p", "1080p", "720p" or "480p" (also unknown),
    /// plus "cam", "scr", "tv", "web", "bluray" or "remux" when the source is known.
    pub fn quality_tags(&self) -> Vec<&'static str> {
        let mut tags = vec![resolution_key(self.resolution())];
        tags.extend(match self.quality() {
            Quality::Cam => Some("cam"),
            Quality::Screener => Some("scr"),
            Quality::Tv => Some("tv"),
            Quality::Web => Some("web"),
            Quality::BluRay => Some("bluray"),
            Quality::Remux => Some("remux"),
            Quality::Unknown => None,
        });
        tags
    }

    /// Spoken languages as codes like "en" or "de", plus "multi" for multi-audio releases.
    /// A release that names no language is taken to be English.
    pub fn languages(&self) -> Vec<&'static str> {
        let mut found = languages(&self.text());
        if found.is_empty() || found == ["multi"] {
            found.push("en");
        }
        found
    }

    pub fn hdr_tags(&self) -> Vec<&'static str> {
        hdr_tags(&self.text())
    }

    pub fn codec_tags(&self) -> Vec<&'static str> {
        codec_tags(&self.text())
    }

    pub fn audio_tags(&self) -> Vec<&'static str> {
        audio_tags(&self.text())
    }

    /// The first of `words` found in the name or file (whole words, any case; `/.../` is a regex).
    pub fn first_keyword(&self, words: &[String]) -> Option<usize> {
        let text = [Some(self.name.as_str()), self.filename.as_deref()].into_iter().flatten().collect::<Vec<_>>().join("\n");
        words.iter().position(|w| keyword_matches(&text, w))
    }

    /// For a series: whether this is clearly a different episode (named, but not ours).
    pub fn is_other_episode(&self, ep: Episode) -> bool {
        static ANY_EPISODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)s\d{1,2}[ ._-]*e\d{1,3}(?:\D|$)").unwrap());
        ANY_EPISODE.is_match(&self.name) && !matches_episode(&self.name, ep)
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

/// Key for a resolution in quality tags and per-resolution size limits.
pub fn resolution_key(res: u32) -> &'static str {
    match res {
        2160.. => "2160p",
        1440.. => "1440p",
        1080.. => "1080p",
        720.. => "720p",
        _ => "480p",
    }
}

/// Older settings and the UI use a few other names for the same tags.
pub fn normalize_tag(tag: &str) -> String {
    match tag.to_ascii_lowercase().as_str() {
        "4k" | "uhd" => "2160p".into(),
        "sd" => "480p".into(),
        "screener" => "scr".into(),
        other => other.to_owned(),
    }
}

/// Words that mark each language, and the flags Torrentio shows for it.
const LANGUAGES: &[(&str, &str, &str)] = &[
    ("en", "eng|english", "🇬🇧|🇺🇸"),
    ("de", "ger|german|deu|deutsch", "🇩🇪"),
    ("fr", "fre|fra|french|truefrench|vff|vfq|vf2", "🇫🇷"),
    ("es", "spa|spanish|esp|castellano|latino", "🇪🇸|🇲🇽"),
    ("it", "ita|italian", "🇮🇹"),
    ("pt", "por|portuguese|dublado|ptbr|pt-br", "🇵🇹|🇧🇷"),
    ("nl", "dut|dutch|nld|flemish", "🇳🇱"),
    ("pl", "pol|polish|pldub|lektor", "🇵🇱"),
    ("ru", "rus|russian", "🇷🇺"),
    ("uk", "ukr|ukrainian", "🇺🇦"),
    ("cs", "cze|ces|czech", "🇨🇿"),
    ("hu", "hun|hungarian", "🇭🇺"),
    ("tr", "tur|turkish", "🇹🇷"),
    ("el", "gre|greek", "🇬🇷"),
    ("sv", "swe|swedish", "🇸🇪"),
    ("da", "danish", "🇩🇰"),
    ("no", "nor|norwegian", "🇳🇴"),
    ("fi", "finnish", "🇫🇮"),
    ("ar", "ara|arabic", "🇸🇦|🇦🇪|🇪🇬"),
    ("he", "heb|hebrew", "🇮🇱"),
    ("hi", "hin|hindi", "🇮🇳"),
    ("ta", "tam|tamil", ""),
    ("te", "tel|telugu", ""),
    ("ja", "jpn|jap|japanese", "🇯🇵"),
    ("ko", "kor|korean", "🇰🇷"),
    ("zh", "chi|chinese|mandarin|cantonese", "🇨🇳|🇹🇼|🇭🇰"),
    ("th", "thai", "🇹🇭"),
    ("vi", "vie|vietnamese", "🇻🇳"),
    ("multi", r"multi|multi[ ._-]?audio|dual(?:[ ._-]?audio)?", ""),
];

/// Language codes named in a release, in the order of [`LANGUAGES`].
pub fn languages(text: &str) -> Vec<&'static str> {
    static RES: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
        LANGUAGES
            .iter()
            .map(|(code, words, flags)| {
                let words = format!(r"(?:^|[^a-z0-9])(?:{words})(?:$|[^a-z0-9])");
                let pattern = if flags.is_empty() { format!("(?i){words}") } else { format!("(?i)(?:{flags}|{words})") };
                (*code, Regex::new(&pattern).unwrap())
            })
            .collect()
    });
    RES.iter().filter(|(_, re)| re.is_match(text)).map(|(code, _)| *code).collect()
}

/// Matches `pattern` as a case-insensitive word in `text`, without letters or digits on either side.
fn word(pattern: &str) -> Regex {
    Regex::new(&format!(r"(?i)(?:^|[^a-z0-9])(?:{pattern})(?:$|[^a-z0-9])")).unwrap()
}

/// "dv", "dv-only" (Dolby Vision with no HDR10 fallback), "hdr10plus", "hdr" or "sdr".
pub fn hdr_tags(text: &str) -> Vec<&'static str> {
    static DV: LazyLock<Regex> = LazyLock::new(|| word(r"dv|dovi|dolby[ ._-]?vision"));
    static PLUS: LazyLock<Regex> = LazyLock::new(|| word(r"hdr10(?:\+|plus|p)"));
    static HDR: LazyLock<Regex> = LazyLock::new(|| word(r"hdr(?:10)?(?:\+|plus|p)?"));
    let (dv, plus, hdr) = (DV.is_match(text), PLUS.is_match(text), HDR.is_match(text));
    let mut tags = Vec::new();
    if dv {
        tags.push("dv");
        if !hdr {
            tags.push("dv-only");
        }
    }
    if plus {
        tags.push("hdr10plus");
    }
    if hdr {
        tags.push("hdr");
    }
    if tags.is_empty() {
        tags.push("sdr");
    }
    tags
}

/// "av1", "hevc", "avc" or "xvid".
pub fn codec_tags(text: &str) -> Vec<&'static str> {
    static CODECS: LazyLock<[(&str, Regex); 4]> = LazyLock::new(|| {
        [
            ("av1", word("av1")),
            ("hevc", word(r"hevc|[xh][ .]?265")),
            ("avc", word(r"avc|[xh][ .]?264")),
            ("xvid", word("xvid|divx")),
        ]
    });
    CODECS.iter().filter(|(_, re)| re.is_match(text)).map(|(tag, _)| *tag).collect()
}

/// Audio formats ("atmos", "truehd", "dts-x", "dts-hd", "dts" for any DTS, "ddp", "dd", "aac",
/// "flac") and channel layouts ("7.1", "5.1", "2.0").
pub fn audio_tags(text: &str) -> Vec<&'static str> {
    // Release names often glue the channels on, as in "DDP5.1".
    const CH: &str = r"(?:[ ._-]?[257][ .][01])?";
    static AUDIO: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
        let channels = |c: &str| Regex::new(&format!(r"(?:^|[^0-9])(?:{c})(?:$|[^0-9])")).unwrap();
        vec![
            ("atmos", word("atmos")),
            ("truehd", word(&format!(r"true[ ._-]?hd{CH}"))),
            ("dts-x", word(&format!(r"dts[ ._-]?x{CH}"))),
            ("dts-hd", word(&format!(r"dts[ ._-]?hd(?:[ ._-]?ma)?{CH}|dts[ ._-]?ma{CH}"))),
            ("dts", word(&format!(r"dts(?:[ ._-]?(?:x|hd|ma|es))*{CH}"))),
            ("ddp", word(&format!(r"ddp{CH}|dd\+{CH}|e[ ._-]?ac[ ._-]?3{CH}"))),
            ("dd", word(&format!(r"dd{CH}|ac3{CH}|dolby[ ._-]?digital"))),
            ("aac", word(&format!(r"aac{CH}"))),
            ("flac", word(&format!(r"flac{CH}"))),
            ("7.1", channels(r"7[ .]1")),
            ("5.1", channels(r"5[ .]1")),
            ("2.0", channels(r"2[ .]0")),
        ]
    });
    let mut tags: Vec<_> = AUDIO.iter().filter(|(_, re)| re.is_match(text)).map(|(tag, _)| *tag).collect();
    // "DD+" also reads as plain "DD" followed by a symbol.
    if tags.contains(&"ddp") {
        tags.retain(|t| *t != "dd");
    }
    tags
}

/// A plain word or phrase matches whole words in any case; `/pattern/` is a regex.
pub fn keyword_matches(text: &str, keyword: &str) -> bool {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return false;
    }
    if let Some(pattern) = keyword.strip_prefix('/').and_then(|k| k.strip_suffix('/')).filter(|p| !p.is_empty()) {
        return Regex::new(&format!("(?i){pattern}")).is_ok_and(|re| re.is_match(text));
    }
    // Dots, dashes and underscores in release names count as spaces.
    let parts: Vec<_> = keyword.split(|c: char| c.is_whitespace() || ".-_".contains(c)).filter(|p| !p.is_empty()).map(regex::escape).collect();
    word(&parts.join(r"[ ._-]+")).is_match(text)
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

pub fn is_video_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Whether a file or release name is this episode, as in "S01E02" or "1x02".
pub fn matches_episode(name: &str, ep: Episode) -> bool {
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
    let videos = || files.iter().filter(|f| is_video_name(&f.path));
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
        let cam = Torrent { name: "Movie.2024.1080p.HDCAM".into(), ..Torrent::default() };
        assert_eq!(cam.quality_tags(), ["1080p", "cam"]);
        let unknown = Torrent { name: "Movie.2024.DVDRip".into(), ..Torrent::default() };
        assert_eq!(unknown.quality_tags(), ["480p", "tv"]);

        // A 4 GB file needs ~4.8 Mbps over a 2-hour movie, but ~12.7 Mbps over a 45-min episode.
        let four_gb = Some(4u64 << 30);
        assert!(fits_speed(four_gb, Duration::from_secs(120 * 60), 10.0));
        assert!(!fits_speed(four_gb, Duration::from_secs(45 * 60), 10.0));
    }

    #[test]
    fn reads_languages_from_names_and_flags() {
        let t = |name: &str, notes: Option<&str>| Torrent {
            name: name.into(),
            notes: notes.map(Into::into),
            ..Torrent::default()
        };
        assert_eq!(t("Movie.2024.1080p.WEB", None).languages(), ["en"]);
        assert_eq!(t("Movie.2024.German.DL.1080p", None).languages(), ["de"]);
        assert_eq!(t("Movie.2024.MULTi.1080p", None).languages(), ["multi", "en"]);
        assert_eq!(t("Movie.2024.1080p", Some("Multi Audio / 🇬🇧 / 🇮🇹")).languages(), ["en", "it", "multi"]);
        assert_eq!(t("Film.2024.TRUEFRENCH.1080p", None).languages(), ["fr"]);
        // Tags inside other words don't count.
        assert_eq!(t("Spanish.Prisoner.Italian.Job.Gerry.1080p", None).languages(), ["es", "it"]);
    }

    #[test]
    fn reads_hdr_codec_and_audio() {
        assert_eq!(hdr_tags("Movie.2160p.DV.HDR10.HEVC"), ["dv", "hdr"]);
        assert_eq!(hdr_tags("Movie.2160p.DoVi.WEB"), ["dv", "dv-only"]);
        assert_eq!(hdr_tags("Movie.2160p.HDR10+.WEB"), ["hdr10plus", "hdr"]);
        assert_eq!(hdr_tags("Movie.1080p.WEB.DVDRip"), ["sdr"]);

        assert_eq!(codec_tags("Movie.1080p.x265-GRP"), ["hevc"]);
        assert_eq!(codec_tags("Movie.1080p.H.264"), ["avc"]);
        assert_eq!(codec_tags("Movie.2160p.AV1.Opus"), ["av1"]);

        assert_eq!(audio_tags("Movie.2160p.TrueHD.7.1.Atmos"), ["atmos", "truehd", "7.1"]);
        assert_eq!(audio_tags("Movie.1080p.WEB.DDP5.1.x264"), ["ddp", "5.1"]);
        assert_eq!(audio_tags("Movie.1080p.DD+5.1"), ["ddp", "5.1"]);
        assert_eq!(audio_tags("Movie.1080p.DTS-HD.MA.5.1"), ["dts-hd", "dts", "5.1"]);
        assert_eq!(audio_tags("Movie.2020.1080p.AAC2.0"), ["aac", "2.0"]);
    }

    #[test]
    fn matches_keywords() {
        assert!(keyword_matches("Movie.2024.1080p.HC.WEB", "hc"));
        assert!(!keyword_matches("Movie.2024.1080p.CHC.WEB", "hc"));
        assert!(keyword_matches("Movie.2024.1080p.WEB-DL-FLUX", "flux"));
        assert!(keyword_matches("Movie 2024 Directors Cut 1080p", "Directors.Cut"));
        assert!(keyword_matches("Movie.2024.3D.HSBS", "/h?sbs/"));
        assert!(!keyword_matches("Movie", "/[/"));
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
