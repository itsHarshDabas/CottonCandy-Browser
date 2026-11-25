//! Shields at run time. Every request a tab makes goes past `TabRequests` on CEF's network thread: ads and
//! trackers are cancelled by the filter engine, plain http pages are moved to https, tracking parameters
//! come off links and Global Privacy Control is sent. The engine is rebuilt off the UI thread whenever the
//! lists or the user's rules change, and the lists are fetched again when they are four days old.

use crate::browser::{later, TabShared};
use crate::cefx::*;
use crate::shared;
use cef::*;
use cottoncandy_core::settings::Settings;
use cottoncandy_core::shields::{self as filters, Shields, Verdict};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::SystemTime;

static ENGINE: OnceLock<RwLock<Arc<Shields>>> = OnceLock::new();
/// Bumped for every rebuild, so a slow old build can't replace a newer one.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Hosts whose http pages were moved to https, and hosts where https failed and http is allowed again.
static UPGRADED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static HTTP_OK: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Hosts we gave a script content setting, so a removed site can go back to the default.
static SCRIPT_HOSTS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
const MAX_REMEMBERED_HOSTS: usize = 2000;
const LIST_CHECK_MS: i64 = 6 * 3600 * 1000;

fn slot() -> &'static RwLock<Arc<Shields>> {
    ENGINE.get_or_init(|| RwLock::new(Arc::new(Shields::built_in())))
}

pub fn engine() -> Arc<Shields> {
    match slot().read() {
        Ok(e) => e.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

fn debug(message: impl AsRef<str>) {
    if std::env::var_os("CC_DEBUG").is_some() {
        eprintln!("[shields] {}", message.as_ref());
    }
}

/// Builds the engine from the built-in lists, the cached lists that are on and the user's rules.
pub fn rebuild() {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let settings = shared::settings();
        let started = std::time::Instant::now();
        let built = Shields::build(
            &shared::get().cache_dir,
            &settings.shields.filter_lists,
            &settings.shields.custom_rules,
        );
        debug(format!(
            "built {} rules in {} ms",
            built.rule_count,
            started.elapsed().as_millis()
        ));
        if GENERATION.load(Ordering::SeqCst) == generation {
            if let Ok(mut e) = slot().write() {
                *e = Arc::new(built);
            }
        }
    });
}

/// Called once the browser is up: build from the cache now, fetch stale lists a little later.
pub fn start() {
    rebuild();
    on_ui_later(5_000, check_lists_periodically);
}

fn check_lists_periodically() {
    update_lists();
    on_ui_later(LIST_CHECK_MS, check_lists_periodically);
}

/// Downloads the lists that are on and missing or out of date, then rebuilds once they are in.
pub fn update_lists() {
    let settings = shared::settings();
    let cache = shared::get().cache_dir.clone();
    let now = SystemTime::now();
    let stale: Vec<&'static filters::FilterList> = filters::LISTS
        .iter()
        .filter(|l| settings.shields.filter_lists.iter().any(|e| e == l.id))
        .filter(|l| filters::list_is_stale(&filters::list_cache_path(&cache, l.id), now))
        .collect();
    if stale.is_empty() {
        return;
    }
    let pending = Arc::new(AtomicUsize::new(stale.len()));
    let fetched = Arc::new(AtomicUsize::new(0));
    for list in stale {
        let path = filters::list_cache_path(&cache, list.id);
        let (pending, fetched) = (pending.clone(), fetched.clone());
        let id = list.id;
        crate::settings_api::fetch(
            list.url,
            filters::LIST_MAX_BYTES,
            Arc::new(move |result| {
                match result.map(String::from_utf8) {
                    Ok(Ok(text)) if filters::looks_like_filter_list(&text) => {
                        let tmp = path.with_extension("part");
                        let saved = path
                            .parent()
                            .map(std::fs::create_dir_all)
                            .unwrap_or(Ok(()))
                            .and_then(|_| std::fs::write(&tmp, &text))
                            .and_then(|_| std::fs::rename(&tmp, &path));
                        match saved {
                            Ok(()) => {
                                fetched.fetch_add(1, Ordering::SeqCst);
                                debug(format!("{id}: {} bytes", text.len()));
                            }
                            Err(e) => debug(format!("{id}: couldn't save ({e})")),
                        }
                    }
                    Ok(_) => debug(format!("{id}: not a filter list")),
                    Err(e) => debug(format!("{id}: {e}")),
                }
                if pending.fetch_sub(1, Ordering::SeqCst) == 1 && fetched.load(Ordering::SeqCst) > 0
                {
                    rebuild();
                }
            }),
        );
    }
}

/// When the https version of a page we upgraded fails, the http address to load instead (once per host).
pub fn https_fallback(failed_url: &str) -> Option<String> {
    let mut parsed = url::Url::parse(failed_url).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !UPGRADED.lock().ok()?.as_ref()?.contains(&host) {
        return None;
    }
    if !remember(&HTTP_OK, &host) {
        return None;
    }
    parsed.set_scheme("http").ok()?;
    debug(format!("https failed, back to {parsed}"));
    Some(parsed.to_string())
}

