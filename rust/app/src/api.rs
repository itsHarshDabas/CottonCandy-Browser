//! What CottonCandy's own pages can ask the browser to do. Each page gets only the calls it needs:
//! the history page can read history, but only the toolbar can open windows or change settings.

use crate::browser::{self, focus_address, hide_popup, show_popup, Ui, NEWTAB_URL};
use crate::cefx::*;
use crate::client::Role;
use crate::shared;
use cef::*;
use serde_json::{json, Value as Json};

/// The calls each kind of page may make. "ui" is the toolbar, tab list and popup.
const UI_CALLS: &[&str] = &[
    "ready",
    "tab_new",
    "tab_close",
    "tab_activate",
    "tab_move",
    "tab_mute",
    "nav",
    "navigate",
    "suggest",
    "suggest_pick",
    "popup",
    "popup_hide",
    "popup_resize",
    "command",
    "window",
    "bookmark_toggle",
    "history_delete_url",
    "focus_page",
    "focus_address",
    "download_action",
    "site_shields",
    "proxy_set",
    "grace_toggle",
    "grace_ask",
    "grace_enhance",
    "grace_stop",
    "site_clear",
    "site_reset",
    "find",
    "vtabs_expand",
    "popup_warm",
];

pub fn allowed(host: &str, cmd: &str) -> bool {
    let calls: &[&str] = match host {
        "ui" => UI_CALLS,
        "newtab" => &["newtab_info", "navigate", "focus_address"],
        "history" => &["history", "history_delete", "command"],
        "bookmarks" => &[
            "bookmarks",
            "bookmark_remove",
            "bookmarks_import",
            "bookmarks_export",
        ],
        "downloads" => &["downloads", "download_action"],
        "settings" => &[
            "settings_get",
            "settings_set",
            "settings_reset",
            "ext_install",
            "ext_toggle",
            "ext_remove",
            "import_sources",
            "import_run",
            "bookmarks_import",
            "bookmarks_export",
            "pick_folder",
            "clear_data",
            "restart",
            "about",
            "grace_models",
            "grace_use",
            "grace_pull",
            "grace_install",
            "pick_wallpaper",
            "wallpaper_from_link",
            "free_memory",
            "open_gpu_report",
        ],
        "error" => &["error_retry", "nav"],
        "grace" => &[
            "grace_ready",
            "grace_close",
            "grace_page",
            "grace_chat",
            "grace_stop",
            "grace_models",
            "grace_use",
            "grace_pull",
            "grace_install",
            "navigate",
        ],
        "about" => &["about"],
        _ => &[],
    };
    calls.contains(&cmd)
}

fn arg_str<'a>(args: &'a Json, key: &str) -> &'a str {
    args.get(key).and_then(Json::as_str).unwrap_or("")
}

fn arg_u32(args: &Json, key: &str) -> Option<u32> {
    args.get(key).and_then(Json::as_u64).map(|n| n as u32)
}

fn arg_bool(args: &Json, key: &str) -> bool {
    args.get(key).and_then(Json::as_bool).unwrap_or(false)
}

fn arg_rect(args: &Json) -> Rect {
    let r = args.get("rect").cloned().unwrap_or(Json::Null);
    let n = |k: &str| r.get(k).and_then(Json::as_f64).unwrap_or(0.0) as i32;
    rect(n("x"), n("y"), n("width"), n("height"))
}

type Res = Result<Json, String>;

/// Runs one call from a page in browser `browser_id`. Runs on the UI thread.
pub fn dispatch(cmd: &str, args: &Json, browser_id: i32, from_host: &str) -> Res {
    browser::with(|ui| {
        let (win, role) = match ui.roles.get(&browser_id) {
            Some(r) => *r,
            // The page loaded before its browser was filed under a window; it asks again.
            None if matches!(cmd, "ready" | "grace_ready") => return Err("starting".into()),
            None => (ui.focused().map(|w| w.id).ok_or("no window")?, Role::Tab(0)),
        };
        if cmd.starts_with("grace_") {
            return crate::grace::call(ui, win, browser_id, cmd, args);
        }
        let settings_call = from_host == "settings" && cmd != "about";
        if settings_call || matches!(cmd, "bookmarks_import" | "bookmarks_export") {
            return crate::settings_api::call(ui, browser_id, cmd, args);
        }
        call(ui, win, role, cmd, args, from_host)
    })
}

