//! The user's checks: which streams to hide, which to push to the bottom, and
//! how their preferences rank the rest.

use std::{cmp::Ordering, time::Duration};

use crate::{
    config::{Action, Filters, TagCheck},
    torrent::{fits_speed, normalize_tag, resolution_key, Torrent},
};

const GB: f64 = 1_073_741_824.0;

/// Reads one kind of tag from a stream, such as its HDR formats.
type Tagger = fn(&Torrent) -> Vec<&'static str>;

/// What the checks decided for one stream.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Verdict {
    /// Failed a check set to hide.
    pub hidden: bool,
    /// Failed a check set to show at the bottom.
    pub demoted: bool,
    /// Names of the checks it failed, such as "size" or "hdr".
    pub failed: Vec<&'static str>,
}

impl Verdict {
    fn fail(&mut self, check: &'static str, action: Action) {
        self.failed.push(check);
        match action {
            Action::Hide => self.hidden = true,
            Action::Demote => self.demoted = true,
        }
    }
}

/// What a request knows beyond the stream itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    pub speed_mbps: Option<f64>,
    pub runtime: Option<Duration>,
}

/// Runs every switched-on check against one stream.
pub fn judge(t: &Torrent, filters: &Filters, cx: Context) -> Verdict {
    let mut v = Verdict::default();

    let speed = &filters.speed;
    if speed.enabled {
        if let (Some(mbps), Some(runtime)) = (cx.speed_mbps.filter(|&m| m > 0.0), cx.runtime) {
            if !fits_speed(t.size_bytes, runtime, mbps) {
                v.fail("speed", speed.action);
            }
        }
    }

    let size = &filters.size;
    if let (true, Some(bytes)) = (size.enabled, t.size_bytes) {
        let gb = bytes as f64 / GB;
        let max = size.max_gb_by_resolution.get(resolution_key(t.resolution())).copied().or(size.max_gb);
        let too_small = size.min_gb.is_some_and(|min| min > 0.0 && gb < min);
        let too_big = max.is_some_and(|max| max > 0.0 && gb > max);
        if too_small || too_big {
            v.fail("size", size.action);
        }
    }

    let tag_checks: [(&'static str, &TagCheck, Tagger); 4] = [
        ("quality", &filters.quality, Torrent::quality_tags),
        ("hdr", &filters.hdr, Torrent::hdr_tags),
        ("codec", &filters.codec, Torrent::codec_tags),
        ("audio", &filters.audio, Torrent::audio_tags),
    ];
    for (name, check, tags) in tag_checks {
        if check.enabled && !check.avoid.is_empty() {
            let tags = tags(t);
            if check.avoid.iter().any(|a| tags.contains(&normalize_tag(a).as_str())) {
                v.fail(name, check.action);
            }
        }
    }

    let words = &filters.keywords;
    if words.enabled && t.first_keyword(&words.avoid).is_some() {
        v.fail("keywords", words.action);
    }

    let lang = &filters.language;
    if lang.enabled && lang.only_preferred && !lang.preferred.is_empty() {
        let spoken = t.languages();
        if !lang.preferred.iter().any(|p| spoken.contains(&p.as_str())) {
            v.fail("language", lang.action);
        }
    }

    v
}

/// Position of the most wanted of `prefer` among `tags`; `prefer.len()` when none match.
fn preference(tags: &[&str], prefer: &[String]) -> usize {
    prefer.iter().position(|p| tags.contains(&normalize_tag(p).as_str())).unwrap_or(prefer.len())
}

/// Compares two streams on one preference key ("language", "hdr", "codec", "audio" or
/// "keywords"). `None` when the key's check is off or has nothing preferred, so the
/// caller can fall back to its own default.
pub fn compare_preference(key: &str, a: &Torrent, b: &Torrent, filters: &Filters) -> Option<Ordering> {
    let by_tags = |check: &TagCheck, tags: Tagger| {
        (check.enabled && !check.prefer.is_empty())
            .then(|| preference(&tags(a), &check.prefer).cmp(&preference(&tags(b), &check.prefer)))
    };
    match key {
        "language" => {
            let lang = &filters.language;
            (lang.enabled && !lang.preferred.is_empty()).then(|| {
                preference(&a.languages(), &lang.preferred).cmp(&preference(&b.languages(), &lang.preferred))
            })
        }
        "hdr" => by_tags(&filters.hdr, Torrent::hdr_tags),
        "codec" => by_tags(&filters.codec, Torrent::codec_tags),
        "audio" => by_tags(&filters.audio, Torrent::audio_tags),
        "keywords" => {
            let words = &filters.keywords;
            (words.enabled && !words.prefer.is_empty()).then(|| {
                let rank = |t: &Torrent| t.first_keyword(&words.prefer).unwrap_or(words.prefer.len());
                rank(a).cmp(&rank(b))
            })
        }
        _ => None,
    }
}

/// Cuts a ranked list down to the user's limits, keeping the best of each resolution.
pub fn limit<T>(items: Vec<T>, filters: &Filters, resolution: impl Fn(&T) -> u32) -> Vec<T> {
    let limits = &filters.limits;
    if !limits.enabled {
        return items;
    }
    let mut per_res = std::collections::HashMap::new();
    items
        .into_iter()
        .filter(|item| {
            let count = per_res.entry(resolution_key(resolution(item))).or_insert(0usize);
            *count += 1;
            limits.per_resolution == 0 || *count <= limits.per_resolution
        })
        .take(if limits.total == 0 { usize::MAX } else { limits.total })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Filters, Limits};

    fn t(name: &str, gb: f64) -> Torrent {
        Torrent { info_hash: name.into(), name: name.into(), size_bytes: Some((gb * GB) as u64), ..Torrent::default() }
    }

    #[test]
    fn plain_size_limits_with_per_resolution_override() {
        let mut f = Filters::default();
        f.size.max_gb = Some(10.0);
        f.size.min_gb = Some(1.0);
        f.size.max_gb_by_resolution.insert("2160p".into(), 40.0);
        let cx = Context::default();
        assert!(!judge(&t("Movie.1080p", 8.0), &f, cx).hidden);
        assert!(judge(&t("Movie.1080p", 12.0), &f, cx).hidden);
        assert!(!judge(&t("Movie.2160p", 30.0), &f, cx).hidden);
        assert!(judge(&t("Movie.720p", 0.5), &f, cx).hidden);

        f.size.enabled = false;
        assert!(!judge(&t("Movie.1080p", 12.0), &f, cx).hidden);
    }

    #[test]
    fn demote_instead_of_hide() {
        let mut f = Filters::default();
        f.hdr.avoid = vec!["dv-only".into()];
        f.hdr.action = Action::Demote;
        let v = judge(&t("Movie.2160p.DV.WEB", 10.0), &f, Context::default());
        assert!(!v.hidden && v.demoted);
        assert_eq!(v.failed, ["hdr"]);
        // DV with an HDR10 fallback layer is fine.
        assert!(!judge(&t("Movie.2160p.DV.HDR10.WEB", 10.0), &f, Context::default()).demoted);
    }

    #[test]
    fn speed_check_can_be_switched_off() {
        let mut f = Filters::default();
        let cx = Context { speed_mbps: Some(10.0), runtime: Some(Duration::from_secs(45 * 60)) };
        assert!(judge(&t("Show.S01E01.1080p", 4.0), &f, cx).hidden);
        f.speed.enabled = false;
        assert!(!judge(&t("Show.S01E01.1080p", 4.0), &f, cx).hidden);
    }

    #[test]
    fn quality_codec_audio_keywords_and_language() {
        let mut f = Filters::default();
        let cx = Context::default();
        assert!(judge(&t("Movie.2024.HDCAM", 1.0), &f, cx).hidden);

        f.codec.avoid = vec!["av1".into()];
        f.audio.avoid = vec!["truehd".into()];
        f.keywords.avoid = vec!["HC".into(), "/\\b3d\\b/".into()];
        assert!(judge(&t("Movie.2160p.AV1", 1.0), &f, cx).hidden);
        assert!(judge(&t("Movie.2160p.BluRay.TrueHD.7.1.Atmos", 1.0), &f, cx).hidden);
        assert!(judge(&t("Movie.2024.1080p.HC.WEB", 1.0), &f, cx).hidden);
        assert!(judge(&t("Movie.2024.1080p.3D.BluRay", 1.0), &f, cx).hidden);
        assert!(!judge(&t("Movie.2024.1080p.CHC.WEB.x264.DDP5.1", 1.0), &f, cx).hidden);

        f.language.preferred = vec!["de".into()];
        f.language.only_preferred = true;
        assert!(judge(&t("Movie.2024.1080p.WEB", 1.0), &f, cx).hidden);
        assert!(!judge(&t("Movie.2024.German.DL.1080p.WEB", 1.0), &f, cx).hidden);
        f.language.enabled = false;
        assert!(!judge(&t("Movie.2024.1080p.WEB", 1.0), &f, cx).hidden);
    }

    #[test]
    fn ranks_by_preferences() {
        let mut f = Filters::default();
        f.language.preferred = vec!["de".into(), "en".into()];
        let (de, en, fr) = (t("Movie.GERMAN.1080p", 1.0), t("Movie.1080p", 1.0), t("Movie.FRENCH.1080p", 1.0));
        assert_eq!(compare_preference("language", &de, &en, &f), Some(Ordering::Less));
        assert_eq!(compare_preference("language", &fr, &en, &f), Some(Ordering::Greater));

        assert_eq!(compare_preference("hdr", &de, &en, &f), None);
        f.hdr.prefer = vec!["dv".into(), "hdr".into()];
        let (dv, hdr) = (t("Movie.2160p.DV.HDR", 1.0), t("Movie.2160p.HDR10", 1.0));
        assert_eq!(compare_preference("hdr", &dv, &hdr, &f), Some(Ordering::Less));
        f.hdr.enabled = false;
        assert_eq!(compare_preference("hdr", &dv, &hdr, &f), None);
    }

    #[test]
    fn limits_total_and_per_resolution() {
        let mut f = Filters::default();
        let items = vec![2160, 2160, 2160, 1080, 1080, 720];
        assert_eq!(limit(items.clone(), &f, |r| *r).len(), 6);
        f.limits = Limits { enabled: true, total: 4, per_resolution: 2 };
        assert_eq!(limit(items, &f, |r| *r), [2160, 2160, 1080, 1080]);
    }
}