/// Adds `host` to a set; false if it was already there.
fn remember(set: &Mutex<Option<HashSet<String>>>, host: &str) -> bool {
    let Ok(mut set) = set.lock() else {
        return false;
    };
    let set = set.get_or_insert_with(HashSet::new);
    if set.len() >= MAX_REMEMBERED_HOSTS {
        set.clear();
    }
    set.insert(host.to_string())
}

fn http_allowed(host: &str) -> bool {
    HTTP_OK
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|s| s.contains(host)))
        .unwrap_or(false)
}

/// adblock's name for a CEF resource type.
fn resource_kind(kind: ResourceType) -> &'static str {
    match kind {
        ResourceType::MAIN_FRAME | ResourceType::NAVIGATION_PRELOAD_MAIN_FRAME => "main_frame",
        ResourceType::SUB_FRAME | ResourceType::NAVIGATION_PRELOAD_SUB_FRAME => "sub_frame",
        ResourceType::STYLESHEET => "stylesheet",
        ResourceType::SCRIPT
        | ResourceType::WORKER
        | ResourceType::SHARED_WORKER
        | ResourceType::SERVICE_WORKER => "script",
        ResourceType::IMAGE | ResourceType::FAVICON => "image",
        ResourceType::FONT_RESOURCE => "font",
        ResourceType::OBJECT | ResourceType::PLUGIN_RESOURCE => "object",
        ResourceType::MEDIA => "media",
        ResourceType::XHR => "xmlhttprequest",
        ResourceType::PING => "ping",
        ResourceType::CSP_REPORT => "csp_report",
        _ => "other",
    }
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
}

fn is_web(url: &str) -> bool {
    ["http://", "https://", "ws://", "wss://"]
        .iter()
        .any(|p| url.starts_with(p))
}

wrap_resource_request_handler! {
    pub struct TabRequests {
        win: u32,
        tab: Arc<TabShared>,
    }

    impl ResourceRequestHandler {
        fn on_before_resource_load(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            let Some(request) = request else { return ReturnValue::CONTINUE };
            let url = us(&request.url());
            if !is_web(&url) {
                return ReturnValue::CONTINUE;
            }
            let kind = resource_kind(request.resource_type());
            let Ok(settings) = shared::get().settings.read() else { return ReturnValue::CONTINUE };
            if settings.shields.send_gpc {
                request.set_header_by_name(Some(&cs("Sec-GPC")), Some(&cs("1")), 1);
            }
            match kind {
                "main_frame" => {
                    self.tab.reset_blocked();
                    if let Some(new) = rewrite_navigation(&settings, &url, true) {
                        request.set_url(Some(&cs(&new)));
                    }
                    ReturnValue::CONTINUE
                }
                "sub_frame" => {
                    if let Some(new) = rewrite_navigation(&settings, &url, false) {
                        request.set_url(Some(&cs(&new)));
                    }
                    self.filter(&settings, browser, &url, kind)
                }
                _ => self.filter(&settings, browser, &url, kind),
            }
        }
    }
}

impl TabRequests {
    fn filter(
        &self,
        settings: &Settings,
        browser: Option<&mut Browser>,
        url: &str,
        kind: &str,
    ) -> ReturnValue {
        let page = browser
            .and_then(|b| b.main_frame())
            .map(|f| us(&f.url()))
            .unwrap_or_default();
        if !(page.starts_with("http://") || page.starts_with("https://")) {
            return ReturnValue::CONTINUE;
        }
        let host = host_of(&page).unwrap_or_default();
        let site = settings.shields_for(&host);
        if !site.shields_up {
            return ReturnValue::CONTINUE;
        }
        if site.scripts_blocked && kind == "script" {
            return ReturnValue::CANCEL;
        }
        match engine().check(url, &page, kind, site.ads, site.trackers) {
            Verdict::Allow => ReturnValue::CONTINUE,
            Verdict::Block { tracker } => {
                debug(format!("blocked {kind} {url}"));
                self.count(tracker, url);
                ReturnValue::CANCEL
            }
            // A stand-in resource (a no-op script) keeps pages that wait for it working. Chromium refuses
            // some redirects to data: URLs, which then simply fails like a block.
            Verdict::Redirect(_) => {
                self.count(false, url);
                ReturnValue::CANCEL
            }
        }
    }

    fn count(&self, tracker: bool, url: &str) {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        let new = self
            .tab
            .blocked_seen
            .lock()
            .map(|mut seen| seen.len() < 10_000 && seen.insert(hasher.finish()))
            .unwrap_or(false);
        if !new {
            return;
        }
        let counter = if tracker {
            &self.tab.trackers_blocked
        } else {
            &self.tab.ads_blocked
        };
        counter.fetch_add(1, Ordering::Relaxed);
        // One toolbar refresh at most every 300 ms, however many requests are blocked.
        if !self.tab.counts_dirty.swap(true, Ordering::AcqRel) {
            let (tab, win) = (self.tab.clone(), self.win);
            on_ui_later(300, move || {
                tab.counts_dirty.store(false, Ordering::Release);
                later(move |ui| ui.mark(win));
            });
        }
    }
}