fn call(ui: &mut Ui, win: u32, role: Role, cmd: &str, args: &Json, from_host: &str) -> Res {
    match cmd {
        "ready" => {
            if std::env::var_os("CC_DEBUG").is_some() {
                if let Some(w) = ui.win(win) {
                    let b = |r: Rect| format!("{},{} {}x{}", r.x, r.y, r.width, r.height);
                    eprintln!(
                        "[layout] window={} chrome={} tabs={} vtabs={} tabs_in_host={}",
                        b(w.window.bounds()),
                        b(w.chrome.bounds()),
                        b(w.tabs_host.bounds()),
                        w.vtabs.as_ref().map(|v| b(v.bounds())).unwrap_or_default(),
                        w.tabs_host.child_view_count()
                    );
                }
            }
            if matches!(role, Role::Popup | Role::Suggest) {
                if let Some(w) = ui.win(win) {
                    browser::popup_ready(w, role == Role::Suggest);
                }
            }
            Ok(json!({}))
        }
        "tab_new" => {
            ui.new_tab(win, NEWTAB_URL, true, None);
            Ok(json!({}))
        }
        "tab_close" => {
            ui.close_tab(win, arg_u32(args, "id").ok_or("which tab?")?);
            Ok(json!({}))
        }
        "tab_activate" => {
            ui.activate(win, arg_u32(args, "id").ok_or("which tab?")?);
            Ok(json!({}))
        }
        "tab_move" => {
            let id = arg_u32(args, "id").ok_or("which tab?")?;
            let index = args.get("index").and_then(Json::as_u64).unwrap_or(0) as usize;
            if let Some(w) = ui.win(win) {
                if let Some(from) = w.tabs.iter().position(|t| t.id == id) {
                    let tab = w.tabs.remove(from);
                    let to = index.min(w.tabs.len());
                    w.tabs.insert(to, tab);
                }
            }
            ui.mark(win);
            ui.queue_session_save();
            Ok(json!({}))
        }
        "tab_mute" => command(ui, win, "tab_mute", args.get("id").unwrap_or(&Json::Null)),
        "nav" => {
            let action = arg_str(args, "action");
            let action = match action {
                "back" | "forward" | "reload" | "reload_hard" | "stop" | "home" => action,
                _ => return Err("unknown action".into()),
            };
            command(ui, win, action, &Json::Null)
        }
        "navigate" => {
            let text = arg_str(args, "text").to_string();
            let new_tab = arg_bool(args, "new_tab");
            hide_popup(ui, win);
            // Pages other than the toolbar may only open web addresses and our own pages, in a new tab
            // unless it's the new tab page itself.
            if from_host != "ui" {
                let url = cottoncandy_core::omnibox::resolve_input(
                    &text,
                    &shared::settings().search.template(),
                )
                .ok_or("Nothing to open")?;
                return ui
                    .navigate(win, &url, new_tab || from_host != "newtab")
                    .map(|_| json!({}));
            }
            ui.navigate(win, &text, new_tab).map(|_| json!({}))
        }
        "suggest" => suggest(arg_str(args, "text")),
        "suggest_pick" => {
            let index = args.get("index").cloned().unwrap_or(Json::Null);
            if let Some(b) = ui.win(win).and_then(|w| w.chrome_browser.clone()) {
                run_js(&b, &emit_js("suggest_pick", &index));
            }
            Ok(json!({}))
        }
        "popup" => {
            let kind = arg_str(args, "kind").to_string();
            let mut data = args.clone();
            if let Some(info) = popup_info(ui, win, &kind) {
                data["info"] = info;
            }
            let r = arg_rect(args);
            show_popup(ui, win, &kind, r, data);
            Ok(json!({}))
        }
        "popup_hide" => {
            let kind = arg_str(args, "kind");
            if kind == "suggest" {
                browser::hide_suggestions(ui, win);
            } else {
                let current = ui
                    .win(win)
                    .and_then(|w| w.popup.as_ref())
                    .map(|p| p.kind.clone())
                    .unwrap_or_default();
                if kind.is_empty() || kind == current {
                    hide_popup(ui, win);
                }
            }
            Ok(json!({}))
        }
        "popup_resize" => {
            let height = args.get("height").and_then(Json::as_i64).unwrap_or(200) as i32;
            browser::resize_popup(ui, win, role == Role::Suggest, height);
            Ok(json!({}))
        }
        "command" => {
            let id = arg_str(args, "id").to_string();
            if from_host == "history" && id != "clear_data" {
                return Err("not allowed".into());
            }
            command(ui, win, &id, args.get("arg").unwrap_or(&Json::Null))
        }
        "window" => {
            let Some(w) = ui.win(win) else {
                return Ok(json!({}));
            };
            match arg_str(args, "action") {
                "close" => close_window(&w.window),
                "minimize" => w.window.minimize(),
                "maximize" if w.window.is_maximized() != 0 => w.window.restore(),
                "maximize" => w.window.maximize(),
                _ => return Err("unknown action".into()),
            }
            Ok(json!({}))
        }
        "bookmark_toggle" => command(ui, win, "bookmark_toggle", &Json::Null),
        "bookmark_remove" => {
            let url = arg_str(args, "url");
            shared::with_store(|s| s.remove_bookmark(url))
                .transpose()
                .map_err(|e| e.to_string())?;
            ui.mark_all();
            Ok(json!({}))
        }
        "bookmarks" => {
            let items = shared::with_store(|s| s.bookmarks(arg_str(args, "query")))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            Ok(json!({ "items": items }))
        }
        "history" => {
            let private = ui.win(win).map(|w| w.private).unwrap_or(false);
            let limit = args
                .get("limit")
                .and_then(Json::as_u64)
                .unwrap_or(200)
                .min(2000) as usize;
            let items = shared::with_store(|s| s.history(arg_str(args, "query"), limit))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            Ok(json!({ "items": items, "private": private }))
        }
        "history_delete" => {
            let id = args
                .get("id")
                .and_then(Json::as_i64)
                .ok_or("which entry?")?;
            shared::with_store(|s| s.delete_history(id))
                .transpose()
                .map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        "history_delete_url" => {
            let url = arg_str(args, "url");
            shared::with_store(|s| {
                for e in s.history(url, 50).unwrap_or_default() {
                    if e.url == url {
                        let _ = s.delete_history(e.id);
                    }
                }
            });
            Ok(json!({}))
        }
        "downloads" => Ok(
            json!({ "items": ui.downloads.iter().map(browser::download_json).collect::<Vec<_>>() }),
        ),
        "download_action" => download_action(ui, args),
        "newtab_info" => newtab_info(ui, win),
        "focus_address" => {
            let text = arg_str(args, "text").to_string();
            if let Some(w) = ui.win(win) {
                focus_address(w, Some(&text));
            }
            Ok(json!({}))
        }
        "focus_page" => {
            if let Some(t) = ui.active_tab(win) {
                t.focus();
            }
            Ok(json!({}))
        }
        "find" => {
            let text = arg_str(args, "text");
            if let Some(host) = ui
                .active_tab(win)
                .and_then(|t| t.browser.as_ref())
                .and_then(|b| b.host())
            {
                if text.is_empty() {
                    host.stop_finding(1);
                } else {
                    host.find(
                        Some(&cs(text)),
                        (!arg_bool(args, "back")) as i32,
                        0,
                        arg_bool(args, "next") as i32,
                    );
                }
            }
            Ok(json!({}))
        }
        "error_retry" => {
            let url = arg_str(args, "url").to_string();
            let tab = ui.active_tab(win).ok_or("no tab")?;
            if url.starts_with("http://") || url.starts_with("https://") {
                browser::load(tab, &url);
            }
            Ok(json!({}))
        }
        "about" => Ok(about()),
        "popup_warm" => {
            if let Some(w) = ui.win(win) {
                browser::ensure_popup(w, arg_str(args, "kind") == "suggest");
            }
            Ok(json!({}))
        }
        "vtabs_expand" => {
            ui.set_vtabs_expanded(win, arg_bool(args, "open"));
            Ok(json!({}))
        }
        "proxy_set" => crate::settings_api::proxy_set(ui, args),
        "site_shields" => site_shields(ui, win, args),
        "site_clear" => site_clear(ui, win, args),
        "site_reset" => site_reset(ui, win, args),
        _ => Err(format!("unknown call {cmd}")),
    }
}

/// A host the popup names, checked against the page it was opened for.
fn site_host(ui: &mut Ui, win: u32, args: &Json) -> Result<String, String> {
    let asked = arg_str(args, "host").trim().to_ascii_lowercase();
    let url = ui
        .active_tab(win)
        .map(|t| t.url.clone())
        .unwrap_or_default();
    let shown = cottoncandy_core::omnibox::url_parts(&url)
        .host
        .to_ascii_lowercase();
    let host = web_host(&url);
    if host.is_empty() || (asked != shown && asked != host) {
        return Err("That site isn't open in this tab.".into());
    }
    Ok(host.trim_start_matches("www.").to_string())
}

/// The host of a web page (no port), as site settings are kept; "" for anything else.
fn web_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .unwrap_or_default()
}

