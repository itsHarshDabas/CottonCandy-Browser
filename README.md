# CottonCandy

The web browser of [GraceOS](https://github.com/itsHarshDabas/GraceOS). It's Chromium-based: Qt 6 + QtWebEngine through PySide6 ([ADR 0007](https://github.com/itsHarshDabas/GraceOS/blob/main/docs/decisions/0007-cottoncandy-browser-engine.md)).

Like CandySole, it's one Python file (`cottoncandy`) with a `.desktop` file, an icon and tests next to it.

## Screenshots
These are the real app, captured from test runs under Xvfb.

| Dark | Light (GraceOS Dark Style off, pink accent) |
|---|---|
| ![New tab, dark](docs/screenshots/newtab-dark.png) | ![New tab, light](docs/screenshots/newtab-light.png) |
| ![Address suggestions](docs/screenshots/suggestions.png) | ![Error page, light](docs/screenshots/error-light.png) |
| ![Permission card](docs/screenshots/permission.png) | ![Downloads](docs/screenshots/downloads.png) |
| ![Find in page](docs/screenshots/find.png) | ![Many tabs](docs/screenshots/tabs-crowded.png) |

The design behind it:
- Spec: [docs/design/2026-10-20-cottoncandy-ui.md](docs/design/2026-10-20-cottoncandy-ui.md). §7 tracks what's built.
- Interactive mockup: [docs/design/mockups/cottoncandy-ui.html](docs/design/mockups/cottoncandy-ui.html).
- Contrast check: `python3 docs/design/contrast.py`.

## Features
- **Window:** it draws its own frame, with traffic lights on the left (close, minimize, maximize). Drag the empty part of the tab row to move it; double-click there to maximize.
- **Tabs**
  - New, close, move and cycle through them. Ctrl+Shift+T reopens a closed tab.
  - States: a loading spinner, the site's favicon (remembered between visits), a speaker or mute button while a tab plays audio, and "Page crashed".
  - Right-click a tab to pin, mute, duplicate or close others. Pinned tabs show only their icon.
  - When titles don't fit, inactive tabs shrink to their icon. The ⌄ button lists every tab.
  - Links that open a new window open as a tab instead.
  - Ctrl+click or middle-click opens a link in a background tab.
- **Address bar**
  - It takes web addresses, `localhost:8000` and file paths. Anything else is searched on DuckDuckGo.
  - It shows the host in full color and the rest of the URL dimmed, with a lock, an "info" icon for local servers, or **Not secure** for plain http.
  - Suggestions are grouped (open tabs, bookmarks, history, search) and come only from this computer. Nothing is sent anywhere while you type.
- **Navigation:** back, forward, reload or stop, find in page, zoom, full screen, and Chromium DevTools (F12).
- **Downloads**
  - Files go to `~/Downloads`.
  - A file name can't escape that folder or start with a dot: `../../.evil.sh` arrives as `evil.sh`. Same-named files get `(1)`.
  - A ring on the toolbar button shows progress.
  - The panel shows speed and time left, and can pause, resume, cancel, retry and "Show in folder".
- **History and bookmarks** are kept in SQLite at `~/.local/share/cottoncandy/cottoncandy.db` (mode 600). The History and Bookmarks windows can search, open and remove entries, and clear history.
- **Private windows** (`--private`, Ctrl+Shift+N) keep cookies and cache in memory only and save no history.
- **Default browser:** `cottoncandy --set-default` (or the menu item) makes it the default for `http`, `https` and HTML in `~/.config/mimeapps.list`, and leaves the rest of that file alone.
- **One running copy:** a second `cottoncandy URL`, from `xdg-open` for example, opens a tab in the running browser over a user-only socket in `$XDG_RUNTIME_DIR`.
- **New tab page and error pages** are CottonCandy's own.
  - The new tab page has a search field, your most visited sites and recently closed tabs.
  - Error pages explain what happened, for example offline, an unknown host or a bad certificate.
  - These pages run no scripts and load nothing from the network.
- **Grace look:** dark and light themes from the GraceOS design tokens, the shell's line icons, and the accent.
  - It follows `style` and `accent` in `~/.config/grace/desktop.conf` live: change them in Grace Settings and every window re-themes.
  - Sites that support `prefers-color-scheme` follow GraceOS too.

## Security
Page content is untrusted. These defaults aren't settings you can change:
- **Chromium's sandbox is always on.** CottonCandy refuses to run as root instead of turning it off, and it ignores Chromium switches on its command line, so `cottoncandy --no-sandbox` is an error.
- **Bad certificates are rejected,** with no click-through.
- **Only http, https, file, about, data and blob load in pages.** A clicked `mailto:` goes to your mail app. Every other scheme (`steam:`, `ms-msdt:` and so on) is refused, never passed to `xdg-open`.
- **Pop-ups need a click.**
- **Permissions:**
  - A site can use the camera, microphone, screen, location, notifications, clipboard or pointer lock only after you click **Allow once** on the card under the site icon.
  - Enter and Esc both mean **Deny**. The card stays until it's answered.
  - The answer lasts for that tab; other tabs ask again. Nothing is stored.
  - Anything else a site asks for is denied without asking.
- **Downloads are never opened by the browser,** only shown in their folder.

## Keyboard
| Keys | Action |
|---|---|
| Ctrl+T / Ctrl+W / Ctrl+Shift+T | New tab / close tab / reopen closed tab |
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
| Esc | Close the open popover (on a permission card: Deny) |
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

The browser itself is checked by hand. On 2026-10-21 the current version was run as a normal user, with Xvfb and openbox, against a local test site. The runs were driven by xdotool and used PySide6 6.12.0 from PyPI, not Arch's 6.11. What was seen:
- **Startup:** the new tab page opens with the address bar focused. Typing an address loads it.
- **Suggestions:** typing "grace" shows the History and Search groups, and ↓ then Enter opens the history entry.
- **Permission card:**
  - Enter denies. Shift+Tab then Space on **Allow once** grants, and so does a click.
  - After an answer, reloading the same tab keeps it; a new tab asks again.
- **Schemes:** a clicked `mailto:` reached `xdg-open`, and a clicked `steam://` was refused.
- **Error page:** `127.0.0.1:9` showed CottonCandy's own error page ("Can't reach 127.0.0.1:9", `net::ERR_UNSAFE_PORT`). Back returned to the previous page, and the failed address wasn't added to history.
- **Downloads:** `../../.evil.sh` was saved as `evil.sh`, then as `evil (1).sh` the second time.
- **Favicons:** the site's favicon was stored, and appears on the new tab page.
- **Live theme:** setting `style = light` and `accent = pink` in `desktop.conf` re-themed the open window and its pages.
- **Many tabs:** with ten tabs, inactive ones collapse to icons.
- **Second launch:** a second `cottoncandy URL` opened a tab in the running browser.
- **Sandbox:** `chrome://sandbox` says "You are adequately sandboxed" (namespaces + seccomp-BPF).
- **Quit:** Ctrl+Q quits with status 0.

Not checked yet: Wayland/Hyprland (including how the frameless window and its traffic lights behave there), video codecs, camera and microphone (the test machine had none), audio indicators, and Arch's own packages.

![chrome://sandbox and the find bar](docs/screenshots/sandbox-and-find.png)
