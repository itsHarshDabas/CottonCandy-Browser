//! What each browser does when its page loads, navigates, opens popups, downloads or takes keys.
//! Handlers run inside CEF callbacks, so they copy what they need and change state with `later`.

use crate::browser::{self, later, try_with, DlSnap, TabShared};
use crate::cefx::*;
use crate::{api, shared};
use cef::*;
use cef_dll_sys::cef_window_open_disposition_t as wod;
use cottoncandy_core::omnibox::{self, Navigation};
use std::sync::Arc;

#[cfg(windows)]
type OsEvent = cef_dll_sys::MSG;
#[cfg(not(windows))]
type OsEvent = cef_dll_sys::XEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Chrome,
    VTabs,
    Popup,
    Tab(u32),
}

#[derive(Clone)]
pub struct Ctx {
    pub win: u32,
    pub role: Role,
    pub tab: Option<Arc<TabShared>>,
}

impl Ctx {
    fn tab_id(&self) -> Option<u32> {
        match self.role {
            Role::Tab(id) => Some(id),
            _ => None,
        }
    }
    fn private(&self) -> bool {
        self.tab.as_ref().map(|t| t.private).unwrap_or(false)
    }
}

wrap_client! {
    pub struct CcClient {
        ctx: Ctx,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(LifeSpan::new(self.ctx.clone()))
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(DisplayH::new(self.ctx.clone()))
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(Load::new(self.ctx.clone()))
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(Requests::new(self.ctx.clone()))
        }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(Keys::new(self.ctx.clone()))
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            self.ctx.tab_id().map(|_| Downloads::new(self.ctx.clone()))
        }
        fn drag_handler(&self) -> Option<DragHandler> {
            (self.ctx.role == Role::Chrome).then(|| Drag::new(self.ctx.clone()))
        }
        fn focus_handler(&self) -> Option<FocusHandler> {
            Some(Focus::new(self.ctx.clone()))
        }
        fn find_handler(&self) -> Option<FindHandler> {
            Some(Find::new(self.ctx.clone()))
        }
    }
}

/// A client for one of the window's browsers.
pub fn client(win: u32, role: Role, tab: Option<Arc<TabShared>>) -> Client {
    CcClient::new(Ctx { win, role, tab })
}

wrap_life_span_handler! {
    struct LifeSpan {
        ctx: Ctx,
    }

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            // Popups open as tabs. Pages that try one without a click are stopped when Shields block popups.
            let Some(tab) = self.ctx.tab_id() else { return 1 };
            let url = os(target_url);
            if std::env::var_os("CC_DEBUG").is_some() {
                eprintln!("[popup] {url} gesture={user_gesture} disposition={:?}", target_disposition);
            }
            if user_gesture == 0 && shared::settings().shields.block_popups {
                return 1;
            }
            let background = *target_disposition.as_ref() == wod::CEF_WOD_NEW_BACKGROUND_TAB;
            let win = self.ctx.win;
            later(move |ui| open_beside(ui, win, tab, &url, !background));
            1
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(browser) = browser.cloned() else { return };
            let ctx = self.ctx.clone();
            later(move |ui| {
                let id = browser.identifier();
                ui.roles.insert(id, (ctx.win, ctx.role));
                let Some(w) = ui.win(ctx.win) else { return };
                w.browsers.insert(id, browser.clone());
                match ctx.role {
                    Role::Chrome => w.chrome_browser = Some(browser),
                    Role::VTabs => w.vtabs_browser = Some(browser),
                    Role::Popup => {
                        if let Some(p) = w.popup.as_mut() {
                            p.browser = Some(browser);
                        }
                    }
                    Role::Tab(tab) => {
                        let active = w.active == Some(tab);
                        if let Some(t) = w.tabs.iter_mut().find(|t| t.id == tab) {
                            if let Some(host) = browser.host() {
                                if t.zoom != 100 {
                                    host.set_zoom_level(browser::zoom_level(t.zoom));
                                }
                                if !active {
                                    host.was_hidden(1);
                                }
                            }
                            t.browser = Some(browser);
                        }
                    }
                }
                ui.mark(ctx.win);
            });
        }

        fn do_close(&self, browser: Option<&mut Browser>) -> ::std::os::raw::c_int {
            // While the whole window is closing, let CEF go ahead (0). Otherwise return 1, or CEF would close
            // the window: a Views browser finishes closing once its view leaves the window, so take the tab
            // out ourselves.
            let ctx = self.ctx.clone();
            let id = browser.map(|b| b.identifier()).unwrap_or(0);
            let window_closing = try_with(|ui| {
                ui.win(ctx.win)
                    .map(|w| {
                        // A browser that got here is on its way out either way (a closed tab too), so the
                        // window shouldn't wait for it.
                        w.close_ready.insert(id);
                        w.closing
                    })
                    .unwrap_or(true)
            })
            .unwrap_or(false);
            if window_closing {
                return 0;
            }
            if let Role::Tab(tab) = ctx.role {
                later(move |ui| {
                    if ui.win(ctx.win).is_some_and(|w| !w.closing) {
                        ui.remove_tab(ctx.win, tab);
                    }
                });
            }
            1
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            let Some(id) = browser.map(|b| b.identifier()) else { return };
            let ctx = self.ctx.clone();
            later(move |ui| {
                ui.roles.remove(&id);
                let Some(w) = ui.win(ctx.win) else { return };
                w.browsers.remove(&id);
                let closing = w.closing;
                let empty = w.browsers.is_empty();
                if let (Role::Tab(tab), false) = (ctx.role, closing) {
                    ui.remove_tab(ctx.win, tab);
                }
                if closing && empty {
                    if let Some(w) = ui.win(ctx.win) {
                        close_window(&w.window);
                    }
                }
            });
        }
    }
}

