//! Where CottonCandy keeps its files on Linux (XDG) and Windows (Known Folders through the environment).

use std::path::PathBuf;

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub fn home() -> PathBuf {
    env_dir("HOME")
        .or_else(|| env_dir("USERPROFILE"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Settings: `~/.config/cottoncandy` or `%APPDATA%\CottonCandy`.
pub fn config_dir() -> PathBuf {
    if cfg!(windows) {
        return env_dir("APPDATA")
            .unwrap_or_else(|| home().join("AppData").join("Roaming"))
            .join("CottonCandy");
    }
    env_dir("XDG_CONFIG_HOME")
        .unwrap_or_else(|| home().join(".config"))
        .join("cottoncandy")
}

/// CherryCola's settings, which say where its model servers are: `~/.config/grace/cherrycola.conf` or
/// `%APPDATA%\grace\cherrycola.conf`.
pub fn cherrycola_conf() -> PathBuf {
    let base = if cfg!(windows) {
        env_dir("APPDATA").unwrap_or_else(|| home().join("AppData").join("Roaming"))
    } else {
        env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config"))
    };
    base.join("grace").join("cherrycola.conf")
}

/// History, bookmarks, the browser profile, extensions: `~/.local/share/cottoncandy` or `%LOCALAPPDATA%\CottonCandy`.
pub fn data_dir() -> PathBuf {
    if cfg!(windows) {
        return env_dir("LOCALAPPDATA")
            .unwrap_or_else(|| home().join("AppData").join("Local"))
            .join("CottonCandy");
    }
    env_dir("XDG_DATA_HOME")
        .unwrap_or_else(|| home().join(".local").join("share"))
        .join("cottoncandy-rs")
}

/// Filter lists and other things that can be fetched again.
pub fn cache_dir() -> PathBuf {
    if cfg!(windows) {
        return data_dir().join("Cache");
    }
    env_dir("XDG_CACHE_HOME")
        .unwrap_or_else(|| home().join(".cache"))
        .join("cottoncandy-rs")
}

/// The default downloads folder.
pub fn downloads_dir() -> PathBuf {
    if let Some(dir) = env_dir("XDG_DOWNLOAD_DIR") {
        return dir;
    }
    home().join("Downloads")
}

/// Creates `dir` readable by this user only (on Unix).
pub fn ensure_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes `bytes` to `path` through a temporary file and a rename, so a crash never leaves half a file.
/// On Unix the file is readable by this user only.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    ensure_private_dir(dir)?;
    let tmp = path.with_extension("tmp");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}