/// One switch on the Shields or site popup, for the site in the active tab. Reloads it so the change shows.
fn site_shields(ui: &mut Ui, win: u32, args: &Json) -> Res {
    let host = site_host(ui, win, args)?;
    let value = args
        .get("value")
        .and_then(Json::as_bool)
        .ok_or("value must be true or false")?;
    let field = arg_str(args, "field").to_string();
    crate::settings_api::change(ui, |old| {
        let mut new = old.clone();
        let site = new.sites.entry(host.clone()).or_default();
        match field.as_str() {
            "shields_up" => site.shields_up = (!value).then_some(false),
            "ads" => site.ads = (value != old.shields.block_ads).then_some(value),
            // `value` is "scripts blocked"; the setting stores "scripts allowed".
            "scripts" => site.scripts = (value != old.shields.block_scripts).then_some(!value),
            _ => return Err(format!("unknown switch {field}")),
        }
        if *site == Default::default() {
            new.sites.remove(&host);
        }
        Ok(new)
    })?;
    if let Some(tab) = ui.active_tab(win) {
        if let Some(b) = &tab.browser {
            b.reload();
        }
    }
    popup_info(ui, win, "shields").ok_or_else(|| "no tab".into())
}

/// Deletes the cookies and stored data (local storage, IndexedDB, cache) of the site in the active tab.
fn site_clear(ui: &mut Ui, win: u32, args: &Json) -> Res {
    let host = site_host(ui, win, args)?;
    let tab = ui.active_tab(win).ok_or("no tab")?;
    let browser = tab.browser.clone().ok_or("no tab")?;
    let context = browser.host().and_then(|h| h.request_context());
    let mut origins: Vec<String> = url::Url::parse(&tab.url)
        .map(|u| u.origin().ascii_serialization())
        .into_iter()
        .collect();
    for scheme in ["https", "http"] {
        for h in [host.clone(), format!("www.{host}")] {
            let origin = format!("{scheme}://{h}");
            if !origins.contains(&origin) {
                origins.push(origin);
            }
        }
    }
    for origin in &origins {
        if let Some(m) = context.as_ref().and_then(|c| c.cookie_manager(None)) {
            m.delete_cookies(Some(&cs(&format!("{origin}/"))), None, None);
        }
        if let (Some(host), Some(mut params)) = (browser.host(), dictionary_value_create()) {
            params.set_string(Some(&cs("origin")), Some(&cs(origin)));
            params.set_string(Some(&cs("storageTypes")), Some(&cs("all")));
            host.execute_dev_tools_method(
                0,
                Some(&cs("Storage.clearDataForOrigin")),
                Some(&mut params),
            );
        }
    }
    browser.reload();
    Ok(json!({ "cleared": host }))
}

