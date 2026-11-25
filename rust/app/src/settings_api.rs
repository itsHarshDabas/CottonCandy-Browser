//! The Settings page's calls (cottoncandy://settings/) and the toolbar's quick proxy switch: reading and
//! changing settings, putting changes into effect, extensions, bookmark files and clearing browsing data.

use crate::browser::{self, Ui};
use crate::cefx::*;
use crate::shared;
use cef::*;
use cottoncandy_core::settings::{self as cs_settings, Settings};
use cottoncandy_core::{bookmarks_html, extensions as ext, import};
use serde_json::{json, Value as Json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type Res = Result<Json, String>;

/// Set when a change only takes effect after a restart (extensions, dark web pages, WebRTC policy).
static RESTART_NEEDED: AtomicBool = AtomicBool::new(false);
/// Set by the Restart button; main() starts CottonCandy again after shutting down.
pub static RESTART_REQUESTED: AtomicBool = AtomicBool::new(false);

fn arg_str<'a>(args: &'a Json, key: &str) -> &'a str {
    args.get(key).and_then(Json::as_str).unwrap_or("")
}

pub fn call(ui: &mut Ui, browser_id: i32, cmd: &str, args: &Json) -> Res {
    match cmd {
        "settings_get" => Ok(snapshot()),
        "settings_set" => {
            let path = arg_str(args, "path");
            let value = args.get("value").cloned().unwrap_or(Json::Null);
            change(ui, |s| cs_settings::apply_change(s, path, value))?;
            Ok(snapshot())
        }
        "settings_reset" => {
            let section = arg_str(args, "section").to_string();
            change(ui, |s| {
                let d = Settings::default();
                let mut s = s.clone();
                match section.as_str() {
                    "appearance" => s.appearance = d.appearance,
                    "shields" => s.shields = d.shields,
                    "network" => s.network = d.network,
                    "privacy" => s.privacy = d.privacy,
                    "sites" => s.sites = d.sites,
                    "all" => {
                        // Installed extensions stay installed.
                        let extensions = s.extensions.clone();
                        s = d;
                        s.extensions = extensions;
                    }
                    _ => return Err("unknown section".into()),
                }
                Ok(s)
            })?;
            Ok(snapshot())
        }
        "import_sources" => Ok(json!({
            "sources": import::detect(&import::Roots::here()).iter().map(|p| json!({
                "key": p.dir.to_string_lossy(),
                "browser": p.browser,
                "name": p.name,
                "firefox": p.kind == import::Kind::Firefox,
            })).collect::<Vec<_>>()
        })),
        "import_run" => {
            // Only a profile that detection finds again can be read: the page names one, it can't point
            // anywhere else on disk.
            let key = arg_str(args, "key");
            let profile = import::detect(&import::Roots::here())
                .into_iter()
                .find(|p| p.dir.to_string_lossy() == key)
                .ok_or("That browser profile is gone.")?;
            let want = |k: &str| args.get(k).and_then(Json::as_bool).unwrap_or(false);
            let what = ImportWhat {
                bookmarks: want("bookmarks"),
                history: want("history"),
                tabs: want("tabs"),
                extensions: want("extensions"),
            };
            std::thread::spawn(move || run_import(browser_id, profile, what));
            Ok(json!({ "started": true }))
        }
        "ext_install" => {
            let source = ext::parse_store_url(arg_str(args, "url")).map_err(|e| e.to_string())?;
            install_extension(browser_id, source);
            Ok(json!({ "started": true }))
        }
        "ext_toggle" | "ext_remove" => {
            let id = arg_str(args, "id").to_string();
            let remove = cmd == "ext_remove";
            let enabled = args.get("enabled").and_then(Json::as_bool).unwrap_or(false);
            change(ui, |s| {
                let mut s = s.clone();
                if remove {
                    s.extensions.installed.remove(&id).ok_or("not installed")?;
                } else {
                    s.extensions
                        .installed
                        .get_mut(&id)
                        .ok_or("not installed")?
                        .enabled = enabled;
                }
                Ok(s)
            })?;
            if remove {
                if let Some(dir) = ext::install_dir(&shared::get().data_dir, &id) {
                    let _ = std::fs::remove_dir_all(dir);
                }
            }
            Ok(snapshot())
        }
        "bookmarks_import" => {
            let browser = page_browser(ui, browser_id).ok_or("no page")?;
            let host = browser.host().ok_or("no page")?;
            let mut filters = CefStringList::new();
            filters.append(".html");
            filters.append(".htm");
            let mut callback = Picked::new(browser.clone(), "import");
            host.run_file_dialog(
                FileDialogMode::from(cef_dll_sys::cef_file_dialog_mode_t::FILE_DIALOG_OPEN),
                Some(&cs("Import bookmarks")),
                None,
                Some(&mut filters),
                Some(&mut callback),
            );
            Ok(json!({}))
        }
        "bookmarks_export" => {
            let items = shared::with_store(|s| s.all_bookmarks())
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            let folder = download_folder();
            let _ = std::fs::create_dir_all(&folder);
            let name = format!("CottonCandy bookmarks {}.html", today());
            let path = crate::client::unique_path(&folder, &name);
            std::fs::write(&path, bookmarks_html::export(&items)).map_err(|e| e.to_string())?;
            Ok(json!({ "path": path.to_string_lossy(), "count": items.len() }))
        }
        "pick_folder" => {
            let browser = page_browser(ui, browser_id).ok_or("no page")?;
            let host = browser.host().ok_or("no page")?;
            let mut callback = Picked::new(browser.clone(), "folder");
            host.run_file_dialog(
                FileDialogMode::from(cef_dll_sys::cef_file_dialog_mode_t::FILE_DIALOG_OPEN_FOLDER),
                Some(&cs("Save downloads in")),
                Some(&cs(&download_folder().to_string_lossy())),
                None,
                Some(&mut callback),
            );
            Ok(json!({}))
        }
        "clear_data" => clear_data(ui, args),
        "restart" => {
            RESTART_REQUESTED.store(true, Ordering::SeqCst);
            ui.save_session();
            for w in &ui.windows {
                close_window(&w.window);
            }
            Ok(json!({}))
        }
        _ => Err(format!("unknown call {cmd}")),
    }
}

