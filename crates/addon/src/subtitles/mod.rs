//! Subtitles from OpenSubtitles and SubDL, offered through Stremio's subtitles resource.
//!
//! Each user brings their own free API key. Searching is cheap, but downloads count
//! against a daily limit, so a subtitle is only downloaded when Stremio opens it, and kept
//! for a few hours in case the player asks again.

pub mod opensubtitles;
pub mod subdl;

use std::{
    collections::HashMap,
    fmt,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde_json::{json, Value};

use crate::{
    config::{Placement, SubtitleKeys, Subtitles},
    torrent::{self, Episode, Torrent},
    Endpoints,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    OpenSubtitles,
    Subdl,
}

impl Provider {
    pub fn slug(self) -> &'static str {
        match self {
            Provider::OpenSubtitles => "opensubtitles",
            Provider::Subdl => "subdl",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "opensubtitles" => Some(Provider::OpenSubtitles),
            "subdl" => Some(Provider::Subdl),
            _ => None,
        }
    }

    fn short_name(self) -> &'static str {
        match self {
            Provider::OpenSubtitles => "os",
            Provider::Subdl => "subdl",
        }
    }
}

/// One subtitle file a provider has for the title.
#[derive(Clone, Debug, PartialEq)]
pub struct Subtitle {
    pub provider: Provider,
    /// The provider's id for the file: a number for OpenSubtitles, an archive name for SubDL.
    pub file: String,
    pub lang: &'static Lang,
    /// The release it was made for, used to find the one that is in sync.
    pub release: String,
    pub hearing_impaired: bool,
    pub machine_translated: bool,
    pub downloads: u64,
    /// OpenSubtitles matched it to the exact video by its hash.
    pub hash_match: bool,
}

/// A subtitle language and its code at Stremio and at each provider.
#[derive(Debug, PartialEq)]
pub struct Lang {
    /// Matzio's code, as saved in settings.
    pub code: &'static str,
    /// ISO 639-2 code, which Stremio shows as the language's name.
    pub stremio: &'static str,
    pub opensubtitles: &'static [&'static str],
    pub subdl: &'static str,
}

macro_rules! langs {
    ($(($code:literal, $stremio:literal, [$($os:literal),+], $subdl:literal)),+ $(,)?) => {
        &[$(Lang { code: $code, stremio: $stremio, opensubtitles: &[$($os),+], subdl: $subdl }),+]
    };
}

/// Every language the settings window offers.
pub const LANGUAGES: &[Lang] = langs![
    ("en", "eng", ["en"], "EN"),
    ("de", "ger", ["de"], "DE"),
    ("fr", "fre", ["fr"], "FR"),
    ("es", "spa", ["es", "ea"], "ES"),
    ("it", "ita", ["it"], "IT"),
    ("pt", "por", ["pt-pt"], "PT"),
    ("pt-br", "pob", ["pt-br"], "BR_PT"),
    ("nl", "dut", ["nl"], "NL"),
    ("pl", "pol", ["pl"], "PL"),
    ("ru", "rus", ["ru"], "RU"),
    ("uk", "ukr", ["uk"], "UK"),
    ("cs", "cze", ["cs"], "CS"),
    ("sk", "slo", ["sk"], "SK"),
    ("hu", "hun", ["hu"], "HU"),
    ("ro", "rum", ["ro"], "RO"),
    ("bg", "bul", ["bg"], "BG"),
    ("hr", "hrv", ["hr"], "HR"),
    ("sr", "srp", ["sr"], "SR"),
    ("sl", "slv", ["sl"], "SL"),
    ("tr", "tur", ["tr"], "TR"),
    ("el", "gre", ["el"], "EL"),
    ("sv", "swe", ["sv"], "SV"),
    ("da", "dan", ["da"], "DA"),
    ("no", "nor", ["no"], "NO"),
    ("fi", "fin", ["fi"], "FI"),
    ("et", "est", ["et"], "ET"),
    ("lv", "lav", ["lv"], "LV"),
    ("lt", "lit", ["lt"], "LT"),
    ("ar", "ara", ["ar"], "AR"),
    ("he", "heb", ["he"], "HE"),
    ("fa", "per", ["fa"], "FA"),
    ("hi", "hin", ["hi"], "HI"),
    ("ta", "tam", ["ta"], "TA"),
    ("te", "tel", ["te"], "TE"),
    ("ja", "jpn", ["ja"], "JA"),
    ("ko", "kor", ["ko"], "KO"),
    ("zh", "chi", ["zh-cn", "zh-tw"], "ZH"),
    ("th", "tha", ["th"], "TH"),
    ("vi", "vie", ["vi"], "VI"),
    ("id", "ind", ["id"], "ID"),
    ("ms", "may", ["ms"], "MS"),
];

