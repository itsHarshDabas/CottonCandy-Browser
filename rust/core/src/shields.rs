//! Shields: blocking ads and trackers with Brave's open-source filter engine (`adblock`, Adblock Plus syntax:
//! EasyList, EasyPrivacy, uBlock lists), hiding the boxes ads leave, and stripping tracking parameters.

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A filter list people can turn on in Settings: (id, name, URL, on for trackers rather than ads).
pub struct FilterList {
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub trackers: bool,
}

pub const LISTS: &[FilterList] = &[
    FilterList { id: "easylist", name: "EasyList (ads)", url: "https://easylist.to/easylist/easylist.txt", trackers: false },
    FilterList { id: "easyprivacy", name: "EasyPrivacy (trackers)", url: "https://easylist.to/easylist/easyprivacy.txt", trackers: true },
    FilterList {
        id: "ublock-unbreak",
        name: "uBlock: fixes for broken sites",
        url: "https://ublockorigin.github.io/uAssets/filters/unbreak.txt",
        trackers: false,
    },
    FilterList {
        id: "ublock-filters",
        name: "uBlock filters",
        url: "https://ublockorigin.github.io/uAssets/filters/filters.txt",
        trackers: false,
    },
    FilterList {
        id: "ublock-privacy",
        name: "uBlock: privacy",
        url: "https://ublockorigin.github.io/uAssets/filters/privacy.txt",
        trackers: true,
    },
    FilterList { id: "fanboy-annoyance", name: "Fanboy's Annoyance (cookie banners, pop-ups)", url: "https://secure.fanboy.co.nz/fanboy-annoyance.txt", trackers: false },
    FilterList { id: "peter-lowe", name: "Peter Lowe's ad and tracking servers", url: "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=adblockplus&showintro=0&mimetype=plaintext", trackers: true },
];

/// How often lists are fetched again.
pub const LIST_MAX_AGE: Duration = Duration::from_secs(4 * 24 * 3600);
/// Lists bigger than this are refused (EasyList is about 2 MB).
pub const LIST_MAX_BYTES: usize = 16 * 1024 * 1024;

/// A small list that works before the first download (and if downloads keep failing): the biggest ad and
/// tracking networks.
pub const BUILT_IN_ADS: &str = include_str!("../assets/builtin-ads.txt");
pub const BUILT_IN_TRACKERS: &str = include_str!("../assets/builtin-trackers.txt");

pub fn list_cache_path(cache_dir: &Path, id: &str) -> PathBuf {
    cache_dir.join("filters").join(format!("{id}.txt"))
}

/// Whether a cached list should be fetched again.
pub fn list_is_stale(path: &Path, now: SystemTime) -> bool {
    match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(modified) => now
            .duration_since(modified)
            .map(|age| age > LIST_MAX_AGE)
            .unwrap_or(false),
        Err(_) => true,
    }
}

/// A downloaded list is kept only if it looks like a filter list, not an error page.
pub fn looks_like_filter_list(text: &str) -> bool {
    if text.len() > LIST_MAX_BYTES || text.trim_start().starts_with('<') {
        return false;
    }
    let lines: Vec<&str> = text
        .lines()
        .take(400)
        .filter(|l| !l.trim().is_empty())
        .collect();
    !lines.is_empty()
        && lines
            .iter()
            .filter(|l| {
                l.starts_with('!')
                    || l.starts_with("||")
                    || l.contains("##")
                    || l.starts_with('[')
                    || l.starts_with("@@")
                    || l.starts_with('/')
                    || l.starts_with('-')
                    || l.starts_with('.')
            })
            .count()
            * 2
            >= lines.len()
}

/// Two engines: one for ads, one for trackers, so each has its own switch.
pub struct Shields {
    ads: Engine,
    trackers: Engine,
    pub rule_count: usize,
}

/// What the filters say about one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Block {
        tracker: bool,
    },
    /// Load this instead (a harmless stand-in that keeps the page working).
    Redirect(String),
}