/// The toolbar's Proxy and VPN panel: the on/off switch, a preset, and secure DNS.
pub fn proxy_set(ui: &mut Ui, args: &Json) -> Res {
    let path = arg_str(args, "path");
    let value = args.get("value").cloned().unwrap_or(Json::Null);
    let allowed = [
        "network.proxy.enabled",
        "network.proxy.preset",
        "network.dns.mode",
        "network.dns.provider",
    ];
    if !allowed.contains(&path) {
        return Err("not allowed".into());
    }
    change(ui, |s| {
        let mut s = cs_settings::apply_change(s, path, value.clone())?;
        if path == "network.proxy.preset" {
            if let Some(p) = cs_settings::PROXY_PRESETS
                .iter()
                .find(|p| p.0 == s.network.proxy.preset)
            {
                let proxy = &mut s.network.proxy;
                proxy.mode = "fixed".into();
                proxy.scheme = p.2.into();
                proxy.host = p.3.into();
                proxy.port = p.4;
                proxy.enabled = true;
            }
        }
        if s.network.proxy.enabled {
            s.network.proxy.validate()?;
        }
        Ok(s)
    })?;
    let mut info = crate::api::popup_info(ui, 0, "proxy").unwrap_or(json!({}));
    info["ok"] = json!(true);
    Ok(info)
}

