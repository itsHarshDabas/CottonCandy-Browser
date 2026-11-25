//! Windows and tabs. Everything here runs on CEF's UI thread.
//!
//! A window is a frameless CEF Views window holding, top to bottom: the toolbar (an HTML page,
//! cottoncandy://ui/chrome.html), then a row with the vertical tab list, the tab being shown, and the
//! Grace panel. Menus and suggestions float in an overlay over the page.
//!
//! CEF can call our handlers while we are inside a CEF call (adding a view creates its browser, a layout
//! asks for sizes), so handlers never touch `UI` directly: they post a task, or read with `try_with`.

use crate::cefx::*;
use crate::client::{self, Role};
use crate::shared;
use cef::*;
use cef_dll_sys::cef_runtime_style_t as rs;
use cottoncandy_core::{omnibox, settings::Settings};
use serde_json::{json, Value as Json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub const NEWTAB_URL: &str = "cottoncandy://newtab/";
const TABROW_HEIGHT: i32 = 40;
const TOOLBAR_HEIGHT: i32 = 46;
const BOOKMARKS_HEIGHT: i32 = 30;
const VTABS_WIDTH: i32 = 240;
/// Side tabs folded to their icons.
const VTABS_COLLAPSED: i32 = 48;
const DARK_BG: u32 = 0xFF11_1318;

/// Sizes the views ask for during layout, readable without the UI borrow.
#[derive(Default)]
pub struct Sizes {
    pub chrome_height: AtomicI32,
    pub vtabs_width: AtomicI32,
    pub side_width: AtomicI32,
}

/// What a tab's handlers need to know synchronously.
pub struct TabShared {
    /// The URL the user just typed or picked, which may go where a page can't (another scheme, our pages).
    pub typed: Mutex<String>,
    pub private: bool,
    /// What Shields blocked on the page now showing (counted on the network thread).
    pub ads_blocked: AtomicU32,
    pub trackers_blocked: AtomicU32,
    /// A toolbar refresh for the counters is already on its way.
    pub counts_dirty: AtomicBool,
    /// Hashes of the addresses blocked on this page, so a request made twice (by the preloader, then the
    /// parser) counts once.
    pub blocked_seen: Mutex<std::collections::HashSet<u64>>,
}

impl TabShared {
    pub fn new(typed: &str, private: bool) -> TabShared {
        TabShared {
            typed: Mutex::new(typed.to_string()),
            private,
            ads_blocked: AtomicU32::new(0),
            trackers_blocked: AtomicU32::new(0),
            counts_dirty: AtomicBool::new(false),
            blocked_seen: Mutex::new(std::collections::HashSet::new()),
        }
    }
    /// A new page started: its counters start at zero.
    pub fn reset_blocked(&self) {
        self.ads_blocked.store(0, Ordering::Relaxed);
        self.trackers_blocked.store(0, Ordering::Relaxed);
        if let Ok(mut seen) = self.blocked_seen.lock() {
            seen.clear();
        }
    }
    pub fn blocked(&self) -> (u32, u32) {
        (
            self.ads_blocked.load(Ordering::Relaxed),
            self.trackers_blocked.load(Ordering::Relaxed),
        )
    }
}

pub struct Tab {
    pub id: u32,
    pub view: BrowserView,
    pub browser: Option<Browser>,
    pub shared: Arc<TabShared>,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub progress: f64,
    pub can_back: bool,
    pub can_forward: bool,
    pub favicon: Option<String>,
    pub favicon_url: String,
    pub pinned: bool,
    pub audible: bool,
    pub muted: bool,
    pub fullscreen: bool,
    pub zoom: i32,
}

pub struct Popup {
    pub controller: OverlayController,
    pub browser: Option<Browser>,
    pub ready: bool,
    pub kind: String,
    pub rect: Rect,
    pub pending: Option<Json>,
}

/// Grace's side panel. Its browser is made the first time it opens, so windows that never use Grace don't
/// pay for another renderer.
#[derive(Default)]
pub struct GracePanel {
    pub view: Option<BrowserView>,
    pub browser: Option<Browser>,
    pub open: bool,
    pub ready: bool,
    /// Questions from the address bar that arrived before the panel's page was ready.
    pub pending: Vec<String>,
    /// The page info last sent to the panel, so it hears only about changes.
    pub last_page: Option<Json>,
}

pub struct Win {
    pub id: u32,
    pub private: bool,
    pub window: Window,
    pub context: Option<RequestContext>,
    pub sizes: Arc<Sizes>,
    pub chrome: BrowserView,
    pub chrome_browser: Option<Browser>,
    /// Made only when the side tabs are first shown.
    pub vtabs: Option<BrowserView>,
    pub vtabs_browser: Option<Browser>,
    pub tabs_host: Panel,
    /// The row under the toolbar: vertical tabs, the page, and Grace's side panel.
    pub body: Panel,
    pub grace: GracePanel,
    /// Made the first time it is needed (see `ensure_popup`).
    pub popup: Option<Popup>,
    pub tabs: Vec<Tab>,
    pub active: Option<u32>,
    /// Every live browser in this window, by CEF id. The window may close only when this is empty.
    pub browsers: HashMap<i32, Browser>,
    pub closing: bool,
    /// Browsers that have agreed to close (their DoClose ran) while the window closes.
    pub close_ready: std::collections::HashSet<i32>,
    pub close_attempts: u32,
    pub maximized: bool,
    pub fullscreen: bool,
    /// Folded side tabs are open while the pointer is over them.
    pub vtabs_expanded: bool,
}

fn vtabs_width(settings: &Settings, expanded: bool) -> i32 {
    if settings.appearance.collapse_tabs && !expanded {
        VTABS_COLLAPSED
    } else {
        VTABS_WIDTH
    }
}

pub struct Download {
    pub id: u32,
    pub name: String,
    pub url: String,
    pub path: String,
    pub received: i64,
    pub total: i64,
    pub percent: i32,
    pub speed: i64,
    pub state: &'static str,
    pub paused: bool,
    pub warning: String,
    pub callback: Option<DownloadItemCallback>,
}

#[derive(Default)]
pub struct Ui {
    pub windows: Vec<Win>,
    pub next_id: u32,
    /// CEF browser id → (window, role).
    pub roles: HashMap<i32, (u32, Role)>,
    pub closed_tabs: Vec<(u32, String)>,
    pub downloads: Vec<Download>,
    pub dirty: Vec<u32>,
    pub flush_queued: bool,
    pub last_focused: u32,
    pub session_queued: bool,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// Runs `f` with the UI state. Only from tasks, never from inside a CEF callback (see the module notes).
pub fn with<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
    UI.with(|ui| f(&mut ui.borrow_mut()))
}

/// Like `with`, for callbacks that must answer at once; `None` when the state is busy.
pub fn try_with<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|ui| ui.try_borrow_mut().ok().map(|mut ui| f(&mut ui)))
}