/// Opens `url` in a new tab next to `tab`.
fn open_beside(ui: &mut browser::Ui, win: u32, tab: u32, url: &str, foreground: bool) {
    let index = ui
        .win(win)
        .and_then(|w| w.tabs.iter().position(|t| t.id == tab))
        .map(|i| i + 1);
    ui.new_tab(win, url, foreground, index);
}

wrap_display_handler! {
    struct DisplayH {
        ctx: Ctx,
    }

    impl DisplayHandler {
        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            let (Some(tab), true) = (self.ctx.tab_id(), frame.map(|f| f.is_main() != 0).unwrap_or(false)) else {
                return;
            };
            let url = os(url);
            let win = self.ctx.win;
            later(move |ui| {
                if let Some(t) = ui.tab(win, tab) {
                    if t.url != url {
                        t.url = url;
                        t.favicon = None;
                    }
                }
                ui.mark(win);
                ui.queue_session_save();
            });
        }

        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            let Some(tab) = self.ctx.tab_id() else { return };
            let title = os(title);
            let win = self.ctx.win;
            let private = self.ctx.private();
            later(move |ui| {
                if let Some(t) = ui.tab(win, tab) {
                    t.title = title.clone();
                    if !private {
                        let url = t.url.clone();
                        shared::with_store(|s| s.set_title(&url, &title));
                    }
                }
                ui.mark(win);
            });
        }

        fn on_favicon_urlchange(&self, browser: Option<&mut Browser>, icon_urls: Option<&mut CefStringList>) {
            let (Some(tab), Some(browser), Some(list)) = (self.ctx.tab_id(), browser.cloned(), icon_urls) else {
                return;
            };
            let urls: Vec<String> = std::mem::take(list).into_iter().collect();
            let Some(icon) = urls.into_iter().find(|u| u.starts_with("https://") || u.starts_with("http://") || u.starts_with("data:image/")) else {
                return;
            };
            let win = self.ctx.win;
            let private = self.ctx.private();
            later(move |ui| {
                let Some(t) = ui.tab(win, tab) else { return };
                if t.favicon_url == icon {
                    return;
                }
                t.favicon_url = icon.clone();
                let host = url::Url::parse(&t.url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
                if let Some(h) = browser.host() {
                    let mut cb = Favicon::new(win, tab, host, private);
                    h.download_image(Some(&cs(&icon)), 1, 32, 0, Some(&mut cb));
                }
            });
        }

        fn on_fullscreen_mode_change(&self, _browser: Option<&mut Browser>, fullscreen: ::std::os::raw::c_int) {
            let Some(tab) = self.ctx.tab_id() else { return };
            let win = self.ctx.win;
            later(move |ui| {
                if let Some(t) = ui.tab(win, tab) {
                    t.fullscreen = fullscreen != 0;
                }
                ui.set_fullscreen(win, fullscreen != 0);
            });
        }

        fn on_loading_progress_change(&self, _browser: Option<&mut Browser>, progress: f64) {
            let Some(tab) = self.ctx.tab_id() else { return };
            let win = self.ctx.win;
            later(move |ui| {
                if let Some(t) = ui.tab(win, tab) {
                    t.progress = progress;
                }
            });
        }

        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            _level: LogSeverity,
            message: Option<&CefString>,
            source: Option<&CefString>,
            line: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            if std::env::var_os("CC_DEBUG").is_some() {
                eprintln!("[ui] {}:{line} {}", os(source), os(message));
            }
            0
        }
    }
}