/// Everything the Settings page shows: the settings and the choices it offers.
fn snapshot() -> Json {
    let s = shared::settings();
    let pairs = |list: &[(&str, &str, &str)]| -> Vec<Json> {
        list.iter()
            .map(|p| json!({"id": p.0, "label": p.1}))
            .collect()
    };
    let accents: Vec<Json> = cs_settings::ACCENTS
        .iter()
        .map(|a| json!({"id": a.0, "color": a.1}))
        .collect();
    let presets: Vec<Json> = cs_settings::PROXY_PRESETS
        .iter()
        .map(|p| json!({"id": p.0, "label": p.1, "scheme": p.2, "host": p.3, "port": p.4}))
        .collect();
    let lists: Vec<Json> = cottoncandy_core::shields::LISTS
        .iter()
        .map(|l| json!({"id": l.id, "label": l.name}))
        .collect();
    let extensions: Vec<Json> = s
        .extensions
        .installed
        .iter()
        .map(|(id, e)| json!({"id": id, "name": e.name, "version": e.version, "source": e.source, "enabled": e.enabled}))
        .collect();
    json!({
        "settings": s,
        "proxy_error": if s.network.proxy.enabled { s.network.proxy.validate().err() } else { None },
        "dns_template": s.network.dns.template(),
        "restart_needed": RESTART_NEEDED.load(Ordering::Relaxed),
        "options": {
            "engines": pairs(cs_settings::SEARCH_ENGINES),
            "dns": pairs(cs_settings::DNS_PROVIDERS),
            "accents": accents,
            "presets": presets,
            "lists": lists,
            "extensions": extensions,
            "downloads_default": cottoncandy_core::paths::downloads_dir().to_string_lossy(),
        },
    })
}

/// Applies `f` to the settings, saves the result and puts it into effect.
pub fn change(
    ui: &mut Ui,
    f: impl FnOnce(&Settings) -> Result<Settings, String>,
) -> Result<(), String> {
    let old = shared::settings();
    let new = f(&old)?;
    if new == old {
        return Ok(());
    }
    shared::set_settings(new.clone())?;
    apply(ui, &old, &new);
    Ok(())
}

fn apply(ui: &mut Ui, old: &Settings, new: &Settings) {
    let network_changed = old.network != new.network
        || old.privacy != new.privacy
        || old.shields.send_gpc != new.shields.send_gpc
        || old.shields.block_third_party_cookies != new.shields.block_third_party_cookies;
    if network_changed {
        browser::apply_network_settings(None);
        for w in &ui.windows {
            if let Some(c) = &w.context {
                browser::apply_network_settings(Some(c));
            }
        }
    }
    if old.shields.filter_lists != new.shields.filter_lists
        || old.shields.custom_rules != new.shields.custom_rules
    {
        crate::shields::rebuild();
        crate::shields::update_lists();
    }
    if old.sites != new.sites || old.shields.block_scripts != new.shields.block_scripts {
        browser::apply_site_settings(ui, new);
    }
    if old.appearance != new.appearance {
        refresh_theme(ui);
        ui.relayout();
    }
    if old.appearance.dark_web_pages != new.appearance.dark_web_pages
        || old.extensions != new.extensions
        || old.privacy.webrtc_ip_policy != new.privacy.webrtc_ip_policy
    {
        RESTART_NEEDED.store(true, Ordering::Relaxed);
    }
    ui.mark_all();
}

/// Makes every CottonCandy page pick up the new colors and sizes without reloading.
fn refresh_theme(ui: &Ui) {
    const JS: &str = "document.querySelectorAll('link[href^=\"/theme.css\"]').forEach(l => l.href = '/theme.css?' + Date.now());";
    for w in &ui.windows {
        for b in w.browsers.values() {
            if frame_url(b.main_frame().as_mut()).starts_with("cottoncandy://") {
                run_js(b, JS);
            }
        }
    }
}

fn page_browser(ui: &Ui, browser_id: i32) -> Option<Browser> {
    ui.windows
        .iter()
        .find_map(|w| w.browsers.get(&browser_id).cloned())
}

fn download_folder() -> std::path::PathBuf {
    let s = shared::settings();
    if s.downloads.folder.trim().is_empty() {
        cottoncandy_core::paths::downloads_dir()
    } else {
        s.downloads.folder.into()
    }
}