/// Forgets what the site in the active tab was allowed or blocked: permissions and its Shields overrides.
fn site_reset(ui: &mut Ui, win: u32, args: &Json) -> Res {
    let host = site_host(ui, win, args)?;
    let context = ui
        .active_tab(win)
        .and_then(|t| t.browser.as_ref())
        .and_then(|b| b.host())
        .and_then(|h| h.request_context());
    for h in [host.clone(), format!("www.{host}")] {
        if let Some(c) = &context {
            crate::shields::reset_site(c, &h);
        }
        if let Some(c) = request_context_get_global_context() {
            crate::shields::reset_site(&c, &h);
        }
    }
    crate::settings_api::change(ui, |old| {
        let mut new = old.clone();
        if let Some(site) = new.sites.get_mut(&host) {
            // Zoom isn't a permission; keep it.
            let zoom = site.zoom;
            *site = cottoncandy_core::settings::SiteSettings {
                zoom,
                ..Default::default()
            };
            if *site == Default::default() {
                new.sites.remove(&host);
            }
        }
        Ok(new)
    })?;
    if let Some(b) = ui.active_tab(win).and_then(|t| t.browser.clone()) {
        b.reload();
    }
    Ok(json!({ "reset": host }))
}

fn suggest(text: &str) -> Res {
    let settings = shared::settings();
    let template = settings.search.template();
    let mut items: Vec<Json> = Vec::new();
    let trimmed = text.trim();
    let resolved = cottoncandy_core::omnibox::resolve_input(trimmed, &template);
    let is_search = resolved
        .as_deref()
        .map(|u| u == cottoncandy_core::omnibox::search_url(&template, trimmed))
        .unwrap_or(true);
    if let (Some(url), false) = (&resolved, is_search) {
        items.push(json!({"kind": "url", "url": url, "title": url}));
    }
    if settings.search.local_suggestions {
        let found =
            shared::with_store(|s| s.suggest(trimmed, 6).unwrap_or_default()).unwrap_or_default();
        for s in found {
            if !items.iter().any(|i| i["url"] == s.url) {
                items.push(json!({"kind": s.kind, "url": s.url, "title": s.title}));
            }
        }
    }
    let engine = cottoncandy_core::settings::SEARCH_ENGINES
        .iter()
        .find(|e| e.0 == settings.search.engine)
        .map(|e| e.1)
        .unwrap_or("the web");
    let search = json!({
        "kind": "search",
        "url": cottoncandy_core::omnibox::search_url(&template, trimmed),
        "title": format!("Search {engine} for “{trimmed}”"),
        "fill": trimmed,
    });
    if is_search {
        items.insert(0, search);
    } else {
        items.push(search);
    }
    items.truncate(if crate::grace::offered() { 6 } else { 8 });
    if crate::grace::offered() && !trimmed.is_empty() {
        // Words, not an address: Grace can improve them. Nothing runs until one of these is picked.
        if is_search {
            items.push(json!({"kind": "enhance", "url": "", "title": "Enhance search with Grace", "fill": trimmed, "query": trimmed}));
        }
        items.push(json!({"kind": "ask", "url": "", "title": format!("Ask Grace “{trimmed}”"), "fill": trimmed, "query": trimmed}));
    }
    Ok(json!({ "items": items, "autocomplete": true }))
}