impl Shields {
    /// Builds the engines from the built-in lists, every cached list that is turned on, and the user's rules.
    pub fn build(cache_dir: &Path, enabled: &[String], custom_rules: &str) -> Shields {
        let mut ads = FilterSet::new(false);
        let mut trackers = FilterSet::new(false);
        let mut count = 0;
        let mut add = |set: &mut FilterSet, text: String| {
            count += text
                .lines()
                .filter(|l| {
                    let l = l.trim();
                    !l.is_empty() && !l.starts_with('!') && !l.starts_with('[')
                })
                .count();
            set.add_filter_list(text, ParseOptions::default());
        };
        add(&mut ads, BUILT_IN_ADS.to_string());
        add(&mut trackers, BUILT_IN_TRACKERS.to_string());
        for list in LISTS.iter().filter(|l| enabled.iter().any(|e| e == l.id)) {
            if let Ok(text) = std::fs::read_to_string(list_cache_path(cache_dir, list.id)) {
                if looks_like_filter_list(&text) {
                    add(
                        if list.trackers {
                            &mut trackers
                        } else {
                            &mut ads
                        },
                        text,
                    );
                }
            }
        }
        if !custom_rules.trim().is_empty() {
            add(&mut ads, custom_rules.to_string());
        }
        Shields {
            ads: Engine::new_with_filter_set(ads),
            trackers: Engine::new_with_filter_set(trackers),
            rule_count: count,
        }
    }

    /// Only the built-in lists (for tests and the first second after start).
    pub fn built_in() -> Shields {
        Shields::build(Path::new("/nonexistent"), &[], "")
    }

    /// Checks one request. `resource_type` is the CEF/Chromium resource type in adblock's words
    /// (script, image, stylesheet, xmlhttprequest, sub_frame, media, font, websocket, ping, other).
    pub fn check(
        &self,
        url: &str,
        page_url: &str,
        resource_type: &str,
        ads: bool,
        trackers: bool,
    ) -> Verdict {
        if !(ads || trackers) || resource_type == "main_frame" {
            return Verdict::Allow;
        }
        let Ok(request) = Request::new(url, page_url, resource_type, "GET") else {
            return Verdict::Allow;
        };
        for (on, engine, tracker) in [(trackers, &self.trackers, true), (ads, &self.ads, false)] {
            if !on {
                continue;
            }
            let result = engine.check_network_request(&request);
            if let Some(redirect) = result.redirect {
                return Verdict::Redirect(redirect);
            }
            if result.should_block() {
                return Verdict::Block { tracker };
            }
        }
        Verdict::Allow
    }

    /// CSS that hides ad boxes on `url`, for a style sheet injected into the page.
    pub fn cosmetic_css(&self, url: &str) -> String {
        let resources = self.ads.url_cosmetic_resources(url);
        let mut selectors: Vec<&String> = resources
            .hide_selectors
            .iter()
            .filter(|s| !s.contains('{') && !s.contains('}') && !s.contains("</"))
            .collect();
        selectors.sort();
        selectors.truncate(4000);
        if selectors.is_empty() {
            return String::new();
        }
        let mut css = String::new();
        for chunk in selectors.chunks(200) {
            css.push_str(
                &chunk
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(",\n"),
            );
            css.push_str(" { display: none !important; }\n");
        }
        css
    }
}

/// The https:// address for a plain http page, or None to leave it alone. Hosts on this computer or the local
/// network (IP addresses, names without a dot, .local, .lan, .internal, .home.arpa) rarely have certificates.
pub fn https_upgrade(raw: &str) -> Option<String> {
    let mut parsed = url::Url::parse(raw).ok()?;
    if parsed.scheme() != "http" {
        return None;
    }
    let host = match parsed.host()? {
        url::Host::Domain(d) => d.trim_end_matches('.').to_ascii_lowercase(),
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => return None,
    };
    let local_suffix = [
        ".local",
        ".lan",
        ".internal",
        ".home.arpa",
        ".localhost",
        ".test",
        ".onion",
    ]
    .iter()
    .any(|s| host.ends_with(s));
    if !host.contains('.') || local_suffix || crate::omnibox::is_local_host(&host) {
        return None;
    }
    // A port that isn't 80 is usually a dev server that only speaks http.
    if parsed.port().is_some() {
        return None;
    }
    parsed.set_scheme("https").ok()?;
    Some(parsed.to_string())
}

/// Query parameters that only track people, removed from links before they load.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "gclsrc",
    "dclid",
    "gbraid",
    "wbraid",
    "msclkid",
    "mc_cid",
    "mc_eid",
    "igshid",
    "yclid",
    "_hsenc",
    "_hsmi",
    "mkt_tok",
    "oly_anon_id",
    "oly_enc_id",
    "rb_clickid",
    "s_cid",
    "vero_id",
    "twclid",
    "ttclid",
    "li_fat_id",
    "srsltid",
];

