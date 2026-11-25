//! Small helpers over the CEF bindings: strings, running code on the UI thread, enum shorthands.

use cef::*;
use std::sync::{Arc, Mutex};

/// A Rust string as a CEF string.
pub fn cs(text: &str) -> CefString {
    CefString::from(text)
}

/// A CEF string the browser handed us, as a Rust string.
pub fn us(text: &CefStringUserfree) -> String {
    CefString::from(text).to_string()
}

/// An optional borrowed CEF string, as a Rust string ("" when absent).
pub fn os(text: Option<&CefString>) -> String {
    text.map(|t| t.to_string()).unwrap_or_default()
}

type Job = Box<dyn FnOnce() + Send + 'static>;

wrap_task! {
    pub struct Closure {
        job: Arc<Mutex<Option<Job>>>,
    }

    impl Task {
        fn execute(&self) {
            let job = self.job.lock().ok().and_then(|mut job| job.take());
            if let Some(job) = job {
                job();
            }
        }
    }
}

fn post(thread: cef_dll_sys::cef_thread_id_t, delay_ms: i64, job: Job) {
    let mut task = Closure::new(Arc::new(Mutex::new(Some(job))));
    if delay_ms > 0 {
        post_delayed_task(ThreadId::from(thread), Some(&mut task), delay_ms);
    } else {
        post_task(ThreadId::from(thread), Some(&mut task));
    }
}

/// Runs `job` on the browser's UI thread, after the current task.
pub fn on_ui(job: impl FnOnce() + Send + 'static) {
    post(cef_dll_sys::cef_thread_id_t::TID_UI, 0, Box::new(job));
}

/// Closes a window from a posted task. Closing asks `can_close`, which needs the UI state, so it can't run
/// while a caller is still holding that state.
pub fn close_window(window: &Window) {
    let window = window.clone();
    on_ui(move || window.close());
}

/// Runs `job` on the UI thread in `ms` milliseconds.
pub fn on_ui_later(ms: i64, job: impl FnOnce() + Send + 'static) {
    post(cef_dll_sys::cef_thread_id_t::TID_UI, ms, Box::new(job));
}

/// The page URL of a frame, or "".
pub fn frame_url(frame: Option<&mut Frame>) -> String {
    frame.map(|f| us(&f.url())).unwrap_or_default()
}

/// Runs a script in a browser's main frame.
pub fn run_js(browser: &Browser, code: &str) {
    if let Some(frame) = browser.main_frame() {
        frame.execute_java_script(Some(&cs(code)), Some(&cs("")), 0);
    }
}

/// Builds `cc.emit(event, data)` for one of our own pages.
pub fn emit_js(event: &str, data: &serde_json::Value) -> String {
    format!(
        "window.cc&&cc.emit({},{})",
        serde_json::Value::from(event),
        data
    )
}

/// A CEF value from JSON, for preferences.
pub fn to_value(json: &serde_json::Value) -> Option<Value> {
    let value = value_create()?;
    match json {
        serde_json::Value::Null => {
            value.set_null();
        }
        serde_json::Value::Bool(b) => {
            value.set_bool(*b as i32);
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64().filter(|i| i32::try_from(*i).is_ok()) {
                value.set_int(i as i32);
            } else {
                value.set_double(n.as_f64().unwrap_or(0.0));
            }
        }
        serde_json::Value::String(s) => {
            value.set_string(Some(&cs(s)));
        }
        serde_json::Value::Array(items) => {
            let mut list = list_value_create()?;
            list.set_size(items.len());
            for (i, item) in items.iter().enumerate() {
                if let Some(mut v) = to_value(item) {
                    list.set_value(i, Some(&mut v));
                }
            }
            value.set_list(Some(&mut list));
        }
        serde_json::Value::Object(map) => {
            let mut dict = dictionary_value_create()?;
            for (k, item) in map {
                if let Some(mut v) = to_value(item) {
                    dict.set_value(Some(&cs(k)), Some(&mut v));
                }
            }
            value.set_dictionary(Some(&mut dict));
        }
    }
    Some(value)
}

/// Sets a Chromium preference on a request context, and says why it failed.
pub fn set_pref(
    context: &impl ImplPreferenceManager,
    name: &str,
    json: &serde_json::Value,
) -> Result<(), String> {
    let name = cs(name);
    if context.can_set_preference(Some(&name)) == 0 {
        return Err(format!("{name} can't be changed"));
    }
    let mut value = to_value(json).ok_or("bad value")?;
    // CEF refuses the call outright if the error string is null, which an empty CefString would be.
    let mut error = cs(" ");
    let ok = context.set_preference(Some(&name), Some(&mut value), Some(&mut error)) != 0;
    if std::env::var_os("CC_DEBUG").is_some() {
        eprintln!(
            "[pref] {name} = {json} -> {ok} (valid={})",
            value.is_valid()
        );
    }
    if !ok {
        return Err(format!("{name} wasn't accepted"));
    }
    Ok(())
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}
