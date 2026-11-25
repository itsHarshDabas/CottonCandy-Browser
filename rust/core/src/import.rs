//! Bringing bookmarks, history, open tabs and extensions over from other browsers on this computer:
//! Chrome, Edge, Brave, Vivaldi, Opera, Chromium and Firefox, on Linux and Windows.
//!
//! Everything here only reads the other browser's files. Databases are copied to a private scratch folder
//! first, so a browser that is still running is never disturbed and its locks don't get in the way.
//! Passwords and cookies are not imported: the other browsers encrypt them with keys tied to themselves.

use crate::bookmarks_html::Imported;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// How many history entries one import brings in, newest first.
pub const MAX_HISTORY: usize = 100_000;
/// How many open tabs one import brings in.
pub const MAX_TABS: usize = 500;
/// The largest file read whole (bookmarks, session, preferences).
const MAX_FILE: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Chromium,
    Firefox,
}

/// One profile of another browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// "Google Chrome", "Microsoft Edge", "Firefox"...
    pub browser: String,
    /// The profile's own name ("Person 1", "Work", "default-release").
    pub name: String,
    pub kind: Kind,
    pub dir: PathBuf,
}

/// A visited page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub url: String,
    pub title: String,
    pub visits: i64,
    /// Seconds since 1970.
    pub last_visit: i64,
}

/// An open tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub url: String,
    pub title: String,
    pub pinned: bool,
}

/// An extension the other browser has installed from a store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    /// A Chrome Web Store id, or an addons.mozilla.org id for Firefox.
    pub id: String,
    pub name: String,
    pub kind: Kind,
}

/// Where browsers keep their profiles. Filled from the environment by `Roots::here`, or by hand in tests.
#[derive(Debug, Clone, Default)]
pub struct Roots {
    pub home: PathBuf,
    /// `XDG_CONFIG_HOME`, or `~/.config` (Linux).
    pub config: PathBuf,
    /// `%LOCALAPPDATA%` (Windows).
    pub local_app_data: Option<PathBuf>,
    /// `%APPDATA%` (Windows).
    pub app_data: Option<PathBuf>,
}

impl Roots {
    pub fn here() -> Roots {
        let home = crate::paths::home();
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        Roots {
            home,
            config,
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            app_data: std::env::var_os("APPDATA").map(PathBuf::from),
        }
    }
}

/// Chromium-based browsers: their name and where their "User Data" folder can be. Opera keeps one
/// profile in the folder itself.
fn chromium_roots(r: &Roots) -> Vec<(&'static str, PathBuf)> {
    let mut out = Vec::new();
    let c = &r.config;
    let h = &r.home;
    for (name, dirs) in [
        (
            "Google Chrome",
            vec![
                c.join("google-chrome"),
                h.join(".var/app/com.google.Chrome/config/google-chrome"),
            ],
        ),
        ("Google Chrome Beta", vec![c.join("google-chrome-beta")]),
        (
            "Chromium",
            vec![
                c.join("chromium"),
                h.join("snap/chromium/common/chromium"),
                h.join(".var/app/org.chromium.Chromium/config/chromium"),
            ],
        ),
        (
            "Microsoft Edge",
            vec![
                c.join("microsoft-edge"),
                h.join(".var/app/com.microsoft.Edge/config/microsoft-edge"),
            ],
        ),
        (
            "Brave",
            vec![
                c.join("BraveSoftware/Brave-Browser"),
                h.join(".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser"),
            ],
        ),
        ("Vivaldi", vec![c.join("vivaldi")]),
        ("Opera", vec![c.join("opera")]),
    ] {
        for d in dirs {
            out.push((name, d));
        }
    }
    if let Some(l) = &r.local_app_data {
        for (name, rel) in [
            ("Google Chrome", "Google/Chrome/User Data"),
            ("Chromium", "Chromium/User Data"),
            ("Microsoft Edge", "Microsoft/Edge/User Data"),
            ("Brave", "BraveSoftware/Brave-Browser/User Data"),
            ("Vivaldi", "Vivaldi/User Data"),
        ] {
            out.push((name, l.join(rel)));
        }
    }
    if let Some(a) = &r.app_data {
        out.push(("Opera", a.join("Opera Software/Opera Stable")));
        out.push(("Opera GX", a.join("Opera Software/Opera GX Stable")));
    }
    out
}

fn firefox_roots(r: &Roots) -> Vec<PathBuf> {
    let mut out = vec![
        r.home.join(".mozilla/firefox"),
        r.home.join("snap/firefox/common/.mozilla/firefox"),
        r.home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
    ];
    if let Some(a) = &r.app_data {
        out.push(a.join("Mozilla/Firefox"));
    }
    out
}

fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    std::fs::read(path)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn has_data(dir: &Path) -> bool {
    ["Bookmarks", "History", "places.sqlite"]
        .iter()
        .any(|f| dir.join(f).is_file())
}

/// Every browser profile found on this computer, in a stable order.
pub fn detect(r: &Roots) -> Vec<Profile> {
    let mut out: Vec<Profile> = Vec::new();
    let mut push = |p: Profile| {
        if !out.iter().any(|o| o.dir == p.dir) {
            out.push(p);
        }
    };
    for (browser, root) in chromium_roots(r) {
        if !root.is_dir() {
            continue;
        }
        if has_data(&root) {
            // Opera: the folder is the profile.
            push(Profile {
                browser: browser.into(),
                name: "Default".into(),
                kind: Kind::Chromium,
                dir: root.clone(),
            });
        }
        // Profile names come from "Local State"; folders it doesn't list are still found by looking.
        let names: Value = read_small(&root.join("Local State"))
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        let mut dirs: Vec<String> = std::fs::read_dir(&root)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| n == "Default" || n.starts_with("Profile "))
                    .collect()
            })
            .unwrap_or_default();
        dirs.sort_by_key(|n| (n != "Default", n.len(), n.clone()));
        for d in dirs {
            let dir = root.join(&d);
            if !has_data(&dir) {
                continue;
            }
            let name = names["profile"]["info_cache"][&d]["name"]
                .as_str()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(&d)
                .to_string();
            push(Profile {
                browser: browser.into(),
                name,
                kind: Kind::Chromium,
                dir,
            });
        }
    }
    for root in firefox_roots(r) {
        let Some(ini) = read_small(&root.join("profiles.ini")) else {
            continue;
        };
        for p in firefox_profiles(&ini) {
            let dir = if p.relative {
                root.join(&p.path)
            } else {
                PathBuf::from(&p.path)
            };
            if has_data(&dir) {
                push(Profile {
                    browser: "Firefox".into(),
                    name: p.name,
                    kind: Kind::Firefox,
                    dir,
                });
            }
        }
    }
    out
}

struct IniProfile {
    name: String,
    path: String,
    relative: bool,
}

fn firefox_profiles(ini: &str) -> Vec<IniProfile> {
    let mut out = Vec::new();
    let mut cur: Option<IniProfile> = None;
    let finish = |p: Option<IniProfile>, out: &mut Vec<IniProfile>| {
        if let Some(p) = p.filter(|p| !p.path.is_empty()) {
            out.push(p);
        }
    };
    for line in ini.lines().map(str::trim) {
        if line.starts_with('[') {
            finish(cur.take(), &mut out);
            if line.to_ascii_lowercase().starts_with("[profile") {
                cur = Some(IniProfile {
                    name: String::new(),
                    path: String::new(),
                    relative: true,
                });
            }
        } else if let (Some(p), Some((k, v))) = (cur.as_mut(), line.split_once('=')) {
            match k.trim() {
                "Name" => p.name = v.trim().to_string(),
                "Path" => p.path = v.trim().replace('\\', "/"),
                "IsRelative" => p.relative = v.trim() != "0",
                _ => {}
            }
        }
    }
    finish(cur, &mut out);
    for p in &mut out {
        if p.name.is_empty() {
            p.name = p.path.rsplit('/').next().unwrap_or("Firefox").to_string();
        }
    }
    out
}

fn web(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://")) && url.len() <= 8192
}

// ---- bookmarks ------------------------------------------------------------------------------------

pub fn bookmarks(p: &Profile, scratch: &Path) -> Result<Vec<Imported>, String> {
    match p.kind {
        Kind::Chromium => {
            let text = read_small(&p.dir.join("Bookmarks")).ok_or("No bookmarks found.")?;
            Ok(chromium_bookmarks(&text))
        }
        Kind::Firefox => {
            let db = copy_db(&p.dir.join("places.sqlite"), scratch)?;
            firefox_bookmarks(db.conn()).map_err(|e| e.to_string())
        }
    }
}

/// Chromium's "Bookmarks" file: JSON with a tree under roots.bookmark_bar, roots.other and roots.synced.
pub fn chromium_bookmarks(json: &str) -> Vec<Imported> {
    let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    let mut out = Vec::new();
    for (key, label) in [
        ("bookmark_bar", "Bookmarks bar"),
        ("other", "Other bookmarks"),
        ("synced", "Mobile bookmarks"),
    ] {
        walk_chromium(&v["roots"][key], label, &mut out);
    }
    out
}