fn today() -> String {
    // Days since 1970 to a civil date (Howard Hinnant's algorithm), so no date crate is needed.
    let days = now() / 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Clears what the user ticked on the Clear browsing data dialog.
fn clear_data(ui: &mut Ui, args: &Json) -> Res {
    let since = match arg_str(args, "range") {
        "hour" => now() - 3600,
        "day" => now() - 86_400,
        "week" => now() - 7 * 86_400,
        _ => 0,
    };
    let what = |k: &str| args.get(k).and_then(Json::as_bool).unwrap_or(false);
    let mut done = Vec::new();
    if what("history") {
        let _ = shared::with_store(|s| s.clear_history(since));
        done.push("history");
    }
    let mut contexts: Vec<RequestContext> =
        request_context_get_global_context().into_iter().collect();
    contexts.extend(ui.windows.iter().filter_map(|w| w.context.clone()));
    for c in &contexts {
        if what("cookies") {
            if let Some(m) = c.cookie_manager(None) {
                m.delete_cookies(None, None, None);
            }
        }
        if what("cache") {
            c.clear_http_cache(None);
        }
    }
    if what("cookies") {
        done.push("cookies");
    }
    if what("cache") {
        done.push("cache");
    }
    if what("downloads") {
        ui.downloads.retain(|d| d.state == "progress");
        done.push("downloads");
    }
    ui.mark_all();
    Ok(json!({ "cleared": done }))
}

// ---- file dialogs ---------------------------------------------------------------------------------

wrap_run_file_dialog_callback! {
    struct Picked {
        browser: Browser,
        purpose: &'static str,
    }

    impl RunFileDialogCallback {
        fn on_file_dialog_dismissed(&self, file_paths: Option<&mut CefStringList>) {
            let paths: Vec<String> = file_paths
                // Cloning a borrowed CefStringList loses its contents; take it instead.
                .map(|list| std::mem::take(list).into_iter().map(|p| p.to_string()).collect())
                .unwrap_or_default();
            if std::env::var_os("CC_DEBUG").is_some() {
                eprintln!("[dialog] {} picked {paths:?}", self.purpose);
            }
            let path = paths.into_iter().next().unwrap_or_default();
            if path.is_empty() {
                return;
            }
            let reply = match self.purpose {
                "import" => import_bookmarks(&path),
                _ => json!({ "folder": path }),
            };
            run_js(&self.browser, &emit_js(self.purpose, &reply));
        }
    }
}

struct ImportWhat {
    bookmarks: bool,
    history: bool,
    tabs: bool,
    extensions: bool,
}

/// Reads what was asked for from another browser's profile (on a worker thread), stores it, opens the
/// tabs in a new window and starts installing the extensions, then tells the page what came over.
fn run_import(browser_id: i32, profile: import::Profile, what: ImportWhat) {
    let scratch = shared::get().cache_dir.join("import");
    let now = now();
    let mut done = serde_json::Map::new();
    let mut errors: Vec<String> = Vec::new();
    let label = format!("{} ({})", profile.browser, profile.name);
    if what.bookmarks {
        match import::bookmarks(&profile, &scratch) {
            Ok(items) => {
                let folder_root = format!("Imported from {}", profile.browser);
                let n = shared::with_store(|s| {
                    items
                        .iter()
                        .filter(|b| {
                            let folder = format!("{folder_root}/{}", b.folder);
                            s.add_bookmark(&b.url, &b.title, &folder, now)
                                .unwrap_or(false)
                        })
                        .count()
                })
                .unwrap_or(0);
                done.insert("bookmarks".into(), json!(n));
            }
            Err(e) => errors.push(format!("Bookmarks: {e}")),
        }
    }
    let mut titles = std::collections::HashMap::new();
    if what.history {
        match import::history(&profile, &scratch) {
            Ok(visits) => {
                titles.extend(visits.iter().map(|v| (v.url.clone(), v.title.clone())));
                let n = shared::with_store(|s| s.import_visits(&visits))
                    .transpose()
                    .map_err(|e| e.to_string());
                match n {
                    Ok(n) => {
                        done.insert("history".into(), json!(n.unwrap_or(0)));
                    }
                    Err(e) => errors.push(format!("History: {e}")),
                }
            }
            Err(e) => errors.push(format!("History: {e}")),
        }
    }
    let mut urls = Vec::new();
    if what.tabs {
        match import::tabs(&profile) {
            Ok(tabs) => {
                // A session file often has no titles yet; the other browser's history does.
                if titles.is_empty() && tabs.iter().any(|t| t.title.is_empty()) {
                    if let Ok(visits) = import::history(&profile, &scratch) {
                        titles.extend(visits.into_iter().map(|v| (v.url, v.title)));
                    }
                }
                let folder = format!("Imported from {}/Open tabs", profile.browser);
                shared::with_store(|s| {
                    for t in &tabs {
                        let title = if t.title.is_empty() {
                            titles.get(&t.url).map(String::as_str).unwrap_or("")
                        } else {
                            &t.title
                        };
                        let _ = s.add_bookmark(&t.url, title, &folder, now);
                    }
                });
                urls = tabs.into_iter().map(|t| t.url).collect();
                done.insert("tabs".into(), json!(urls.len()));
            }
            Err(e) => errors.push(format!("Open tabs: {e}")),
        }
    }
    let extensions = if what.extensions {
        import::extensions(&profile)
    } else {
        Vec::new()
    };
    if what.extensions {
        done.insert(
            "extensions".into(),
            json!(extensions
                .iter()
                .map(|e| e.name.clone())
                .collect::<Vec<_>>()),
        );
    }
    done.insert("from".into(), json!(label));
    done.insert("errors".into(), json!(errors));
    if !urls.is_empty() {
        on_ui(move || browser::open_window(false, urls));
    }
    browser::later(move |ui| {
        for e in extensions {
            install_extension(
                browser_id,
                match e.kind {
                    import::Kind::Chromium => ext::Source::Chrome(e.id),
                    import::Kind::Firefox => ext::Source::Firefox(e.id),
                },
            );
        }
        if let Some(b) = page_browser(ui, browser_id) {
            run_js(&b, &emit_js("import_done", &Json::Object(done)));
        }
    });
}

fn import_bookmarks(path: &str) -> Json {
    let Ok(meta) = std::fs::metadata(path) else {
        return json!({ "error": "Couldn't open that file." });
    };
    if meta.len() > 50 * 1024 * 1024 {
        return json!({ "error": "That file is too big to be a bookmark file." });
    }
    let text = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => return json!({ "error": e.to_string() }),
    };
    let items = bookmarks_html::parse(&text);
    let now = now();
    let added = shared::with_store(|s| {
        items
            .iter()
            .filter(|b| {
                s.add_bookmark(&b.url, &b.title, &b.folder, now)
                    .unwrap_or(false)
            })
            .count()
    })
    .unwrap_or(0);
    if items.is_empty() {
        json!({ "error": "No bookmarks found in that file." })
    } else {
        json!({ "count": added })
    }
}