/// Runs `f` with the UI state after the current CEF callback returns.
pub fn later(f: impl FnOnce(&mut Ui) + Send + 'static) {
    on_ui(move || with(f));
}

impl Ui {
    fn next(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }
    pub fn win(&mut self, id: u32) -> Option<&mut Win> {
        self.windows.iter_mut().find(|w| w.id == id)
    }
    /// The window a command without one should act on.
    pub fn focused(&mut self) -> Option<&mut Win> {
        let id = self.last_focused;
        if self.windows.iter().any(|w| w.id == id) {
            self.win(id)
        } else {
            self.windows.last_mut()
        }
    }
    pub fn mark(&mut self, win: u32) {
        if !self.dirty.contains(&win) {
            self.dirty.push(win);
        }
        if !self.flush_queued {
            self.flush_queued = true;
            later(|ui| {
                ui.flush_queued = false;
                let dirty = std::mem::take(&mut ui.dirty);
                for id in dirty {
                    ui.push_state(id);
                }
            });
        }
    }
    pub fn mark_all(&mut self) {
        let ids: Vec<u32> = self.windows.iter().map(|w| w.id).collect();
        for id in ids {
            self.mark(id);
        }
    }
    pub fn queue_session_save(&mut self) {
        if self.session_queued {
            return;
        }
        self.session_queued = true;
        on_ui_later(2000, || {
            with(|ui| {
                ui.session_queued = false;
                ui.save_session();
            })
        });
    }
    pub fn save_session(&self) {
        let tabs: Vec<(String, bool)> = self
            .windows
            .iter()
            .filter(|w| !w.private)
            .flat_map(|w| {
                w.tabs
                    .iter()
                    .map(move |t| (t.url.clone(), Some(t.id) == w.active))
            })
            .filter(|(url, _)| !url.is_empty() && url != NEWTAB_URL)
            .collect();
        if self.windows.iter().any(|w| !w.private) {
            shared::with_store(|s| s.save_session(&tabs));
        }
    }
}

fn runtime_style() -> RuntimeStyle {
    RuntimeStyle::from(rs::CEF_RUNTIME_STYLE_ALLOY)
}

fn browser_settings(role: Role) -> BrowserSettings {
    BrowserSettings {
        // Web pages expect white behind them; the popup is see-through around its rounded box.
        background_color: match role {
            Role::Tab(_) => 0xFFFF_FFFF,
            Role::Popup => 0x0000_0000,
            _ => DARK_BG,
        },
        ..Default::default()
    }
}

// A toolbar, side list or popup: a fixed height or width.
wrap_browser_view_delegate! {
    pub struct FixedView {
        sizes: Arc<Sizes>,
        which: u8,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            match self.which {
                // CEF ignores an empty size (either side 0), so the stretched side says 1.
                0 => Size { width: 1, height: self.sizes.chrome_height.load(Ordering::Relaxed) },
                1 => Size { width: self.sizes.vtabs_width.load(Ordering::Relaxed), height: 1 },
                _ => Size { width: self.sizes.side_width.load(Ordering::Relaxed), height: 1 },
            }
        }
    }

    impl BrowserViewDelegate {
        fn browser_runtime_style(&self) -> RuntimeStyle {
            runtime_style()
        }
        fn chrome_toolbar_type(&self, _browser_view: Option<&mut BrowserView>) -> ChromeToolbarType {
            ChromeToolbarType::from(cef_dll_sys::cef_chrome_toolbar_type_t::CEF_CTT_NONE)
        }
    }
}

wrap_browser_view_delegate! {
    pub struct TabView {}

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn browser_runtime_style(&self) -> RuntimeStyle {
            runtime_style()
        }
        fn chrome_toolbar_type(&self, _browser_view: Option<&mut BrowserView>) -> ChromeToolbarType {
            ChromeToolbarType::from(cef_dll_sys::cef_chrome_toolbar_type_t::CEF_CTT_NONE)
        }
    }
}