/// The user's languages in their order, leaving out unknown codes and repeats.
pub fn languages(codes: &[String]) -> Vec<&'static Lang> {
    let mut found: Vec<&'static Lang> = Vec::new();
    for code in codes {
        if let Some(lang) = LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(code.trim())) {
            if !found.contains(&lang) {
                found.push(lang);
            }
        }
    }
    found
}

/// What Stremio asked for.
#[derive(Clone, Debug)]
pub struct Request<'a> {
    pub imdb_id: &'a str,
    pub episode: Option<Episode>,
    pub languages: &'a [&'static Lang],
    /// OpenSubtitles hash of the video, when Stremio could work it out.
    pub video_hash: Option<&'a str>,
}

#[derive(Debug)]
pub enum SubtitleError {
    Http(reqwest::Error),
    Api(String),
    NotFound,
}

impl fmt::Display for SubtitleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubtitleError::Http(e) => write!(f, "{e}"),
            SubtitleError::Api(message) => f.write_str(message),
            SubtitleError::NotFound => f.write_str("subtitle not found"),
        }
    }
}

impl From<reqwest::Error> for SubtitleError {
    fn from(e: reqwest::Error) -> Self {
        SubtitleError::Http(e)
    }
}

/// How long one provider may take before its results are left out.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(10);
/// How long search results are reused for the same title.
const SEARCH_TTL: Duration = Duration::from_secs(6 * 60 * 60);
/// How long a downloaded file is kept, so reopening it doesn't use up the daily limit.
const FILE_TTL: Duration = Duration::from_secs(3 * 60 * 60);
const MAX_CACHED_SEARCHES: usize = 100;
const MAX_CACHED_FILES: usize = 20;

type SearchKey = (String, String, bool, bool);

/// Search results, downloaded files and the OpenSubtitles sign-in, kept between requests.
#[derive(Default)]
pub struct Cache {
    searches: Mutex<HashMap<SearchKey, (Instant, Vec<Subtitle>)>>,
    files: Mutex<HashMap<(Provider, String, Option<(u32, u32)>), (Instant, File)>>,
    pub(crate) login: opensubtitles::Login,
}

/// A downloaded subtitle file.
#[derive(Clone, Debug, PartialEq)]
pub struct File {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
}

/// Searches the switched-on providers the user has a key for. Results are cached only when
/// every provider answered, so one that was down is asked again next time.
pub async fn search(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    cache: &Cache,
    keys: &SubtitleKeys,
    prefs: &Subtitles,
    request: &Request<'_>,
) -> Vec<Subtitle> {
    let os_key = keys.open_subtitles_key.as_deref().filter(|_| prefs.open_subtitles);
    let subdl_key = keys.subdl_key.as_deref().filter(|_| prefs.subdl);
    if request.languages.is_empty() || (os_key.is_none() && subdl_key.is_none()) {
        return Vec::new();
    }

    let id = match request.episode {
        Some(ep) => format!("{}:{}:{}", request.imdb_id, ep.season, ep.episode),
        None => request.imdb_id.to_owned(),
    };
    let langs = request.languages.iter().map(|l| l.code).collect::<Vec<_>>().join(",");
    let key: SearchKey = (format!("{id}#{}", request.video_hash.unwrap_or_default()), langs, os_key.is_some(), subdl_key.is_some());
    if let Some((at, found)) = cache.searches.lock().unwrap().get(&key) {
        if at.elapsed() < SEARCH_TTL {
            return found.clone();
        }
    }

    let (os, subdl) = tokio::join!(
        ask(
            "OpenSubtitles",
            os_key.is_some(),
            opensubtitles::search(http, &endpoints.open_subtitles, os_key.unwrap_or_default(), request),
        ),
        ask("SubDL", subdl_key.is_some(), subdl::search(http, &endpoints.subdl, subdl_key.unwrap_or_default(), request)),
    );
    let complete = os.is_some() && subdl.is_some();
    let found: Vec<Subtitle> = os.into_iter().chain(subdl).flatten().collect();
    if complete {
        let mut searches = cache.searches.lock().unwrap();
        searches.retain(|_, (at, _)| at.elapsed() < SEARCH_TTL);
        if searches.len() >= MAX_CACHED_SEARCHES {
            searches.clear();
        }
        searches.insert(key, (Instant::now(), found.clone()));
    }
    found
}