/// A page address with tracking parameters taken off and http moved to https, or None to leave it.
fn rewrite_navigation(settings: &Settings, url: &str, main: bool) -> Option<String> {
    let mut out = None;
    if settings.privacy.strip_tracking_params {
        out = filters::strip_tracking_params(url);
    }
    if main && settings.shields.https_only {
        let current = out.as_deref().unwrap_or(url);
        let host = host_of(current).unwrap_or_default();
        if settings.shields_for(&host).shields_up && !http_allowed(&host) {
            if let Some(https) = filters::https_upgrade(current) {
                remember(&UPGRADED, &host);
                debug(format!("upgraded to {https}"));
                out = Some(https);
            }
        }
    }
    out
}

/// Hides the boxes ads leave behind, once the new page's document exists.
pub fn inject_cosmetic(frame: &Frame, url: &str) {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    let host = host_of(url).unwrap_or_default();
    if !shared::settings().shields_for(&host).cosmetic {
        return;
    }
    let css = engine().cosmetic_css(url);
    if css.is_empty() {
        return;
    }
    let css = serde_json::to_string(&css).unwrap_or_default();
    let js = format!(
        "(()=>{{const add=()=>{{const s=document.createElement('style');s.dataset.cottoncandy='shields';\
         s.textContent={css};(document.head||document.documentElement).appendChild(s)}};\
         document.documentElement?add():document.addEventListener('readystatechange',add,{{once:true}})}})()"
    );
    frame.execute_java_script(Some(&cs(&js)), Some(&cs("")), 0);
}

/// Puts the script switches into Chromium's content settings, so inline scripts stop too (requests for
/// script files are also cancelled in `TabRequests`).
pub fn apply_scripts(context: &RequestContext, settings: &Settings) {
    let default = if settings.shields.block_scripts {
        ContentSettingValues::BLOCK
    } else {
        ContentSettingValues::DEFAULT
    };
    context.set_content_setting(None, None, ContentSettingTypes::JAVASCRIPT, default);
    let Ok(mut known) = SCRIPT_HOSTS.lock() else {
        return;
    };
    let known = known.get_or_insert_with(HashSet::new);
    let now: HashSet<String> = settings.sites.keys().cloned().collect();
    for host in known.difference(&now) {
        set_script(context, host, ContentSettingValues::DEFAULT);
    }
    for host in &now {
        let value = if settings.shields_for(host).scripts_blocked {
            ContentSettingValues::BLOCK
        } else {
            ContentSettingValues::ALLOW
        };
        set_script(context, host, value);
    }
    *known = now;
}

/// Before a tab goes to `url`: Chromium keys script settings by origin, port included, which the host-wide
/// settings above can't cover (a dev server on :8080), so the exact origin gets the site's setting too.
pub fn before_navigation(browser: &Browser, url: &str) {
    let Ok(parsed) = url::Url::parse(url) else {
        return;
    };
    if !matches!(parsed.scheme(), "http" | "https") || parsed.port().is_none() {
        return;
    }
    let Some(context) = browser.host().and_then(|h| h.request_context()) else {
        return;
    };
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let settings = shared::settings();
    let value = match settings.site(&host).and_then(|s| s.scripts) {
        _ if settings.shields_for(&host).scripts_blocked => ContentSettingValues::BLOCK,
        Some(true) => ContentSettingValues::ALLOW,
        _ if !settings.shields_for(&host).shields_up => ContentSettingValues::ALLOW,
        _ => ContentSettingValues::DEFAULT,
    };
    let origin = cs(&format!("{}/", parsed.origin().ascii_serialization()));
    context.set_content_setting(
        Some(&origin),
        Some(&origin),
        ContentSettingTypes::JAVASCRIPT,
        value,
    );
}

fn set_script(context: &RequestContext, host: &str, value: ContentSettingValues) {
    for scheme in ["https", "http"] {
        for h in [host.to_string(), format!("www.{host}")] {
            let url = cs(&format!("{scheme}://{h}/"));
            context.set_content_setting(
                Some(&url),
                Some(&url),
                ContentSettingTypes::JAVASCRIPT,
                value,
            );
        }
    }
}

/// Puts a site's permissions (camera, microphone, location, notifications, pop-ups, scripts) back to asking.
pub fn reset_site(context: &RequestContext, host: &str) {
    for scheme in ["https", "http"] {
        let url = cs(&format!("{scheme}://{host}/"));
        for kind in [
            ContentSettingTypes::GEOLOCATION,
            ContentSettingTypes::NOTIFICATIONS,
            ContentSettingTypes::MEDIASTREAM_MIC,
            ContentSettingTypes::MEDIASTREAM_CAMERA,
            ContentSettingTypes::POPUPS,
            ContentSettingTypes::JAVASCRIPT,
        ] {
            context.set_content_setting(
                Some(&url),
                Some(&url),
                kind,
                ContentSettingValues::DEFAULT,
            );
        }
    }
}