wrap_window_delegate! {
    pub struct WinDelegate {
        id: u32,
        frameless: bool,
        created: Arc<AtomicBool>,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 1280, height: 820 }
        }
        fn minimum_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 480, height: 320 }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, _window: Option<&mut Window>) {
            self.created.store(true, Ordering::Relaxed);
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            let id = self.id;
            later(move |ui| {
                ui.windows.retain(|w| w.id != id);
                ui.roles.retain(|_, (w, _)| *w != id);
                if ui.windows.is_empty() {
                    quit_message_loop();
                }
            });
        }

        fn can_close(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            let id = self.id;
            // Each browser runs its beforeunload handlers and then agrees to close (DoClose, which records it in
            // `close_ready`). The window closes once every one has, and takes its browsers with it.
            let pending = try_with(|ui| {
                ui.win(id).map(|w| {
                    w.closing = true;
                    w.browsers
                        .iter()
                        .filter(|(bid, _)| !w.close_ready.contains(bid))
                        .map(|(_, b)| b.clone())
                        .collect::<Vec<Browser>>()
                })
            });
            let Some(pending) = pending else { return 0 };
            let pending = pending.unwrap_or_default();
            for b in &pending {
                if let Some(host) = b.host() {
                    host.try_close_browser();
                }
            }
            // Browsers often agree while we're asking (their DoClose runs inside try_close_browser), so look
            // again; if some are still busy (beforeunload), try again shortly, for up to ten seconds.
            let ready = try_with(|ui| {
                ui.win(id).map(|w| {
                    w.close_attempts += 1;
                    let ready = w.browsers.keys().all(|b| w.close_ready.contains(b));
                    if !ready && w.close_attempts < 50 {
                        let window = w.window.clone();
                        on_ui_later(200, move || window.close());
                    }
                    ready
                })
            })
            .flatten()
            .unwrap_or(true);
            ready as i32
        }

        fn on_window_activation_changed(&self, _window: Option<&mut Window>, active: ::std::os::raw::c_int) {
            if active != 0 {
                let id = self.id;
                later(move |ui| ui.last_focused = id);
            } else {
                let id = self.id;
                later(move |ui| hide_popup(ui, id));
            }
        }

        fn on_window_bounds_changed(&self, window: Option<&mut Window>, _new_bounds: Option<&Rect>) {
            let maximized = window.map(|w| w.is_maximized() != 0).unwrap_or(false);
            let id = self.id;
            later(move |ui| {
                if let Some(win) = ui.win(id) {
                    if win.maximized != maximized {
                        win.maximized = maximized;
                        ui.mark(id);
                    }
                }
                hide_popup(ui, id);
            });
        }

        fn is_frameless(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            self.frameless as i32
        }

        fn can_resize(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            1
        }
        fn can_maximize(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            1
        }
        fn can_minimize(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            1
        }

        fn initial_show_state(&self, _window: Option<&mut Window>) -> ShowState {
            ShowState::from(cef_dll_sys::cef_show_state_t::CEF_SHOW_STATE_NORMAL)
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            runtime_style()
        }
    }
}

/// The toolbar height for these settings.
fn chrome_height(settings: &Settings) -> i32 {
    TABROW_HEIGHT
        + TOOLBAR_HEIGHT
        + if settings.appearance.show_bookmarks_bar {
            BOOKMARKS_HEIGHT
        } else {
            0
        }
}

/// Whether CottonCandy draws its own title bar. On by default; `CC_SYSTEM_TITLEBAR=1` uses the system's.
fn frameless() -> bool {
    std::env::var_os("CC_SYSTEM_TITLEBAR").is_none()
}

pub fn create_view(
    win: u32,
    role: Role,
    url: &str,
    context: Option<&RequestContext>,
    delegate: &mut BrowserViewDelegate,
    tab: Option<Arc<TabShared>>,
) -> Option<BrowserView> {
    let mut client = client::client(win, role, tab);
    let mut context = context.cloned();
    browser_view_create(
        Some(&mut client),
        Some(&cs(url)),
        Some(&browser_settings(role)),
        None,
        context.as_mut(),
        Some(delegate),
    )
}

/// Opens a new window with `urls` as tabs (a new tab page when empty).
pub fn open_window(private: bool, urls: Vec<String>) {
    with(|ui| {
        let id = ui.next();
        let settings = shared::settings();
        let sizes = Arc::new(Sizes::default());
        sizes
            .chrome_height
            .store(chrome_height(&settings), Ordering::Relaxed);
        sizes
            .vtabs_width
            .store(vtabs_width(&settings, false), Ordering::Relaxed);
        sizes.side_width.store(400, Ordering::Relaxed);
        let context = if private {
            // An empty cache path keeps everything in memory, gone when the window closes.
            let mut handler = PrivateContext::new();
            let context = request_context_create_context(
                Some(&RequestContextSettings::default()),
                Some(&mut handler),
            );
            if let Some(c) = &context {
                crate::scheme::register_on(c);
            }
            context
        } else {
            None
        };
        let created = Arc::new(AtomicBool::new(false));
        let mut delegate = WinDelegate::new(id, frameless(), created);
        let Some(window) = window_create_top_level(Some(&mut delegate)) else {
            return;
        };
        window.set_background_color(DARK_BG);
        let Some(layout) = window.set_to_box_layout(Some(&BoxLayoutSettings {
            horizontal: 0,
            cross_axis_alignment: AxisAlignment::from(
                cef_dll_sys::cef_axis_alignment_t::CEF_AXIS_ALIGNMENT_STRETCH,
            ),
            ..Default::default()
        })) else {
            return;
        };
        // The toolbar's page is made now but added to the window after the first tabs, so their renderers
        // start first: a browser starts when its view joins the window.
        let mut chrome_delegate = FixedView::new(sizes.clone(), 0);
        let Some(chrome) = create_view(
            id,
            Role::Chrome,
            "cottoncandy://ui/chrome.html",
            context.as_ref(),
            &mut chrome_delegate,
            None,
        ) else {
            return;
        };
        let Some(body) = panel_create(None) else {
            return;
        };
        let Some(body_layout) = body.set_to_box_layout(Some(&BoxLayoutSettings {
            horizontal: 1,
            cross_axis_alignment: AxisAlignment::from(
                cef_dll_sys::cef_axis_alignment_t::CEF_AXIS_ALIGNMENT_STRETCH,
            ),
            ..Default::default()
        })) else {
            return;
        };
        let Some(tabs_host) = panel_create(None) else {
            return;
        };
        tabs_host.set_to_fill_layout();
        tabs_host.set_background_color(DARK_BG);
        window.add_child_view(Some(&mut View::from(&body)));
        layout.set_flex_for_view(Some(&mut View::from(&body)), 1);
        body.add_child_view(Some(&mut View::from(&tabs_host)));
        body_layout.set_flex_for_view(Some(&mut View::from(&tabs_host)), 1);

        window.set_title(Some(&cs(if private {
            "CottonCandy (Private)"
        } else {
            "CottonCandy"
        })));
        if let Some(icon) = app_icon() {
            let mut icon = icon;
            window.set_window_app_icon(Some(&mut icon));
        }
        window.center_window(Some(&Size {
            width: 1280,
            height: 820,
        }));
        window.show();
        window.layout();
        ui.windows.push(Win {
            id,
            private,
            window,
            context,
            sizes,
            chrome,
            chrome_browser: None,
            vtabs: None,
            vtabs_browser: None,
            tabs_host,
            body,
            grace: GracePanel::default(),
            popup: None,
            tabs: Vec::new(),
            active: None,
            browsers: HashMap::new(),
            closing: false,
            close_ready: Default::default(),
            close_attempts: 0,
            maximized: false,
            fullscreen: false,
            vtabs_expanded: false,
        });
        ui.last_focused = id;
        let urls = if urls.is_empty() {
            vec![NEWTAB_URL.to_string()]
        } else {
            urls
        };
        let count = urls.len();
        for (i, url) in urls.into_iter().enumerate() {
            ui.new_tab(id, &url, i + 1 == count, None);
        }
        if let Some(w) = ui.win(id) {
            w.window
                .add_child_view_at(Some(&mut View::from(&w.chrome)), 0);
            if settings.appearance.tab_layout == "vertical" {
                ensure_vtabs(w);
            }
            w.window.layout();
        }
    });
}

