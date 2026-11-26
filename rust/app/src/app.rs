//! The CEF application: switches for every process, the cottoncandy:// scheme, and startup.

use crate::cefx::*;
use crate::{browser, scheme, shared};
use cef::*;
use cef_dll_sys::cef_scheme_options_t as opt;

/// Chromium features CottonCandy turns off: they phone home, show Google services we don't ship, or cost
/// memory for something the browser doesn't use. The spare renderer is an idle process Chromium keeps
/// ready for the next site; on Linux a new one starts from the zygote in a few milliseconds anyway.
const DISABLED_FEATURES: &str =
    "Translate,MediaRouter,DialMediaRouteProvider,OptimizationHints,OptimizationHintsFetching,\
OptimizationGuideModelDownloading,InterestFeedContentSuggestions,PrivacySandboxSettings4,FedCm,\
AutofillServerCommunication,CertificateTransparencyComponentUpdater,UseChromeOSDirectVideoDecoder,\
HeavyAdPrivacyMitigations,SubresourceFilter,NetworkTimeServiceQuerying,TabHoverCardImages,\
SpareRendererForSitePerProcess";

wrap_app! {
    pub struct CcApp {}

    impl App {
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let (Some(cmd), true) = (command_line, os(process_type).is_empty()) else {
                return;
            };
            let switch = |name: &str| cmd.append_switch(Some(&cs(name)));
            let value = |name: &str, v: &str| cmd.append_switch_with_value(Some(&cs(name)), Some(&cs(v)));
            switch("no-first-run");
            switch("no-default-browser-check");
            switch("disable-sync");
            switch("disable-domain-reliability");
            switch("disable-breakpad");
            switch("disable-client-side-phishing-detection");
            switch("disable-search-engine-choice-screen");
            switch("disable-features-chrome-labs");
            value("disable-features", DISABLED_FEATURES);
            // Keeps memory down when many tabs are open, without giving up site isolation.
            let settings = shared::settings();
            let mut features = String::from("PartitionAllocMemoryReclaimer,HighEfficiencyModeAvailable");
            if settings.appearance.dark_web_pages {
                // Chromium's auto dark mode for sites without a dark theme of their own.
                features += ",WebContentsForceDark";
            }
            value("enable-features", &features);
            let perf = &settings.performance;
            if !perf.hardware_acceleration {
                // Only for a graphics driver that crashes or draws garbage: everything is drawn by the processor.
                switch("disable-gpu");
                switch("disable-gpu-compositing");
            } else if perf.ignore_gpu_blocklist {
                // Chromium keeps a list of drivers it doesn't trust and draws with the processor on them, which
                // makes WebGL, video and scrolling slow. This uses the graphics card anyway.
                switch("ignore-gpu-blocklist");
                switch("enable-gpu-rasterization");
            }
            // Chromium glides on Windows but jumps on Linux unless asked; this makes both follow the setting.
            switch(if perf.smooth_scrolling { "enable-smooth-scrolling" } else { "disable-smooth-scrolling" });
            if settings.privacy.webrtc_ip_policy != "default" {
                value("force-webrtc-ip-handling-policy", &settings.privacy.webrtc_ip_policy);
            }
            let extensions = browser::extension_dirs(&settings);
            if !extensions.is_empty() {
                value("load-extension", &extensions.join(","));
            }
            if std::env::var_os("CC_NO_SANDBOX").is_some() {
                // Only for test machines without user namespaces. Never set this on a real computer.
                switch("no-sandbox");
            }
        }

        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            if let Some(registrar) = registrar {
                let options = opt::CEF_SCHEME_OPTION_STANDARD as i32
                    | opt::CEF_SCHEME_OPTION_SECURE as i32
                    | opt::CEF_SCHEME_OPTION_CORS_ENABLED as i32
                    | opt::CEF_SCHEME_OPTION_FETCH_ENABLED as i32;
                registrar.add_custom_scheme(Some(&cs(cottoncandy_core::INTERNAL_SCHEME)), options);
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(CcProcess::new())
        }
    }
}

wrap_browser_process_handler! {
    pub struct CcProcess {}

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            scheme::register();
            browser::apply_network_settings(None);
            if let Some(global) = request_context_get_global_context() {
                crate::shields::apply_scripts(&global, &crate::shared::settings());
            }
            crate::shields::start();
            crate::grace::warm_up();
            let urls = browser::startup_urls(&std::env::args().skip(1).collect::<Vec<_>>());
            browser::open_window(false, urls);
        }

        fn on_already_running_app_relaunch(
            &self,
            command_line: Option<&mut CommandLine>,
            _current_directory: Option<&CefString>,
        ) -> ::std::os::raw::c_int {
            let mut args = Vec::new();
            if let Some(cmd) = command_line {
                let mut list = CefStringList::new();
                cmd.arguments(Some(&mut list));
                args = list.into_iter().collect();
            }
            let private = args.iter().any(|a| a == "--private");
            let urls: Vec<String> = args.into_iter().filter(|a| !a.starts_with("--")).collect();
            browser::open_window(private, urls);
            1
        }
    }
}