// ---- extensions -------------------------------------------------------------------------------------

type Done = Arc<dyn Fn(Result<Vec<u8>, String>) + Send + Sync>;

wrap_urlrequest_client! {
    struct Fetch {
        body: Arc<Mutex<Vec<u8>>>,
        max: usize,
        done: Done,
    }

    impl UrlrequestClient {
        fn on_download_data(&self, request: Option<&mut Urlrequest>, data: *const u8, data_length: usize) {
            if data.is_null() {
                return;
            }
            let mut body = self.body.lock().unwrap();
            if body.len() + data_length > self.max {
                if let Some(r) = request {
                    r.cancel();
                }
                return;
            }
            // SAFETY: CEF hands us `data_length` readable bytes for the length of this call.
            body.extend_from_slice(unsafe { std::slice::from_raw_parts(data, data_length) });
        }

        fn on_request_complete(&self, request: Option<&mut Urlrequest>) {
            let Some(request) = request else { return };
            let ok = request.request_status()
                == UrlrequestStatus::from(cef_dll_sys::cef_urlrequest_status_t::UR_SUCCESS);
            let status = request.response().map(|r| r.status()).unwrap_or(0);
            let body = std::mem::take(&mut *self.body.lock().unwrap());
            let result = if ok && status == 200 {
                Ok(body)
            } else if ok {
                Err(format!("The server answered with error {status}."))
            } else {
                Err("Couldn't reach the server. Check your connection or proxy.".into())
            };
            (self.done)(result);
        }
    }
}