/// Makes the side tabs' page the first time the window needs it. Windows with tabs on top never pay for it.
fn ensure_vtabs(w: &mut Win) {
    if w.vtabs.is_some() {
        return;
    }
    let mut delegate = FixedView::new(w.sizes.clone(), 1);
    let Some(view) = create_view(
        w.id,
        Role::VTabs,
        "cottoncandy://ui/vtabs.html",
        w.context.as_ref(),
        &mut delegate,
        None,
    ) else {
        return;
    };
    w.body.add_child_view_at(Some(&mut View::from(&view)), 0);
    w.vtabs = Some(view);
}

/// Makes the floating popup (suggestions, menus, Shields) the first time it is needed, or when the pointer
/// first comes near the toolbar, so it is ready by the time it is asked for.
pub fn ensure_popup(w: &mut Win) {
    if w.popup.is_some() {
        return;
    }
    let mut delegate = FixedView::new(w.sizes.clone(), 2);
    let Some(view) = create_view(
        w.id,
        Role::Popup,
        "cottoncandy://ui/popup.html",
        w.context.as_ref(),
        &mut delegate,
        None,
    ) else {
        return;
    };
    let Some(controller) = w.window.add_overlay_view(
        Some(&mut View::from(&view)),
        DockingMode::from(cef_dll_sys::cef_docking_mode_t::CEF_DOCKING_MODE_CUSTOM),
        1,
    ) else {
        return;
    };
    controller.set_visible(0);
    w.popup = Some(Popup {
        controller,
        browser: None,
        ready: false,
        kind: String::new(),
        rect: rect(0, 0, 0, 0),
        pending: None,
    });
}

fn app_icon() -> Option<Image> {
    let image = image_create()?;
    let png = crate::icon::PNG_64;
    (image.add_png(1.0, Some(png)) != 0).then_some(image)
}

impl Ui {
    /// Opens `url` in a new tab of window `win`, at `index` (the end when `None`).
    pub fn new_tab(
        &mut self,
        win: u32,
        url: &str,
        foreground: bool,
        index: Option<usize>,
    ) -> Option<u32> {
        let id = self.next();
        let w = self.win(win)?;
        let shared = Arc::new(TabShared::new(url, w.private));
        let mut delegate = TabView::new();
        let view = create_view(
            win,
            Role::Tab(id),
            url,
            w.context.as_ref(),
            &mut delegate,
            Some(shared.clone()),
        )?;
        view.set_visible(0);
        w.tabs_host.add_child_view(Some(&mut View::from(&view)));
        let tab = Tab {
            id,
            view,
            browser: None,
            shared,
            url: url.to_string(),
            title: if url == NEWTAB_URL {
                "New tab".into()
            } else {
                String::new()
            },
            loading: true,
            progress: 0.0,
            can_back: false,
            can_forward: false,
            favicon: None,
            favicon_url: String::new(),
            pinned: false,
            audible: false,
            muted: false,
            fullscreen: false,
            zoom: shared::settings().appearance.default_zoom as i32,
        };
        let at = index.unwrap_or(w.tabs.len()).min(w.tabs.len());
        w.tabs.insert(at, tab);
        if foreground || w.active.is_none() {
            self.activate(win, id);
        }
        self.mark(win);
        Some(id)
    }

    pub fn activate(&mut self, win: u32, tab: u32) {
        let Some(w) = self.win(win) else { return };
        if !w.tabs.iter().any(|t| t.id == tab) {
            return;
        }
        for t in &w.tabs {
            t.view.set_visible((t.id == tab) as i32);
            if let (Some(b), false) = (&t.browser, t.id == tab) {
                if let Some(host) = b.host() {
                    host.was_hidden(1);
                }
            }
        }
        w.active = Some(tab);
        w.tabs_host.layout();
        if let Some(t) = w.tabs.iter().find(|t| t.id == tab) {
            if let Some(host) = t.browser.as_ref().and_then(|b| b.host()) {
                host.was_hidden(0);
            }
            if t.url == NEWTAB_URL {
                focus_address(w, None);
            } else {
                t.view.request_focus();
            }
        }
        hide_popup(self, win);
        self.mark(win);
    }

