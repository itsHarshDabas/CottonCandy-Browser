//! Grace, the local AI: Enhance search in the address bar, and the side panel that summarizes or answers
//! questions about the page and holds Grace Chat. Everything goes through `cherrycola ask` on this computer,
//! run on a worker thread with the text on stdin (never on the command line). What it writes is streamed to
//! the page that asked. One run per page and kind: Stop, or a newer question, ends the old one.

use crate::browser::{self, later, Ui, Win};
use crate::cefx::*;
use crate::client::Role;
use crate::shared;
use cef::*;
use cottoncandy_core::grace as g;
use serde_json::{json, Value as Json};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Res = Result<Json, String>;

pub const GRACE_URL: &str = "cottoncandy://grace/";
/// Enhance search gives up after this (the first run may have to load the model).
const ENHANCE_TIMEOUT: Duration = Duration::from_secs(60);
/// Page questions and chat replies can take a while on a slow computer, but not forever.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const OUTPUT_LIMIT: usize = 128 * 1024;
const STDERR_LIMIT: usize = 8 * 1024;
const STREAM_EVERY: Duration = Duration::from_millis(120);

/// Runs going on now: (browser id, slot, state). State: RUNNING, STOPPED (by the user) or REPLACED.
type Runs = Vec<(i32, &'static str, Arc<AtomicU8>)>;
const RUNNING: u8 = 0;
const STOPPED: u8 = 1;
const REPLACED: u8 = 2;
static RUNS: Mutex<Runs> = Mutex::new(Vec::new());
/// `cherrycola ask --help`, checked once per command: None if it isn't installed.
static FEATURES: Mutex<Option<(String, Option<g::Features>)>> = Mutex::new(None);

/// The CherryCola command to run. A bare name that isn't on PATH falls back to where Grace's own installer
/// puts it: a CherryColaLLM installed a moment ago isn't on this process's PATH yet.
fn command() -> String {
    let cmd = shared::settings().grace.command;
    if cmd.contains(['/', '\\']) || on_path(&cmd) {
        return cmd;
    }
    installed_command()
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(cmd)
}

fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let names: Vec<String> = if cfg!(windows) {
        vec![
            format!("{name}.exe"),
            format!("{name}.cmd"),
            format!("{name}.bat"),
            name.to_string(),
        ]
    } else {
        vec![name.to_string()]
    };
    std::env::split_paths(&path).any(|dir| names.iter().any(|n| dir.join(n).is_file()))
}

/// Where Grace's installer puts CherryColaLLM's command.
fn installed_command() -> Option<std::path::PathBuf> {
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(
            std::path::PathBuf::from(local)
                .join("Programs")
                .join("CherryColaLLM")
                .join("cherrycola.exe"),
        )
    } else {
        Some(
            linux_install_dir()
                .join("usr")
                .join("bin")
                .join("cherrycola"),
        )
    }
}

fn linux_install_dir() -> std::path::PathBuf {
    cottoncandy_core::paths::data_dir().join("cherrycolallm")
}

/// Forgets what CherryCola can do, so the next question checks again (after installing it).
fn rescan() {
    if let Ok(mut cached) = FEATURES.lock() {
        *cached = None;
    }
}

/// What the installed CherryCola can do. Runs `cherrycola ask --help` the first time (well under a second).
fn features() -> Option<g::Features> {
    let cmd = command();
    if let Ok(cached) = FEATURES.lock() {
        if let Some((c, f)) = cached.as_ref() {
            if *c == cmd {
                return *f;
            }
        }
    }
    let found = run_quiet(&cmd, &["ask", "--help"])
        .map(|help| g::ask_features(&help))
        .ok();
    if let Ok(mut cached) = FEATURES.lock() {
        *cached = Some((cmd, found));
    }
    found
}

/// Checks CherryCola in the background so the panel opens without waiting for it.
pub fn warm_up() {
    if shared::settings().grace.enabled {
        std::thread::spawn(|| {
            let _ = features();
        });
    }
}

