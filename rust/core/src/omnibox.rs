//! The address bar: what typed text opens, how a URL is shown, and which navigations are allowed.

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

/// Schemes a tab may show.
pub const PAGE_SCHEMES: &[&str] = &[
    "http",
    "https",
    "file",
    "about",
    "data",
    "blob",
    crate::INTERNAL_SCHEME,
];
/// Schemes typed or clicked that open in another app (mail, phone), never in a tab.
pub const HANDOFF_SCHEMES: &[&str] = &["mailto", "tel"];
/// Schemes a user may type to open directly.
pub const OPENABLE_SCHEMES: &[&str] = &["http", "https", "file", "about", crate::INTERNAL_SCHEME];

/// The scheme of `text`, lowercase, if it starts with one.
pub fn url_scheme(text: &str) -> Option<String> {
    let end = text.find(':')?;
    let scheme = &text[..end];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic()
        || !chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

/// The host part (with port) of something typed without a scheme, if it looks like one.
fn host_of(text: &str) -> Option<&str> {
    let end = text.find(['/', '?', '#']).unwrap_or(text.len());
    let authority = &text[..end];
    let host = if authority.starts_with('[') {
        let close = authority.find(']')?;
        let rest = &authority[close + 1..];
        if !(rest.is_empty() || rest.starts_with(':')) {
            return None;
        }
        if let Some(port) = rest.strip_prefix(':') {
            if port.is_empty() || port.len() > 5 || !port.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
        }
        return authority[1..close]
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
            .then_some(&authority[..=close]);
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => {
                if port.is_empty() || port.len() > 5 || !port.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                host
            }
            None => authority,
        }
    };
    Some(host)
}

/// True for `example.org`, `localhost:8000/x` or `10.0.0.1`: things to open, not to search for.
pub fn looks_like_host(text: &str) -> bool {
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return false;
    }
    let Some(host) = host_of(text) else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") || host.starts_with('[') {
        return true;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() == 4
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.len() <= 3 && l.chars().all(|c| c.is_ascii_digit()))
    {
        return true;
    }
    if labels.len() < 2 || !labels.iter().all(|l| valid_label(l)) {
        return false;
    }
    let tld = labels[labels.len() - 1];
    (tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
        || (tld.starts_with("xn--") && tld.len() > 4)
}

/// Hosts that never leave this computer, so plain http to them is not a warning.
pub fn is_local_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if host == "localhost" || host == "::1" || host.ends_with(".localhost") {
        return true;
    }
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts[0] == "127"
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()))
}

fn local_or_ip(text: &str) -> bool {
    let host = host_of(text).unwrap_or("");
    host.eq_ignore_ascii_case("localhost")
        || host.starts_with('[')
        || host.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// The search URL for `query` with a search engine's template (`{}` stands for the query).
pub fn search_url(template: &str, query: &str) -> String {
    let encoded = utf8_percent_encode(query, NON_ALPHANUMERIC)
        .to_string()
        .replace("%20", "+");
    template.replacen("{}", &encoded, 1)
}

/// The URL to load for what was typed in the address bar, or None for nothing.
pub fn resolve_input(text: &str, search_template: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(scheme) = url_scheme(text) {
        if OPENABLE_SCHEMES.contains(&scheme.as_str()) || HANDOFF_SCHEMES.contains(&scheme.as_str())
        {
            return Some(text.to_string());
        }
    }
    if looks_like_host(text) {
        // Local servers rarely have certificates; everything else is tried over HTTPS first.
        let prefix = if local_or_ip(text) {
            "http://"
        } else {
            "https://"
        };
        return Some(format!("{prefix}{text}"));
    }
    Some(search_url(search_template, text))
}

/// What to do with a navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    Allow,
    /// Open in the user's default app (mail, phone).
    Handoff,
    Block,
}

/// Whether a navigation may happen. Web pages can't open CottonCandy's own pages, and a whole tab can't be
/// navigated to a `data:` URL (Chromium's rule too: phishing).
pub fn navigation_decision(
    url: &str,
    from_url: &str,
    is_main_frame: bool,
    typed: bool,
    user_initiated: bool,
) -> Navigation {
    let Some(scheme) = url_scheme(url) else {
        return Navigation::Block;
    };
    if typed && OPENABLE_SCHEMES.contains(&scheme.as_str()) {
        return Navigation::Allow;
    }
    if HANDOFF_SCHEMES.contains(&scheme.as_str()) {
        return if is_main_frame && (typed || user_initiated) {
            Navigation::Handoff
        } else {
            Navigation::Block
        };
    }
    if scheme == "data" && is_main_frame {
        return Navigation::Block;
    }
    if scheme == crate::INTERNAL_SCHEME {
        let from_internal = url_scheme(from_url).as_deref() == Some(crate::INTERNAL_SCHEME);
        return if from_internal || (from_url.is_empty() && is_main_frame) {
            Navigation::Allow
        } else {
            Navigation::Block
        };
    }
    if scheme == "file" && !from_url.is_empty() && url_scheme(from_url).as_deref() != Some("file") {
        return Navigation::Block; // web pages can't open local files
    }
    if PAGE_SCHEMES.contains(&scheme.as_str()) {
        Navigation::Allow
    } else {
        Navigation::Block
    }
}

/// How the address bar shows a URL.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UrlParts {
    /// secure, insecure, local, file, internal, empty or other
    pub kind: &'static str,
    pub host: String,
    pub rest: String,
}

