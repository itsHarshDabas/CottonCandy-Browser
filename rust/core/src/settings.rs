//! Every setting, saved as JSON in the config folder. Unknown or broken values fall back to defaults, so an
//! edited file can't stop the browser from starting.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    pub startup: Startup,
    pub search: Search,
    pub shields: Shields,
    pub privacy: Privacy,
    pub network: Network,
    pub downloads: Downloads,
    pub grace: Grace,
    pub extensions: ExtensionSettings,
    /// Per-site overrides of the shields, by host (`example.org`).
    pub sites: BTreeMap<String, SiteSettings>,
}

// ---- appearance ----------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// system, dark or light
    pub theme: String,
    /// blue, purple, pink, red, orange, yellow, green, graphite, or a #rrggbb color
    pub accent: String,
    /// top or vertical (tabs down the left side)
    pub tab_layout: String,
    /// compact, comfortable or spacious
    pub density: String,
    /// Corner roundness, 0 (square) to 16.
    pub corner_radius: u8,
    /// UI text size, 11 to 18 px.
    pub font_size: u8,
    /// Page zoom for new tabs, 50 to 300 percent.
    pub default_zoom: u16,
    pub show_bookmarks_bar: bool,
    pub show_home_button: bool,
    /// Glassy translucent toolbar and panels.
    pub translucency: bool,
    /// Animations and transitions.
    pub motion: bool,
    /// gradient, aurora, mesh, plain, or image (with `wallpaper_path`)
    pub newtab_background: String,
    pub wallpaper_path: String,
    pub newtab_show_shortcuts: bool,
    pub newtab_show_clock: bool,
    /// Extra CSS for CottonCandy's own pages and toolbar (not web pages).
    pub custom_css: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: "system".into(),
            accent: "blue".into(),
            tab_layout: "top".into(),
            density: "comfortable".into(),
            corner_radius: 10,
            font_size: 13,
            default_zoom: 100,
            show_bookmarks_bar: false,
            show_home_button: false,
            translucency: true,
            motion: true,
            newtab_background: "aurora".into(),
            wallpaper_path: String::new(),
            newtab_show_shortcuts: true,
            newtab_show_clock: true,
            custom_css: String::new(),
        }
    }
}

pub const ACCENTS: &[(&str, &str)] = &[
    ("blue", "#3b82f6"),
    ("purple", "#8b5cf6"),
    ("pink", "#ec4899"),
    ("red", "#ef4444"),
    ("orange", "#f97316"),
    ("yellow", "#eab308"),
    ("green", "#22c55e"),
    ("graphite", "#6b7280"),
];

/// The accent as #rrggbb.
pub fn accent_color(accent: &str) -> String {
    if let Some((_, hex)) = ACCENTS.iter().find(|(name, _)| *name == accent) {
        return (*hex).into();
    }
    if is_hex_color(accent) {
        return accent.to_ascii_lowercase();
    }
    ACCENTS[0].1.into()
}

pub fn is_hex_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|c| c.is_ascii_hexdigit())
}

// ---- startup, search -------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Startup {
    /// newtab, restore (last session) or homepage
    pub open: String,
    pub homepage: String,
}

