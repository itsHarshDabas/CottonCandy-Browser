//! Downloads the browser itself needs (a new tab picture from a link, the CherryColaLLM installer), through
//! Chromium's network stack so they use the same proxy, secure DNS and certificates as pages do.
//! Runs on the UI thread; the answer comes back on the UI thread.

use crate::cefx::*;
use cef::*;
use cef_dll_sys::cef_urlrequest_status_t as st;
use std::sync::{Arc, Mutex};

pub struct Fetched {
    pub body: Vec<u8>,
}

type Done = Box<dyn FnOnce(Result<Fetched, String>) + Send>;
type Progress = Arc<dyn Fn(i64, i64) + Send + Sync>;

wrap_urlrequest_client! {
    struct Client {
        body: Arc<Mutex<Vec<u8>>>,
        limit: usize,
        too_big: Arc<Mutex<bool>>,
        progress: Option<Progress>,
        done: Arc<Mutex<Option<Done>>>,
    }

    impl UrlrequestClient {
        fn on_download_progress(&self, _request: Option<&mut Urlrequest>, current: i64, total: i64) {
            if let Some(p) = &self.progress {
                p(current, total);
            }
        }

        fn on_download_data(&self, request: Option<&mut Urlrequest>, data: *const u8, data_length: usize) {
            if data.is_null() || data_length == 0 {
                return;
            }
            let Ok(mut body) = self.body.lock() else { return };
            if body.len() + data_length > self.limit {
                if let Ok(mut big) = self.too_big.lock() {
                    *big = true;
                }
                if let Some(r) = request {
                    r.cancel();
                }
                return;
            }
            // SAFETY: CEF hands us `data_length` readable bytes for the length of this call.
            body.extend_from_slice(unsafe { std::slice::from_raw_parts(data, data_length) });
        }

        fn on_request_complete(&self, request: Option<&mut Urlrequest>) {
            let Some(done) = self.done.lock().ok().and_then(|mut d| d.take()) else { return };
            let too_big = self.too_big.lock().map(|b| *b).unwrap_or(false);
            let result = match request {
                _ if too_big => Err(format!("The file is bigger than {} MB.", self.limit / (1024 * 1024))),
                None => Err("The download stopped.".into()),
                Some(r) => {
                    let status = r.response().map(|x| x.status()).unwrap_or(0);
                    if *r.request_status().as_ref() != st::UR_SUCCESS {
                        Err(format!("Couldn't download it (network error {}).", *r.request_error().as_ref() as i32))
                    } else if !(200..300).contains(&status) {
                        Err(format!("The server answered {status}."))
                    } else {
                        Ok(Fetched { body: std::mem::take(&mut *self.body.lock().unwrap()) })
                    }
                }
            };
            on_ui(move || done(result));
        }
    }
}

/// Downloads `url` (https only), at most `limit` bytes, then calls `done` on the UI thread.
pub fn get(
    url: &str,
    accept: &str,
    limit: usize,
    progress: Option<Progress>,
    done: impl FnOnce(Result<Fetched, String>) + Send + 'static,
) {
    if !url.starts_with("https://") {
        done(Err("Only https:// links can be downloaded.".into()));
        return;
    }
    let Some(mut request) = request_create() else {
        done(Err("Couldn't start the download.".into()));
        return;
    };
    request.set_url(Some(&cs(url)));
    request.set_method(Some(&cs("GET")));
    if !accept.is_empty() {
        request.set_header_by_name(Some(&cs("Accept")), Some(&cs(accept)), 1);
    }
    let done: Arc<Mutex<Option<Done>>> = Arc::new(Mutex::new(Some(Box::new(done))));
    let mut client = Client::new(
        Arc::new(Mutex::new(Vec::new())),
        limit,
        Arc::new(Mutex::new(false)),
        progress,
        done.clone(),
    );
    let mut context = request_context_get_global_context();
    if urlrequest_create(Some(&mut request), Some(&mut client), context.as_mut()).is_none() {
        if let Some(done) = done.lock().ok().and_then(|mut d| d.take()) {
            done(Err("Couldn't start the download.".into()));
        }
    }
}

/// What kind of picture `data` is, from its first bytes (never from the name or the server's word).
pub fn image_kind(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if data.len() > 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("webp")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("gif")
    } else if data.len() > 12
        && &data[4..8] == b"ftyp"
        && (&data[8..12] == b"avif" || &data[8..12] == b"avis")
    {
        Some("avif")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::image_kind;

    #[test]
    fn knows_pictures_by_their_bytes() {
        assert_eq!(image_kind(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(image_kind(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_kind(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_kind(b"<html>"), None);
    }
}
