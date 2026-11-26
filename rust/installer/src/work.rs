//! What the installer and uninstaller do, without any UI: unpack the browser that is appended to the
//! installer's own file, record what was installed, and take exactly that away again.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The last 16 bytes of a finished installer: where the payload (a zip) starts, then this magic.
pub const MAGIC: &[u8; 8] = b"CCSETUP1";
/// The record of an install, kept in the install folder.
pub const MANIFEST: &str = "install.json";

#[cfg(windows)]
pub const APP_EXE: &str = "cottoncandy.exe";
#[cfg(not(windows))]
pub const APP_EXE: &str = "cottoncandy";
#[cfg(windows)]
pub const UNINSTALL_EXE: &str = "uninstall.exe";
#[cfg(not(windows))]
pub const UNINSTALL_EXE: &str = "uninstall";

/// How far a job has got, shared between the worker thread and the window.
#[derive(Default)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub file: String,
    pub finished: Option<Result<(), String>>,
}

pub type Shared = Arc<Mutex<Progress>>;

#[derive(Clone, Debug)]
pub struct Options {
    pub dir: PathBuf,
    pub desktop: bool,
    pub start_menu: bool,
}

/// Where the browser goes unless the user picks another folder.
pub fn default_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        base.join("Programs").join("CottonCandy")
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        home.join(".local").join("opt").join("cottoncandy")
    }
}

/// A folder the user picked: the browser goes into a "CottonCandy" folder inside it, so picking
/// "Documents" never spreads files around Documents (and uninstalling never touches its other files).
pub fn install_dir_in(picked: &Path) -> PathBuf {
    let name = picked
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name == "cottoncandy" {
        picked.to_path_buf()
    } else {
        picked.join("CottonCandy")
    }
}

/// The payload appended to this program, as (offset, length), or None for a plain uninstaller.
pub fn payload_range(exe: &mut File) -> io::Result<Option<(u64, u64)>> {
    let len = exe.seek(SeekFrom::End(0))?;
    if len < 16 {
        return Ok(None);
    }
    exe.seek(SeekFrom::End(-16))?;
    let mut tail = [0u8; 16];
    exe.read_exact(&mut tail)?;
    if &tail[8..] != MAGIC {
        return Ok(None);
    }
    let start = u64::from_le_bytes(tail[..8].try_into().expect("8 bytes"));
    if start >= len - 16 {
        return Ok(None);
    }
    Ok(Some((start, len - 16 - start)))
}

/// How big the payload unpacks to, for the "needs about" line. 0 when there is none.
pub fn payload_size() -> u64 {
    let Ok(exe) = std::env::current_exe() else {
        return 0;
    };
    let Ok(mut f) = File::open(exe) else { return 0 };
    let Ok(Some((start, len))) = payload_range(&mut f) else {
        return 0;
    };
    let Ok(mut zip) = zip::ZipArchive::new(Slice::new(f, start, len)) else {
        return 0;
    };
    (0..zip.len())
        .filter_map(|i| zip.by_index_raw(i).ok().map(|e| e.size()))
        .sum()
}

/// A window onto part of a file, so the zip reader sees only the payload.
struct Slice {
    file: File,
    start: u64,
    len: u64,
    pos: u64,
}

impl Slice {
    fn new(file: File, start: u64, len: u64) -> Slice {
        Slice {
            file,
            start,
            len,
            pos: 0,
        }
    }
}

impl Read for Slice {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.len.saturating_sub(self.pos);
        let n = (buf.len() as u64).min(left) as usize;
        if n == 0 {
            return Ok(0);
        }
        self.file.seek(SeekFrom::Start(self.start + self.pos))?;
        let got = self.file.read(&mut buf[..n])?;
        self.pos += got as u64;
        Ok(got)
    }
}

impl Seek for Slice {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.len as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if pos < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.pos = pos as u64;
        Ok(self.pos)
    }
}

fn set(progress: &Shared, f: impl FnOnce(&mut Progress)) {
    if let Ok(mut p) = progress.lock() {
        f(&mut p);
    }
}

/// Whether CottonCandy is running from `dir`: a running program's file can't be renamed on Windows.
fn running(dir: &Path) -> bool {
    let exe = dir.join(APP_EXE);
    if !exe.exists() || !cfg!(windows) {
        return false;
    }
    let probe = dir.join(format!("{APP_EXE}.check"));
    match fs::rename(&exe, &probe) {
        Ok(()) => {
            let _ = fs::rename(&probe, &exe);
            false
        }
        Err(_) => true,
    }
}