/// Runs one provider's search if it is used; `None` when it failed or was too slow.
async fn ask(
    name: &str,
    on: bool,
    search: impl std::future::Future<Output = Result<Vec<Subtitle>, SubtitleError>>,
) -> Option<Vec<Subtitle>> {
    if !on {
        return Some(Vec::new());
    }
    match tokio::time::timeout(SEARCH_TIMEOUT, search).await {
        Ok(Ok(found)) => Some(found),
        Ok(Err(e)) => {
            eprintln!("{name} subtitle search failed: {e}");
            None
        }
        Err(_) => {
            eprintln!("{name} subtitles took longer than {}s", SEARCH_TIMEOUT.as_secs());
            None
        }
    }
}

/// Downloads one subtitle, or returns it from the cache.
pub async fn download(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    cache: &Cache,
    keys: &SubtitleKeys,
    provider: Provider,
    file: &str,
    episode: Option<Episode>,
) -> Result<File, SubtitleError> {
    let cache_key = (provider, file.to_owned(), episode.map(|e| (e.season, e.episode)));
    if let Some((at, found)) = cache.files.lock().unwrap().get(&cache_key) {
        if at.elapsed() < FILE_TTL {
            return Ok(found.clone());
        }
    }
    let found = match provider {
        Provider::OpenSubtitles => {
            let key = keys.open_subtitles_key.as_deref().ok_or(SubtitleError::Api("no OpenSubtitles key set".into()))?;
            opensubtitles::download(http, &endpoints.open_subtitles, &cache.login, key, keys, file).await?
        }
        Provider::Subdl => subdl::download(http, &endpoints.subdl_files, file, episode).await?,
    };
    let mut files = cache.files.lock().unwrap();
    files.retain(|_, (at, _)| at.elapsed() < FILE_TTL);
    if files.len() >= MAX_CACHED_FILES {
        if let Some(oldest) = files.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone()) {
            files.remove(&oldest);
        }
    }
    files.insert(cache_key, (Instant::now(), found.clone()));
    Ok(found)
}

/// Checks a provider's key (and OpenSubtitles account) and describes what it allows.
pub async fn check(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    provider: Provider,
    keys: &SubtitleKeys,
) -> Result<String, SubtitleError> {
    match provider {
        Provider::OpenSubtitles => opensubtitles::check(http, &endpoints.open_subtitles, keys).await,
        Provider::Subdl => {
            let key = keys.subdl_key.as_deref().ok_or(SubtitleError::Api("paste a key first".into()))?;
            subdl::check(http, &endpoints.subdl, key).await
        }
    }
}