fn base_command(cmd: &str) -> Command {
    let mut c = Command::new(cmd);
    c.env("NO_COLOR", "1").env("PYTHONIOENCODING", "utf-8");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up for every question.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

fn run_quiet(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = base_command(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Takes out terminal color codes, in case a CherryCola prints them anyway.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if c == '\r' || (c.is_control() && c != '\n' && c != '\t') {
            continue;
        }
        out.push(c);
    }
    out
}

/// Sends `event` to the page in browser `browser_id`, if it is still open.
fn emit(browser_id: i32, event: &'static str, data: Json) {
    later(move |ui| {
        let found = ui
            .windows
            .iter()
            .find_map(|w| w.browsers.get(&browser_id).cloned());
        if let Some(b) = found {
            run_js(&b, &emit_js(event, &data));
        }
    });
}

type Failure = (&'static str, String);

struct Run {
    browser_id: i32,
    slot: &'static str,
    args: Vec<String>,
    input: String,
    timeout: Duration,
    /// Send what has been written so far as `grace_partial` while it runs.
    stream: bool,
}

/// Starts `cherrycola` on a worker thread; `done` gets its output, or why it failed.
fn start(run: Run, done: impl FnOnce(Result<String, Failure>) + Send + 'static) {
    let stop = Arc::new(AtomicU8::new(RUNNING));
    if let Ok(mut runs) = RUNS.lock() {
        runs.retain(|(b, s, flag)| {
            let same = *b == run.browser_id && *s == run.slot;
            if same {
                flag.store(REPLACED, Ordering::SeqCst);
            }
            !same
        });
        runs.push((run.browser_id, run.slot, stop.clone()));
    }
    let cmd = command();
    std::thread::spawn(move || {
        let result = execute(&cmd, &run, &stop);
        if let Ok(mut runs) = RUNS.lock() {
            runs.retain(|(_, _, flag)| !Arc::ptr_eq(flag, &stop));
        }
        // A run that a newer one replaced says nothing: the page is waiting for the new one.
        if stop.load(Ordering::SeqCst) != REPLACED {
            done(result);
        }
    });
}

fn execute(cmd: &str, run: &Run, stop: &AtomicU8) -> Result<String, Failure> {
    let mut child = base_command(cmd)
        .args(&run.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| g::failure(None, ""))?;
    let mut stdin = child.stdin.take();
    let input = run.input.clone();
    let writer = std::thread::spawn(move || {
        if let Some(stdin) = stdin.as_mut() {
            let _ = stdin.write_all(input.as_bytes());
        }
    });
    let out = Arc::new(Mutex::new(Vec::new()));
    let err = Arc::new(Mutex::new(Vec::new()));
    let reader = |pipe: Option<Box<dyn Read + Send>>,
                  buf: Arc<Mutex<Vec<u8>>>,
                  limit: usize,
                  stream: Option<(i32, &'static str)>| {
        std::thread::spawn(move || {
            let Some(mut pipe) = pipe else { return };
            let mut chunk = [0u8; 4096];
            let mut last = Instant::now();
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let text = {
                    let mut b = buf.lock().unwrap();
                    if b.len() < limit {
                        let room = limit - b.len();
                        b.extend_from_slice(&chunk[..n.min(room)]);
                    }
                    stream
                        .filter(|_| last.elapsed() >= STREAM_EVERY)
                        .map(|_| String::from_utf8_lossy(&b).into_owned())
                };
                if let (Some(text), Some((browser_id, slot))) = (text, stream) {
                    last = Instant::now();
                    emit(
                        browser_id,
                        "grace_partial",
                        json!({ "slot": slot, "text": strip_ansi(&text).trim() }),
                    );
                }
            }
        })
    };
    let stream = run.stream.then_some((run.browser_id, run.slot));
    let out_reader = reader(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        out.clone(),
        OUTPUT_LIMIT,
        stream,
    );
    let err_reader = reader(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        err.clone(),
        STDERR_LIMIT,
        None,
    );
    let started = Instant::now();
    let status = loop {
        if stop.load(Ordering::SeqCst) != RUNNING {
            let _ = child.kill();
            let _ = child.wait();
            return Err(("stopped", "Stopped.".into()));
        }
        if started.elapsed() > run.timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err((
                "timeout",
                "Grace took too long to answer, so it was stopped.".into(),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(("error", e.to_string())),
        }
    };
    let _ = writer.join();
    let _ = out_reader.join();
    let _ = err_reader.join();
    let out = strip_ansi(&String::from_utf8_lossy(&out.lock().unwrap()));
    let err = String::from_utf8_lossy(&err.lock().unwrap()).into_owned();
    if status.success() {
        Ok(out.trim().to_string())
    } else {
        Err(g::failure(status.code(), &err))
    }
}

/// Why Grace can't run now, or None if it can.
fn blocked() -> Option<String> {
    let settings = shared::settings();
    if !settings.grace.enabled {
        return Some("Grace is turned off. Turn it on in Settings, Grace.".into());
    }
    if features().is_none() {
        return Some(g::failure(None, "").1);
    }
    let conf =
        std::fs::read_to_string(cottoncandy_core::paths::cherrycola_conf()).unwrap_or_default();
    if let Some(host) = g::remote_model_server(&conf) {
        return Some(format!(
            "Grace only uses a model on this computer, and CherryCola is set to use {host}. Change that in CherryCola to use Grace."
        ));
    }
    None
}

// ---- the side panel ----------------------------------------------------------------------------------

/// Opens or closes Grace's side panel (`open` None toggles it).
pub fn toggle(ui: &mut Ui, win: u32, open: Option<bool>) {
    let Some(w) = ui.win(win) else { return };
    let open = open.unwrap_or(!w.grace.open);
    if open && w.grace.view.is_none() {
        let mut delegate = browser::FixedView::new(w.sizes.clone(), 3);
        let context = w.context.clone();
        let Some(view) = browser::create_view(
            win,
            Role::Side,
            GRACE_URL,
            context.as_ref(),
            &mut delegate,
            None,
        ) else {
            return;
        };
        w.body.add_child_view(Some(&mut View::from(&view)));
        w.grace.view = Some(view);
    }
    w.grace.open = open;
    if open {
        if let Some(v) = &w.grace.view {
            if v.is_visible() == 0 {
                // It slides in from the right edge (or carries on from where a closing slide got to).
                w.sizes.side_width.store(1, Ordering::Relaxed);
            }
            v.set_visible((!w.fullscreen) as i32);
            v.request_focus();
        }
        w.window.layout();
        browser::slide(
            ui,
            win,
            browser::Slide::Grace,
            browser::GRACE_WIDTH,
            |_, _| {},
        );
    } else {
        if let Some(tab) = w.active.and_then(|id| w.tabs.iter().find(|t| t.id == id)) {
            tab.focus();
        }
        // It slides out, then hides (unless it was opened again meanwhile).
        browser::slide(ui, win, browser::Slide::Grace, 1, |ui, win| {
            if let Some(w) = ui.win(win) {
                if !w.grace.open {
                    if let Some(v) = &w.grace.view {
                        v.set_visible(0);
                    }
                    w.sizes
                        .side_width
                        .store(browser::GRACE_WIDTH, Ordering::Relaxed);
                    w.window.layout();
                }
            }
        });
    }
    ui.mark(win);
}

/// What the panel knows about the page in the active tab.
fn page_info(w: &Win) -> Json {
    let Some(tab) = w.active.and_then(|id| w.tabs.iter().find(|t| t.id == id)) else {
        return json!({ "kind": "none" });
    };
    let url = tab.url.as_str();
    if url.starts_with("http://") || url.starts_with("https://") {
        return json!({ "kind": "web", "url": url, "title": tab.title });
    }
    if let Some(query) = url.strip_prefix("cottoncandy://error/?") {
        let param = |k: &str| {
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default()
        };
        return json!({ "kind": "error", "url": param("url"), "code": param("code") });
    }
    json!({ "kind": "other", "url": url })
}

/// Tells an open panel about a new page in the active tab (called whenever the window's state goes out).
pub fn update_panel(w: &mut Win) {
    if !(w.grace.open && w.grace.ready) {
        return;
    }
    let info = page_info(w);
    if w.grace.last_page.as_ref() == Some(&info) {
        return;
    }
    if let Some(b) = &w.grace.browser {
        run_js(b, &emit_js("grace_page", &info));
    }
    w.grace.last_page = Some(info);
}

/// Sends a question from the address bar to Grace Chat, opening the panel if needed.
fn ask_from_address_bar(ui: &mut Ui, win: u32, question: &str) -> Res {
    let question = question.trim();
    if question.is_empty() {
        return Ok(json!({}));
    }
    toggle(ui, win, Some(true));
    let w = ui.win(win).ok_or("no window")?;
    match (&w.grace.browser, w.grace.ready) {
        (Some(b), true) => run_js(b, &emit_js("grace_ask", &json!(question))),
        _ => w.grace.pending.push(question.to_string()),
    }
    Ok(json!({}))
}

type OnText = Box<dyn FnOnce(String) + Send>;

wrap_string_visitor! {
    struct PageText {
        done: Arc<Mutex<Option<OnText>>>,
    }

    impl CefStringVisitor {
        fn visit(&self, string: Option<&CefString>) {
            let text = string.map(|s| s.to_string()).unwrap_or_default();
            if let Some(done) = self.done.lock().ok().and_then(|mut d| d.take()) {
                done(text);
            }
        }
    }
}

fn answer_args() -> Vec<String> {
    vec!["ask".into(), "-".into()]
}

/// Summarize, ask about, or explain the error on the page in the active tab.
fn ask_about_page(ui: &mut Ui, win: u32, browser_id: i32, args: &Json) -> Res {
    if let Some(why) = blocked() {
        return Err(why);
    }
    let action = match args.get("action").and_then(Json::as_str).unwrap_or("") {
        "summarize" => "summarize",
        "ask" => "ask",
        "error" => "error",
        _ => return Err("unknown action".into()),
    };
    let question = args
        .get("question")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string();
    let w = ui.win(win).ok_or("no window")?;
    let info = page_info(w);
    let finish = move |result: Result<String, Failure>| match result {
        Ok(text) => emit(
            browser_id,
            "grace_done",
            json!({ "slot": "page", "text": text }),
        ),
        Err((kind, message)) => emit(
            browser_id,
            "grace_error",
            json!({ "slot": "page", "kind": kind, "message": message }),
        ),
    };
    let run = move |input: String| Run {
        browser_id,
        slot: "page",
        args: answer_args(),
        input,
        timeout: ANSWER_TIMEOUT,
        stream: true,
    };
    if action == "error" {
        if info["kind"] != "error" {
            return Err("This page loaded fine.".into());
        }
        let prompt = g::page_prompt(
            "error",
            "",
            info["url"].as_str().unwrap_or(""),
            info["code"].as_str().unwrap_or(""),
            "",
        );
        start(run(prompt), finish);
        return Ok(json!({ "started": true }));
    }
    if info["kind"] != "web" {
        return Err("Grace reads web pages. Open one in this tab first.".into());
    }
    let frame = w
        .active
        .and_then(|id| w.tabs.iter().find(|t| t.id == id))
        .and_then(|t| t.browser.as_ref())
        .and_then(|b| b.main_frame())
        .ok_or("The page isn't ready yet.")?;
    let (title, url) = (
        info["title"].as_str().unwrap_or("").to_string(),
        info["url"].as_str().unwrap_or("").to_string(),
    );
    let on_text: OnText = Box::new(move |text: String| {
        let (text, _cut) = g::clean_page_text(&text);
        if text.is_empty() {
            finish(Err((
                "empty",
                "This page has no text Grace can read.".into(),
            )));
            return;
        }
        let prompt = g::page_prompt(action, &title, &url, &text, &question);
        start(run(prompt), finish);
    });
    let mut visitor = PageText::new(Arc::new(Mutex::new(Some(on_text))));
    frame.text(Some(&mut visitor));
    Ok(json!({ "started": true }))
}

fn chat(browser_id: i32, args: &Json) -> Res {
    if let Some(why) = blocked() {
        return Err(why);
    }
    let features = features().unwrap_or_default();
    if !features.chat {
        return Err("Grace Chat needs a newer CherryCola (one with “ask --chat-json”). Update the cherrycolallm package.".into());
    }
    let question = args
        .get("question")
        .and_then(Json::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if question.is_empty() {
        return Err("Type a message first.".into());
    }
    let history: Vec<g::Turn> = args
        .get("history")
        .cloned()
        .and_then(|h| serde_json::from_value(h).ok())
        .unwrap_or_default();
    let wants_web = args.get("web").and_then(Json::as_bool).unwrap_or(false);
    let web = wants_web && features.web;
    let note = (wants_web && !web).then_some("This CherryCola can't search the web yet, so Grace answered from what it knows. Update the cherrycolallm package for web results.");
    start(
        Run {
            browser_id,
            slot: "chat",
            args: g::chat_arguments(web)
                .into_iter()
                .map(String::from)
                .collect(),
            input: g::chat_request(&history, &question),
            timeout: ANSWER_TIMEOUT,
            stream: true,
        },
        move |result| match result {
            Ok(out) => {
                let (text, sources) = g::split_sources(&out);
                emit(
                    browser_id,
                    "grace_done",
                    json!({ "slot": "chat", "text": text, "sources": sources, "note": note }),
                );
            }
            Err((kind, message)) => emit(
                browser_id,
                "grace_error",
                json!({ "slot": "chat", "kind": kind, "message": message }),
            ),
        },
    );
    Ok(json!({ "started": true }))
}

/// Enhance search: Grace rewrites the typed words into a better search, which the address bar then offers
/// first. Only the words go to CherryCola, and nothing is searched until the user picks a row.
fn enhance(browser_id: i32, args: &Json) -> Res {
    if let Some(why) = blocked() {
        return Err(why);
    }
    let query = args.get("query").and_then(Json::as_str).unwrap_or("");
    let query: String = query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(g::ENHANCE_QUERY_LIMIT)
        .collect();
    if query.is_empty() {
        return Err("Type a search first.".into());
    }
    let settings = shared::settings();
    let features = features().unwrap_or_default();
    let template = settings.search.template();
    let prompt = g::enhance_prompt(&query);
    let original = query.clone();
    start(
        Run {
            browser_id,
            slot: "enhance",
            args: g::enhance_arguments(features, settings.grace.unload_after_enhance)
                .into_iter()
                .map(String::from)
                .collect(),
            input: prompt,
            timeout: ENHANCE_TIMEOUT,
            stream: false,
        },
        move |result| {
            let search = |q: &str| cottoncandy_core::omnibox::search_url(&template, q);
            let data = match result.map(|out| g::clean_enhanced_query(&out)) {
                Ok(Some(better)) => {
                    let mut items = vec![
                        json!({ "kind": "enhanced", "url": search(&better), "title": better, "fill": better, "group": "Improved by Grace" }),
                    ];
                    if better.to_lowercase() != original.to_lowercase() {
                        items.push(json!({ "kind": "search", "url": search(&original), "title": original, "fill": original, "group": "As you typed it" }));
                    }
                    json!({ "query": original, "items": items })
                }
                Ok(None) => {
                    json!({ "query": original, "error": "Grace couldn't improve this search. Search as typed instead." })
                }
                Err((_, message)) => json!({ "query": original, "error": message }),
            };
            emit(browser_id, "enhanced", data);
        },
    );
    Ok(json!({ "started": true }))
}

fn stop(browser_id: i32, args: &Json) -> Res {
    let slot = args.get("slot").and_then(Json::as_str).unwrap_or("");
    if let Ok(runs) = RUNS.lock() {
        for (b, s, flag) in runs.iter() {
            if *b == browser_id && (slot.is_empty() || *s == slot) {
                flag.store(STOPPED, Ordering::SeqCst);
            }
        }
    }
    Ok(json!({}))
}

/// Stops everything a browser started, when it closes.
pub fn forget(browser_id: i32) {
    let _ = stop(browser_id, &json!({}));
}

fn ready(ui: &mut Ui, win: u32) -> Res {
    let settings = shared::settings();
    let features = features();
    let message = blocked();
    let w = ui.win(win).ok_or("no window")?;
    w.grace.ready = true;
    let page = page_info(w);
    w.grace.last_page = Some(page.clone());
    let pending = std::mem::take(&mut w.grace.pending);
    let f = features.unwrap_or_default();
    Ok(json!({
        "installed": features.is_some(), "chat": f.chat, "web": f.web,
        "web_default": settings.grace.web_results, "message": message, "page": page, "pending": pending,
    }))
}

/// Grace's calls, from its panel and from the toolbar.
pub fn call(ui: &mut Ui, win: u32, browser_id: i32, cmd: &str, args: &Json) -> Res {
    match cmd {
        "grace_toggle" => {
            toggle(ui, win, None);
            Ok(json!({}))
        }
        "grace_close" => {
            toggle(ui, win, Some(false));
            Ok(json!({}))
        }
        "grace_ready" => ready(ui, win),
        "grace_page" => ask_about_page(ui, win, browser_id, args),
        "grace_chat" => chat(browser_id, args),
        "grace_ask" => ask_from_address_bar(
            ui,
            win,
            args.get("question").and_then(Json::as_str).unwrap_or(""),
        ),
        "grace_enhance" => enhance(browser_id, args),
        "grace_stop" => stop(browser_id, args),
        "grace_models" => models(browser_id),
        "grace_use" => use_model(browser_id, args),
        "grace_pull" => pull(browser_id, args),
        "grace_install" => install(browser_id),
        _ => Err(format!("unknown call {cmd}")),
    }
}

/// Whether Grace rows belong in the address bar's suggestions.
pub fn offered() -> bool {
    shared::settings().grace.enabled
        && FEATURES
            .lock()
            .ok()
            .and_then(|f| f.as_ref().map(|(_, f)| f.is_some()))
            .unwrap_or(false)
}

// ---- models (Settings, Grace) ----------------------------------------------------------------------

/// A model name or download as CherryCola takes it: an Ollama tag, OWNER/REPO:QUANT or an https link.
/// Never starts with "-", so it can't be read as an option.
fn model_name(args: &Json) -> Result<String, String> {
    let m = args
        .get("model")
        .and_then(Json::as_str)
        .unwrap_or("")
        .trim();
    let ok = !m.is_empty()
        && m.len() <= 300
        && !m.starts_with('-')
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/-@+".contains(c));
    ok.then(|| m.to_string())
        .ok_or_else(|| "That isn't a model name.".into())
}

/// Runs CherryCola, returning what it printed, or its last error line.
fn run_checked(args: &[&str]) -> Result<String, String> {
    let out = base_command(&command())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| g::failure(None, "").1)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(g::failure(out.status.code(), &String::from_utf8_lossy(&out.stderr)).1)
    }
}

/// Which models are installed and which is in use, and what suits this computer. Answers with a
/// `grace_models` event, since CherryCola can take a second or two.
fn models(browser_id: i32) -> Res {
    std::thread::spawn(move || {
        if features().is_none() {
            emit(
                browser_id,
                "grace_models",
                json!({ "installed": false, "message": g::failure(None, "").1 }),
            );
            return;
        }
        let status = run_checked(&["status", "--json"])
            .and_then(|o| serde_json::from_str::<Json>(&o).map_err(|e| e.to_string()));
        let recommend = run_checked(&["recommend", "--json"])
            .ok()
            .and_then(|o| serde_json::from_str::<Json>(&o).ok());
        let data = match status {
            Ok(st) => json!({ "installed": true, "status": st, "recommend": recommend }),
            Err(e) => json!({ "installed": true, "message": e, "recommend": recommend }),
        };
        emit(browser_id, "grace_models", data);
    });
    Ok(json!({}))
}

fn use_model(browser_id: i32, args: &Json) -> Res {
    let model = model_name(args)?;
    std::thread::spawn(move || {
        let result = run_checked(&["use", &model]);
        if let Err(e) = result {
            emit(
                browser_id,
                "grace_models",
                json!({ "installed": true, "message": e }),
            );
            return;
        }
        let _ = models(browser_id);
    });
    Ok(json!({}))
}

static PULLING: Mutex<Option<String>> = Mutex::new(None);

/// Downloads a model through CherryCola's engine and makes it the one Grace uses. Progress comes as
/// `grace_pull` events ({model, status, percent}); the last has `done` or `error`.
fn pull(browser_id: i32, args: &Json) -> Res {
    start_pull(browser_id, model_name(args)?, false)
}

/// Uses a .gguf file on this computer, picked in a file dialog the browser itself showed (the page never
/// names the path). CherryCola adds it where it is; Ollama keeps its own copy, so that can take a minute.
pub fn use_model_file(browser_id: i32, path: &std::path::Path) -> Res {
    let is_gguf = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
        && std::fs::File::open(path)
            .and_then(|mut f| {
                let mut magic = [0u8; 4];
                f.read_exact(&mut magic).map(|_| magic == *b"GGUF")
            })
            .unwrap_or(false);
    if !is_gguf {
        return Err("That isn't a GGUF model file.".into());
    }
    start_pull(browser_id, path.to_string_lossy().into_owned(), true)
}

fn start_pull(browser_id: i32, model: String, file: bool) -> Res {
    {
        let mut pulling = PULLING.lock().map_err(|_| "busy")?;
        if let Some(other) = pulling.as_ref() {
            return Err(format!("{other} is still being added."));
        }
        *pulling = Some(model.clone());
    }
    let cmd = command();
    std::thread::spawn(move || {
        let result = pull_run(&cmd, &model, file, browser_id);
        if let Ok(mut p) = PULLING.lock() {
            *p = None;
        }
        match result {
            Ok(()) => emit(
                browser_id,
                "grace_pull",
                json!({ "model": model, "done": true, "file": file }),
            ),
            Err(e) => emit(
                browser_id,
                "grace_pull",
                json!({ "model": model, "error": e }),
            ),
        }
        let _ = models(browser_id);
    });
    Ok(json!({}))
}

fn pull_run(cmd: &str, model: &str, file: bool, browser_id: i32) -> Result<(), String> {
    // "--" so neither a name nor a path is ever read as an option.
    let mut child = base_command(cmd)
        .args(["pull", "--use", "--", model])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| g::failure(None, "").1)?;
    let mut err = child.stderr.take().ok_or("no output")?;
    let err_reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = err
            .by_ref()
            .take(STDERR_LIMIT as u64)
            .read_to_string(&mut text);
        text
    });
    // CherryCola writes "\r<status> 42.0%" as it goes; send the newest line now and then.
    let mut out = child.stdout.take().ok_or("no output")?;
    let mut buf = [0u8; 4096];
    let mut line = String::new();
    let mut last = Instant::now() - STREAM_EVERY;
    loop {
        let n = out.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        for c in String::from_utf8_lossy(&buf[..n]).chars() {
            if c == '\r' || c == '\n' {
                if !line.trim().is_empty() && last.elapsed() >= STREAM_EVERY {
                    last = Instant::now();
                    let text = strip_ansi(line.trim());
                    emit(
                        browser_id,
                        "grace_pull",
                        json!({ "model": model, "status": text, "percent": g::percent(&text), "file": file }),
                    );
                }
                line.clear();
            } else if line.len() < 400 {
                line.push(c);
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let err = err_reader.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(g::failure(status.code(), &err).1)
    }
}

// ---- installing CherryColaLLM (the Grace panel's Download button) -----------------------------------

/// The newest release's files, through GitHub's plain download links (no API, so no API rate limit).
const LATEST: &str = "https://github.com/itsHarshDabas/CherryColaLLM/releases/latest/download/";
static INSTALLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn install_progress(browser_id: i32, step: &str, percent: Option<f64>) {
    emit(
        browser_id,
        "grace_install",
        json!({ "step": step, "percent": percent }),
    );
}

fn install_failed(browser_id: i32, e: String) {
    INSTALLING.store(false, Ordering::SeqCst);
    emit(browser_id, "grace_install", json!({ "error": e }));
}

/// Downloads CherryColaLLM's newest release from its GitHub page, checks it against the release's
/// SHA256SUMS, and installs it for this user only (no administrator): the Windows installer runs quietly
/// into %LOCALAPPDATA%\Programs\CherryColaLLM; on Linux the files go into CottonCandy's data folder.
/// Progress comes as `grace_install` events; the last has `done` or `error`.
fn install(browser_id: i32) -> Res {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("CherryColaLLM is already being installed.".into());
    }
    install_progress(browser_id, "Finding the newest CherryColaLLM", None);
    crate::fetch::get(
        &format!("{LATEST}SHA256SUMS"),
        "",
        64 * 1024,
        None,
        move |r| {
            let sums = match r {
                Ok(f) => String::from_utf8_lossy(&f.body).into_owned(),
                Err(e) => return install_failed(browser_id, e),
            };
            let Some((name, want)) = g::pick_asset(&sums, cfg!(windows)) else {
                return install_failed(
                    browser_id,
                    "The newest CherryColaLLM release has no file for this computer.".into(),
                );
            };
            install_progress(browser_id, &format!("Downloading {name}"), Some(0.0));
            let progress: Arc<dyn Fn(i64, i64) + Send + Sync> = Arc::new(move |done, total| {
                if total > 0 {
                    install_progress(
                        browser_id,
                        "Downloading",
                        Some(100.0 * done as f64 / total as f64),
                    );
                }
            });
            crate::fetch::get(
                &format!("{LATEST}{name}"),
                "",
                300 << 20,
                Some(progress),
                move |r| match r {
                    Err(e) => install_failed(browser_id, e),
                    Ok(f) => {
                        let got = ring::digest::digest(&ring::digest::SHA256, &f.body);
                        let got: String = got.as_ref().iter().map(|b| format!("{b:02x}")).collect();
                        if got != want {
                            return install_failed(browser_id, "The download doesn't match the release's checksum, so it wasn't installed.".into());
                        }
                        install_progress(browser_id, "Installing", None);
                        std::thread::spawn(move || {
                            let result = install_file(&name, &f.body);
                            INSTALLING.store(false, Ordering::SeqCst);
                            rescan();
                            match result {
                                Ok(()) if features().is_some() => {
                                    emit(browser_id, "grace_install", json!({ "done": true }))
                                }
                                Ok(()) => emit(
                                    browser_id,
                                    "grace_install",
                                    json!({ "error": "CherryColaLLM was installed but doesn't start. On Linux it needs Python 3." }),
                                ),
                                Err(e) => emit(browser_id, "grace_install", json!({ "error": e })),
                            }
                        });
                    }
                },
            );
        },
    );
    Ok(json!({}))
}