wrap_download_image_callback! {
    struct Favicon {
        win: u32,
        tab: u32,
        host: String,
        private: bool,
    }

    impl DownloadImageCallback {
        fn on_download_image_finished(&self, _image_url: Option<&CefString>, _http_status_code: ::std::os::raw::c_int, image: Option<&mut Image>) {
            let Some(image) = image else { return };
            let (mut w, mut h) = (0, 0);
            let Some(png) = image.as_png(1.0, 1, Some(&mut w), Some(&mut h)) else { return };
            let mut bytes = vec![0u8; png.size()];
            png.data(Some(&mut bytes), 0);
            if bytes.is_empty() || bytes.len() > 256 * 1024 {
                return;
            }
            let (win, tab, host, private) = (self.win, self.tab, self.host.clone(), self.private);
            later(move |ui| {
                let src = if private || host.is_empty() {
                    format!("data:image/png;base64,{}", base64(&bytes))
                } else {
                    shared::with_store(|s| s.set_favicon(&host, &bytes, now()));
                    format!("/favicon?host={}&v={}", percent_encoding::utf8_percent_encode(&host, percent_encoding::NON_ALPHANUMERIC), now())
                };
                if let Some(t) = ui.tab(win, tab) {
                    t.favicon = Some(src);
                }
                ui.mark(win);
            });
        }
    }
}

pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

wrap_load_handler! {
    struct Load {
        ctx: Ctx,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: ::std::os::raw::c_int,
            can_go_back: ::std::os::raw::c_int,
            can_go_forward: ::std::os::raw::c_int,
        ) {
            let Some(tab) = self.ctx.tab_id() else { return };
            let win = self.ctx.win;
            later(move |ui| {
                if let Some(t) = ui.tab(win, tab) {
                    t.loading = is_loading != 0;
                    t.can_back = can_go_back != 0;
                    t.can_forward = can_go_forward != 0;
                }
                ui.mark(win);
            });
        }

        fn on_load_end(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, http_status_code: ::std::os::raw::c_int) {
            let Some(frame) = frame.filter(|f| f.is_main() != 0) else { return };
            if self.ctx.tab_id().is_none() || self.ctx.private() || !(200..400).contains(&http_status_code) {
                return;
            }
            let url = us(&frame.url());
            if !shared::settings().privacy.save_history {
                return;
            }
            let Some(tab) = self.ctx.tab_id() else { return };
            let win = self.ctx.win;
            later(move |ui| {
                let title = ui.tab(win, tab).map(|t| t.title.clone()).unwrap_or_default();
                shared::with_store(|s| s.add_visit(&url, &title, now()));
            });
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            let code = *error_code.as_ref() as i32;
            let Some(frame) = frame.filter(|f| f.is_main() != 0) else { return };
            // -3 is "aborted": the user stopped it or a download took over.
            if code == -3 || self.ctx.tab_id().is_none() {
                return;
            }
            let url = os(failed_url);
            let page = error_page(&url, &os(error_text), "");
            if let Some(tab) = &self.ctx.tab {
                if let Ok(mut typed) = tab.typed.lock() {
                    *typed = page.clone();
                }
            }
            frame.load_url(Some(&cs(&page)));
        }
    }
}

pub fn error_page(url: &str, code: &str, kind: &str) -> String {
    let q = |s: &str| {
        percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
    };
    format!(
        "cottoncandy://error/?url={}&code={}&kind={}",
        q(url),
        q(code),
        q(kind)
    )
}

