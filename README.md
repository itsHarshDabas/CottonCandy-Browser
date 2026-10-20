# CottonCandy

The web browser of [GraceOS](https://github.com/itsHarshDabas/GraceOS). It's Chromium-based: Qt 6 + QtWebEngine through PySide6 ([ADR 0007](https://github.com/itsHarshDabas/GraceOS/blob/main/docs/decisions/0007-cottoncandy-browser-engine.md)).

Like CandySole, it's one Python file (`cottoncandy`) with a `.desktop` file, an icon and tests next to it.

![chrome://sandbox and find in page](docs/screenshots/sandbox-and-find.png)

## Features
- **Tabs**
  - New, close, move and cycle through them.
  - Links that open a new window open as a tab instead.
  - Ctrl+click or middle-click opens a link in a background tab.
- **Address bar**
  - It takes web addresses, `localhost:8000` and file paths. Anything else is searched on DuckDuckGo.
  - Suggestions come from your bookmarks and history.
  - Nothing is sent anywhere while you type.
- **Navigation:** back, forward, reload or stop, find in page, zoom, full screen, and Chromium DevTools (F12).
- **Downloads**
  - Files go to `~/Downloads`.
  - A file name can't escape that folder or start with a dot: `../../.evil.sh` arrives as `evil.sh`. Same-named files get `(1)`.
  - The panel shows progress, can cancel, and has "Show in folder".
- **History and bookmarks** are kept in SQLite at `~/.local/share/cottoncandy/cottoncandy.db` (mode 600). The History and Bookmarks windows can search, open and remove entries, and clear history.
- **Private windows** (`--private`, Ctrl+Shift+N) keep cookies and cache in memory only and save no history.
- **Default browser:** `cottoncandy --set-default` (or the menu item) makes it the default for `http`, `https` and HTML in `~/.config/mimeapps.list`, and leaves the rest of that file alone.
- **One running copy:** a second `cottoncandy URL`, from `xdg-open` for example, opens a tab in the running browser over a user-only socket in `$XDG_RUNTIME_DIR`.
- **Grace colors:** it uses the Grace dark theme, the shell's line icons, and the accent from `~/.config/grace/desktop.conf`.

## Security
Page content is untrusted. These defaults aren't settings you can change:
- **Chromium's sandbox is always on.** CottonCandy refuses to run as root instead of turning it off, and it ignores Chromium switches on its command line, so `cottoncandy --no-sandbox` is an error.
- **Bad certificates are rejected,** with no click-through.
- **Only http, https, file, about, data and blob load in pages.** A clicked `mailto:` goes to your mail app. Every other scheme (`steam:`, `ms-msdt:` and so on) is refused, never passed to `xdg-open`.
- **Pop-ups need a click.**
- **Permissions:**
  - A site can only use the camera, microphone, screen, location, notifications, clipboard or pointer lock after you click **Allow**. Enter and Esc both mean **Block**.
  - Answers last until CottonCandy quits.
  - Anything else a site asks for is denied without asking.
- **Downloads are never opened by the browser,** only shown in their folder.

## Keyboard
| Keys | Action |
|---|---|
| Ctrl+T / Ctrl+W | New tab / close tab |
| Ctrl+N / Ctrl+Shift+N | New window / new private window |
| Ctrl+L, Alt+D, F6 | Focus the address bar |
| Alt+Left / Alt+Right | Back / forward |
| Ctrl+R, F5 | Reload |
| Ctrl+Tab, Ctrl+PgDown / Ctrl+Shift+Tab, Ctrl+PgUp | Next / previous tab |
| Ctrl+D | Bookmark or unbookmark the page |
| Ctrl+H / Ctrl+Shift+O / Ctrl+Shift+Y | History / bookmarks / downloads |
| Ctrl+F | Find in page (Enter next, Shift+Enter previous, Esc close) |
| Ctrl+= / Ctrl+- / Ctrl+0 | Zoom in / out / reset |
| F11 / F12 | Full screen / developer tools |
| Ctrl+Q | Quit |

## Install
On Arch, use `packaging/arch/PKGBUILD`. It depends on `python pyside6 qt6-webengine qt6-svg`. On other systems:

```sh
sudo make install            # PREFIX=/usr by default; DESTDIR works for packaging
cottoncandy --set-default    # optional
```

## Test
```sh
make check   # unit tests (no Qt needed) + desktop-file-validate
```

The browser itself is checked by hand. On 2026-10-20 it was run as a normal user, with Xvfb and openbox, against a local test site. That run used PySide6 6.12.0 from PyPI, not Arch's 6.11. What was seen:
- Pages load.
- In the address bar:
  - A typed address loads, and so does a suggestion picked with ↓ and Enter or with the mouse. Each loads exactly once.
  - Words are searched.
- A second `cottoncandy URL` opens a tab in the running browser.
- The automatic `window.open` and `steam://` with no click were blocked. The pop-up after a click opened as a tab.
- A clicked `mailto:` reached `xdg-open`, and a clicked `steam://` was refused.
- Enter on the notification prompt gave `denied`.
- A download named `../../.evil.sh` was saved as `~/Downloads/evil.sh`.
- History and bookmarks were written to SQLite.
- `chrome://sandbox` said "You are adequately sandboxed" (namespaces + seccomp-BPF).
- Ctrl+Q quit with status 0.

Not checked yet: Wayland/Hyprland, video codecs, camera and microphone (the test machine had none), and Arch's own packages.