fn walk_chromium(node: &Value, folder: &str, out: &mut Vec<Imported>) {
    for child in node["children"].as_array().into_iter().flatten() {
        if out.len() >= crate::bookmarks_html::MAX_IMPORT {
            return;
        }
        let name = child["name"].as_str().unwrap_or("");
        match child["type"].as_str() {
            Some("url") => {
                let url = child["url"].as_str().unwrap_or("");
                if web(url) {
                    out.push(Imported {
                        url: url.into(),
                        title: name.into(),
                        folder: folder.into(),
                    });
                }
            }
            Some("folder") => walk_chromium(child, &format!("{folder}/{name}"), out),
            _ => {}
        }
    }
}

fn firefox_bookmarks(db: &rusqlite::Connection) -> rusqlite::Result<Vec<Imported>> {
    // Folders first, so each bookmark's path can be built from its parents.
    let mut folders: std::collections::HashMap<i64, (i64, String, String)> = Default::default();
    {
        let mut stmt = db.prepare(
            "SELECT id, parent, IFNULL(title, ''), IFNULL(guid, '') FROM moz_bookmarks WHERE type = 2",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        for (id, parent, title, guid) in rows.flatten() {
            folders.insert(id, (parent, title, guid));
        }
    }
    let root_name = |guid: &str| match guid {
        "toolbar_____" => Some("Bookmarks bar"),
        "menu________" => Some("Bookmarks menu"),
        "unfiled_____" => Some("Other bookmarks"),
        "mobile______" => Some("Mobile bookmarks"),
        _ => None,
    };
    let path_of = |mut id: i64| {
        let mut parts = Vec::new();
        for _ in 0..64 {
            let Some((parent, title, guid)) = folders.get(&id) else {
                break;
            };
            if let Some(root) = root_name(guid) {
                parts.push(root.to_string());
                break;
            }
            if guid == "root________" {
                break;
            }
            parts.push(title.clone());
            id = *parent;
        }
        parts.reverse();
        if parts.is_empty() {
            "Other bookmarks".to_string()
        } else {
            parts.join("/")
        }
    };
    let mut stmt = db.prepare(
        "SELECT p.url, IFNULL(b.title, IFNULL(p.title, '')), b.parent
         FROM moz_bookmarks b JOIN moz_places p ON p.id = b.fk
         WHERE b.type = 1 ORDER BY b.parent, b.position",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    Ok(rows
        .flatten()
        .filter(|(url, _, _)| web(url))
        .take(crate::bookmarks_html::MAX_IMPORT)
        .map(|(url, title, parent)| Imported {
            url,
            title,
            folder: path_of(parent),
        })
        .collect())
}

// ---- history --------------------------------------------------------------------------------------

pub fn history(p: &Profile, scratch: &Path) -> Result<Vec<Visit>, String> {
    let (file, sql) = match p.kind {
        // Chromium counts microseconds since 1601.
        Kind::Chromium => (
            "History",
            "SELECT url, IFNULL(title, ''), visit_count, last_visit_time / 1000000 - 11644473600
             FROM urls WHERE hidden = 0 AND last_visit_time > 0 ORDER BY last_visit_time DESC LIMIT ?1",
        ),
        // Firefox counts microseconds since 1970.
        Kind::Firefox => (
            "places.sqlite",
            "SELECT url, IFNULL(title, ''), visit_count, last_visit_date / 1000000
             FROM moz_places WHERE hidden = 0 AND visit_count > 0 AND last_visit_date IS NOT NULL
             ORDER BY last_visit_date DESC LIMIT ?1",
        ),
    };
    let db = copy_db(&p.dir.join(file), scratch)?;
    let mut stmt = db.conn().prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([MAX_HISTORY as i64], |r| {
            Ok(Visit {
                url: r.get(0)?,
                title: r.get(1)?,
                visits: r.get::<_, i64>(2)?.max(1),
                last_visit: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().filter(|v| web(&v.url)).collect())
}

/// A private copy of a browser's database (and its write-ahead log), removed when dropped.
struct CopiedDb {
    conn: Option<rusqlite::Connection>,
    files: Vec<PathBuf>,
}

impl CopiedDb {
    fn conn(&self) -> &rusqlite::Connection {
        self.conn.as_ref().expect("open until dropped")
    }
}

impl Drop for CopiedDb {
    fn drop(&mut self) {
        // Close the database first: Windows can't delete a file that is still open.
        drop(self.conn.take());
        for f in &self.files {
            let _ = std::fs::remove_file(f);
        }
    }
}

fn copy_db(src: &Path, scratch: &Path) -> Result<CopiedDb, String> {
    if !src.is_file() {
        return Err("Nothing to import.".into());
    }
    crate::paths::ensure_private_dir(scratch).map_err(|e| e.to_string())?;
    let mut tag = [0u8; 8];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut tag)
        .map_err(|_| "no randomness")?;
    let name = format!(
        "import-{}.sqlite",
        tag.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let dest = scratch.join(name);
    let mut files = Vec::new();
    for suffix in ["", "-wal"] {
        let from = PathBuf::from(format!("{}{suffix}", src.display()));
        let to = PathBuf::from(format!("{}{suffix}", dest.display()));
        if from.is_file() {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("Couldn't read {}: {e}", src.display()))?;
            files.push(to);
        }
    }
    files.push(PathBuf::from(format!("{}-shm", dest.display())));
    let conn = rusqlite::Connection::open(&dest).map_err(|e| e.to_string())?;
    Ok(CopiedDb {
        conn: Some(conn),
        files,
    })
}

// ---- open tabs ------------------------------------------------------------------------------------

pub fn tabs(p: &Profile) -> Result<Vec<Tab>, String> {
    match p.kind {
        Kind::Chromium => {
            let file = newest_session(&p.dir).ok_or("No open tabs found.")?;
            let meta = std::fs::metadata(&file).map_err(|e| e.to_string())?;
            if meta.len() > MAX_FILE {
                return Err("The session file is too big.".into());
            }
            let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
            Ok(snss_tabs(&bytes))
        }
        Kind::Firefox => {
            // recovery.jsonlz4 while Firefox runs; sessionstore.jsonlz4 after it closed.
            let candidates = [
                p.dir.join("sessionstore-backups/recovery.jsonlz4"),
                p.dir.join("sessionstore.jsonlz4"),
            ];
            let newest = candidates
                .iter()
                .filter_map(|f| Some((std::fs::metadata(f).ok()?.modified().ok()?, f)))
                .max_by_key(|(t, _)| *t)
                .map(|(_, f)| f.clone())
                .ok_or("No open tabs found.")?;
            let meta = std::fs::metadata(&newest).map_err(|e| e.to_string())?;
            if meta.len() > MAX_FILE {
                return Err("The session file is too big.".into());
            }
            let bytes = std::fs::read(&newest).map_err(|e| e.to_string())?;
            let json = mozlz4(&bytes).ok_or("Couldn't read Firefox's session file.")?;
            Ok(firefox_tabs(&String::from_utf8_lossy(&json)))
        }
    }
}

/// Chromium's current session: `Sessions/Session_<time>` (newest), or `Current Session` in older versions.
fn newest_session(dir: &Path) -> Option<PathBuf> {
    let newest = std::fs::read_dir(dir.join("Sessions"))
        .ok()
        .into_iter()
        .flat_map(|rd| rd.flatten())
        .filter(|e| e.file_name().to_string_lossy().starts_with("Session_"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p);
    newest.or_else(|| {
        let old = dir.join("Current Session");
        old.is_file().then_some(old)
    })
}

/// Reads the open tabs from a Chromium session file ("SNSS"): a log of commands, each a 16-bit size and
/// then an 8-bit id and its payload. The tabs are rebuilt by replaying the commands that matter.
pub fn snss_tabs(bytes: &[u8]) -> Vec<Tab> {
    use std::collections::{BTreeMap, HashMap, HashSet};
    const SET_TAB_WINDOW: u8 = 0;
    const SET_TAB_INDEX_IN_WINDOW: u8 = 2;
    const UPDATE_TAB_NAVIGATION: u8 = 6;
    const SET_SELECTED_NAVIGATION_INDEX: u8 = 7;
    const SET_PINNED_STATE: u8 = 12;
    const TAB_CLOSED: u8 = 16;
    const WINDOW_CLOSED: u8 = 17;

    if bytes.len() < 8 || &bytes[..4] != b"SNSS" {
        return Vec::new();
    }
    let i32_at = |b: &[u8], at: usize| -> Option<i32> {
        b.get(at..at + 4)
            .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };
    let mut navs: HashMap<i32, BTreeMap<i32, (String, String)>> = HashMap::new();
    let mut selected: HashMap<i32, i32> = HashMap::new();
    let mut window_of: HashMap<i32, i32> = HashMap::new();
    let mut index_of: HashMap<i32, i32> = HashMap::new();
    let mut pinned: HashSet<i32> = HashSet::new();
    let mut closed_tabs: HashSet<i32> = HashSet::new();
    let mut closed_windows: HashSet<i32> = HashSet::new();
    let mut order: Vec<i32> = Vec::new();
    let mut pos = 8;
    while pos + 2 <= bytes.len() {
        let size = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
        pos += 2;
        if size == 0 || pos + size > bytes.len() {
            break;
        }
        let id = bytes[pos];
        let body = &bytes[pos + 1..pos + size];
        pos += size;
        match id {
            SET_TAB_WINDOW => {
                if let (Some(w), Some(t)) = (i32_at(body, 0), i32_at(body, 4)) {
                    window_of.insert(t, w);
                    // A tab that comes back (moved to another window) is open again.
                    closed_tabs.remove(&t);
                }
            }
            SET_TAB_INDEX_IN_WINDOW => {
                if let (Some(t), Some(i)) = (i32_at(body, 0), i32_at(body, 4)) {
                    index_of.insert(t, i);
                }
            }
            SET_SELECTED_NAVIGATION_INDEX => {
                if let (Some(t), Some(i)) = (i32_at(body, 0), i32_at(body, 4)) {
                    selected.insert(t, i);
                }
            }
            SET_PINNED_STATE => {
                if let (Some(t), Some(p)) = (i32_at(body, 0), body.get(4)) {
                    if *p != 0 {
                        pinned.insert(t);
                    } else {
                        pinned.remove(&t);
                    }
                }
            }
            TAB_CLOSED => {
                if let Some(t) = i32_at(body, 0) {
                    closed_tabs.insert(t);
                }
            }
            WINDOW_CLOSED => {
                if let Some(w) = i32_at(body, 0) {
                    closed_windows.insert(w);
                }
            }
            UPDATE_TAB_NAVIGATION => {
                // A pickle: a 32-bit payload size, then tab id, navigation index, the URL as a 32-bit
                // length and its bytes, and the title as a 32-bit length and UTF-16 units, each padded to
                // four bytes.
                let (Some(t), Some(i), Some(len)) =
                    (i32_at(body, 4), i32_at(body, 8), i32_at(body, 12))
                else {
                    continue;
                };
                let len = len.max(0) as usize;
                let Some(url) = body.get(16..16 + len) else {
                    continue;
                };
                let at = 16 + len.div_ceil(4) * 4;
                let title = i32_at(body, at)
                    .map(|n| n.clamp(0, 4096) as usize)
                    .and_then(|n| body.get(at + 4..at + 4 + n * 2))
                    .map(|b| {
                        let units: Vec<u16> = b
                            .chunks_exact(2)
                            .map(|c| u16::from_le_bytes([c[0], c[1]]))
                            .collect();
                        String::from_utf16_lossy(&units)
                    })
                    .unwrap_or_default();
                if !order.contains(&t) {
                    order.push(t);
                }
                navs.entry(t)
                    .or_default()
                    .insert(i, (String::from_utf8_lossy(url).into_owned(), title));
            }
            _ => {}
        }
    }
    let mut open: Vec<(i32, i32, usize, Tab)> = Vec::new();
    for (n, t) in order.iter().enumerate() {
        if closed_tabs.contains(t) {
            continue;
        }
        let window = window_of.get(t).copied().unwrap_or(0);
        if closed_windows.contains(&window) {
            continue;
        }
        let Some(entries) = navs.get(t) else { continue };
        let entry = selected
            .get(t)
            .and_then(|i| entries.get(i))
            .or_else(|| entries.values().next_back());
        if let Some((url, title)) = entry.filter(|(u, _)| web(u)) {
            open.push((
                window,
                index_of.get(t).copied().unwrap_or(i32::MAX),
                n,
                Tab {
                    url: url.clone(),
                    title: title.clone(),
                    pinned: pinned.contains(t),
                },
            ));
        }
    }
    open.sort_by_key(|(w, i, n, _)| (*w, *i, *n));
    open.into_iter()
        .map(|(_, _, _, t)| t)
        .take(MAX_TABS)
        .collect()
}

/// Decompresses Firefox's "mozLz40" files: a magic, the decompressed size, then one LZ4 block.
pub fn mozlz4(bytes: &[u8]) -> Option<Vec<u8>> {
    let rest = bytes.strip_prefix(b"mozLz40\0")?;
    let size = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
    if size as u64 > 4 * MAX_FILE {
        return None;
    }
    lz4_block(&rest[4..], size)
}

/// An LZ4 block decoder (the format is a sequence of literal runs and back-references).
fn lz4_block(src: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut i = 0;
    let length = |i: &mut usize, mut n: usize| -> Option<usize> {
        if n == 15 {
            loop {
                let b = *src.get(*i)?;
                *i += 1;
                n += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        Some(n)
    };
    while i < src.len() {
        let token = src[i];
        i += 1;
        let lit = length(&mut i, (token >> 4) as usize)?;
        out.extend_from_slice(src.get(i..i + lit)?);
        i += lit;
        if i >= src.len() {
            break; // the last run has literals only
        }
        let offset = u16::from_le_bytes([*src.get(i)?, *src.get(i + 1)?]) as usize;
        i += 2;
        if offset == 0 || offset > out.len() {
            return None;
        }
        let n = length(&mut i, (token & 15) as usize)? + 4;
        if out.len() + n > size {
            return None;
        }
        let start = out.len() - offset;
        for k in 0..n {
            let b = out[start + k];
            out.push(b);
        }
    }
    (out.len() == size).then_some(out)
}

/// The open tabs in Firefox's session JSON: windows[].tabs[], each showing entries[index - 1].
pub fn firefox_tabs(json: &str) -> Vec<Tab> {
    let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    let mut out = Vec::new();
    for w in v["windows"].as_array().into_iter().flatten() {
        for t in w["tabs"].as_array().into_iter().flatten() {
            let entries = t["entries"].as_array();
            let index = t["index"].as_u64().unwrap_or(0) as usize;
            let entry = entries.and_then(|e| e.get(index.saturating_sub(1)).or(e.last()));
            let url = entry.and_then(|e| e["url"].as_str()).unwrap_or("");
            if web(url) && out.len() < MAX_TABS {
                out.push(Tab {
                    url: url.into(),
                    title: entry.and_then(|e| e["title"].as_str()).unwrap_or("").into(),
                    pinned: t["pinned"].as_bool().unwrap_or(false),
                });
            }
        }
    }
    out
}

// ---- extensions -----------------------------------------------------------------------------------

/// Extensions installed from a store by the user (not ones that came with the browser).
pub fn extensions(p: &Profile) -> Vec<Extension> {
    match p.kind {
        Kind::Chromium => chromium_extensions(&p.dir),
        Kind::Firefox => read_small(&p.dir.join("extensions.json"))
            .map(|t| firefox_extensions(&t))
            .unwrap_or_default(),
    }
}

fn chromium_extensions(dir: &Path) -> Vec<Extension> {
    let prefs: Vec<Value> = ["Secure Preferences", "Preferences"]
        .iter()
        .filter_map(|f| read_small(&dir.join(f)))
        .filter_map(|t| serde_json::from_str(&t).ok())
        .collect();
    let setting = |id: &str| {
        prefs
            .iter()
            .map(|p| &p["extensions"]["settings"][id])
            .find(|s| !s.is_null())
    };
    let mut ids: Vec<String> = std::fs::read_dir(dir.join("Extensions"))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|id| crate::extensions::is_chrome_id(id))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    let mut out = Vec::new();
    for id in ids {
        if let Some(s) = setting(&id) {
            // 1 is "internal": installed from the Web Store. Skip ones the browser put there itself.
            let from_store = s["location"].as_i64().is_none_or(|l| l == 1);
            if !from_store || s["was_installed_by_default"].as_bool() == Some(true) {
                continue;
            }
        }
        let name = extension_name(&dir.join("Extensions").join(&id)).unwrap_or_else(|| id.clone());
        out.push(Extension {
            id,
            name,
            kind: Kind::Chromium,
        });
    }
    out
}

/// The extension's name from its newest version's manifest, looking up a `__MSG_name__` in English.
fn extension_name(ext_dir: &Path) -> Option<String> {
    let mut versions: Vec<PathBuf> = std::fs::read_dir(ext_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("manifest.json").is_file())
        .collect();
    versions.sort();
    let dir = versions.pop()?;
    let manifest: Value = serde_json::from_str(&read_small(&dir.join("manifest.json"))?).ok()?;
    let name = manifest["name"].as_str()?.to_string();
    let Some(key) = name
        .strip_prefix("__MSG_")
        .and_then(|n| n.strip_suffix("__"))
    else {
        return Some(name);
    };
    let locale = manifest["default_locale"].as_str().unwrap_or("en");
    for loc in ["en", "en_US", locale] {
        let Some(text) = read_small(&dir.join("_locales").join(loc).join("messages.json")) else {
            continue;
        };
        let msgs: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if let Some(obj) = msgs.as_object() {
            if let Some((_, v)) = obj.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
                if let Some(m) = v["message"].as_str() {
                    return Some(m.to_string());
                }
            }
        }
    }
    Some(name)
}

fn firefox_extensions(json: &str) -> Vec<Extension> {
    let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    v["addons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| {
            a["type"].as_str() == Some("extension") && a["location"].as_str() == Some("app-profile")
        })
        .filter_map(|a| {
            let id = a["id"].as_str()?.to_string();
            let name = a["defaultLocale"]["name"]
                .as_str()
                .unwrap_or(&id)
                .to_string();
            let ok = id.len() <= 200
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.@{}".contains(c));
            ok.then_some(Extension {
                id,
                name,
                kind: Kind::Firefox,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pickle_nav(tab: i32, index: i32, url: &str) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&0i32.to_le_bytes()); // payload size (not checked)
        p.extend_from_slice(&tab.to_le_bytes());
        p.extend_from_slice(&index.to_le_bytes());
        p.extend_from_slice(&(url.len() as i32).to_le_bytes());
        p.extend_from_slice(url.as_bytes());
        while p.len() % 4 != 0 {
            p.push(0);
        }
        let title: Vec<u16> = format!("T{tab}").encode_utf16().collect();
        p.extend_from_slice(&(title.len() as i32).to_le_bytes());
        for u in title {
            p.extend_from_slice(&u.to_le_bytes());
        }
        p
    }

    fn cmd(out: &mut Vec<u8>, id: u8, body: &[u8]) {
        out.extend_from_slice(&((body.len() + 1) as u16).to_le_bytes());
        out.push(id);
        out.extend_from_slice(body);
    }

    fn pair(a: i32, b: i32) -> Vec<u8> {
        [a.to_le_bytes(), b.to_le_bytes()].concat()
    }

    #[test]
    fn replays_a_chromium_session() {
        let mut f = b"SNSS".to_vec();
        f.extend_from_slice(&3i32.to_le_bytes());
        cmd(&mut f, 0, &pair(1, 10));
        cmd(&mut f, 0, &pair(1, 11));
        cmd(&mut f, 0, &pair(2, 12));
        cmd(&mut f, 2, &pair(10, 1));
        cmd(&mut f, 2, &pair(11, 0));
        cmd(&mut f, 6, &pickle_nav(10, 0, "https://a.example/"));
        cmd(&mut f, 6, &pickle_nav(10, 1, "https://a.example/next"));
        cmd(&mut f, 7, &pair(10, 1));
        cmd(&mut f, 6, &pickle_nav(11, 0, "https://b.example/"));
        cmd(
            &mut f,
            12,
            &[11i32.to_le_bytes().as_slice(), &[1, 0, 0, 0]].concat(),
        );
        cmd(
            &mut f,
            6,
            &pickle_nav(12, 0, "https://closed-window.example/"),
        );
        cmd(
            &mut f,
            17,
            &[2i32.to_le_bytes(), 0i32.to_le_bytes()].concat(),
        );
        cmd(&mut f, 0, &pair(1, 13));
        cmd(&mut f, 6, &pickle_nav(13, 0, "chrome://settings/"));
        cmd(&mut f, 0, &pair(1, 14));
        cmd(&mut f, 6, &pickle_nav(14, 0, "https://closed-tab.example/"));
        cmd(
            &mut f,
            16,
            &[14i32.to_le_bytes(), 0i32.to_le_bytes()].concat(),
        );
        let tabs = snss_tabs(&f);
        assert_eq!(
            tabs,
            vec![
                Tab {
                    url: "https://b.example/".into(),
                    title: "T11".into(),
                    pinned: true
                },
                Tab {
                    url: "https://a.example/next".into(),
                    title: "T10".into(),
                    pinned: false
                },
            ]
        );
        assert!(snss_tabs(b"not a session").is_empty());
        // A truncated file gives what was read before the cut.
        assert_eq!(snss_tabs(&f[..f.len() - 3]).len(), 3);
    }

    #[test]
    fn reads_chromium_bookmarks() {
        let json = r#"{"roots":{"bookmark_bar":{"children":[
            {"type":"url","name":"Site","url":"https://site.example/"},
            {"type":"folder","name":"News","children":[{"type":"url","name":"N","url":"https://news.example/"}]},
            {"type":"url","name":"JS","url":"javascript:alert(1)"}]},
            "other":{"children":[{"type":"url","name":"O","url":"http://other.example/"}]}}}"#;
        let b = chromium_bookmarks(json);
        let got: Vec<_> = b
            .iter()
            .map(|b| (b.url.as_str(), b.folder.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("https://site.example/", "Bookmarks bar"),
                ("https://news.example/", "Bookmarks bar/News"),
                ("http://other.example/", "Other bookmarks"),
            ]
        );
        assert!(chromium_bookmarks("garbage").is_empty());
    }

    #[test]
    fn decodes_mozlz4() {
        // "abcabcabcabc!" as LZ4: literals "abc", then a match of 9 at offset 3, then the literal "!".
        let block = [0x35, b'a', b'b', b'c', 3, 0, 0x10, b'!'];
        let mut file = b"mozLz40\0".to_vec();
        file.extend_from_slice(&13u32.to_le_bytes());
        file.extend_from_slice(&block);
        assert_eq!(mozlz4(&file).as_deref(), Some(&b"abcabcabcabc!"[..]));
        // A back-reference before the start is refused, not read out of bounds.
        let mut bad = b"mozLz40\0".to_vec();
        bad.extend_from_slice(&13u32.to_le_bytes());
        bad.extend_from_slice(&[0x15, b'a', 9, 0, 0x10, b'!']);
        assert_eq!(mozlz4(&bad), None);
    }

    #[test]
    fn reads_firefox_session_and_addons() {
        let s = r#"{"windows":[{"tabs":[
            {"entries":[{"url":"https://one.example/"},{"url":"https://two.example/"}],"index":1},
            {"entries":[{"url":"about:newtab"}],"index":1},
            {"entries":[{"url":"https://p.example/"}],"index":1,"pinned":true}]}]}"#;
        let t = firefox_tabs(s);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].url, "https://one.example/");
        assert!(t[1].pinned);
        let a = r#"{"addons":[
            {"id":"uBlock0@raymondhill.net","type":"extension","location":"app-profile","defaultLocale":{"name":"uBlock Origin"}},
            {"id":"formautofill@mozilla.org","type":"extension","location":"app-builtin"},
            {"id":"bad id; rm","type":"extension","location":"app-profile"}]}"#;
        let e = firefox_extensions(a);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].name, "uBlock Origin");
    }

    #[test]
    fn finds_profiles() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().to_path_buf();
        let chrome = home.join(".config/google-chrome");
        std::fs::create_dir_all(chrome.join("Default")).unwrap();
        std::fs::create_dir_all(chrome.join("Profile 2")).unwrap();
        std::fs::create_dir_all(chrome.join("Profile 3")).unwrap();
        std::fs::write(chrome.join("Default/Bookmarks"), "{}").unwrap();
        std::fs::write(chrome.join("Profile 2/History"), "").unwrap();
        std::fs::write(
            chrome.join("Local State"),
            r#"{"profile":{"info_cache":{"Default":{"name":"Personal"},"Profile 2":{"name":"Work"}}}}"#,
        )
        .unwrap();
        let ff = home.join(".mozilla/firefox");
        std::fs::create_dir_all(ff.join("abc.default-release")).unwrap();
        std::fs::write(ff.join("abc.default-release/places.sqlite"), "").unwrap();
        std::fs::write(
            ff.join("profiles.ini"),
            "[General]\nStartWithLastProfile=1\n\n[Profile0]\nName=default-release\nIsRelative=1\nPath=abc.default-release\n\n[Profile1]\nName=gone\nIsRelative=1\nPath=missing\n",
        )
        .unwrap();
        let found = detect(&Roots {
            config: home.join(".config"),
            home,
            ..Default::default()
        });
        let names: Vec<_> = found
            .iter()
            .map(|p| (p.browser.as_str(), p.name.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Google Chrome", "Personal"),
                ("Google Chrome", "Work"),
                ("Firefox", "default-release"),
            ]
        );
    }

    #[test]
    fn reads_history_and_firefox_bookmarks_from_copies() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("ff");
        std::fs::create_dir_all(&dir).unwrap();
        let db = rusqlite::Connection::open(dir.join("places.sqlite")).unwrap();
        db.execute_batch(
            "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER,
                 hidden INTEGER DEFAULT 0, last_visit_date INTEGER);
             CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER, parent INTEGER,
                 position INTEGER, title TEXT, guid TEXT);
             INSERT INTO moz_places VALUES (1, 'https://a.example/', 'A', 3, 0, 1700000000000000);
             INSERT INTO moz_places VALUES (2, 'place:sort=8', NULL, 1, 0, 1700000000000000);
             INSERT INTO moz_bookmarks VALUES (1, 2, NULL, 0, 0, '', 'root________');
             INSERT INTO moz_bookmarks VALUES (2, 2, NULL, 1, 0, 'toolbar', 'toolbar_____');
             INSERT INTO moz_bookmarks VALUES (3, 2, NULL, 2, 0, 'Reading', 'abcdefghijkl');
             INSERT INTO moz_bookmarks VALUES (4, 1, 1, 3, 0, 'My A', 'bookmark0001');",
        )
        .unwrap();
        drop(db);
        let p = Profile {
            browser: "Firefox".into(),
            name: "t".into(),
            kind: Kind::Firefox,
            dir,
        };
        let scratch = root.path().join("scratch");
        let h = history(&p, &scratch).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].last_visit, 1_700_000_000);
        let b = bookmarks(&p, &scratch).unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].folder, "Bookmarks bar/Reading");
        assert_eq!(b[0].title, "My A");
        // The copies are gone afterwards.
        assert_eq!(std::fs::read_dir(&scratch).unwrap().count(), 0);
    }
}