wrap_request_handler! {
    struct Requests {
        ctx: Ctx,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let (Some(frame), Some(request)) = (frame, request) else { return 1 };
            let url = us(&request.url());
            let Some(tab) = &self.ctx.tab else {
                // The toolbar, tab list and popup only ever show their own page.
                return (!url.starts_with("cottoncandy://ui/")) as i32;
            };
            let is_main = frame.is_main() != 0;
            let from = us(&frame.url());
            let typed = is_main
                && tab
                    .typed
                    .lock()
                    .map(|mut t| {
                        let hit = *t == url;
                        if hit {
                            t.clear();
                        }
                        hit
                    })
                    .unwrap_or(false);
            match omnibox::navigation_decision(&url, &from, is_main, typed, user_gesture != 0) {
                Navigation::Allow => 0,
                Navigation::Handoff => {
                    browser::open_external(&url);
                    1
                }
                Navigation::Block => 1,
            }
        }

        fn on_open_urlfrom_tab(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            target_url: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let Some(tab) = self.ctx.tab_id() else { return 1 };
            let url = os(target_url);
            let foreground = *target_disposition.as_ref() == wod::CEF_WOD_NEW_FOREGROUND_TAB;
            let win = self.ctx.win;
            later(move |ui| open_beside(ui, win, tab, &url, foreground));
            1
        }

        fn on_render_process_terminated(
            &self,
            browser: Option<&mut Browser>,
            _status: TerminationStatus,
            error_code: ::std::os::raw::c_int,
            _error_string: Option<&CefString>,
        ) {
            let Some(browser) = browser.cloned() else { return };
            let ctx = self.ctx.clone();
            later(move |_| {
                let Some(frame) = browser.main_frame() else { return };
                match &ctx.tab {
                    Some(tab) => {
                        let url = us(&frame.url());
                        let page = error_page(&url, &format!("The page crashed ({error_code})"), "crashed");
                        if let Ok(mut typed) = tab.typed.lock() {
                            *typed = page.clone();
                        }
                        frame.load_url(Some(&cs(&page)));
                    }
                    // Our own toolbar or popup: just load it again.
                    None => browser.reload(),
                }
            });
        }
    }
}

wrap_keyboard_handler! {
    struct Keys {
        ctx: Ctx,
    }

    impl KeyboardHandler {
        fn on_pre_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: Option<&mut OsEvent>,
            _is_keyboard_shortcut: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let Some(event) = event else { return 0 };
            if *event.type_.as_ref() != cef_dll_sys::cef_key_event_type_t::KEYEVENT_RAWKEYDOWN {
                return 0;
            }
            let Some(command) = shortcut(event.windows_key_code, event.modifiers) else {
                return 0;
            };
            let win = self.ctx.win;
            later(move |ui| {
                let _ = api::command(ui, win, command, &serde_json::Value::Null);
            });
            1
        }
    }
}

/// The browser's keyboard shortcuts, as commands (see `api::command`). Key codes are Windows VK codes.
pub fn shortcut(key: i32, modifiers: u32) -> Option<&'static str> {
    let shift = modifiers & 2 != 0;
    let ctrl = modifiers & 4 != 0;
    let alt = modifiers & 8 != 0;
    let letter = |c: u8| key == c as i32;
    Some(match (ctrl, shift, alt) {
        (true, false, false) if letter(b'T') => "tab_new",
        (true, false, false) if letter(b'W') => "tab_close",
        (true, false, false) if key == 0x73 => "tab_close", // Ctrl+F4
        (true, true, false) if letter(b'T') => "tab_reopen",
        (true, false, false) if key == 0x09 || key == 0x22 => "tab_next", // Ctrl+Tab, Ctrl+PgDn
        (true, true, false) if key == 0x09 => "tab_prev",
        (true, false, false) if key == 0x21 => "tab_prev", // Ctrl+PgUp
        (true, false, false) if (0x31..=0x38).contains(&key) => [
            "tab_1", "tab_2", "tab_3", "tab_4", "tab_5", "tab_6", "tab_7", "tab_8",
        ][(key - 0x31) as usize],
        (true, false, false) if key == 0x39 => "tab_last",
        (true, false, false) if letter(b'L') => "focus_address",
        (false, false, true) if letter(b'D') => "focus_address",
        (false, false, false) if key == 0x75 => "focus_address", // F6
        (true, false, false) if letter(b'K') || letter(b'E') => "focus_search",
        (true, false, false) if letter(b'R') => "reload",
        (false, false, false) if key == 0x74 => "reload", // F5
        (true, true, false) if letter(b'R') => "reload_hard",
        (true, false, false) if key == 0x74 => "reload_hard",
        (false, false, true) if key == 0x25 => "back",
        (false, false, true) if key == 0x27 => "forward",
        (false, false, true) if key == 0x24 => "home",
        (true, false, false) if letter(b'D') => "bookmark_toggle",
        (true, false, false) if letter(b'H') => "open_history",
        (true, false, false) if letter(b'J') => "open_downloads",
        (true, true, false) if letter(b'O') => "open_bookmarks",
        (true, true, false) if letter(b'B') => "toggle_bookmarks_bar",
        (true, false, false) if letter(b'N') => "window_new",
        (true, true, false) if letter(b'N') => "window_private",
        (true, false, false) if letter(b'F') => "find",
        (false, false, false) if key == 0x72 => "find_next", // F3
        (true, false, false) if letter(b'G') => "find_next",
        (true, true, false) if letter(b'G') => "grace_toggle",
        (true, false, false) if letter(b'P') => "print",
        (true, false, false) if letter(b'S') => "save_page",
        (true, false, false) if letter(b'U') => "view_source",
        (true, false, false) if key == 0xBB || key == 0x6B => "zoom_in", // = and numpad +
        (true, true, false) if key == 0xBB => "zoom_in",
        (true, false, false) if key == 0xBD || key == 0x6D => "zoom_out",
        (true, false, false) if key == 0x30 || key == 0x60 => "zoom_reset",
        (false, false, false) if key == 0x7A => "fullscreen", // F11
        (false, false, false) if key == 0x7B => "devtools",   // F12
        (true, true, false) if letter(b'I') => "devtools",
        (true, false, false) if key == 0xBC => "open_settings", // Ctrl+,
        (true, true, false) if key == 0x2E => "clear_data",     // Ctrl+Shift+Del
        (true, true, false) if letter(b'Q') => "quit",
        (true, true, false) if letter(b'W') => "window_close",
        (false, false, false) if key == 0x1B => "escape",
        _ => return None,
    })
}