pub(crate) fn popup_info(ui: &mut Ui, win: u32, kind: &str) -> Option<Json> {
    match kind {
        "downloads" => Some(
            json!({ "items": ui.downloads.iter().map(browser::download_json).collect::<Vec<_>>() }),
        ),
        "menu" => {
            let zoom = ui.active_tab(win).map(|t| t.zoom).unwrap_or(100);
            Some(json!({ "zoom": zoom }))
        }
        "site" | "shields" => {
            let url = ui
                .active_tab(win)
                .map(|t| t.url.clone())
                .unwrap_or_default();
            let (ads, trackers) = ui
                .active_tab(win)
                .map(|t| t.shared.blocked())
                .unwrap_or((0, 0));
            let parts = cottoncandy_core::omnibox::url_parts(&url);
            let settings = shared::settings();
            let s = settings.shields_for(&web_host(&url));
            Some(json!({
                "url": url, "host": parts.host, "kind": parts.kind, "on": s.shields_up, "ads": s.ads,
                "scripts_blocked": s.scripts_blocked, "ads_blocked": ads, "trackers_blocked": trackers,
            }))
        }
        "proxy" => {
            let settings = shared::settings();
            let presets: Vec<Json> = cottoncandy_core::settings::PROXY_PRESETS
                .iter()
                .map(|p| json!({"id": p.0, "label": p.1}))
                .collect();
            let dns: Vec<Json> = cottoncandy_core::settings::DNS_PROVIDERS
                .iter()
                .map(|p| json!({"id": p.0, "label": p.1}))
                .collect();
            Some(json!({
                "enabled": settings.network.proxy.enabled, "preset": settings.network.proxy.preset,
                "label": browser::proxy_label(&settings), "presets": presets, "dns_mode": settings.network.dns.mode,
                "dns_provider": settings.network.dns.provider, "dns_providers": dns,
            }))
        }
        "extensions" => {
            let settings = shared::settings();
            let items: Vec<Json> = settings
                .extensions
                .installed
                .iter()
                .map(|(id, e)| json!({"id": id, "name": e.name, "enabled": e.enabled}))
                .collect();
            Some(json!({ "items": items }))
        }
        _ => None,
    }
}

fn download_action(ui: &mut Ui, args: &Json) -> Res {
    let id = arg_u32(args, "id").unwrap_or(0);
    let action = arg_str(args, "action");
    match action {
        "folder" => {
            let settings = shared::settings();
            let folder = if settings.downloads.folder.is_empty() {
                cottoncandy_core::paths::downloads_dir()
            } else {
                settings.downloads.folder.into()
            };
            let _ = browser::system_open(&folder.to_string_lossy());
        }
        "clear" => ui.downloads.retain(|d| d.state == "progress"),
        _ => {
            let Some(index) = ui.downloads.iter().position(|d| d.id == id) else {
                return Err("no such download".into());
            };
            let d = &ui.downloads[index];
            match action {
                "open" if d.state == "done" => {
                    let _ = browser::system_open(&d.path);
                }
                "show" => browser::show_in_folder(&d.path),
                "cancel" => {
                    if let Some(cb) = &d.callback {
                        cb.cancel();
                    }
                }
                "pause" => {
                    if let Some(cb) = &d.callback {
                        cb.pause();
                    }
                }
                "resume" => {
                    if let Some(cb) = &d.callback {
                        cb.resume();
                    }
                }
                "remove" => {
                    ui.downloads.remove(index);
                }
                _ => {}
            }
        }
    }
    ui.mark_all();
    Ok(json!({ "items": ui.downloads.iter().map(browser::download_json).collect::<Vec<_>>() }))
}

