//! History, bookmarks and the last session, in SQLite (`cottoncandy.db`, readable by this user only).

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

pub struct Store {
    db: Connection,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub id: i64,
    pub url: String,
    pub title: String,
    /// Unix seconds.
    pub time: i64,
    pub visits: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    /// tab, bookmark, history or search
    pub kind: &'static str,
    pub url: String,
    pub title: String,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS history (
    id INTEGER PRIMARY KEY, url TEXT NOT NULL UNIQUE, title TEXT NOT NULL DEFAULT '',
    last_visit INTEGER NOT NULL, visits INTEGER NOT NULL DEFAULT 1);
CREATE INDEX IF NOT EXISTS history_last ON history(last_visit DESC);
CREATE TABLE IF NOT EXISTS bookmarks (
    id INTEGER PRIMARY KEY, url TEXT NOT NULL UNIQUE, title TEXT NOT NULL DEFAULT '',
    folder TEXT NOT NULL DEFAULT '', added INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS session (position INTEGER PRIMARY KEY, url TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS favicons (host TEXT PRIMARY KEY, png BLOB NOT NULL, updated INTEGER NOT NULL);
";

/// Only web and file pages are remembered.
fn rememberable(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://") || url.starts_with("file://"))
        && url.len() <= 8192
}

/// Tabs worth reopening: web pages, plus CottonCandy's own pages except the error and new tab pages.
fn restorable(url: &str) -> bool {
    rememberable(url)
        || (url.starts_with("cottoncandy://")
            && url.len() <= 512
            && !url.starts_with("cottoncandy://error")
            && !url.starts_with("cottoncandy://newtab")
            && !url.starts_with("cottoncandy://ui"))
}

fn clip(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Store> {
        if let Some(dir) = path.parent() {
            let _ = crate::paths::ensure_private_dir(dir);
        }
        let db = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Store::init(db)
    }

    pub fn in_memory() -> rusqlite::Result<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> rusqlite::Result<Store> {
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA secure_delete=ON;",
        )?;
        db.execute_batch(SCHEMA)?;
        Ok(Store { db })
    }

    pub fn add_visit(&self, url: &str, title: &str, now: i64) -> rusqlite::Result<()> {
        if !rememberable(url) {
            return Ok(());
        }
        self.db.execute(
            "INSERT INTO history (url, title, last_visit) VALUES (?1, ?2, ?3)
             ON CONFLICT(url) DO UPDATE SET visits = visits + 1, last_visit = ?3,
             title = CASE WHEN ?2 = '' THEN title ELSE ?2 END",
            params![url, clip(title, 500), now],
        )?;
        Ok(())
    }

    pub fn set_title(&self, url: &str, title: &str) -> rusqlite::Result<()> {
        self.db.execute(
            "UPDATE history SET title = ?2 WHERE url = ?1",
            params![url, clip(title, 500)],
        )?;
        Ok(())
    }

    pub fn history(&self, query: &str, limit: usize) -> rusqlite::Result<Vec<Entry>> {
        let like = format!(
            "%{}%",
            query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = self.db.prepare(
            "SELECT id, url, title, last_visit, visits FROM history
             WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\' ORDER BY last_visit DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![like, limit as i64], |r| {
            Ok(Entry {
                id: r.get(0)?,
                url: r.get(1)?,
                title: r.get(2)?,
                time: r.get(3)?,
                visits: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn delete_history(&self, id: i64) -> rusqlite::Result<()> {
        self.db
            .execute("DELETE FROM history WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Deletes history since `since` (unix seconds; 0 for all of it).
    pub fn clear_history(&self, since: i64) -> rusqlite::Result<()> {
        self.db
            .execute("DELETE FROM history WHERE last_visit >= ?1", params![since])?;
        self.db
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        Ok(())
    }

    pub fn most_visited(&self, limit: usize) -> rusqlite::Result<Vec<Entry>> {
        let mut stmt = self.db.prepare(
            "SELECT id, url, title, last_visit, visits FROM history ORDER BY visits DESC, last_visit DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(Entry {
                id: r.get(0)?,
                url: r.get(1)?,
                title: r.get(2)?,
                time: r.get(3)?,
                visits: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn add_bookmark(
        &self,
        url: &str,
        title: &str,
        folder: &str,
        now: i64,
    ) -> rusqlite::Result<bool> {
        if !rememberable(url) {
            return Ok(false);
        }
        let n = self.db.execute(
            "INSERT INTO bookmarks (url, title, folder, added) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(url) DO UPDATE SET title = ?2, folder = ?3",
            params![url, clip(title, 500), clip(folder, 100), now],
        )?;
        Ok(n > 0)
    }

    pub fn remove_bookmark(&self, url: &str) -> rusqlite::Result<()> {
        self.db
            .execute("DELETE FROM bookmarks WHERE url = ?1", params![url])?;
        Ok(())
    }

    pub fn is_bookmarked(&self, url: &str) -> rusqlite::Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM bookmarks WHERE url = ?1",
                params![url],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Every bookmark as (url, title, folder), for exporting.
    pub fn all_bookmarks(&self) -> rusqlite::Result<Vec<(String, String, String)>> {
        let mut stmt = self
            .db
            .prepare("SELECT url, title, folder FROM bookmarks ORDER BY folder, added")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    pub fn bookmarks(&self, query: &str) -> rusqlite::Result<Vec<Entry>> {
        let like = format!(
            "%{}%",
            query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = self.db.prepare(
            "SELECT id, url, title, added, 0 FROM bookmarks WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
             ORDER BY folder, title COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![like], |r| {
            Ok(Entry {
                id: r.get(0)?,
                url: r.get(1)?,
                title: r.get(2)?,
                time: r.get(3)?,
                visits: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Address-bar suggestions from this computer only: bookmarks, then history, then a search.
    pub fn suggest(&self, typed: &str, limit: usize) -> rusqlite::Result<Vec<Suggestion>> {
        let typed = typed.trim();
        let mut out: Vec<Suggestion> = Vec::new();
        if typed.is_empty() {
            return Ok(out);
        }
        for b in self.bookmarks(typed)?.into_iter().take(3) {
            out.push(Suggestion {
                kind: "bookmark",
                url: b.url,
                title: b.title,
            });
        }
        let like = format!(
            "%{}%",
            typed
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = self.db.prepare(
            "SELECT url, title FROM history WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
             ORDER BY (url LIKE ?2 ESCAPE '\\') DESC, visits DESC, last_visit DESC LIMIT ?3",
        )?;
        let prefix = format!(
            "%://{}%",
            typed
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let rows = stmt.query_map(params![like, prefix, limit as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (url, title) = row?;
            if out.len() >= limit {
                break;
            }
            if !out.iter().any(|s| s.url == url) {
                out.push(Suggestion {
                    kind: "history",
                    url,
                    title,
                });
            }
        }
        Ok(out)
    }

    pub fn save_session(&self, tabs: &[(String, bool)]) -> rusqlite::Result<()> {
        let tx = self.db.unchecked_transaction()?;
        tx.execute("DELETE FROM session", [])?;
        for (i, (url, pinned)) in tabs.iter().enumerate().take(500) {
            if restorable(url) {
                tx.execute(
                    "INSERT INTO session (position, url, pinned) VALUES (?1, ?2, ?3)",
                    params![i as i64, url, *pinned as i64],
                )?;
            }
        }
        tx.commit()
    }

    pub fn last_session(&self) -> rusqlite::Result<Vec<(String, bool)>> {
        let mut stmt = self
            .db
            .prepare("SELECT url, pinned FROM session ORDER BY position")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0))
        })?;
        rows.collect()
    }

    pub fn set_favicon(&self, host: &str, png: &[u8], now: i64) -> rusqlite::Result<()> {
        if png.len() > 64 * 1024 || host.is_empty() {
            return Ok(());
        }
        self.db.execute(
            "INSERT INTO favicons (host, png, updated) VALUES (?1, ?2, ?3) ON CONFLICT(host) DO UPDATE SET png = ?2, updated = ?3",
            params![host, png, now],
        )?;
        Ok(())
    }

    pub fn favicon(&self, host: &str) -> rusqlite::Result<Option<Vec<u8>>> {
        self.db
            .query_row(
                "SELECT png FROM favicons WHERE host = ?1",
                params![host],
                |r| r.get(0),
            )
            .optional()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_and_suggestions() {
        let s = Store::in_memory().unwrap();
        s.add_visit("https://rust-lang.org/", "Rust", 10).unwrap();
        s.add_visit("https://rust-lang.org/", "", 20).unwrap();
        s.add_visit("https://docs.rs/serde", "serde - Rust", 15)
            .unwrap();
        s.add_visit("cottoncandy://settings", "Settings", 30)
            .unwrap();
        s.add_visit("javascript:alert(1)", "x", 30).unwrap();
        let h = s.history("", 10).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(
            (h[0].title.as_str(), h[0].visits, h[0].time),
            ("Rust", 2, 20)
        );
        assert_eq!(s.history("100%", 10).unwrap().len(), 0);
        s.add_bookmark("https://docs.rs/serde", "serde docs", "", 1)
            .unwrap();
        let sug = s.suggest("rust", 5).unwrap();
        assert_eq!(sug[0].kind, "history");
        let sug = s.suggest("serde", 5).unwrap();
        assert_eq!((sug[0].kind, sug.len()), ("bookmark", 1));
        assert_eq!(s.most_visited(1).unwrap()[0].url, "https://rust-lang.org/");
        s.clear_history(0).unwrap();
        assert!(s.history("", 10).unwrap().is_empty());
        assert!(s.is_bookmarked("https://docs.rs/serde").unwrap());
    }

    #[test]
    fn session() {
        let s = Store::in_memory().unwrap();
        s.save_session(&[
            ("https://a.test/".into(), true),
            ("cottoncandy://newtab".into(), false),
            ("https://b.test/".into(), false),
            ("cottoncandy://settings/#network".into(), false),
            ("cottoncandy://error/?url=x".into(), false),
            ("javascript:alert(1)".into(), false),
        ])
        .unwrap();
        assert_eq!(
            s.last_session().unwrap(),
            vec![
                ("https://a.test/".into(), true),
                ("https://b.test/".into(), false),
                ("cottoncandy://settings/#network".into(), false)
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn database_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/cottoncandy.db");
        let s = Store::open(&path).unwrap();
        s.add_visit("https://a.test/", "a", 1).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