    pub fn close_tab(&mut self, win: u32, tab: u32) {
        let Some(w) = self.win(win) else { return };
        let Some(t) = w.tabs.iter().find(|t| t.id == tab) else {
            return;
        };
        let url = t.url.clone();
        match t.browser.as_ref().and_then(|b| b.host()) {
            // The tab leaves once its page agrees (beforeunload); see `browser_closed`.
            Some(host) => host.close_browser(0),
            None => self.remove_tab(win, tab),
        }
        if !url.is_empty() && url != NEWTAB_URL && !self.win(win).map(|w| w.private).unwrap_or(true)
        {
            self.closed_tabs.push((win, url));
            if self.closed_tabs.len() > 25 {
                self.closed_tabs.remove(0);
            }
        }
    }

    /// Takes a closed tab out of its window, and closes the window after its last tab.
    pub fn remove_tab(&mut self, win: u32, tab: u32) {
        let Some(w) = self.win(win) else { return };
        let Some(index) = w.tabs.iter().position(|t| t.id == tab) else {
            return;
        };
        let t = w.tabs.remove(index);
        w.tabs_host
            .remove_child_view(Some(&mut View::from(&t.view)));
        if w.tabs.is_empty() {
            if !w.closing {
                close_window(&w.window);
            }
            return;
        }
        if w.active == Some(tab) {
            let next = w.tabs[index.min(w.tabs.len() - 1)].id;
            self.activate(win, next);
        }
        self.mark(win);
        self.queue_session_save();
    }

    pub fn tab(&mut self, win: u32, tab: u32) -> Option<&mut Tab> {
        self.win(win)?.tabs.iter_mut().find(|t| t.id == tab)
    }

    pub fn active_tab(&mut self, win: u32) -> Option<&mut Tab> {
        let w = self.win(win)?;
        let id = w.active?;
        w.tabs.iter_mut().find(|t| t.id == id)
    }

    /// Opens what the user typed or picked: in the current tab, or a new one.
    pub fn navigate(&mut self, win: u32, text: &str, new_tab: bool) -> Result<(), String> {
        let template = shared::settings().search.template();
        let url = omnibox::resolve_input(text, &template).ok_or("Nothing to open")?;
        if omnibox::navigation_decision(&url, "", true, true, true) == omnibox::Navigation::Handoff
        {
            open_external(&url);
            return Ok(());
        }
        if new_tab {
            self.new_tab(win, &url, true, None);
            return Ok(());
        }
        let Some(tab) = self.active_tab(win) else {
            self.new_tab(win, &url, true, None);
            return Ok(());
        };
        load(tab, &url);
        tab.view.request_focus();
        Ok(())
    }

    pub fn push_state(&mut self, win: u32) {
        let downloads = downloads_summary(&self.downloads);
        let Some(w) = self.win(win) else { return };
        let settings = shared::settings();
        let state = window_state(w, &settings, downloads);
        let js = emit_js("state", &state);
        for b in [&w.chrome_browser, &w.vtabs_browser].into_iter().flatten() {
            run_js(b, &js);
        }
        crate::grace::update_panel(w);
        let title = w
            .active
            .and_then(|id| w.tabs.iter().find(|t| t.id == id))
            .map(|t| t.title.clone())
            .filter(|t| !t.is_empty())
            .map(|t| format!("{t} - CottonCandy"))
            .unwrap_or_else(|| "CottonCandy".into());
        w.window.set_title(Some(&cs(&title)));
    }

    /// Re-reads layout settings (bookmarks bar, tab layout) into every window.
    pub fn relayout(&mut self) {
        let settings = shared::settings();
        for w in &mut self.windows {
            w.sizes
                .chrome_height
                .store(chrome_height(&settings), Ordering::Relaxed);
            w.sizes
                .vtabs_width
                .store(vtabs_width(&settings, w.vtabs_expanded), Ordering::Relaxed);
            let vertical = settings.appearance.tab_layout == "vertical";
            if vertical {
                ensure_vtabs(w);
            }
            if let Some(v) = &w.vtabs {
                v.invalidate_layout();
                v.set_visible((vertical && !w.fullscreen) as i32);
            }
            w.chrome.invalidate_layout();
            w.window.layout();
        }
        self.mark_all();
    }

    /// Opens or folds the side tabs (only while they fold by themselves).
    pub fn set_vtabs_expanded(&mut self, win: u32, open: bool) {
        let settings = shared::settings();
        let Some(w) = self.win(win) else { return };
        if w.vtabs_expanded == open || !settings.appearance.collapse_tabs {
            return;
        }
        w.vtabs_expanded = open;
        w.sizes
            .vtabs_width
            .store(vtabs_width(&settings, open), Ordering::Relaxed);
        if let Some(v) = &w.vtabs {
            v.invalidate_layout();
        }
        w.window.layout();
        self.mark(win);
    }

    pub fn set_fullscreen(&mut self, win: u32, on: bool) {
        let settings = shared::settings();
        let Some(w) = self.win(win) else { return };
        w.fullscreen = on;
        w.window.set_fullscreen(on as i32);
        w.chrome.set_visible((!on) as i32);
        if let Some(v) = &w.vtabs {
            v.set_visible((settings.appearance.tab_layout == "vertical" && !on) as i32);
        }
        if let Some(v) = &w.grace.view {
            v.set_visible((w.grace.open && !on) as i32);
        }
        w.window.layout();
    }
}

/// Loads `url` in a tab, remembering that the user asked for it.
pub fn load(tab: &mut Tab, url: &str) {
    if let Ok(mut typed) = tab.shared.typed.lock() {
        *typed = url.to_string();
    }
    if let Some(frame) = tab.browser.as_ref().and_then(|b| b.main_frame()) {
        frame.load_url(Some(&cs(url)));
    }
}

