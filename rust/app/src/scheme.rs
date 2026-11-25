//! The cottoncandy:// scheme: CottonCandy's own pages, their files, and the /api/ they call.
//!
//! Files are built into the binary, so nothing on disk can change them. /api/ answers only CottonCandy's
//! own pages: the request has to come from a cottoncandy:// frame, be a POST, and carry the session token
//! that only those pages are given. Web pages can't navigate to cottoncandy:// at all (see `client.rs`).

use crate::cefx::*;
use crate::{api, browser, shared};
use cef::*;
use std::sync::{Arc, Mutex};

macro_rules! files {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../ui/", $name)) as &[u8])),*]
    };
}

const FILES: &[(&str, &[u8])] = files![
    "about.html",
    "about.js",
    "base.css",
    "bookmarks.html",
    "bookmarks.js",
    "cc.js",
    "chrome.html",
    "chrome.js",
    "downloads.html",
    "downloads.js",
    "error.html",
    "error.js",
    "history.html",
    "history.js",
    "settings.html",
    "settings.js",
    "newtab.html",
    "newtab.js",
    "page.css",
    "popup.html",
    "popup.js",
    "vtabs.html",
];

/// cottoncandy://<host>/ shows ui/<host>.html.
const PAGES: &[&str] = &[
    "newtab",
    "history",
    "bookmarks",
    "downloads",
    "error",
    "about",
    "settings",
];

const GLOBE_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#8b92a0" stroke-width="1.8"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/></svg>"##;

pub fn file(name: &str) -> Option<&'static [u8]> {
    FILES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, data)| *data)
}

fn mime(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html",
        "js" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

#[derive(Default)]
struct Reply {
    status: i32,
    mime: String,
    body: Vec<u8>,
    sent: usize,
}

impl Reply {
    fn new(status: i32, mime: &str, body: Vec<u8>) -> Reply {
        Reply {
            status,
            mime: mime.into(),
            body,
            sent: 0,
        }
    }
    fn json(status: i32, value: &serde_json::Value) -> Reply {
        Reply::new(status, "application/json", value.to_string().into_bytes())
    }
    fn not_found() -> Reply {
        Reply::new(404, "text/plain", b"Not found".to_vec())
    }
}

/// Answers a GET for a page or file.
fn serve(host: &str, path: &str, query: &str) -> Reply {
    if path == "/theme.css" {
        return Reply::new(200, "text/css", browser::theme_css().into_bytes());
    }
    if path == "/favicon" {
        return favicon(query);
    }
    if path == "/wallpaper" {
        return wallpaper();
    }
    let name = if let Some(rest) = path.strip_prefix("/ui/") {
        rest.to_string()
    } else if host == "ui" {
        path.trim_start_matches('/').to_string()
    } else if PAGES.contains(&host) && (path == "/" || path.is_empty()) {
        format!("{host}.html")
    } else {
        return Reply::not_found();
    };
    let Some(data) = file(&name) else {
        return Reply::not_found();
    };
    if name.ends_with(".html") {
        let html = String::from_utf8_lossy(data).replace("{{TOKEN}}", &shared::get().token);
        return Reply::new(200, "text/html", html.into_bytes());
    }
    Reply::new(200, mime(&name), data.to_vec())
}

fn favicon(query: &str) -> Reply {
    let mut host = String::new();
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        match k.as_ref() {
            "host" => host = v.to_string(),
            "url" => {
                host = url::Url::parse(&v)
                    .ok()
                    .and_then(|u| u.host_str().map(String::from))
                    .unwrap_or_default()
            }
            _ => {}
        }
    }
    if let Some(Ok(Some(png))) = shared::with_store(|s| s.favicon(&host)) {
        return Reply::new(200, "image/png", png);
    }
    Reply::new(200, "image/svg+xml", GLOBE_SVG.to_vec())
}

fn wallpaper() -> Reply {
    let path = shared::settings().appearance.wallpaper_path;
    let lower = path.to_ascii_lowercase();
    let kind = ["png", "jpg", "jpeg", "webp"]
        .into_iter()
        .find(|ext| lower.ends_with(&format!(".{ext}")));
    let (Some(kind), Ok(meta)) = (kind, std::fs::metadata(&path)) else {
        return Reply::not_found();
    };
    if !meta.is_file() || meta.len() > 25 * 1024 * 1024 {
        return Reply::not_found();
    }
    match std::fs::read(&path) {
        Ok(data) => Reply::new(200, mime(&format!("x.{kind}")), data),
        Err(_) => Reply::not_found(),
    }
}