wrap_download_handler! {
    struct Downloads {
        ctx: Ctx,
    }

    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _request_method: Option<&CefString>) -> ::std::os::raw::c_int {
            1
        }

        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            _download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(callback) = callback else { return 0 };
            let settings = shared::settings();
            let folder = if settings.downloads.folder.trim().is_empty() {
                cottoncandy_core::paths::downloads_dir()
            } else {
                std::path::PathBuf::from(&settings.downloads.folder)
            };
            let _ = std::fs::create_dir_all(&folder);
            let name = omnibox::safe_download_name(&os(suggested_name));
            let path = unique_path(&folder, &name);
            callback.cont(Some(&cs(&path.to_string_lossy())), settings.downloads.ask_where as i32);
            1
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            callback: Option<&mut DownloadItemCallback>,
        ) {
            let Some(item) = download_item else { return };
            if item.is_valid() == 0 {
                return;
            }
            let snap = DlSnap::of(item);
            let callback = callback.cloned();
            later(move |ui| browser::download_updated(ui, snap, callback));
        }
    }
}

/// `folder/name`, or `folder/name (2)` and so on when that's taken.
pub fn unique_path(folder: &std::path::Path, name: &str) -> std::path::PathBuf {
    let first = folder.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..10_000)
        .map(|n| folder.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

wrap_drag_handler! {
    struct Drag {
        ctx: Ctx,
    }

    impl DragHandler {
        fn on_draggable_regions_changed(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            regions: Option<&[DraggableRegion]>,
        ) {
            let regions: Vec<DraggableRegion> = regions.map(|r| r.to_vec()).unwrap_or_default();
            let win = self.ctx.win;
            later(move |ui| {
                if let Some(w) = ui.win(win) {
                    w.window.set_draggable_regions(Some(&regions));
                }
            });
        }
    }
}

wrap_focus_handler! {
    struct Focus {
        ctx: Ctx,
    }

    impl FocusHandler {
        fn on_got_focus(&self, _browser: Option<&mut Browser>) {
            if matches!(self.ctx.role, Role::Tab(_)) {
                let win = self.ctx.win;
                later(move |ui| browser::hide_popup(ui, win));
            }
        }
    }
}

wrap_find_handler! {
    struct Find {
        ctx: Ctx,
    }

    impl FindHandler {
        fn on_find_result(
            &self,
            _browser: Option<&mut Browser>,
            _identifier: ::std::os::raw::c_int,
            count: ::std::os::raw::c_int,
            _selection_rect: Option<&Rect>,
            active_match_ordinal: ::std::os::raw::c_int,
            final_update: ::std::os::raw::c_int,
        ) {
            if final_update == 0 {
                return;
            }
            let win = self.ctx.win;
            later(move |ui| {
                let Some(p) = ui.win(win).and_then(|w| w.popup.as_ref()) else { return };
                if p.kind != "find" {
                    return;
                }
                if let Some(b) = &p.browser {
                    let found = serde_json::json!({"count": count, "active": active_match_ordinal});
                    run_js(b, &emit_js("find_result", &found));
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts() {
        assert_eq!(shortcut(b'T' as i32, 4), Some("tab_new"));
        assert_eq!(shortcut(b'T' as i32, 4 | 2), Some("tab_reopen"));
        assert_eq!(shortcut(0x31, 4), Some("tab_1"));
        assert_eq!(shortcut(0x25, 8), Some("back"));
        assert_eq!(shortcut(b'T' as i32, 0), None);
        assert_eq!(shortcut(b'A' as i32, 4), None);
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn unique_paths_count_up() {
        let dir = std::env::temp_dir().join(format!("cc-unique-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a.txt"));
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a (2).txt"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
