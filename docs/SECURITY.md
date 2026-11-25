# CottonCandy security

This covers the Rust browser in `rust/` (Chromium through CEF), which runs on Windows and Linux. It describes
what the browser protects, where the trust boundaries are, how each one is enforced, and what the audits
found. The Python browser (`cottoncandy`) has the same Grace and Shields design, on QtWebEngine.

To report a security problem, open a private security advisory on GitHub
(Security tab, "Report a vulnerability") rather than a public issue.

## What we protect

- **Your data**: history, bookmarks, settings, cookies and saved passwords. They live in the profile folder
  (`~/.local/share/cottoncandy`, `%LOCALAPPDATA%\CottonCandy`), which is created readable only by you.
- **The browser's own powers**: CottonCandy's pages (the toolbar, Settings, history and so on) can change
  settings, open windows, install extensions and run CherryCola. A website must never be able to use them.
- **What you browse**: from trackers (Shields), from the network (https upgrade, secure DNS, proxy) and from
  Grace. Page text sent to the model never leaves the computer.

## Trust boundaries and how they hold

### Websites and CottonCandy's own pages

CottonCandy's pages are served from the `cottoncandy://` scheme by `app/src/scheme.rs`, and they reach the
browser through `cottoncandy://<page>/api/<call>`.

- **The token.** A call is accepted only when all of these hold:
  - it is a POST;
  - it carries this session's random token (24 bytes from the OS random source, compared in constant time);
  - it comes from a frame that is one of our own pages, and that page's host matches the API host;
  - the call is on that page's allowlist in `api::allowed`. For example, the history page can read history but
    can't change settings, and the Grace panel can't install extensions.
- **The token stays out of reach.** It appears only in the HTML of our own pages. A website can't read those
  pages, because:
  - `cottoncandy://` is a separate origin;
  - every request for one of our pages or files from a web page is refused, scripts, images and frames
    included (see "History sniffing" under audit findings).
- **Navigation is vetted.** Web pages can't navigate to our pages or open them in a frame
  (`omnibox::navigation_decision`). Only what you type, the toolbar and our own pages can go there.