pub fn focus_address(w: &Win, text: Option<&str>) {
    if let Some(b) = &w.chrome_browser {
        if let Some(host) = b.host() {
            host.set_focus(1);
        }
        w.chrome.request_focus();
        run_js(b, &emit_js("focus_address", &json!(text)));
    }
}

fn downloads_summary(downloads: &[Download]) -> Json {
    let active: Vec<&Download> = downloads.iter().filter(|d| d.state == "progress").collect();
    let (got, total) = active.iter().fold((0i64, 0i64), |(g, t), d| {
        (g + d.received, t + d.total.max(0))
    });
    json!({
        "count": downloads.len(),
        "active": !active.is_empty(),
        "percent": if total > 0 { (got * 100 / total) as i32 } else { 0 },
    })
}

fn display_url(url: &str) -> &str {
    if url == NEWTAB_URL {
        ""
    } else {
        url
    }
}

fn window_state(w: &Win, settings: &Settings, downloads: Json) -> Json {
    let tabs: Vec<Json> = w
        .tabs
        .iter()
        .map(|t| {
            json!({
                "id": t.id, "title": t.title, "url": display_url(&t.url), "loading": t.loading,
                "progress": t.progress, "pinned": t.pinned, "audible": t.audible, "muted": t.muted,
                "favicon": t.favicon, "internal": t.url.starts_with("cottoncandy:"),
            })
        })
        .collect();
    let active = w.active.and_then(|id| w.tabs.iter().find(|t| t.id == id));
    let url = active.map(|t| t.url.as_str()).unwrap_or("");
    let parts = omnibox::url_parts(display_url(url));
    let bookmarked = !url.is_empty()
        && shared::with_store(|s| s.is_bookmarked(url).unwrap_or(false)).unwrap_or(false);
    let bookmarks: Vec<Json> = if settings.appearance.show_bookmarks_bar {
        shared::with_store(|s| s.bookmarks("").unwrap_or_default())
            .unwrap_or_default()
            .into_iter()
            .take(40)
            .map(|e| json!({"url": e.url, "title": e.title}))
            .collect()
    } else {
        Vec::new()
    };
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
        .unwrap_or_default();
    let shields = settings.shields_for(&host);
    let proxy = &settings.network.proxy;
    json!({
        "window": w.id,
        "private": w.private,
        "tabs": tabs,
        "active": w.active,
        "nav": {
            "url": display_url(url),
            "parts": parts,
            "loading": active.map(|t| t.loading).unwrap_or(false),
            "can_back": active.map(|t| t.can_back).unwrap_or(false),
            "can_forward": active.map(|t| t.can_forward).unwrap_or(false),
            "bookmarked": bookmarked,
        },
        "shields": {
            "applies": url.starts_with("http"),
            "on": shields.shields_up,
            "blocked": active.map(|t| { let (a, b) = t.shared.blocked(); a + b }).unwrap_or(0),
        },
        "proxy": {
            "on": proxy.enabled,
            "label": if proxy.enabled { proxy_label(settings) } else { "Direct".into() },
        },
        "downloads": downloads,
        "bookmarks": bookmarks,
        "ui": {
            "theme": resolved_theme(settings),
            "tab_layout": settings.appearance.tab_layout,
            "tabs_collapsed": settings.appearance.collapse_tabs && !w.vtabs_expanded,
            "collapse_tabs": settings.appearance.collapse_tabs,
            "bookmarks_bar": settings.appearance.show_bookmarks_bar,
            "home_button": settings.appearance.show_home_button,
            "controls": if frameless() { if cfg!(target_os = "macos") { "left" } else { "right" } } else { "none" },
            "maximized": w.maximized,
            "extensions": settings.extensions.enabled,
            "grace": settings.grace.enabled,
            "grace_open": w.grace.open,
        },
    })
}

pub fn proxy_label(settings: &Settings) -> String {
    let proxy = &settings.network.proxy;
    cottoncandy_core::settings::PROXY_PRESETS
        .iter()
        .find(|p| p.0 == proxy.preset)
        .map(|p| p.1.to_string())
        .unwrap_or_else(|| match proxy.mode.as_str() {
            "pac" => "PAC script".into(),
            "system" => "System proxy".into(),
            _ => format!("{}:{}", proxy.host, proxy.port),
        })
}

fn resolved_theme(settings: &Settings) -> &str {
    match settings.appearance.theme.as_str() {
        "light" => "light",
        "dark" => "dark",
        _ => "system",
    }
}

const LIGHT_VARS: &str = "color-scheme: light; --bg: #f5f6f8; --bg-2: #eceef2; --panel: #ffffff; --card: #ffffff; \
--hover: #e7e9ee; --selected: #dde1e9; --hair: rgba(0,0,0,.09); --text: #14161b; --text-2: #5b6270; --text-3: #8b92a0; \
--glass: rgba(255,255,255,.75);";

/// The style sheet every CottonCandy page loads after base.css, from Settings → Appearance.
pub fn theme_css() -> String {
    let settings = shared::settings();
    let a = &settings.appearance;
    let accent = cottoncandy_core::settings::accent_color(&a.accent);
    let mut css = format!(
        ":root {{ --accent: {accent}; --accent-soft: color-mix(in srgb, {accent} 18%, transparent); \
         --radius: {}px; --font-size: {}px; }}\n",
        a.corner_radius, a.font_size
    );
    match a.theme.as_str() {
        "light" => css += &format!(":root {{ {LIGHT_VARS} }}\n"),
        "dark" => {}
        _ => {
            css += &format!("@media (prefers-color-scheme: light) {{ :root {{ {LIGHT_VARS} }} }}\n")
        }
    }
    match a.density.as_str() {
        "compact" => css += ":root { --pad: 5px; --row: 28px; }\n",
        "spacious" => css += ":root { --pad: 11px; --row: 40px; }\n",
        _ => {}
    }
    if !a.motion {
        css += ":root { --speed: 0s; } *, *::after { animation: none !important; }\n";
    }
    if !a.translucency {
        css += ":root { --glass: var(--bg-2); --blur: 0px; }\n";
    }
    // The user's own CSS can't close the style sheet or pull anything in from outside.
    let custom = a.custom_css.replace("</", "").replace("@import", "");
    css += &custom;
    css
}