impl Default for Startup {
    fn default() -> Self {
        Startup {
            open: "newtab".into(),
            homepage: "cottoncandy://newtab".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Search {
    /// duckduckgo, brave, startpage, mojeek, google, bing, ecosia, or custom
    pub engine: String,
    /// For `custom`: an https URL with `{}` where the query goes.
    pub custom_url: String,
    /// Suggestions from history and bookmarks on this computer (never sent anywhere).
    pub local_suggestions: bool,
}

impl Default for Search {
    fn default() -> Self {
        Search {
            engine: "duckduckgo".into(),
            custom_url: String::new(),
            local_suggestions: true,
        }
    }
}

pub const SEARCH_ENGINES: &[(&str, &str, &str)] = &[
    ("duckduckgo", "DuckDuckGo", "https://duckduckgo.com/?q={}"),
    (
        "brave",
        "Brave Search",
        "https://search.brave.com/search?q={}",
    ),
    (
        "startpage",
        "Startpage",
        "https://www.startpage.com/do/search?q={}",
    ),
    ("mojeek", "Mojeek", "https://www.mojeek.com/search?q={}"),
    ("ecosia", "Ecosia", "https://www.ecosia.org/search?q={}"),
    ("google", "Google", "https://www.google.com/search?q={}"),
    ("bing", "Bing", "https://www.bing.com/search?q={}"),
];

impl Search {
    /// The search URL template to use.
    pub fn template(&self) -> String {
        if self.engine == "custom" && valid_search_template(&self.custom_url) {
            return self.custom_url.clone();
        }
        SEARCH_ENGINES
            .iter()
            .find(|(id, _, _)| *id == self.engine)
            .unwrap_or(&SEARCH_ENGINES[0])
            .2
            .into()
    }
}

pub fn valid_search_template(text: &str) -> bool {
    text.starts_with("https://")
        && text.matches("{}").count() == 1
        && url::Url::parse(&text.replace("{}", "q")).is_ok()
}

// ---- shields, privacy --------------------------------------------------------------------------------

/// Brave-style protections, each with its own switch. Sites can override them (see `SiteSettings`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shields {
    /// Block ads (EasyList and the lists in `filter_lists`).
    pub block_ads: bool,
    /// Block trackers (EasyPrivacy) and known fingerprinting scripts.
    pub block_trackers: bool,
    /// Hide the empty boxes ads leave behind (cosmetic filtering).
    pub hide_ad_boxes: bool,
    /// Block JavaScript on every site unless a site is allowed.
    pub block_scripts: bool,
    /// Block third-party cookies.
    pub block_third_party_cookies: bool,
    /// Upgrade http to https, and warn before an http-only page.
    pub https_only: bool,
    /// Ask before letting a page use the camera, microphone, location or notifications (always on) and block
    /// pop-ups that weren't clicked.
    pub block_popups: bool,
    /// Send Global Privacy Control (Sec-GPC: 1) and Do Not Track.
    pub send_gpc: bool,
    /// Ids of the filter lists turned on (see `shields::LISTS`).
    pub filter_lists: Vec<String>,
    /// Extra filter rules the user typed (Adblock Plus syntax).
    pub custom_rules: String,
}

impl Default for Shields {
    fn default() -> Self {
        Shields {
            block_ads: true,
            block_trackers: true,
            hide_ad_boxes: true,
            block_scripts: false,
            block_third_party_cookies: true,
            https_only: true,
            block_popups: true,
            send_gpc: true,
            filter_lists: vec![
                "easylist".into(),
                "easyprivacy".into(),
                "ublock-unbreak".into(),
            ],
            custom_rules: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteSettings {
    /// None: use the global switch.
    pub shields_up: Option<bool>,
    pub scripts: Option<bool>,
    pub ads: Option<bool>,
    pub zoom: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Privacy {
    /// Clear cookies and site data when the last window closes.
    pub clear_on_exit: bool,
    pub save_history: bool,
    /// Remember passwords in the Chromium password manager.
    pub save_passwords: bool,
    /// WebRTC can reveal your real IP address behind a proxy or VPN: default, public_only or disable_non_proxied_udp.
    pub webrtc_ip_policy: String,
    /// Strip tracking parameters (utm_*, fbclid, gclid...) from links.
    pub strip_tracking_params: bool,
}

impl Default for Privacy {
    fn default() -> Self {
        Privacy {
            clear_on_exit: false,
            save_history: true,
            save_passwords: true,
            webrtc_ip_policy: "default_public_interface_only".into(),
            strip_tracking_params: true,
        }
    }
}

// ---- network: proxy, VPN, secure DNS ------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Network {
    pub proxy: Proxy,
    pub dns: SecureDns,
}

/// How CottonCandy reaches the internet. A VPN that runs on the computer (WireGuard, OpenVPN, a VPN app)
/// works with `system` or `direct`; one that gives you a proxy (Tor, a SOCKS VPN, an SSH tunnel) is `fixed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Proxy {
    /// The quick switch in the toolbar: off means `system` whatever else is set.
    pub enabled: bool,
    /// system, direct, fixed or pac
    pub mode: String,
    /// socks5, socks4, http or https (for `fixed`)
    pub scheme: String,
    pub host: String,
    pub port: u16,
    /// Hosts that skip the proxy, comma separated (`localhost, *.lan, 192.168.0.0/16`).
    pub bypass: String,
    /// For `pac`: an https:// or file:// PAC script.
    pub pac_url: String,
    /// Presets offered in Settings ("Tor on this computer" and friends) only fill the fields above.
    pub preset: String,
}

impl Default for Proxy {
    fn default() -> Self {
        Proxy {
            enabled: false,
            mode: "system".into(),
            scheme: "socks5".into(),
            host: "127.0.0.1".into(),
            port: 9050,
            bypass: "localhost, 127.0.0.1, ::1".into(),
            pac_url: String::new(),
            preset: String::new(),
        }
    }
}

pub const PROXY_PRESETS: &[(&str, &str, &str, &str, u16)] = &[
    ("tor", "Tor (on this computer)", "socks5", "127.0.0.1", 9050),
    (
        "tor-browser",
        "Tor Browser's Tor",
        "socks5",
        "127.0.0.1",
        9150,
    ),
    (
        "ssh",
        "SSH tunnel (ssh -D 1080)",
        "socks5",
        "127.0.0.1",
        1080,
    ),
    (
        "http-local",
        "HTTP proxy on this computer",
        "http",
        "127.0.0.1",
        8080,
    ),
];

impl Proxy {
    /// Whether the settings are complete enough to use.
    pub fn validate(&self) -> Result<(), String> {
        match self.mode.as_str() {
            "system" | "direct" => Ok(()),
            "fixed" => {
                if !["socks5", "socks4", "http", "https"].contains(&self.scheme.as_str()) {
                    return Err("Pick SOCKS5, SOCKS4, HTTP or HTTPS.".into());
                }
                if !valid_proxy_host(&self.host) {
                    return Err("The proxy address isn't a host name or IP address.".into());
                }
                if self.port == 0 {
                    return Err("The proxy port must be between 1 and 65535.".into());
                }
                Ok(())
            }
            "pac" => {
                if self.pac_url.starts_with("https://") || self.pac_url.starts_with("file://") {
                    Ok(())
                } else {
                    Err("A PAC script must be an https:// or file:// address.".into())
                }
            }
            _ => Err("Unknown proxy mode.".into()),
        }
    }

    /// The `proxy` preference Chromium takes (CefRequestContext::SetPreference("proxy", ...)).
    pub fn chromium_pref(&self) -> serde_json::Value {
        if !self.enabled || self.validate().is_err() {
            return serde_json::json!({ "mode": "system" });
        }
        match self.mode.as_str() {
            "direct" => serde_json::json!({ "mode": "direct" }),
            "fixed" => {
                let host = if self.host.contains(':') && !self.host.starts_with('[') {
                    format!("[{}]", self.host)
                } else {
                    self.host.clone()
                };
                let bypass: Vec<String> = self
                    .bypass
                    .split([',', ';'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && s.len() < 256)
                    .map(String::from)
                    .collect();
                serde_json::json!({
                    "mode": "fixed_servers",
                    "server": format!("{}://{}:{}", self.scheme, host, self.port),
                    "bypass_list": bypass.join(";"),
                })
            }
            "pac" => serde_json::json!({ "mode": "pac_script", "pac_url": self.pac_url }),
            _ => serde_json::json!({ "mode": "system" }),
        }
    }
}

fn valid_proxy_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    host.split('.').all(|l| {
        !l.is_empty() && l.len() <= 63 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// DNS over HTTPS: lookups encrypted so the network (and whoever runs it) can't see or change them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SecureDns {
    /// off, automatic (use it when the system's resolver supports it) or secure (always; fail rather than leak)
    pub mode: String,
    /// cloudflare, quad9, google, mullvad, adguard, nextdns or custom
    pub provider: String,
    /// For `custom` (or a NextDNS profile): an https DoH template, e.g. https://dns.example/dns-query{?dns}
    pub custom_template: String,
}

impl Default for SecureDns {
    fn default() -> Self {
        SecureDns {
            mode: "automatic".into(),
            provider: "quad9".into(),
            custom_template: String::new(),
        }
    }
}

pub const DNS_PROVIDERS: &[(&str, &str, &str)] = &[
    (
        "cloudflare",
        "Cloudflare (1.1.1.1)",
        "https://chrome.cloudflare-dns.com/dns-query",
    ),
    (
        "quad9",
        "Quad9 (blocks malware)",
        "https://dns.quad9.net/dns-query",
    ),
    (
        "google",
        "Google Public DNS",
        "https://dns.google/dns-query{?dns}",
    ),
    (
        "mullvad",
        "Mullvad (no logs)",
        "https://dns.mullvad.net/dns-query",
    ),
    (
        "adguard",
        "AdGuard (blocks ads)",
        "https://dns.adguard-dns.com/dns-query",
    ),
];

impl SecureDns {
    pub fn template(&self) -> Option<String> {
        if self.provider == "custom" || self.provider == "nextdns" {
            let t = self.custom_template.trim();
            return (t.starts_with("https://")
                && !t.chars().any(char::is_whitespace)
                && url::Url::parse(&t.replace("{?dns}", "")).is_ok())
            .then(|| t.to_string());
        }
        DNS_PROVIDERS
            .iter()
            .find(|(id, _, _)| *id == self.provider)
            .map(|(_, _, t)| (*t).to_string())
    }

    /// Chromium's local-state preferences `dns_over_https.mode` and `dns_over_https.templates`.
    pub fn chromium_prefs(&self) -> Vec<(&'static str, serde_json::Value)> {
        let template = self.template();
        let mode = match (self.mode.as_str(), &template) {
            ("off", _) | (_, None) => "off",
            ("secure", Some(_)) => "secure",
            _ => "automatic",
        };
        vec![
            ("dns_over_https.mode", serde_json::json!(mode)),
            (
                "dns_over_https.templates",
                serde_json::json!(template.unwrap_or_default()),
            ),
        ]
    }
}

// ---- downloads, Grace, extensions ----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Downloads {
    /// Empty: the Downloads folder.
    pub folder: String,
    pub ask_where: bool,
    /// Ask before downloading files that run (.exe, .msi, .bat, .sh, .AppImage...).
    pub warn_executables: bool,
}

impl Default for Downloads {
    fn default() -> Self {
        Downloads {
            folder: String::new(),
            ask_where: false,
            warn_executables: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grace {
    pub enabled: bool,
    /// Grace Chat's Web results switch starts on.
    pub web_results: bool,
    /// Enhance search loads the model only for itself and unloads it after.
    pub unload_after_enhance: bool,
    /// The CherryCola command (`cherrycola`, or a full path).
    pub command: String,
}

impl Default for Grace {
    fn default() -> Self {
        Grace {
            enabled: true,
            web_results: true,
            unload_after_enhance: true,
            command: "cherrycola".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtensionSettings {
    /// Installed extensions: Chromium extension id -> where it came from.
    pub installed: BTreeMap<String, InstalledExtension>,
    /// Off: no extension runs (they stay installed).
    pub enabled: bool,
}

impl Default for ExtensionSettings {
    fn default() -> Self {
        ExtensionSettings {
            installed: BTreeMap::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct InstalledExtension {
    pub name: String,
    pub version: String,
    /// chrome (Chrome Web Store), firefox (addons.mozilla.org, converted) or unpacked
    pub source: String,
    pub enabled: bool,
}

// ---- loading and saving ------------------------------------------------------------------------------

pub fn settings_path() -> PathBuf {
    crate::paths::config_dir().join("settings.json")
}

impl Settings {
    /// Reads the settings, or the defaults if the file is missing or broken (a broken file is kept as .bad).
    pub fn load(path: &Path) -> Settings {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Settings::default();
        };
        match serde_json::from_str::<Settings>(&text) {
            Ok(settings) => settings.sanitized(),
            Err(_) => {
                let _ = std::fs::rename(path, path.with_extension("json.bad"));
                Settings::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text =
            serde_json::to_vec_pretty(&self.clone().sanitized()).expect("settings serialize");
        crate::paths::write_atomic(path, &text)
    }

    /// Clamps numbers and replaces unknown choices with defaults.
    pub fn sanitized(mut self) -> Settings {
        let d = Settings::default();
        let a = &mut self.appearance;
        pick(
            &mut a.theme,
            &["system", "dark", "light"],
            &d.appearance.theme,
        );
        if !ACCENTS.iter().any(|(n, _)| *n == a.accent) && !is_hex_color(&a.accent) {
            a.accent = d.appearance.accent.clone();
        }
        pick(
            &mut a.tab_layout,
            &["top", "vertical"],
            &d.appearance.tab_layout,
        );
        pick(
            &mut a.density,
            &["compact", "comfortable", "spacious"],
            &d.appearance.density,
        );
        pick(
            &mut a.newtab_background,
            &["gradient", "aurora", "mesh", "plain", "image"],
            &d.appearance.newtab_background,
        );
        a.corner_radius = a.corner_radius.min(16);
        a.font_size = a.font_size.clamp(11, 18);
        a.default_zoom = a.default_zoom.clamp(50, 300);
        a.custom_css.truncate(20_000);
        pick(
            &mut self.startup.open,
            &["newtab", "restore", "homepage"],
            &d.startup.open,
        );
        let engines: Vec<&str> = SEARCH_ENGINES
            .iter()
            .map(|e| e.0)
            .chain(["custom"])
            .collect();
        pick(&mut self.search.engine, &engines, &d.search.engine);
        pick(
            &mut self.network.proxy.mode,
            &["system", "direct", "fixed", "pac"],
            &d.network.proxy.mode,
        );
        pick(
            &mut self.network.dns.mode,
            &["off", "automatic", "secure"],
            &d.network.dns.mode,
        );
        let providers: Vec<&str> = DNS_PROVIDERS
            .iter()
            .map(|p| p.0)
            .chain(["nextdns", "custom"])
            .collect();
        pick(
            &mut self.network.dns.provider,
            &providers,
            &d.network.dns.provider,
        );
        pick(
            &mut self.privacy.webrtc_ip_policy,
            &[
                "default",
                "default_public_interface_only",
                "disable_non_proxied_udp",
            ],
            &d.privacy.webrtc_ip_policy,
        );
        self.shields.custom_rules.truncate(200_000);
        self.sites
            .retain(|host, _| crate::omnibox::looks_like_host(host) && host.len() < 254);
        for site in self.sites.values_mut() {
            site.zoom = site.zoom.map(|z| z.clamp(25, 500));
        }
        if self.grace.command.trim().is_empty() || self.grace.command.contains(['\n', '\0']) {
            self.grace.command = d.grace.command;
        }
        self
    }

    /// The shields that apply to `host`, after its own overrides.
    pub fn shields_for(&self, host: &str) -> EffectiveShields {
        let site = self.site(host);
        let up = site.and_then(|s| s.shields_up).unwrap_or(true);
        EffectiveShields {
            ads: up && site.and_then(|s| s.ads).unwrap_or(self.shields.block_ads),
            trackers: up && self.shields.block_trackers,
            cosmetic: up && self.shields.hide_ad_boxes,
            scripts_blocked: !site
                .and_then(|s| s.scripts)
                .unwrap_or(!self.shields.block_scripts),
            shields_up: up,
        }
    }

    /// A host's own settings: `www.example.org` uses `example.org`'s when it has none.
    pub fn site(&self, host: &str) -> Option<&SiteSettings> {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        self.sites
            .get(&host)
            .or_else(|| host.strip_prefix("www.").and_then(|h| self.sites.get(h)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EffectiveShields {
    pub shields_up: bool,
    pub ads: bool,
    pub trackers: bool,
    pub cosmetic: bool,
    pub scripts_blocked: bool,
}

fn pick(value: &mut String, allowed: &[&str], default: &str) {
    if !allowed.contains(&value.as_str()) {
        *value = default.to_string();
    }
}

/// Applies one change from the Settings page: a dotted path (`network.proxy.port`) and a JSON value.
/// Unknown paths and values of the wrong type are refused, and the result is sanitized.
pub fn apply_change(
    settings: &Settings,
    path: &str,
    value: serde_json::Value,
) -> Result<Settings, String> {
    if path.is_empty()
        || path.len() > 200
        || !path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
    {
        return Err("bad setting name".into());
    }
    let mut tree = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    let mut parts: Vec<String> = path.split('.').map(String::from).collect();
    // Site hosts have dots of their own: sites.<host>.<setting>.
    if parts.len() > 3 && parts[0] == "sites" {
        let last = parts.pop().unwrap();
        let host = parts[1..].join(".");
        parts = vec!["sites".into(), host, last];
    }
    let (last, parents) = parts.split_last().ok_or("bad setting name")?;
    let mut node = &mut tree;
    for (i, part) in parents.iter().enumerate() {
        let obj = node.as_object_mut().ok_or("bad setting name")?;
        if i == 1 && parts[0] == "sites" && !obj.contains_key(part) {
            if !crate::omnibox::looks_like_host(part) {
                return Err("not a site".into());
            }
            obj.insert(
                part.clone(),
                serde_json::to_value(SiteSettings::default()).unwrap(),
            );
        }
        node = obj.get_mut(part).ok_or("unknown setting")?;
    }
    let obj = node.as_object_mut().ok_or("bad setting name")?;
    let old = obj.get(last).ok_or("unknown setting")?;
    let same_type = matches!(
        (old, &value),
        (serde_json::Value::Bool(_), serde_json::Value::Bool(_))
            | (serde_json::Value::Number(_), serde_json::Value::Number(_))
            | (serde_json::Value::String(_), serde_json::Value::String(_))
            | (serde_json::Value::Array(_), serde_json::Value::Array(_))
            | (serde_json::Value::Null, _)
            | (_, serde_json::Value::Null)
    );
    if !same_type {
        return Err(format!("{path} has a different type"));
    }
    obj.insert(last.clone(), value);
    let parsed: Settings = serde_json::from_value(tree).map_err(|e| format!("{path}: {e}"))?;
    Ok(parsed.sanitized())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_and_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        let mut s = Settings::default();
        s.appearance.accent = "#AbCdEf".into();
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).appearance.accent, "#AbCdEf");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert!(dir.path().join("settings.json.bad").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            Settings::default().save(&path).unwrap();
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn unknown_values_fall_back() {
        let s: Settings = serde_json::from_str(r#"{"appearance":{"theme":"neon","font_size":99,"accent":"url(x)"},"search":{"engine":"evil"}}"#).unwrap();
        let s = s.sanitized();
        assert_eq!(s.appearance.theme, "system");
        assert_eq!(s.appearance.font_size, 18);
        assert_eq!(s.appearance.accent, "blue");
        assert_eq!(s.search.engine, "duckduckgo");
    }

    #[test]
    fn changes_from_the_settings_page() {
        let s = Settings::default();
        let s = apply_change(&s, "network.proxy.port", serde_json::json!(1080)).unwrap();
        assert_eq!(s.network.proxy.port, 1080);
        assert!(apply_change(&s, "network.proxy.port", serde_json::json!("x")).is_err());
        assert!(apply_change(&s, "network.nope", serde_json::json!(1)).is_err());
        assert!(apply_change(&s, "../etc", serde_json::json!(1)).is_err());
        let s = apply_change(&s, "sites.example.org.scripts", serde_json::json!(false)).unwrap();
        assert!(s.shields_for("www.example.org").scripts_blocked);
        assert!(!s.shields_for("other.test").scripts_blocked);
        let s = apply_change(&s, "appearance.theme", serde_json::json!("neon")).unwrap();
        assert_eq!(s.appearance.theme, "system");
    }

    #[test]
    fn shields_per_site() {
        let mut s = Settings::default();
        assert!(s.shields_for("a.test").ads);
        s.sites.insert(
            "a.test".into(),
            SiteSettings {
                shields_up: Some(false),
                ..Default::default()
            },
        );
        let e = s.shields_for("a.test");
        assert!(!e.ads && !e.trackers && !e.shields_up);
        s.shields.block_scripts = true;
        assert!(s.shields_for("b.test").scripts_blocked);
        s.sites.insert(
            "b.test".into(),
            SiteSettings {
                scripts: Some(true),
                ..Default::default()
            },
        );
        assert!(!s.shields_for("b.test").scripts_blocked);
    }

    #[test]
    fn proxy_prefs() {
        let mut p = Proxy::default();
        assert_eq!(p.chromium_pref(), serde_json::json!({"mode": "system"}));
        p.enabled = true;
        p.mode = "fixed".into();
        assert_eq!(
            p.chromium_pref(),
            serde_json::json!({"mode": "fixed_servers", "server": "socks5://127.0.0.1:9050", "bypass_list": "localhost;127.0.0.1;::1"})
        );
        p.host = "::1".into();
        assert_eq!(p.chromium_pref()["server"], "socks5://[::1]:9050");
        p.host = "bad host;x".into();
        assert!(p.validate().is_err());
        assert_eq!(p.chromium_pref(), serde_json::json!({"mode": "system"}));
        p.mode = "pac".into();
        p.pac_url = "http://x/p.pac".into();
        assert!(p.validate().is_err());
    }

    #[test]
    fn secure_dns_prefs() {
        let mut d = SecureDns::default();
        assert_eq!(d.chromium_prefs()[0].1, "automatic");
        assert_eq!(d.chromium_prefs()[1].1, "https://dns.quad9.net/dns-query");
        d.mode = "secure".into();
        d.provider = "custom".into();
        d.custom_template = "http://plain/dns".into();
        assert_eq!(d.chromium_prefs()[0].1, "off");
        d.custom_template = "https://dns.example/dns-query{?dns}".into();
        assert_eq!(d.chromium_prefs()[0].1, "secure");
    }

    #[test]
    fn search_templates() {
        let mut s = Search::default();
        assert!(s.template().starts_with("https://duckduckgo.com"));
        s.engine = "custom".into();
        s.custom_url = "javascript:{}".into();
        assert!(s.template().starts_with("https://duckduckgo.com"));
        s.custom_url = "https://kagi.com/search?q={}".into();
        assert_eq!(s.template(), "https://kagi.com/search?q={}");
    }
}