/// Whether this process can write into `dir` (creating it if needed).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".cottoncandy-write-test");
    let ok = File::create(&probe).is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

pub fn install(opts: &Options, progress: &Shared) -> Result<(), String> {
    // A fresh install that fails part way is taken back out, so nothing is left half-installed. (An
    // update over an older version keeps what is there: its files may already be replaced.)
    let fresh = !opts.dir.join(MANIFEST).exists();
    let existed = opts.dir.exists();
    let mut written = Vec::new();
    let r = install_files(opts, progress, &mut written);
    if r.is_err() && fresh {
        for f in written.iter().rev() {
            let _ = fs::remove_file(f);
        }
        for f in &written {
            let mut d = f.parent();
            while let Some(p) = d.filter(|p| p.starts_with(&opts.dir)) {
                if fs::remove_dir(p).is_err() {
                    break;
                }
                d = p.parent();
            }
        }
        crate::platform::unregister(&serde_json::json!({
            "desktop": opts.desktop,
            "start_menu": opts.start_menu,
        }));
        if !existed {
            let _ = fs::remove_dir(&opts.dir);
        }
    }
    r
}

fn install_files(
    opts: &Options,
    progress: &Shared,
    written: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut exe = File::open(&exe_path).map_err(|e| e.to_string())?;
    let (start, len) = payload_range(&mut exe)
        .map_err(|e| e.to_string())?
        .ok_or("This installer is missing its files. Download it again.")?;
    let dir = &opts.dir;
    fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    if running(dir) {
        return Err("CottonCandy is open. Close it, then try again.".into());
    }
    let mut zip = zip::ZipArchive::new(Slice::new(
        exe.try_clone().map_err(|e| e.to_string())?,
        start,
        len,
    ))
    .map_err(|e| format!("The installer's files are damaged: {e}"))?;
    let total: u64 = (0..zip.len())
        .filter_map(|i| zip.by_index_raw(i).ok().map(|e| e.size()))
        .sum();
    set(progress, |p| p.total = total);
    let mut files: Vec<String> = Vec::new();
    let mut done = 0u64;
    let mut buf = vec![0u8; 256 * 1024];
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        // Only plain paths inside the install folder: no "..", no absolute paths.
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            continue;
        }
        let out = dir.join(&rel);
        let mkdir = |d: &Path| {
            fs::create_dir_all(d).map_err(|e| format!("Couldn't create {}: {e}", d.display()))
        };
        if entry.is_dir() {
            mkdir(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            mkdir(parent)?;
        }
        let name = rel.to_string_lossy().replace('\\', "/");
        set(progress, |p| p.file = name.clone());
        let mut f =
            File::create(&out).map_err(|e| format!("Couldn't write {}: {e}", out.display()))?;
        written.push(out.clone());
        loop {
            let n = entry.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            f.write_all(&buf[..n])
                .map_err(|e| format!("Couldn't write {}: {e}", out.display()))?;
            done += n as u64;
            set(progress, |p| p.done = done);
        }
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode & 0o755));
        }
        files.push(name);
    }
    // The uninstaller is this program without the payload.
    let uninstaller = dir.join(UNINSTALL_EXE);
    {
        let mut src = File::open(&exe_path).map_err(|e| e.to_string())?;
        let mut dst = File::create(&uninstaller).map_err(|e| e.to_string())?;
        written.push(uninstaller.clone());
        io::copy(&mut (&mut src).take(start), &mut dst).map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&uninstaller, fs::Permissions::from_mode(0o755));
    }
    let manifest = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "files": files,
        "desktop": opts.desktop,
        "start_menu": opts.start_menu,
    });
    written.push(dir.join(MANIFEST));
    fs::write(dir.join(MANIFEST), manifest.to_string()).map_err(|e| e.to_string())?;
    crate::platform::register(opts, total / 1024).map_err(|e| format!("Installed, but: {e}"))?;
    set(progress, |p| p.file.clear());
    Ok(())
}