/// Puts the subtitles in the user's language order, best first within each language,
/// and keeps at most `per_language` of each.
pub fn rank(mut found: Vec<Subtitle>, prefs: &Subtitles, languages: &[&'static Lang], filename: Option<&str>) -> Vec<Subtitle> {
    let placement = |s: &Subtitle| {
        let mut worst = Placement::Show;
        for (applies, p) in [(s.hearing_impaired, prefs.hearing_impaired), (s.machine_translated, prefs.machine_translated)] {
            if applies && p != Placement::Show && (worst == Placement::Show || p == Placement::Hide) {
                worst = p;
            }
        }
        worst
    };
    found.retain(|s| languages.contains(&s.lang) && placement(s) != Placement::Hide);

    // Same file listed by both providers: keep one.
    let mut seen = std::collections::HashSet::new();
    found.retain(|s| seen.insert((s.provider, s.file.clone())));

    let video = filename.map(Torrent::from_name);
    let score = |s: &Subtitle| video.as_ref().map_or(0, |v| release_score(&s.release, v));
    let position = |s: &Subtitle| languages.iter().position(|l| *l == s.lang).unwrap_or(usize::MAX);
    found.sort_by(|a, b| {
        position(a)
            .cmp(&position(b))
            .then_with(|| (placement(a) == Placement::Demote).cmp(&(placement(b) == Placement::Demote)))
            .then_with(|| b.hash_match.cmp(&a.hash_match))
            .then_with(|| score(b).cmp(&score(a)))
            .then_with(|| b.downloads.cmp(&a.downloads))
    });

    let mut counts: HashMap<&str, usize> = HashMap::new();
    found.retain(|s| {
        let n = counts.entry(s.lang.code).or_default();
        *n += 1;
        prefs.per_language == 0 || *n <= prefs.per_language
    });
    found
}

/// How closely a subtitle's release matches the video, so the one in sync comes first:
/// the same release group counts most, then the same source (WEB, BluRay...) and resolution.
pub fn release_score(release: &str, video: &Torrent) -> u32 {
    let sub = Torrent::from_name(release);
    let mut score = 0;
    if let (Some(a), Some(b)) = (group(release), video.filename.as_deref().or(Some(&video.name)).and_then(group)) {
        if a.eq_ignore_ascii_case(b) {
            score += 8;
        }
    }
    if sub.quality() == video.quality() && sub.quality() != torrent::Quality::Unknown {
        score += 4;
    }
    if sub.resolution() != 0 && sub.resolution() == video.resolution() {
        score += 2;
    }
    score
}

/// The release group: the part after the last dash, as in "Movie.2020.1080p.WEB-DL.x264-GROUP".
fn group(name: &str) -> Option<&str> {
    let name = name.trim();
    let name = match name.rsplit_once('.') {
        Some((stem, ext)) if ext.len() <= 4 && ext.chars().all(|c| c.is_ascii_alphanumeric()) && !ext.chars().all(|c| c.is_ascii_digit()) => stem,
        _ => name,
    };
    let (_, group) = name.rsplit_once('-')?;
    let group = group.split(['[', ' ', '(']).next()?.trim();
    (group.len() >= 2 && group.chars().all(|c| c.is_ascii_alphanumeric())).then_some(group)
}

/// The subtitle as Stremio lists it. Its address points back at this server, which only
/// downloads it from the provider when the player opens it.
pub fn to_stremio(base_url: &str, sub: &Subtitle, episode: Option<Episode>) -> Value {
    let mut url = format!("{base_url}/subtitle/{}/{}.srt", sub.provider.slug(), sub.file);
    if let (Provider::Subdl, Some(ep)) = (sub.provider, episode) {
        url.push_str(&format!("?s={}&e={}", ep.season, ep.episode));
    }
    json!({
        "id": format!("matzio-{}-{}", sub.provider.short_name(), sub.file),
        "url": url,
        "lang": sub.lang.stremio,
    })
}

/// Reads Stremio's extra path segment, such as `filename=Movie.mkv&videoSize=123&videoHash=abc`.
pub fn parse_extra(raw: &str) -> HashMap<String, String> {
    let raw = raw.strip_suffix(".json").unwrap_or(raw);
    reqwest::Url::parse(&format!("http://x/?{raw}"))
        .map(|u| u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect())
        .unwrap_or_default()
}

/// Whether a provider's file id is safe to put in a URL path.
pub fn valid_file(file: &str) -> bool {
    !file.is_empty() && file.len() <= 100 && file.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Guesses the content type from a subtitle file name.
fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).as_deref() {
        Some("vtt") => "text/vtt; charset=utf-8",
        Some("ass" | "ssa") => "text/x-ssa",
        _ => "application/x-subrip",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(code: &str) -> &'static Lang {
        LANGUAGES.iter().find(|l| l.code == code).unwrap()
    }

    fn sub(provider: Provider, file: &str, code: &str, release: &str, downloads: u64) -> Subtitle {
        Subtitle {
            provider,
            file: file.into(),
            lang: lang(code),
            release: release.into(),
            hearing_impaired: false,
            machine_translated: false,
            downloads,
            hash_match: false,
        }
    }

    #[test]
    fn keeps_known_languages_in_order() {
        let codes: Vec<String> = ["de", "xx", "EN", "de", "pt-br"].map(String::from).to_vec();
        let found: Vec<_> = languages(&codes).iter().map(|l| l.code).collect();
        assert_eq!(found, ["de", "en", "pt-br"]);
    }

    #[test]
    fn finds_release_groups() {
        assert_eq!(group("Movie.2020.1080p.WEB-DL.x264-NTb.mkv"), Some("NTb"));
        assert_eq!(group("Movie 2020 1080p BluRay x265-RARBG"), Some("RARBG"));
        assert_eq!(group("Movie.2020.1080p.WEB-DL"), Some("DL"));
        assert_eq!(group("Movie 2020"), None);
    }

    #[test]
    fn ranks_by_language_then_sync_then_downloads() {
        let prefs = Subtitles { per_language: 2, ..Subtitles::default() };
        let langs = [lang("de"), lang("en")];
        let mut hi = sub(Provider::Subdl, "4", "en", "Movie.2020.1080p.WEB-DL.x264-NTb", 9000);
        hi.hearing_impaired = true;
        let mut machine = sub(Provider::OpenSubtitles, "6", "en", "Movie.2020.1080p.WEB-DL.x264-NTb", 9000);
        machine.machine_translated = true;
        let found = vec![
            sub(Provider::OpenSubtitles, "1", "en", "Movie.2020.720p.BluRay.x264-SPARKS", 5000),
            sub(Provider::OpenSubtitles, "2", "en", "Movie.2020.1080p.WEB-DL.x264-NTb", 10),
            sub(Provider::OpenSubtitles, "3", "de", "Movie.2020.German.BluRay", 1),
            hi,
            sub(Provider::OpenSubtitles, "5", "fr", "Movie.2020.1080p.WEB-DL.x264-NTb", 1),
            machine,
            sub(Provider::OpenSubtitles, "7", "en", "Movie.2020.HDTV", 1),
        ];
        let ranked = rank(found, &prefs, &langs, Some("Movie.2020.1080p.WEB-DL.DDP5.1.x264-NTb.mkv"));
        let files: Vec<_> = ranked.iter().map(|s| s.file.as_str()).collect();
        // German first; French isn't wanted; machine translated is hidden; hearing impaired goes last
        // in its language, and only two English ones are kept.
        assert_eq!(files, ["3", "2", "1"]);
    }

    #[test]
    fn exact_hash_matches_come_first() {
        let mut exact = sub(Provider::OpenSubtitles, "2", "en", "something else", 0);
        exact.hash_match = true;
        let found = vec![sub(Provider::OpenSubtitles, "1", "en", "Movie.2020.1080p.WEB-DL.x264-NTb", 100), exact];
        let ranked = rank(found, &Subtitles::default(), &[lang("en")], Some("Movie.2020.1080p.WEB-DL.x264-NTb.mkv"));
        assert_eq!(ranked[0].file, "2");
    }

    #[test]
    fn lists_subtitles_for_stremio() {
        let s = sub(Provider::Subdl, "3197651-3213944", "pt-br", "x", 0);
        let v = to_stremio("http://127.0.0.1:7777", &s, Some(Episode { season: 1, episode: 2 }));
        assert_eq!(v["url"], "http://127.0.0.1:7777/subtitle/subdl/3197651-3213944.srt?s=1&e=2");
        assert_eq!(v["lang"], "pob");
        let s = sub(Provider::OpenSubtitles, "123", "de", "x", 0);
        let v = to_stremio("http://127.0.0.1:7777", &s, Some(Episode { season: 1, episode: 2 }));
        assert_eq!(v["url"], "http://127.0.0.1:7777/subtitle/opensubtitles/123.srt");
        assert_eq!(v["lang"], "ger");
    }

    #[test]
    fn reads_stremio_extras() {
        let extra = parse_extra("filename=Movie%20Name%2B1.mkv&videoSize=123&videoHash=8e245d9679d31e12.json");
        assert_eq!(extra["filename"], "Movie Name+1.mkv");
        assert_eq!(extra["videoHash"], "8e245d9679d31e12");
        assert_eq!(extra["videoSize"], "123");
    }
}