/// The URL without tracking parameters (utm_*, fbclid, gclid...), or None if there were none.
pub fn strip_tracking_params(raw: &str) -> Option<String> {
    let mut parsed = url::Url::parse(raw).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.query().is_none() {
        return None;
    }
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let kept: Vec<&(String, String)> = pairs
        .iter()
        .filter(|(k, _)| !(k.starts_with("utm_") || TRACKING_PARAMS.contains(&k.as_str())))
        .collect();
    if kept.len() == pairs.len() {
        return None;
    }
    if kept.is_empty() {
        parsed.set_query(None);
    } else {
        parsed
            .query_pairs_mut()
            .clear()
            .extend_pairs(kept.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    Some(parsed.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_lists_block_the_big_networks() {
        let s = Shields::built_in();
        assert!(s.rule_count > 20);
        let page = "https://news.example/";
        assert_eq!(
            s.check(
                "https://securepubads.g.doubleclick.net/tag/js/gpt.js",
                page,
                "script",
                true,
                true
            ),
            Verdict::Block { tracker: false }
        );
        assert_eq!(
            s.check(
                "https://www.google-analytics.com/analytics.js",
                page,
                "script",
                true,
                true
            ),
            Verdict::Block { tracker: true }
        );
        assert_eq!(
            s.check(
                "https://www.google-analytics.com/analytics.js",
                page,
                "script",
                true,
                false
            ),
            Verdict::Allow
        );
        assert_eq!(
            s.check("https://news.example/app.js", page, "script", true, true),
            Verdict::Allow
        );
        // A page itself is never blocked: the user asked for it.
        assert_eq!(
            s.check("https://doubleclick.net/", "", "main_frame", true, true),
            Verdict::Allow
        );
    }

    #[test]
    fn custom_rules_and_cached_lists() {
        let dir = tempfile::tempdir().unwrap();
        let path = list_cache_path(dir.path(), "easylist");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "! Title: test\n||ads.example^\nexample.org##.banner-ad\n",
        )
        .unwrap();
        let s = Shields::build(dir.path(), &["easylist".into()], "||mine.test^$script");
        assert!(matches!(
            s.check(
                "https://ads.example/x.png",
                "https://a.test/",
                "image",
                true,
                false
            ),
            Verdict::Block { .. }
        ));
        assert!(matches!(
            s.check(
                "https://mine.test/x.js",
                "https://a.test/",
                "script",
                true,
                false
            ),
            Verdict::Block { .. }
        ));
        assert!(s
            .cosmetic_css("https://example.org/")
            .contains(".banner-ad"));
        // Turned off: not used.
        let s = Shields::build(dir.path(), &[], "");
        assert_eq!(
            s.check(
                "https://ads.example/x.png",
                "https://a.test/",
                "image",
                true,
                false
            ),
            Verdict::Allow
        );
    }

    #[test]
    fn error_pages_are_not_lists() {
        assert!(!looks_like_filter_list("<!doctype html><title>404</title>"));
        assert!(looks_like_filter_list(
            "[Adblock Plus 2.0]\n! Title: x\n||a.test^\n"
        ));
        assert!(!looks_like_filter_list(""));
    }

    #[test]
    fn stale_lists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.txt");
        assert!(list_is_stale(&path, SystemTime::now()));
        std::fs::write(&path, "x").unwrap();
        assert!(!list_is_stale(&path, SystemTime::now()));
        assert!(list_is_stale(
            &path,
            SystemTime::now() + LIST_MAX_AGE + Duration::from_secs(1)
        ));
    }

    #[test]
    fn tracking_params() {
        assert_eq!(
            strip_tracking_params("https://a.test/p?utm_source=x&id=7&fbclid=y").unwrap(),
            "https://a.test/p?id=7"
        );
        assert_eq!(
            strip_tracking_params("https://a.test/p?gclid=1").unwrap(),
            "https://a.test/p"
        );
        assert_eq!(strip_tracking_params("https://a.test/p?id=7"), None);
        assert_eq!(strip_tracking_params("ftp://a.test/?utm_x=1"), None);
    }

    #[test]
    fn upgrades_only_public_http() {
        assert_eq!(
            https_upgrade("http://example.org/a?b=1").as_deref(),
            Some("https://example.org/a?b=1")
        );
        for url in [
            "https://example.org/",
            "http://localhost/",
            "http://127.0.0.1:8765/",
            "http://192.168.1.1/",
            "http://[::1]/",
            "http://router/",
            "http://printer.local/",
            "http://nas.lan/",
            "http://example.org:8080/",
            "http://abc.onion/",
            "ftp://example.org/",
        ] {
            assert_eq!(https_upgrade(url), None, "{url}");
        }
    }
}