pub fn url_parts(url: &str) -> UrlParts {
    let empty = UrlParts {
        kind: "empty",
        host: String::new(),
        rest: String::new(),
    };
    if url.is_empty() || url == "about:blank" || url.starts_with("cottoncandy://newtab") {
        return empty;
    }
    let Ok(parsed) = url::Url::parse(url) else {
        return UrlParts {
            kind: "other",
            host: String::new(),
            rest: url.to_string(),
        };
    };
    let host = parsed.host_str().unwrap_or("").to_string();
    let host_port = match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.clone(),
    };
    let mut rest = parsed.path().to_string();
    if let Some(q) = parsed.query() {
        rest.push('?');
        rest.push_str(q);
    }
    if rest == "/" {
        rest.clear();
    }
    let kind = match parsed.scheme() {
        "https" => "secure",
        "http" if is_local_host(&host) => "local",
        "http" => "insecure",
        "file" => {
            return UrlParts {
                kind: "file",
                host: String::new(),
                rest: parsed.path().to_string(),
            }
        }
        s if s == crate::INTERNAL_SCHEME => "internal",
        _ => "other",
    };
    UrlParts {
        kind,
        host: host_port,
        rest,
    }
}

/// A file name that can't climb out of the downloads folder or hide itself.
pub fn safe_download_name(suggested: &str) -> String {
    let name = suggested.replace('\\', "/");
    let name = name.rsplit('/').next().unwrap_or("");
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    let name = name.trim().trim_start_matches('.').trim();
    if name.is_empty() {
        return "download".into();
    }
    // Windows reserves these names in every folder.
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    let name: String = name
        .chars()
        .map(|c| if "<>:\"|?*".contains(c) { '_' } else { c })
        .collect();
    let name = if reserved { format!("_{name}") } else { name };
    if name.chars().count() <= 200 {
        return name;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if name.len() - i <= 20 => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    let keep: String = stem.chars().take(200 - ext.chars().count()).collect();
    format!("{keep}{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DDG: &str = "https://duckduckgo.com/?q={}";

    #[test]
    fn typed_text() {
        assert_eq!(
            resolve_input("example.org", DDG).unwrap(),
            "https://example.org"
        );
        assert_eq!(
            resolve_input("localhost:8000/x", DDG).unwrap(),
            "http://localhost:8000/x"
        );
        assert_eq!(resolve_input("10.0.0.1", DDG).unwrap(), "http://10.0.0.1");
        assert_eq!(
            resolve_input("[::1]:3000", DDG).unwrap(),
            "http://[::1]:3000"
        );
        assert_eq!(
            resolve_input("https://a.test/b", DDG).unwrap(),
            "https://a.test/b"
        );
        assert_eq!(
            resolve_input("what is rust", DDG).unwrap(),
            "https://duckduckgo.com/?q=what+is+rust"
        );
        assert_eq!(
            resolve_input("c++ & rust?", DDG).unwrap(),
            "https://duckduckgo.com/?q=c%2B%2B+%26+rust%3F"
        );
        assert_eq!(
            resolve_input("javascript:alert(1)", DDG).unwrap(),
            "https://duckduckgo.com/?q=javascript%3Aalert%281%29"
        );
        assert_eq!(resolve_input("   ", DDG), None);
        assert!(!looks_like_host("file.txt1"));
        assert!(looks_like_host("xn--bcher-kva.example"));
    }

    #[test]
    fn navigations() {
        use Navigation::*;
        assert_eq!(
            navigation_decision("https://a.test", "https://b.test", true, false, false),
            Allow
        );
        assert_eq!(
            navigation_decision(
                "cottoncandy://settings",
                "https://evil.test",
                true,
                false,
                true
            ),
            Block
        );
        assert_eq!(
            navigation_decision("cottoncandy://settings", "", true, true, true),
            Allow
        );
        assert_eq!(
            navigation_decision("data:text/html,x", "https://a.test", true, false, true),
            Block
        );
        assert_eq!(
            navigation_decision(
                "data:image/png;base64,x",
                "https://a.test",
                false,
                false,
                false
            ),
            Allow
        );
        assert_eq!(
            navigation_decision("file:///etc/passwd", "https://a.test", true, false, true),
            Block
        );
        assert_eq!(
            navigation_decision("mailto:a@b.test", "https://a.test", true, false, true),
            Handoff
        );
        assert_eq!(
            navigation_decision("mailto:a@b.test", "https://a.test", true, false, false),
            Block
        );
        assert_eq!(
            navigation_decision("ms-settings:", "", true, false, true),
            Block
        );
    }

    #[test]
    fn parts() {
        let p = url_parts("https://example.org:8443/a?b=1");
        assert_eq!(
            (p.kind, p.host.as_str(), p.rest.as_str()),
            ("secure", "example.org:8443", "/a?b=1")
        );
        assert_eq!(url_parts("http://localhost:8000/").kind, "local");
        assert_eq!(url_parts("http://example.org").kind, "insecure");
        assert_eq!(url_parts("cottoncandy://newtab").kind, "empty");
    }

    #[test]
    fn download_names() {
        assert_eq!(safe_download_name("../../.evil.sh"), "evil.sh");
        assert_eq!(safe_download_name("C:\\x\\..\\report.pdf"), "report.pdf");
        assert_eq!(safe_download_name("con.txt"), "_con.txt");
        assert_eq!(safe_download_name("a<b>.txt"), "a_b_.txt");
        assert_eq!(safe_download_name(""), "download");
        assert!(safe_download_name(&("x".repeat(300) + ".tar.gz")).ends_with(".gz"));
        assert!(safe_download_name(&"x".repeat(300)).chars().count() <= 200);
    }
}
