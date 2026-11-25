//! Bookmarks in the "Netscape bookmark file" format that Chrome, Edge, Brave, Firefox and Safari all export
//! and import (an HTML file of <DT><A HREF=...> lines inside <DL> folders).

/// One bookmark read from a file: its address, title and folder path ("Bookmarks bar/News").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub url: String,
    pub title: String,
    pub folder: String,
}

/// The most bookmarks one file can bring in.
pub const MAX_IMPORT: usize = 50_000;

/// Reads a bookmark file. Only http and https bookmarks are kept; `javascript:` bookmarklets and
/// anything else are skipped. Never fails: a file that isn't a bookmark file just gives nothing.
pub fn parse(html: &str) -> Vec<Imported> {
    let mut out = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    let mut pending_folder: Option<String> = None;
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while let Some(off) = lower[i..].find('<') {
        let start = i + off;
        let Some(end_off) = lower[start..].find('>') else {
            break;
        };
        let end = start + end_off;
        let tag = &lower[start + 1..end];
        if tag.starts_with("h3") {
            let close = lower[end..]
                .find("</h3>")
                .map(|o| end + o)
                .unwrap_or(end + 1);
            pending_folder = Some(text_of(&html[end + 1..close.max(end + 1)]));
            i = close.max(end + 1);
            continue;
        }
        if tag == "dl" || tag.starts_with("dl ") {
            folders.push(pending_folder.take().unwrap_or_default());
        } else if tag == "/dl" {
            folders.pop();
        } else if tag.starts_with("a ") {
            let href = attr(&html[start + 1..end], "href");
            let close = lower[end..]
                .find("</a>")
                .map(|o| end + o)
                .unwrap_or(end + 1);
            let title = text_of(&html[end + 1..close.max(end + 1)]);
            if let Some(url) =
                href.filter(|u| u.starts_with("http://") || u.starts_with("https://"))
            {
                if out.len() < MAX_IMPORT && url.len() <= 4096 {
                    let folder = folders
                        .iter()
                        .filter(|f| !f.is_empty())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("/");
                    out.push(Imported { url, title, folder });
                }
            }
            i = close.max(end + 1);
            continue;
        }
        i = end + 1;
    }
    out
}

/// Writes bookmarks (url, title, folder) as a bookmark file any browser can import.
pub fn export(items: &[(String, String, String)]) -> String {
    let mut out = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<!-- This is an automatically generated file. -->\n\
         <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n\
         <TITLE>Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n",
    );
    let mut by_folder: std::collections::BTreeMap<&str, Vec<(&str, &str)>> = Default::default();
    for (url, title, folder) in items {
        by_folder
            .entry(folder.as_str())
            .or_default()
            .push((url, title));
    }
    for (folder, entries) in by_folder {
        let indent = if folder.is_empty() {
            "    "
        } else {
            out += &format!("    <DT><H3>{}</H3>\n    <DL><p>\n", escape(folder));
            "        "
        };
        for (url, title) in entries {
            out += &format!(
                "{indent}<DT><A HREF=\"{}\">{}</A>\n",
                escape(url),
                escape(if title.is_empty() { url } else { title })
            );
        }
        if !folder.is_empty() {
            out += "    </DL><p>\n";
        }
    }
    out + "</DL><p>\n"
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(off) = lower[from..].find(name) {
        let at = from + off;
        from = at + name.len();
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = lower[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let value_start = tag.len() - rest.len() + 1;
        let value = tag[value_start..].trim_start();
        let (quote, body) = match value.chars().next() {
            Some(q @ ('"' | '\'')) => (Some(q), &value[1..]),
            _ => (None, value),
        };
        let end = match quote {
            Some(q) => body.find(q).unwrap_or(body.len()),
            None => body.find(char::is_whitespace).unwrap_or(body.len()),
        };
        return Some(unescape(&body[..end]).trim().to_string());
    }
    None
}

fn text_of(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    unescape(text.trim()).chars().take(500).collect()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME: &str = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><H3 ADD_DATE="1" PERSONAL_TOOLBAR_FOLDER="true">Bookmarks bar</H3>
    <DL><p>
        <DT><A HREF="https://example.org/" ADD_DATE="1" ICON="data:image/png;base64,AAA">Example &amp; co</A>
        <DT><H3>News</H3>
        <DL><p>
            <DT><A HREF='https://news.test/a?x=1&amp;y=2'>News A</A>
        </DL><p>
        <DT><A HREF="javascript:alert(1)">Bookmarklet</A>
    </DL><p>
    <DT><A HREF="http://plain.test">Plain</A>
</DL><p>"#;

    #[test]
    fn reads_chrome_export() {
        let got = parse(CHROME);
        assert_eq!(
            got,
            vec![
                Imported {
                    url: "https://example.org/".into(),
                    title: "Example & co".into(),
                    folder: "Bookmarks bar".into()
                },
                Imported {
                    url: "https://news.test/a?x=1&y=2".into(),
                    title: "News A".into(),
                    folder: "Bookmarks bar/News".into()
                },
                Imported {
                    url: "http://plain.test".into(),
                    title: "Plain".into(),
                    folder: "".into()
                },
            ]
        );
    }

    #[test]
    fn round_trip_and_junk() {
        let items = vec![
            (
                "https://a.test/?q=<b>".to_string(),
                "A \"quoted\"".to_string(),
                String::new(),
            ),
            (
                "https://b.test/".to_string(),
                String::new(),
                "Work".to_string(),
            ),
        ];
        let back = parse(&export(&items));
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].url, "https://a.test/?q=<b>");
        assert_eq!(back[0].title, "A \"quoted\"");
        assert_eq!(back[1].folder, "Work");
        assert!(parse("not a bookmark file <a").is_empty());
        assert!(parse("<<<>>><a href=>").is_empty());
    }
}
