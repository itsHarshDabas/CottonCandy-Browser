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

fn command() -> String {
    shared::settings().grace.command
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
    if let Some(v) = &w.grace.view {
        v.set_visible((open && !w.fullscreen) as i32);
        if open {
            v.request_focus();
        }
    }
    if !open {
        if let Some(tab) = w.active.and_then(|id| w.tabs.iter().find(|t| t.id == id)) {
            tab.view.request_focus();
        }
    }
    w.window.layout();
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

#[cfg(test)]
mod tests {
    use super::strip_ansi;

    #[test]
    fn strips_terminal_codes() {
        assert_eq!(strip_ansi("\u{1b}[1mHi\u{1b}[0m there\r\n"), "Hi there\n");
    }
}