#[cfg(windows)]
fn install_file(name: &str, data: &[u8]) -> Result<(), String> {
    let dir = shared::get().cache_dir.join("cherrycolallm");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let setup = dir.join(name);
    std::fs::write(&setup, data).map_err(|e| e.to_string())?;
    // Inno Setup: a small progress window and no questions; per-user, so no administrator prompt.
    let status = base_command(&setup.to_string_lossy())
        .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"])
        .status()
        .map_err(|e| format!("The installer didn't start: {e}"))?;
    let _ = std::fs::remove_file(&setup);
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "The installer stopped (code {}).",
            status.code().unwrap_or(-1)
        ))
    }
}

#[cfg(not(windows))]
fn install_file(name: &str, data: &[u8]) -> Result<(), String> {
    let dir = linux_install_dir();
    let unpack = dir.with_extension("new");
    let _ = std::fs::remove_dir_all(&unpack);
    std::fs::create_dir_all(&unpack).map_err(|e| e.to_string())?;
    let archive = unpack.join(name);
    std::fs::write(&archive, data).map_err(|e| e.to_string())?;
    // GNU tar refuses names with ".." or a leading "/" unless told otherwise.
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&unpack)
        .arg("--strip-components=1")
        .arg("--no-same-owner")
        .status()
        .map_err(|e| format!("Couldn't unpack it: {e}"))?;
    let _ = std::fs::remove_file(&archive);
    if !status.success() || !unpack.join("usr/bin/cherrycola").exists() {
        let _ = std::fs::remove_dir_all(&unpack);
        return Err("The download didn't unpack as expected.".into());
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&unpack, &dir).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::strip_ansi;

    #[test]
    fn strips_terminal_codes() {
        assert_eq!(strip_ansi("\u{1b}[1mHi\u{1b}[0m there\r\n"), "Hi there\n");
    }
}