pub fn hide_popup(ui: &mut Ui, win: u32) {
    if let Some(p) = ui.win(win).and_then(|w| w.popup.as_mut()) {
        p.pending = None;
        if p.controller.is_visible() != 0 {
            p.controller.set_visible(0);
            p.kind.clear();
        }
    }
}

/// Shows the floating popup with `data` (kind, items...) at `r`, in window coordinates.
pub fn show_popup(ui: &mut Ui, win: u32, kind: &str, r: Rect, data: Json) {
    let Some(w) = ui.win(win) else { return };
    let bounds = w.window.bounds();
    ensure_popup(w);
    let Some(p) = w.popup.as_mut() else { return };
    let width = r.width.clamp(120, 720);
    let height = if r.height > 0 { r.height } else { 200 }.clamp(40, 640);
    let x = r.x.clamp(0, (bounds.width - width).max(0));
    let y = r.y.clamp(0, (bounds.height - 40).max(0));
    p.kind = kind.to_string();
    p.rect = rect(x, y, width, height);
    p.controller.set_bounds(Some(&p.rect));
    if let (true, Some(b)) = (p.ready, &p.browser) {
        p.controller.set_visible(1);
        run_js(b, &emit_js("show", &data));
        p.pending = None;
    } else {
        // A popup made just now shows once its page is ready, rather than as an empty box.
        p.pending = Some(data);
        return;
    }
    // Suggestions keep focus in the address bar; everything else takes it.
    if kind != "suggest" {
        if let Some(view) = p.controller.contents_view() {
            view.request_focus();
        }
    }
}

pub fn resize_popup(ui: &mut Ui, win: u32, height: i32) {
    let Some(w) = ui.win(win) else { return };
    let max = w.window.bounds().height - 8;
    let Some(p) = w.popup.as_mut() else { return };
    let height = height.clamp(24, (max - p.rect.y).max(24));
    if height != p.rect.height {
        p.rect.height = height;
        p.controller.set_bounds(Some(&p.rect));
    }
}

/// Opens a mail or phone link with the system's app. The URL is one argument, never shell text.
pub fn open_external(url: &str) {
    if !(url.starts_with("mailto:") || url.starts_with("tel:")) {
        return;
    }
    let _ = system_open(url);
}

/// Opens a file, folder or handoff link with the desktop.
pub fn system_open(target: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler").arg(target);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target);
        c
    };
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

/// Shows a file in its folder.
pub fn show_in_folder(path: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{path}"))
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let dir = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let _ = system_open(&dir);
    }
}