- **No scripts from elsewhere.** Each of our pages has a strict Content Security Policy:
  `default-src 'none'; script-src 'self'`. That means no inline scripts and no remote code. Everything shown
  from outside (page titles, URLs, Grace's answers) is escaped or set as text.

### Shields

`app/src/shields.rs` checks every request a tab makes on CEF's network thread, before it is sent.

- **Blocking.** Ads and trackers are cancelled using Brave's `adblock` engine, which reads EasyList,
  EasyPrivacy and the uBlock lists.
- **Filter lists.** They are downloaded through the browser's own proxy and DNS settings. A download is used
  only if it looks like a filter list and is under 16 MB. A failed download keeps the last good copy.
- **No preconnecting.** Chromium's preconnect and prefetch are turned off
  (`net.network_prediction_options = 2`). Otherwise Chromium opens connections to hosts, such as ad servers,
  without a request that Shields can see.
- **https upgrade.** Plain http pages are moved to https, falling back to http once per host if https fails.
  These are left alone:
  - local and network hosts: IP addresses, names without a dot, `.local`, `.lan`, `.internal`, `.onion`;
  - addresses with a port number.
- **Tracking parameters and GPC.** Tracking parameters are removed from page addresses. `Sec-GPC: 1` and
  Do Not Track are sent.
- **Scripts.** Blocking scripts uses Chromium's content settings, set for the exact origin including its port,
  and also cancels requests for script files.
- **Third-party cookies.** Blocked through Chromium's cookie controls.

### Grace (CherryCola)

`app/src/grace.rs` runs CherryCola.

- **How it runs.** The command comes from Settings and can't contain line breaks. Its arguments are fixed.
  Everything typed and all page text goes in on stdin, never on the command line, so there is no shell and
  nothing to inject.
- **Remote model servers are refused.** If CherryCola is set to use a model server that isn't on this
  computer, Grace refuses to run, so page text never leaves the machine. Web results are the one exception:
  you turn them on yourself for a chat, and only your question is searched.
- **Page text is marked as data.** It goes to the model between markers, with instructions to treat it only as
  data and repeat the task after it (`core::grace::page_prompt`). This lowers the risk of a page steering
  Grace, but doesn't remove it, and the panel says so.
- **Answers can't act.** They are shown as plain text. Only the source links of a web-results answer are
  clickable. Each must be a plain `http(s)` URL with no quotes, spaces or control characters, and it opens in
  a new tab.
- **Limits.** Output is capped at 128 KB. Every run has a time limit, and Stop ends it.

### Extensions

`core/src/extensions.rs` installs extensions.

- **Chrome Web Store packages.** These are CRX3 files. The SHA-256 signature is checked against the key the
  package carries, and the extension id is checked against that key. The file is capped at 256 MB.
- **Unpacking.** It happens in a staging folder. Every path must stay inside it: absolute paths, `..` and
  links are refused. The file count and unpacked size are capped. The manifest must be Manifest V3.
- **Firefox add-ons.** These come from addons.mozilla.org over https and are converted (best effort) to
  Chromium's format.

### Network settings

- **Proxy and DNS.** Proxy, PAC and secure DNS settings are checked before use: a valid host and port, a PAC
  address over https or from a local file, and a DNS template that is a valid https address with no spaces. Private
  windows use the same settings.
- **WebRTC.** WebRTC follows the IP policy you choose, which by default keeps your local address hidden behind
  a proxy or VPN.

### Downloads and handing off to other apps

- **Other apps.** Only `mailto:` and `tel:` links are handed to the system, and only when you click them.
  Other schemes are refused.
- **Opening a download.** The finished download opens with its default app, and only after you click it.
  This is like other browsers. Be careful with programs you downloaded.

### The renderer sandbox

Chromium's sandbox is on. `CC_NO_SANDBOX=1` turns it off and exists only for test machines that can't run it;
never set it day to day.

`CC_DEBUG_PORT` opens Chromium's remote debugging port on localhost, which gives any program on the computer
full control of the browser. It exists for automated tests only.

## Audits

These run in CI on every push (`.github/workflows/rust.yml` and `check.yml`).

| Check | What it covers | Result (2026-11-25) |
|---|---|---|
| `cargo audit` | RustSec advisories for all 193 crates | No advisories |
| `cargo deny check` | Advisories, yanked crates, licenses, crate sources (`rust/deny.toml`) | Pass |
| `cargo clippy --all-targets` | Lints, warnings as errors in CI | Pass |
| `bandit` | Python browser static analysis | 2 low (fixed-argument `cherrycola ask --help`, no shell); 0 medium or high |
| `pip-audit` | Python dependencies | Not applicable: the Python browser uses only the standard library and system PySide6/Qt packages, which the distribution patches |

The licenses that `cargo deny` allows are permissive or weak copyleft:

- Apache-2.0, MIT, BSD-3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, CC0-1.0;
- `bzip2-1.0.6` and CDLA-Permissive-2.0, which come only from the build-time CEF downloader and never ship
  in the browser.

### Found and fixed in this audit

- **History sniffing through our pages.** A website could load `cottoncandy://newtab/favicon?url=…` as an
  image. Whether it loaded told the site whether a favicon for that address was cached, which is a sign you
  had visited. Now our scheme serves nothing to web pages. Opening one of our pages as a page is still
  allowed, because navigation is already vetted.
- **Connections Shields couldn't see.** Chromium's preconnect opened connections to ad and tracker hosts
  that a page linked to, even though the requests themselves were blocked. Network prediction is now off.

### Known limits

- **Cosmetic filtering needs JavaScript.** On a site where you block scripts, the ad boxes stay visible (the
  ads themselves are still blocked).
- **Filter lists are trusted.** A list can only block requests or hide parts of a page. The engine can't
  run code from a list.
- **Grace is a language model.** A page can try to mislead it. Check what matters.
- **CEF updates.** Chromium security fixes reach CottonCandy when the pinned CEF version (`rust/app/Cargo.toml`)
  is updated. Update it with each stable Chromium release.