/// Downloads `url` (at most `max` bytes) through the normal network settings (proxy, DNS) and calls `done`
/// on the UI thread.
pub(crate) fn fetch(url: &str, max: usize, done: Done) {
    let Some(mut request) = request_create() else {
        done(Err("Couldn't start the download.".into()));
        return;
    };
    request.set_url(Some(&cs(url)));
    request.set_method(Some(&cs("GET")));
    let mut client = Fetch::new(Arc::new(Mutex::new(Vec::new())), max, done);
    let mut context = request_context_get_global_context();
    let _ = urlrequest_create(Some(&mut request), Some(&mut client), context.as_mut());
}

fn install_extension(browser_id: i32, source: ext::Source) {
    let report = move |msg: Json| {
        browser::later(move |ui| {
            if let Some(b) = page_browser(ui, browser_id) {
                run_js(&b, &emit_js("ext_result", &msg));
            }
        });
    };
    let chromium = format!(
        "{}.{}.{}.{}",
        cef_dll_sys::CHROME_VERSION_MAJOR,
        cef_dll_sys::CHROME_VERSION_MINOR,
        cef_dll_sys::CHROME_VERSION_BUILD,
        cef_dll_sys::CHROME_VERSION_PATCH
    );
    match source {
        ext::Source::Chrome(id) => {
            let report2 = report;
            fetch(
                &ext::chrome_download_url(&id, &chromium),
                ext::MAX_PACKAGE_BYTES,
                Arc::new(move |result| {
                    let report = report2;
                    let id = id.clone();
                    // Checking and unpacking can take a moment; keep it off the UI thread.
                    std::thread::spawn(move || {
                        report(finish_chrome(&id, result));
                    });
                }),
            );
        }
        ext::Source::Firefox(slug) => {
            let report2 = report;
            fetch(
                &ext::firefox_api_url(&slug),
                1024 * 1024,
                Arc::new(move |result| {
                    let xpi = result.and_then(|body| {
                        ext::firefox_xpi_url(&String::from_utf8_lossy(&body))
                            .map_err(|e| e.to_string())
                    });
                    let report = report2;
                    let slug = slug.clone();
                    match xpi {
                        Err(e) => report(json!({ "error": e })),
                        Ok(url) => fetch(
                            &url,
                            ext::MAX_PACKAGE_BYTES,
                            Arc::new(move |result| {
                                let slug = slug.clone();
                                std::thread::spawn(move || report(finish_firefox(&slug, result)));
                            }),
                        ),
                    }
                }),
            );
        }
    }
}

fn finish_chrome(id: &str, result: Result<Vec<u8>, String>) -> Json {
    let outcome = result.and_then(|file| {
        let (got_id, zip) = ext::verify_crx3(&file, Some(id)).map_err(|e| e.to_string())?;
        let dir = ext::install_dir(&shared::get().data_dir, &got_id).ok_or("bad extension id")?;
        ext::unpack(zip, &dir).map_err(|e| e.to_string())?;
        let (name, version) = ext::check_manifest(&dir).map_err(|e| {
            let _ = std::fs::remove_dir_all(&dir);
            e.to_string()
        })?;
        Ok((got_id, name, version, "chrome"))
    });
    record_install(outcome)
}

fn finish_firefox(slug: &str, result: Result<Vec<u8>, String>) -> Json {
    let outcome = result.and_then(|file| {
        let id = ext::firefox_dir_name(slug);
        let dir = ext::install_dir(&shared::get().data_dir, &id).ok_or("bad add-on name")?;
        ext::unpack(&file, &dir).map_err(|e| e.to_string())?;
        let (name, version) = ext::convert_firefox(&dir).map_err(|e| {
            let _ = std::fs::remove_dir_all(&dir);
            e.to_string()
        })?;
        Ok((id, name, version, "firefox"))
    });
    record_install(outcome)
}

fn record_install(outcome: Result<(String, String, String, &'static str), String>) -> Json {
    match outcome {
        Err(e) => json!({ "error": e }),
        Ok((id, name, version, source)) => {
            let reply = json!({ "installed": name, "id": id });
            browser::later(move |ui| {
                let _ = change(ui, |s| {
                    let mut s = s.clone();
                    s.extensions.installed.insert(
                        id,
                        cs_settings::InstalledExtension {
                            name,
                            version,
                            source: source.into(),
                            enabled: true,
                        },
                    );
                    Ok(s)
                });
            });
            reply
        }
    }
}
