//! CottonCandy, the GraceOS web browser, on Chromium through CEF.
//!
//! The same executable runs the browser and Chromium's helper processes (renderers, GPU, network).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod api;
mod app;
mod browser;
mod cefx;
mod client;
mod icon;
mod scheme;
mod shared;

use cef::*;

fn main() {
    let _ = api_hash(cef_dll_sys::CEF_API_VERSION_LAST, 0);
    let args = args::Args::new();
    let mut app = app::CcApp::new();

    // Helper processes return here with their exit code; the browser process gets -1 and carries on.
    let code = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if code >= 0 {
        std::process::exit(code);
    }
    if std::env::args().any(|a| a == "--version") {
        println!("CottonCandy {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let shared = shared::init();
    let profile = shared.data_dir.join("profile");
    let _ = cottoncandy_core::paths::ensure_private_dir(&profile);
    let settings = Settings {
        no_sandbox: std::env::var_os("CC_NO_SANDBOX").is_some() as i32,
        root_cache_path: cefx::cs(&profile.to_string_lossy()),
        cache_path: cefx::cs(&profile.join("Default").to_string_lossy()),
        persist_session_cookies: 1,
        log_severity: LogSeverity::from(cef_dll_sys::cef_log_severity_t::LOGSEVERITY_WARNING),
        log_file: cefx::cs(&shared.cache_dir.join("cef.log").to_string_lossy()),
        background_color: 0xFF11_1318,
        // For automated UI tests only: lets a test driver connect over the DevTools protocol on localhost.
        remote_debugging_port: std::env::var("CC_DEBUG_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(0),
        ..Default::default()
    };
    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) != 1
    {
        // Another CottonCandy is already running with this profile; it opened a window for us.
        std::process::exit(0);
    }
    run_message_loop();
    browser::with(|ui| ui.save_session());
    if shared::settings().privacy.clear_on_exit {
        shared::with_store(|s| s.clear_history(0));
    }
    shutdown();
}