/// What to open at startup, from the command line or Settings → On startup.
pub fn startup_urls(args: &[String]) -> Vec<String> {
    let settings = shared::settings();
    let template = settings.search.template();
    let from_args: Vec<String> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .filter_map(|a| omnibox::resolve_input(a, &template))
        .collect();
    if !from_args.is_empty() {
        return from_args;
    }
    // After the Restart button, pick up where the user was whatever the startup setting says.
    let restarted = args.iter().any(|a| a == "--cc-restarted");
    match if restarted {
        "restore"
    } else {
        settings.startup.open.as_str()
    } {
        "restore" => shared::with_store(|s| s.last_session().unwrap_or_default())
            .unwrap_or_default()
            .into_iter()
            .map(|(url, _)| url)
            .collect(),
        "homepage" => omnibox::resolve_input(&settings.startup.homepage, &template)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// The unpacked extensions to load at startup.
pub fn extension_dirs(settings: &Settings) -> Vec<String> {
    if !settings.extensions.enabled {
        return Vec::new();
    }
    let data = cottoncandy_core::paths::data_dir();
    settings
        .extensions
        .installed
        .iter()
        .filter(|(_, e)| e.enabled)
        .filter_map(|(id, _)| cottoncandy_core::extensions::install_dir(&data, id))
        .filter(|dir| dir.join("manifest.json").is_file())
        .map(|dir| dir.to_string_lossy().into_owned())
        .collect()
}

wrap_request_context_handler! {
    struct PrivateContext {}

    impl RequestContextHandler {
        fn on_request_context_initialized(&self, request_context: Option<&mut RequestContext>) {
            // Private windows go through the same proxy, DNS and privacy settings as the rest.
            if let Some(c) = request_context {
                apply_network_settings(Some(c));
                crate::shields::apply_scripts(c, &shared::settings());
            }
        }
    }
}

/// Puts the per-site script switches into every context, global and private.
pub fn apply_site_settings(ui: &Ui, settings: &Settings) {
    if let Some(global) = request_context_get_global_context() {
        crate::shields::apply_scripts(&global, settings);
    }
    for w in &ui.windows {
        if let Some(c) = &w.context {
            crate::shields::apply_scripts(c, settings);
        }
    }
}

/// Puts the proxy and secure DNS settings into Chromium. Returns the first error, if any.
pub fn apply_network_settings(context: Option<&RequestContext>) -> Option<String> {
    let settings = shared::settings();
    let context = match context {
        Some(c) => c.clone(),
        None => request_context_get_global_context()?,
    };
    let mut error = None;
    let proxy = settings.network.proxy.chromium_pref();
    if let Err(e) = set_pref(&context, "proxy", &proxy) {
        error.get_or_insert(e);
    }
    // Secure DNS is a browser-wide ("local state") setting, so it goes to the global preferences.
    if let Some(global) = preference_manager_get_global() {
        for (name, value) in settings.network.dns.chromium_prefs() {
            if let Err(e) = set_pref(&global, name, &value) {
                error.get_or_insert(e);
            }
        }
    }
    if let (Some(e), true) = (&error, std::env::var_os("CC_DEBUG").is_some()) {
        eprintln!("[network] {e} (proxy {proxy})");
    }
    let _ = set_pref(
        &context,
        "webrtc.ip_handling_policy",
        &json!(settings.privacy.webrtc_ip_policy),
    );
    // No preconnecting or prefetching: Chromium would open connections to hosts (ad servers among them)
    // that Shields never sees a request for.
    let _ = set_pref(&context, "net.network_prediction_options", &json!(2));
    let _ = set_pref(
        &context,
        "enable_do_not_track",
        &json!(settings.shields.send_gpc),
    );
    let _ = set_pref(
        &context,
        "credentials_enable_service",
        &json!(settings.privacy.save_passwords),
    );
    let _ = set_pref(
        &context,
        "profile.cookie_controls_mode",
        &json!(if settings.shields.block_third_party_cookies {
            1
        } else {
            0
        }),
    );
    error
}

/// One download's progress, copied out of CEF's item (which is only valid during the callback).
pub struct DlSnap {
    pub id: u32,
    pub path: String,
    pub suggested: String,
    pub url: String,
    pub received: i64,
    pub total: i64,
    pub percent: i32,
    pub speed: i64,
    pub state: &'static str,
    pub paused: bool,
}

impl DlSnap {
    pub fn of(item: &DownloadItem) -> DlSnap {
        DlSnap {
            id: item.id(),
            path: us(&item.full_path()),
            suggested: us(&item.suggested_file_name()),
            url: us(&item.original_url()),
            received: item.received_bytes(),
            total: item.total_bytes(),
            percent: item.percent_complete().max(0),
            speed: item.current_speed(),
            state: if item.is_complete() != 0 {
                "done"
            } else if item.is_canceled() != 0 {
                "canceled"
            } else if item.is_interrupted() != 0 {
                "failed"
            } else {
                "progress"
            },
            paused: item.is_paused() != 0,
        }
    }
}

/// Records a download update from CEF.
pub fn download_updated(ui: &mut Ui, snap: DlSnap, callback: Option<DownloadItemCallback>) {
    let name = std::path::Path::new(&snap.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| snap.suggested.clone());
    if !ui.downloads.iter().any(|d| d.id == snap.id) {
        ui.downloads.insert(
            0,
            Download {
                id: snap.id,
                name: String::new(),
                url: snap.url.clone(),
                path: String::new(),
                received: 0,
                total: 0,
                percent: 0,
                speed: 0,
                state: "progress",
                paused: false,
                warning: String::new(),
                callback: None,
            },
        );
        ui.downloads.truncate(200);
    }
    let Some(d) = ui.downloads.iter_mut().find(|d| d.id == snap.id) else {
        return;
    };
    d.warning = if risky_file(&name) {
        "Can run programs".into()
    } else {
        String::new()
    };
    d.name = name;
    d.path = snap.path;
    d.received = snap.received;
    d.total = snap.total;
    d.percent = snap.percent;
    d.speed = snap.speed;
    d.state = snap.state;
    d.paused = snap.paused;
    if callback.is_some() && snap.state == "progress" {
        d.callback = callback;
    } else if snap.state != "progress" {
        d.callback = None;
    }
    ui.mark_all();
}

/// Files that run code when opened.
pub fn risky_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        ".exe",
        ".msi",
        ".bat",
        ".cmd",
        ".com",
        ".scr",
        ".ps1",
        ".vbs",
        ".js",
        ".jar",
        ".lnk",
        ".hta",
        ".reg",
        ".sh",
        ".appimage",
        ".run",
        ".deb",
        ".rpm",
        ".desktop",
        ".pkg.tar.zst",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

pub fn download_json(d: &Download) -> Json {
    let status = match d.state {
        "progress" if d.paused => format!("Paused, {}", size_text(d.received)),
        "progress" if d.total > 0 => format!(
            "{} of {}, {}/s",
            size_text(d.received),
            size_text(d.total),
            size_text(d.speed)
        ),
        "progress" => format!("{}, {}/s", size_text(d.received), size_text(d.speed)),
        "done" => size_text(d.total.max(d.received)),
        "canceled" => "Canceled".into(),
        _ => "Failed".into(),
    };
    json!({
        "id": d.id, "name": d.name, "url": d.url, "path": d.path, "state": d.state, "percent": d.percent,
        "paused": d.paused, "status": status, "warning": d.warning,
    })
}

fn size_text(bytes: i64) -> String {
    let b = bytes.max(0) as f64;
    if b < 1024.0 {
        format!("{b:.0} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    } else {
        format!("{:.2} GB", b / 1024.0 / 1024.0 / 1024.0)
    }
}

/// Chromium zoom levels are powers of 1.2; Settings uses percent.
pub fn zoom_level(percent: i32) -> f64 {
    (percent.clamp(25, 500) as f64 / 100.0).ln() / 1.2f64.ln()
}

pub const ZOOM_STEPS: &[i32] = &[
    25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_100_is_level_0() {
        assert!(zoom_level(100).abs() < 1e-9);
        assert!((zoom_level(120) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn executables_are_flagged() {
        assert!(risky_file("setup.EXE"));
        assert!(risky_file("tool.AppImage"));
        assert!(!risky_file("photo.png"));
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(size_text(512), "512 B");
        assert_eq!(size_text(2048), "2 KB");
        assert_eq!(size_text(5 * 1024 * 1024), "5.0 MB");
    }
}
