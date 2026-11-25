//! State every CEF thread can read: settings, the history database, the session token.
//! Tabs and windows live on the UI thread only (see `browser.rs`).

use cottoncandy_core::{paths, settings::Settings, store::Store};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, RwLock};

pub struct Shared {
    /// Proves a request comes from one of CottonCandy's own pages (it's in their HTML, nowhere else).
    pub token: String,
    pub settings: RwLock<Settings>,
    pub settings_path: PathBuf,
    pub store: Mutex<Store>,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

pub fn get() -> &'static Shared {
    SHARED.get().expect("shared state is set up in main")
}

pub fn init() -> &'static Shared {
    let data_dir = paths::data_dir();
    let cache_dir = paths::cache_dir();
    let _ = paths::ensure_private_dir(&data_dir);
    let _ = paths::ensure_private_dir(&cache_dir);
    let settings_path = cottoncandy_core::settings::settings_path();
    let settings = Settings::load(&settings_path);
    let store = Store::open(&data_dir.join("browser.sqlite"))
        .or_else(|_| Store::in_memory())
        .expect("an in-memory database always opens");
    SHARED.get_or_init(|| Shared {
        token: random_token(),
        settings: RwLock::new(settings),
        settings_path,
        store: Mutex::new(store),
        data_dir,
        cache_dir,
    })
}

fn random_token() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 24];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .expect("the system random source works");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn settings() -> Settings {
    get().settings.read().map(|s| s.clone()).unwrap_or_default()
}

/// Saves and swaps in new settings.
pub fn set_settings(new: Settings) -> Result<(), String> {
    let shared = get();
    new.save(&shared.settings_path).map_err(|e| e.to_string())?;
    if let Ok(mut s) = shared.settings.write() {
        *s = new;
    }
    Ok(())
}

pub fn with_store<T>(f: impl FnOnce(&Store) -> T) -> Option<T> {
    get().store.lock().ok().map(|s| f(&s))
}