fn newtab_info(ui: &mut Ui, win: u32) -> Res {
    let settings = shared::settings();
    let private = ui.win(win).map(|w| w.private).unwrap_or(false);
    let tiles: Vec<Json> = if private {
        Vec::new()
    } else {
        shared::with_store(|s| s.most_visited(8).unwrap_or_default())
            .unwrap_or_default()
            .into_iter()
            .map(|e| {
                let host = url::Url::parse(&e.url)
                    .ok()
                    .and_then(|u| u.host_str().map(String::from))
                    .unwrap_or_default();
                let icon = shared::with_store(|s| s.favicon(&host).ok().flatten().is_some())
                    .unwrap_or(false)
                    .then(|| format!("/favicon?host={host}"));
                json!({"url": e.url, "title": e.title, "host": host, "icon": icon})
            })
            .collect()
    };
    let a = &settings.appearance;
    Ok(json!({
        "private": private,
        "background": a.newtab_background,
        // Changes with each new picture, so the page never shows an old one from its cache.
        "wallpaper_version": std::path::Path::new(&a.wallpaper_path).file_name().map(|n| n.to_string_lossy().into_owned()),
        "adaptive": a.adaptive_accent,
        "show_clock": a.newtab_show_clock,
        "show_shortcuts": a.newtab_show_shortcuts,
        "tiles": tiles,
        "blocked_total": 0,
    }))
}

fn about() -> Json {
    let version = format!(
        "{}.{}.{}.{}",
        cef_dll_sys::CHROME_VERSION_MAJOR,
        cef_dll_sys::CHROME_VERSION_MINOR,
        cef_dll_sys::CHROME_VERSION_BUILD,
        cef_dll_sys::CHROME_VERSION_PATCH
    );
    let cef = String::from_utf8_lossy(cef_dll_sys::CEF_VERSION)
        .trim_end_matches('\0')
        .to_string();
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "facts": {
            "Chromium": version,
            "CEF": cef,
            "Settings": shared::get().settings_path.to_string_lossy(),
            "Profile": shared::get().data_dir.to_string_lossy(),
            "Platform": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        },
    })
}