/// Compares secrets without leaking how much matched through timing.
fn same_secret(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn post_body(request: &mut Request) -> Vec<u8> {
    let mut body = Vec::new();
    let Some(data) = request.post_data() else {
        return body;
    };
    // CEF fills as many elements as the vector already holds.
    let mut elements = vec![None; data.element_count()];
    data.elements(Some(&mut elements));
    for element in elements.into_iter().flatten() {
        let n = element.bytes_count();
        if n == 0 || body.len() + n > 4 * 1024 * 1024 {
            continue;
        }
        let start = body.len();
        body.resize(start + n, 0);
        element.bytes(n, body[start..].as_mut_ptr());
    }
    body
}

wrap_resource_handler! {
    struct Handler {
        reply: Arc<Mutex<Reply>>,
        browser_id: i32,
        frame_url: String,
    }

    impl ResourceHandler {
        fn open(
            &self,
            request: Option<&mut Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            let (Some(request), Some(handle_request)) = (request, handle_request) else {
                return 0;
            };
            let Ok(url) = url::Url::parse(&us(&request.url())) else {
                *handle_request = 1;
                *self.reply.lock().unwrap() = Reply::not_found();
                return 1;
            };
            let host = url.host_str().unwrap_or("").to_string();
            let path = url.path().to_string();
            let method = us(&request.method());
            let Some(cmd) = path.strip_prefix("/api/").map(String::from) else {
                *handle_request = 1;
                *self.reply.lock().unwrap() = if method == "GET" {
                    serve(&host, &path, url.query().unwrap_or(""))
                } else {
                    Reply::not_found()
                };
                return 1;
            };
            // Only our own pages, with this session's token, by POST.
            let from = url::Url::parse(&self.frame_url).ok();
            let from_host = from
                .as_ref()
                .filter(|u| u.scheme() == cottoncandy_core::INTERNAL_SCHEME)
                .and_then(|u| u.host_str().map(String::from));
            let token = us(&request.header_by_name(Some(&cs("X-CC-Token"))));
            let trusted = from_host.as_deref() == Some(host.as_str())
                && method == "POST"
                && same_secret(token.as_bytes(), shared::get().token.as_bytes());
            if std::env::var_os("CC_DEBUG").is_some() && !(trusted && api::allowed(&host, &cmd)) {
                eprintln!(
                    "[api] refused {cmd}: frame={} host={host} method={method} token_ok={}",
                    self.frame_url,
                    same_secret(token.as_bytes(), shared::get().token.as_bytes())
                );
            }
            let Some(from_host) = from_host.filter(|_| trusted && api::allowed(&host, &cmd)) else {
                *handle_request = 1;
                *self.reply.lock().unwrap() = Reply::json(403, &serde_json::json!({"error": "not allowed"}));
                return 1;
            };
            let body = post_body(request);
            let args: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
            let Some(callback) = callback.cloned() else {
                return 0;
            };
            *handle_request = 0;
            let reply = self.reply.clone();
            let browser_id = self.browser_id;
            on_ui(move || {
                let result = api::dispatch(&cmd, &args, browser_id, &from_host);
                *reply.lock().unwrap() = match result {
                    Ok(value) => Reply::json(200, &value),
                    Err(error) => Reply::json(400, &serde_json::json!({ "error": error })),
                };
                callback.cont();
            });
            1
        }

        fn response_headers(
            &self,
            response: Option<&mut Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let reply = self.reply.lock().unwrap();
            if let Some(response) = response {
                response.set_status(reply.status);
                response.set_mime_type(Some(&cs(&reply.mime)));
                if reply.mime.starts_with("text/") {
                    response.set_charset(Some(&cs("utf-8")));
                }
                for (name, value) in [
                    ("X-Content-Type-Options", "nosniff"),
                    ("Cache-Control", "no-store"),
                    ("Referrer-Policy", "no-referrer"),
                    ("Cross-Origin-Opener-Policy", "same-origin"),
                    ("Cross-Origin-Resource-Policy", "same-origin"),
                    ("X-Frame-Options", "DENY"),
                ] {
                    response.set_header_by_name(Some(&cs(name)), Some(&cs(value)), 1);
                }
            }
            if let Some(length) = response_length {
                *length = reply.body.len() as i64;
            }
        }

        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: ::std::os::raw::c_int,
            bytes_read: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> ::std::os::raw::c_int {
            let mut reply = self.reply.lock().unwrap();
            let left = reply.body.len().saturating_sub(reply.sent);
            let n = left.min(bytes_to_read.max(0) as usize);
            if let Some(bytes_read) = bytes_read {
                *bytes_read = n as i32;
            }
            if n == 0 || data_out.is_null() {
                return 0;
            }
            // SAFETY: CEF gives a buffer of `bytes_to_read` bytes and n <= bytes_to_read.
            unsafe { std::ptr::copy_nonoverlapping(reply.body[reply.sent..].as_ptr(), data_out, n) };
            reply.sent += n;
            1
        }

        fn cancel(&self) {}
    }
}

wrap_scheme_handler_factory! {
    struct Factory {}

    impl SchemeHandlerFactory {
        fn create(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            _request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            Some(Handler::new(
                Arc::new(Mutex::new(Reply::default())),
                browser.map(|b| b.identifier()).unwrap_or(0),
                frame_url(frame),
            ))
        }
    }
}

pub fn register() {
    let mut factory = Factory::new();
    register_scheme_handler_factory(
        Some(&cs(cottoncandy_core::INTERNAL_SCHEME)),
        None,
        Some(&mut factory),
    );
}

/// A request context made after startup (a private window) doesn't see the global factory.
pub fn register_on(context: &RequestContext) {
    let mut factory = Factory::new();
    context.register_scheme_handler_factory(
        Some(&cs(cottoncandy_core::INTERNAL_SCHEME)),
        None,
        Some(&mut factory),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_has_its_file() {
        for page in PAGES {
            assert!(file(&format!("{page}.html")).is_some(), "{page}");
        }
    }

    #[test]
    fn every_script_and_style_a_page_loads_is_built_in() {
        for (name, data) in FILES {
            if !name.ends_with(".html") {
                continue;
            }
            let html = String::from_utf8_lossy(data);
            assert!(html.contains("{{TOKEN}}"), "{name} has no token meta");
            assert!(
                html.contains("Content-Security-Policy"),
                "{name} has no CSP"
            );
            for part in html.split("/ui/").skip(1) {
                let file_name: String = part.chars().take_while(|c| *c != '"').collect();
                assert!(
                    file(&file_name).is_some(),
                    "{name} loads missing {file_name}"
                );
            }
        }
    }

    #[test]
    fn unknown_paths_are_not_found() {
        assert_eq!(serve("ui", "/../../etc/passwd", "").status, 404);
        assert_eq!(serve("newtab", "/nope", "").status, 404);
        assert_eq!(serve("evil", "/", "").status, 404);
    }
}
