//! What installing means to the system: shortcuts, the entry in Windows' installed apps list (or a
//! desktop entry on Linux), the folder picker, and starting programs.

use crate::work::{Options, APP_EXE};
use std::path::{Path, PathBuf};

/// Where the browser keeps your profile (see `core::paths::data_dir`).
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("CottonCandy")
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("cottoncandy-rs")
    }
}

/// Asks for a folder. None when cancelled (or where there is no picker).
pub fn pick_folder(start: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let mut dialog = rfd::FileDialog::new().set_title("Choose where to install CottonCandy");
        if let Some(parent) = start.parent().filter(|p| p.exists()) {
            dialog = dialog.set_directory(parent);
        }
        dialog.pick_folder()
    }
    #[cfg(not(windows))]
    {
        let _ = start;
        None
    }
}

pub fn launch(dir: &Path) {
    let _ = std::process::Command::new(dir.join(APP_EXE))
        .current_dir(dir)
        .spawn();
}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::work::UNINSTALL_EXE;
    use winreg::enums::*;
    use winreg::RegKey;

    pub const UNINSTALL_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CottonCandy";

    pub fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
        use windows_sys::Win32::System::Com::CoTaskMemFree;
        use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
        let mut out: windows_sys::core::PWSTR = std::ptr::null_mut();
        // SAFETY: SHGetKnownFolderPath writes a CoTaskMem string we free below.
        let hr = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut out) };
        if hr != 0 || out.is_null() {
            return None;
        }
        // SAFETY: `out` is a valid NUL-terminated wide string until it is freed.
        let path = unsafe {
            let mut n = 0;
            while *out.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(out, n));
            CoTaskMemFree(out as _);
            s
        };
        Some(PathBuf::from(path))
    }

    pub fn desktop_lnk() -> Option<PathBuf> {
        known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Desktop)
            .map(|d| d.join("CottonCandy.lnk"))
    }

    pub fn start_menu_lnk() -> Option<PathBuf> {
        known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Programs)
            .map(|d| d.join("CottonCandy.lnk"))
    }

    pub fn shortcut(target: &Path, at: &Path) -> Result<(), String> {
        let mut link = mslnk::ShellLink::new(target).map_err(|e| e.to_string())?;
        link.set_working_dir(target.parent().map(|p| p.to_string_lossy().into_owned()));
        link.set_name(Some("CottonCandy, the GraceOS web browser".into()));
        link.set_icon_location(Some(target.to_string_lossy().into_owned()));
        link.create_lnk(at).map_err(|e| e.to_string())
    }

    pub fn register(opts: &Options, size_kb: u64) -> Result<(), String> {
        let exe = opts.dir.join(APP_EXE);
        if opts.desktop {
            if let Some(at) = desktop_lnk() {
                shortcut(&exe, &at)?;
            }
        }
        if opts.start_menu {
            if let Some(at) = start_menu_lnk() {
                shortcut(&exe, &at)?;
            }
        }
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(UNINSTALL_KEY)
            .map_err(|e| e.to_string())?;
        let uninstall = opts.dir.join(UNINSTALL_EXE);
        let q = |p: &Path| format!("\"{}\"", p.display());
        let values: [(&str, String); 8] = [
            ("DisplayName", "CottonCandy".into()),
            ("DisplayVersion", env!("CARGO_PKG_VERSION").into()),
            ("Publisher", "GraceOS".into()),
            ("DisplayIcon", exe.display().to_string()),
            ("InstallLocation", opts.dir.display().to_string()),
            ("UninstallString", q(&uninstall)),
            (
                "QuietUninstallString",
                format!("{} --silent", q(&uninstall)),
            ),
            (
                "URLInfoAbout",
                "https://github.com/itsHarshDabas/CottonCandy-Browser".into(),
            ),
        ];
        for (name, value) in values {
            key.set_value(name, &value).map_err(|e| e.to_string())?;
        }
        key.set_value("EstimatedSize", &(size_kb.min(u32::MAX as u64) as u32))
            .map_err(|e| e.to_string())?;
        key.set_value("NoModify", &1u32)
            .map_err(|e| e.to_string())?;
        key.set_value("NoRepair", &1u32)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn unregister(manifest: &serde_json::Value) {
        if manifest["desktop"].as_bool().unwrap_or(true) {
            if let Some(p) = desktop_lnk() {
                let _ = std::fs::remove_file(p);
            }
        }
        if manifest["start_menu"].as_bool().unwrap_or(true) {
            if let Some(p) = start_menu_lnk() {
                let _ = std::fs::remove_file(p);
            }
        }
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL_KEY);
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Starts this program again as administrator with `args`. False if that was refused.
    pub fn relaunch_elevated(args: &str) -> bool {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let (verb, file, params) = (wide("runas"), wide(&exe.to_string_lossy()), wide(args));
        // SAFETY: all strings are NUL-terminated and live for the call.
        let r = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                params.as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        r as isize > 32
    }
}

#[cfg(windows)]
pub use win::relaunch_elevated;

pub fn register(opts: &Options, size_kb: u64) -> Result<(), String> {
    #[cfg(windows)]
    {
        win::register(opts, size_kb)
    }
    #[cfg(not(windows))]
    {
        let _ = size_kb;
        // A menu entry, so the browser shows up with the other apps.
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let apps = home.join(".local/share/applications");
        std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=CottonCandy\nComment=The GraceOS web browser\nExec=\"{}\" %U\nIcon={}\nCategories=Network;WebBrowser;\nMimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;\n",
            opts.dir.join(APP_EXE).display(),
            opts.dir.join("cottoncandy.svg").display()
        );
        if opts.start_menu {
            std::fs::write(apps.join("cottoncandy-rs.desktop"), &entry)
                .map_err(|e| e.to_string())?;
        }
        if opts.desktop {
            let desk = home.join("Desktop");
            if desk.is_dir() {
                let _ = std::fs::write(desk.join("cottoncandy-rs.desktop"), &entry);
            }
        }
        Ok(())
    }
}

pub fn unregister(manifest: &serde_json::Value) {
    #[cfg(windows)]
    win::unregister(manifest);
    #[cfg(not(windows))]
    {
        let _ = manifest;
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let _ = std::fs::remove_file(home.join(".local/share/applications/cottoncandy-rs.desktop"));
        let _ = std::fs::remove_file(home.join("Desktop/cottoncandy-rs.desktop"));
    }
}

/// On Windows a running program can't delete its own file, so the uninstaller copies itself to the
/// temp folder and runs from there. Returns true when this process should exit now.
pub fn hand_off_uninstall(target: &Path, extra: &[String]) -> bool {
    #[cfg(windows)]
    {
        let Ok(me) = std::env::current_exe() else {
            return false;
        };
        let temp =
            std::env::temp_dir().join(format!("cottoncandy-uninstall-{}.exe", std::process::id()));
        if std::fs::copy(&me, &temp).is_err() {
            return false;
        }
        let mut cmd = std::process::Command::new(&temp);
        cmd.arg("--uninstall")
            .arg("--from-temp")
            .arg("--target")
            .arg(target)
            .args(extra);
        cmd.spawn().is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = (target, extra);
        false
    }
}

/// After uninstalling from the temp folder: delete this temporary copy once it has exited.
pub fn delete_self_later() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if let Ok(me) = std::env::current_exe() {
            let _ = std::process::Command::new("cmd")
                .args([
                    "/C",
                    "ping",
                    "-n",
                    "3",
                    "127.0.0.1",
                    ">NUL",
                    "&",
                    "del",
                    "/F",
                    "/Q",
                ])
                .arg(&me)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }
    }
}