/// Runs a browser command: a menu item, a keyboard shortcut or a toolbar button.
pub fn command(ui: &mut Ui, win: u32, id: &str, arg: &Json) -> Res {
    let tab_arg = arg.as_u64().map(|n| n as u32);
    let host_of_active = |ui: &mut Ui| {
        ui.active_tab(win)
            .and_then(|t| t.browser.as_ref())
            .and_then(|b| b.host())
    };
    match id {
        "tab_new" => {
            ui.new_tab(win, NEWTAB_URL, true, None);
        }
        "tab_new_right" => {
            let index = ui
                .win(win)
                .and_then(|w| w.tabs.iter().position(|t| Some(t.id) == tab_arg))
                .map(|i| i + 1);
            ui.new_tab(win, NEWTAB_URL, true, index);
        }
        "tab_close" => {
            let id = tab_arg.or_else(|| ui.win(win).and_then(|w| w.active));
            if let Some(id) = id {
                ui.close_tab(win, id);
            }
        }
        "tab_close_others" | "tab_close_right" => {
            let Some(keep) = tab_arg else {
                return Ok(json!({}));
            };
            let ids: Vec<u32> = ui
                .win(win)
                .map(|w| {
                    let at = w.tabs.iter().position(|t| t.id == keep).unwrap_or(0);
                    w.tabs
                        .iter()
                        .enumerate()
                        .filter(|(i, t)| {
                            t.id != keep && !t.pinned && (id == "tab_close_others" || *i > at)
                        })
                        .map(|(_, t)| t.id)
                        .collect()
                })
                .unwrap_or_default();
            for t in ids {
                ui.close_tab(win, t);
            }
        }
        "tab_reopen" => {
            if let Some((_, url)) = ui.closed_tabs.pop() {
                ui.new_tab(win, &url, true, None);
            }
        }
        "tab_duplicate" => {
            let url = tab_arg.and_then(|t| ui.tab(win, t)).map(|t| t.url.clone());
            if let Some(url) = url {
                let index = ui
                    .win(win)
                    .and_then(|w| w.tabs.iter().position(|t| Some(t.id) == tab_arg))
                    .map(|i| i + 1);
                ui.new_tab(win, &url, true, index);
            }
        }
        "tab_pin" => {
            if let Some(w) = ui.win(win) {
                if let Some(i) = w.tabs.iter().position(|t| Some(t.id) == tab_arg) {
                    let mut tab = w.tabs.remove(i);
                    tab.pinned = !tab.pinned;
                    let pinned_count = w.tabs.iter().filter(|t| t.pinned).count();
                    w.tabs.insert(pinned_count, tab);
                }
            }
            ui.mark(win);
        }
        "tab_mute" => {
            let tab = tab_arg.or_else(|| ui.win(win).and_then(|w| w.active));
            if let Some(t) = tab.and_then(|t| ui.tab(win, t)) {
                t.muted = !t.muted;
                if let Some(host) = t.browser.as_ref().and_then(|b| b.host()) {
                    host.set_audio_muted(t.muted as i32);
                }
            }
            ui.mark(win);
        }
        "free_memory" => {
            // The menu's own spot, so the result shows where the user just clicked.
            let at = ui
                .win(win)
                .and_then(|w| w.popup.as_ref())
                .map(|p| p.rect.clone());
            let slept = ui.free_memory();
            let text = match slept {
                0 => "Cleared memory caches. No other tab could sleep.".to_string(),
                1 => "Put 1 tab to sleep and cleared memory caches. It loads again when you open it.".to_string(),
                n => format!("Put {n} tabs to sleep and cleared memory caches. They load again when you open them."),
            };
            if let Some(r) = at {
                let r = rect(r.x, r.y, r.width.max(280), 60);
                show_popup(
                    ui,
                    win,
                    "notice",
                    r,
                    json!({"kind": "notice", "text": text}),
                );
            }
            return Ok(json!({ "slept": slept, "text": text }));
        }
        "tab_sleep" => {
            if let Some(t) = tab_arg {
                ui.sleep_tab(win, t);
            }
        }
        "tab_reload" => {
            if let Some(b) = tab_arg
                .and_then(|t| ui.tab(win, t))
                .and_then(|t| t.browser.clone())
            {
                b.reload();
            }
        }
        "tab_next" | "tab_prev" | "tab_last" | "tab_1" | "tab_2" | "tab_3" | "tab_4" | "tab_5"
        | "tab_6" | "tab_7" | "tab_8" => {
            let target = ui.win(win).and_then(|w| {
                let n = w.tabs.len();
                if n == 0 {
                    return None;
                }
                let at = w
                    .tabs
                    .iter()
                    .position(|t| Some(t.id) == w.active)
                    .unwrap_or(0);
                let index = match id {
                    "tab_next" => (at + 1) % n,
                    "tab_prev" => (at + n - 1) % n,
                    "tab_last" => n - 1,
                    _ => id[4..].parse::<usize>().ok()?.checked_sub(1)?.min(n - 1),
                };
                Some(w.tabs[index].id)
            });
            if let Some(t) = target {
                ui.activate(win, t);
            }
        }
        "focus_address" => {
            if let Some(w) = ui.win(win) {
                focus_address(w, None);
            }
        }
        "focus_search" => {
            if let Some(w) = ui.win(win) {
                focus_address(w, Some("? "));
            }
        }
        "back" | "forward" | "reload" | "reload_hard" | "stop" => {
            if let Some(b) = ui.active_tab(win).and_then(|t| t.browser.clone()) {
                match id {
                    "back" => b.go_back(),
                    "forward" => b.go_forward(),
                    "reload" => b.reload(),
                    "reload_hard" => b.reload_ignore_cache(),
                    _ => b.stop_load(),
                }
            }
        }
        "home" => {
            let settings = shared::settings();
            let home = if settings.startup.homepage.trim().is_empty() {
                NEWTAB_URL.to_string()
            } else {
                settings.startup.homepage.clone()
            };
            ui.navigate(win, &home, false)?;
        }
        "escape" => {
            hide_popup(ui, win);
            if let Some(b) = ui.active_tab(win).and_then(|t| t.browser.clone()) {
                if b.is_loading() != 0 {
                    b.stop_load();
                }
                if let Some(host) = b.host() {
                    host.stop_finding(1);
                }
            }
        }
        "bookmark_toggle" => {
            let Some(t) = ui.active_tab(win) else {
                return Ok(json!({}));
            };
            let (url, title) = (t.url.clone(), t.title.clone());
            shared::with_store(|s| {
                if s.is_bookmarked(&url).unwrap_or(false) {
                    let _ = s.remove_bookmark(&url);
                } else {
                    let _ = s.add_bookmark(&url, &title, "", now());
                }
            });
            ui.mark_all();
        }
        "open_history" | "open_downloads" | "open_bookmarks" | "open_settings" | "open_about"
        | "open_extensions" | "clear_data" => {
            let url = match id {
                "open_history" => "cottoncandy://history/",
                "open_downloads" => "cottoncandy://downloads/",
                "open_bookmarks" => "cottoncandy://bookmarks/",
                "open_settings" => "cottoncandy://settings/",
                "open_extensions" => "cottoncandy://settings/#extensions",
                "clear_data" => "cottoncandy://settings/#privacy",
                _ => "cottoncandy://about/",
            };
            // Reuse a tab that already shows the page (and jump to the section asked for).
            let page = url.split('#').next().unwrap_or(url);
            let existing = ui.win(win).and_then(|w| {
                w.tabs
                    .iter()
                    .find(|t| t.url.starts_with(page))
                    .map(|t| t.id)
            });
            match existing {
                Some(t) => {
                    ui.activate(win, t);
                    if url.contains('#') {
                        if let Some(tab) = ui.tab(win, t) {
                            browser::load(tab, url);
                        }
                    }
                }
                None => {
                    ui.new_tab(win, url, true, None);
                }
            }
        }
        "toggle_bookmarks_bar" => {
            crate::settings_api::change(ui, |s| {
                let mut s = s.clone();
                s.appearance.show_bookmarks_bar = !s.appearance.show_bookmarks_bar;
                Ok(s)
            })?;
        }
        "window_new" | "window_private" => {
            let private = id == "window_private";
            on_ui(move || browser::open_window(private, Vec::new()));
        }
        "window_close" => {
            if let Some(w) = ui.win(win) {
                close_window(&w.window);
            }
        }
        "quit" => {
            ui.save_session();
            for w in &ui.windows {
                close_window(&w.window);
            }
        }
        "find" => {
            let r = ui
                .win(win)
                .map(|w| w.window.bounds())
                .map(|b| rect(b.width - 380, browser_top(ui, win), 360, 52))
                .unwrap_or(rect(0, 0, 360, 52));
            show_popup(ui, win, "find", r, json!({"kind": "find"}));
        }
        "find_next" => {}
        "print" => {
            if let Some(host) = host_of_active(ui) {
                host.print();
            }
        }
        "save_page" => {
            let url = ui
                .active_tab(win)
                .map(|t| t.url.clone())
                .unwrap_or_default();
            if let (Some(host), true) = (host_of_active(ui), url.starts_with("http")) {
                host.start_download(Some(&cs(&url)));
            }
        }
        "zoom_in" | "zoom_out" | "zoom_reset" => {
            let default = shared::settings().appearance.default_zoom as i32;
            let Some(t) = ui.active_tab(win) else {
                return Ok(json!({}));
            };
            let steps = browser::ZOOM_STEPS;
            t.zoom = match id {
                "zoom_in" => steps.iter().copied().find(|z| *z > t.zoom).unwrap_or(500),
                "zoom_out" => steps
                    .iter()
                    .rev()
                    .copied()
                    .find(|z| *z < t.zoom)
                    .unwrap_or(25),
                _ => default,
            };
            let zoom = t.zoom;
            if let Some(host) = t.browser.as_ref().and_then(|b| b.host()) {
                host.set_zoom_level(browser::zoom_level(zoom));
            }
            return Ok(json!({ "zoom": zoom }));
        }
        "fullscreen" => {
            let on = !ui.win(win).map(|w| w.fullscreen).unwrap_or(false);
            ui.set_fullscreen(win, on);
        }
        "devtools" => {
            if let Some(host) = host_of_active(ui) {
                host.show_dev_tools(
                    Some(&WindowInfo::default()),
                    None,
                    Some(&BrowserSettings::default()),
                    None,
                );
            }
        }
        "grace_toggle" => crate::grace::toggle(ui, win, None),
        _ => return Err(format!("unknown command {id}")),
    }
    Ok(json!({}))
}

/// Where the page starts, below the toolbar.
fn browser_top(ui: &mut Ui, win: u32) -> i32 {
    ui.win(win)
        .map(|w| {
            w.sizes
                .chrome_height
                .load(std::sync::atomic::Ordering::Relaxed)
                + 6
        })
        .unwrap_or(92)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_get_only_their_own_calls() {
        assert!(allowed("ui", "window"));
        assert!(allowed("history", "history"));
        assert!(!allowed("history", "window"));
        assert!(!allowed("newtab", "history"));
        assert!(!allowed("error", "download_action"));
        assert!(!allowed("unknown", "about"));
    }
}