/// Removes exactly what `install` put in `dir`, the shortcuts and the Apps entry, and optionally the
/// browsing data. Files the user added to the folder stay.
pub fn uninstall(dir: &Path, remove_data: bool, progress: &Shared) -> Result<(), String> {
    let text = fs::read_to_string(dir.join(MANIFEST))
        .map_err(|_| "This folder doesn't hold a CottonCandy install.".to_string())?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "The install record is damaged.".to_string())?;
    if running(dir) {
        return Err("CottonCandy is open. Close it, then try again.".into());
    }
    let files: Vec<PathBuf> = manifest["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f.as_str())
        .map(PathBuf::from)
        .filter(|p| p.components().all(|c| matches!(c, Component::Normal(_))))
        .collect();
    set(progress, |p| p.total = files.len() as u64 + 2);
    let mut parents: Vec<PathBuf> = Vec::new();
    for (i, rel) in files.iter().enumerate() {
        set(progress, |p| {
            p.done = i as u64;
            p.file = rel.to_string_lossy().into_owned();
        });
        let _ = fs::remove_file(dir.join(rel));
        let mut parent = rel.parent();
        while let Some(p) = parent.filter(|p| !p.as_os_str().is_empty()) {
            if !parents.contains(&p.to_path_buf()) {
                parents.push(p.to_path_buf());
            }
            parent = p.parent();
        }
    }
    // Deepest folders first; a folder goes only once it is empty.
    parents.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for p in parents {
        let _ = fs::remove_dir(dir.join(p));
    }
    crate::platform::unregister(&manifest);
    let _ = fs::remove_file(dir.join(MANIFEST));
    // The uninstall.exe that handed over to its temporary copy may still be exiting, and Windows
    // keeps a running program's file locked: try for a while.
    let uninstaller = dir.join(UNINSTALL_EXE);
    for _ in 0..40 {
        if fs::remove_file(&uninstaller).is_ok() || !uninstaller.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let _ = fs::remove_dir(dir);
    if remove_data {
        let data = crate::platform::data_dir();
        set(progress, |p| p.file = "your browsing data".into());
        let _ = fs::remove_dir_all(data);
    }
    set(progress, |p| {
        p.done = p.total;
        p.file.clear();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picked_folders_get_their_own_subfolder() {
        assert_eq!(
            install_dir_in(Path::new("/home/a/Documents")),
            Path::new("/home/a/Documents/CottonCandy")
        );
        assert_eq!(
            install_dir_in(Path::new("/opt/CottonCandy")),
            Path::new("/opt/CottonCandy")
        );
    }

    #[test]
    fn finds_the_payload_footer() {
        let dir = std::env::temp_dir().join(format!("ccsetup-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("setup");
        let mut f = File::create(&path).unwrap();
        f.write_all(b"stub-program").unwrap();
        f.write_all(b"ZIPDATA").unwrap();
        f.write_all(&12u64.to_le_bytes()).unwrap();
        f.write_all(MAGIC).unwrap();
        drop(f);
        let mut f = File::open(&path).unwrap();
        assert_eq!(payload_range(&mut f).unwrap(), Some((12, 7)));
        fs::write(&path, b"just a program").unwrap();
        let mut f = File::open(&path).unwrap();
        assert_eq!(payload_range(&mut f).unwrap(), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn uninstall_takes_only_what_it_installed() {
        let dir = std::env::temp_dir().join(format!("ccsetup-un-{}", std::process::id()));
        fs::create_dir_all(dir.join("locales")).unwrap();
        fs::write(dir.join(APP_EXE), b"x").unwrap();
        fs::write(dir.join("locales/en-US.pak"), b"x").unwrap();
        fs::write(dir.join("my-notes.txt"), b"mine").unwrap();
        fs::write(dir.join(UNINSTALL_EXE), b"x").unwrap();
        let manifest = serde_json::json!({
            "files": [APP_EXE, "locales/en-US.pak", "../../etc/passwd"],
            "desktop": false, "start_menu": false,
        });
        fs::write(dir.join(MANIFEST), manifest.to_string()).unwrap();
        let progress: Shared = Default::default();
        uninstall(&dir, false, &progress).unwrap();
        assert!(!dir.join(APP_EXE).exists());
        assert!(!dir.join("locales").exists());
        assert!(!dir.join(UNINSTALL_EXE).exists());
        assert!(
            dir.join("my-notes.txt").exists(),
            "the user's own file stays"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
