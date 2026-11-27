// cottoncandy://settings/ — every setting on one scrolling page, with a section list on the left.
// The browser owns the settings: each change goes to settings_set, which checks it, saves it and puts it into
// effect, then sends back the whole state, which this page redraws from.
(() => {
  const $ = id => document.getElementById(id);
  const esc = cc.esc;
  let state = null;
  let about = null;   // the About page's facts, fetched once
  let message = {};   // section id -> {kind, text} shown at the top of that section
  let sources = null; // browsers found on this computer, for importing (fetched once)
  let importing = false;
  let grace = null;      // what CherryCola says: installed models, the one in use, what suits this computer
  let pulling = null;    // {model, status, percent} while a model downloads
  let installing = null; // {step, percent} while CherryColaLLM installs
  let wallpaperBusy = false;

  const get = path => path.split(".").reduce((o, k) => (o == null ? undefined : o[k]), state.settings);

  async function set(path, value) {
    try { state = await cc.call("settings_set", { path, value }); }
    catch (e) { flash(path.split(".")[0], "err", e.message); }
    render();
  }

  function flash(section, kind, text) { message[section] = { kind, text }; render(); }

  // ---- row builders --------------------------------------------------------------------------------
  const lbl = (label, sub) => `<div class="lbl"><div>${esc(label)}</div>${sub ? `<div class="sub">${esc(sub)}</div>` : ""}</div>`;
  const row = (inner, extra = "") => `<div class="set ${extra}" data-text="${esc(inner.replace(/<[^>]+>/g, " ").toLowerCase())}">${inner}</div>`;

  const sw = (path, label, sub) => row(`${lbl(label, sub)}<label class="switch"><input type="checkbox" data-path="${path}" ${get(path) ? "checked" : ""} aria-label="${esc(label)}"><span></span></label>`);
  const sel = (path, label, sub, options) => row(`${lbl(label, sub)}<select class="field" data-path="${path}" aria-label="${esc(label)}">${
    options.map(([v, t]) => `<option value="${esc(v)}" ${String(get(path)) === String(v) ? "selected" : ""}>${esc(t)}</option>`).join("")}</select>`);
  const seg = (path, label, sub, options) => row(`${lbl(label, sub)}<div class="seg" role="radiogroup" aria-label="${esc(label)}">${
    options.map(([v, t]) => `<button role="radio" aria-checked="${get(path) === v}" class="${get(path) === v ? "on" : ""}" data-path="${path}" data-v="${esc(v)}">${esc(t)}</button>`).join("")}</div>`);
  const text = (path, label, sub, placeholder = "", cls = "") => row(`${lbl(label, sub)}<input class="field ${cls}" data-path="${path}" value="${esc(get(path) ?? "")}" placeholder="${esc(placeholder)}" aria-label="${esc(label)}" spellcheck="false">`);
  const num = (path, label, sub, min, max) => row(`${lbl(label, sub)}<input class="field short" type="number" min="${min}" max="${max}" data-path="${path}" data-num="1" value="${get(path)}" aria-label="${esc(label)}">`);
  const range = (path, label, sub, min, max, unit) => row(`${lbl(label, sub)}<input type="range" min="${min}" max="${max}" data-path="${path}" data-num="1" value="${get(path)}" aria-label="${esc(label)}"><span class="val">${get(path)}${unit}</span>`);
  const area = (path, label, sub, placeholder) => row(`${lbl(label, sub)}<textarea class="field" data-path="${path}" placeholder="${esc(placeholder)}" spellcheck="false" aria-label="${esc(label)}">${esc(get(path) || "")}</textarea>
    <div class="row-actions"><button class="btn" data-save="${path}">Save</button></div>`, "stack");
  const card = rows => `<div class="card">${rows.join("")}</div>`;
  const note = id => message[id] ? `<div class="note ${message[id].kind}" role="status">${esc(message[id].text)}</div>` : "";

  // ---- sections ------------------------------------------------------------------------------------
  const SECTIONS = [
    { id: "appearance", icon: "eye", title: "Appearance", lead: "How CottonCandy looks. Changes show right away.", body: () => {
      const s = state.settings.appearance, o = state.options;
      const custom = !o.accents.some(a => a.id === s.accent);
      return card([
        seg("appearance.theme", "Theme", "System follows your computer's light or dark setting", [["system", "System"], ["light", "Light"], ["dark", "Dark"]]),
        sw("appearance.dark_web_pages", "Dark web pages", "Darkens websites that don't have a dark theme of their own (after a restart)"),
        row(`${lbl("Accent color", "Buttons, switches and highlights")}<div class="swatches">${o.accents.map(a =>
          `<button class="swatch ${s.accent === a.id ? "on" : ""}" style="background:${a.color}" data-path="appearance.accent" data-v="${a.id}" aria-label="${a.id}" title="${a.id}"></button>`).join("")}
          <input type="color" data-path="appearance.accent" value="${custom ? esc(s.accent) : "#ff6ec7"}" aria-label="Custom accent color" title="Custom color"></div>`),
        seg("appearance.tab_layout", "Tabs", "", [["top", "Across the top"], ["vertical", "Down the side"]]),
        sw("appearance.collapse_tabs", "Fold side tabs", "Side tabs shrink to their icons and open while you point at them"),
        seg("appearance.density", "Density", "", [["compact", "Compact"], ["comfortable", "Comfortable"], ["spacious", "Spacious"]]),
        range("appearance.corner_radius", "Corner roundness", "", 0, 16, "px"),
        range("appearance.font_size", "Text size", "For CottonCandy's own toolbar and pages", 11, 18, "px"),
        sel("appearance.default_zoom", "Page zoom", "For pages you haven't zoomed yourself", [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200].map(z => [z, z + "%"])),
        sw("appearance.show_bookmarks_bar", "Bookmarks bar", "Ctrl+Shift+B"),
        sw("appearance.show_home_button", "Home button", ""),
        sw("appearance.translucency", "Translucent toolbar and panels", "Turn off for a flat look and a little less GPU work"),
        sw("appearance.motion", "Animations", "Turn off to reduce motion"),
        area("appearance.custom_css", "Custom CSS", "Restyles CottonCandy's own toolbar and pages (not websites). Can't load anything from the internet.", ":root { --radius: 14px; }"),
      ]);
    } },
    { id: "newtab", icon: "tabs", title: "New tab page", body: () => {
      const v = get("appearance.newtab_background");
      return card([
        row(`<div class="lbl" style="width:100%"><div style="margin-bottom:10px">Background</div><div class="bgs">${["aurora", "gradient", "mesh", "plain", "image"].map(b =>
          `<button class="bg ${v === b ? "on" : ""}" data-v="${b}" data-path="appearance.newtab_background" aria-label="${b} background">${b[0].toUpperCase() + b.slice(1)}</button>`).join("")}</div></div>`, "stack"),
        v === "image" ? row(`${lbl("Picture", get("appearance.wallpaper_path") ? "Kept in CottonCandy's own folder, so moving the original doesn't lose it" : "None yet")}
          <div class="row-actions"><button class="btn" id="wpPick" ${wallpaperBusy ? "disabled" : ""}>Choose from this computer…</button></div>`) : "",
        v === "image" ? row(`<div class="lbl" style="width:100%"><div>Or use a picture from the web</div><div class="sub">Paste a link to a PNG, JPEG, WebP, GIF or AVIF. It's downloaded once and kept here.</div>
          <div style="display:flex;gap:8px;margin-top:10px"><input id="wpUrl" class="field" style="flex:1;width:auto" placeholder="https://example.org/picture.jpg" aria-label="Picture link" spellcheck="false">
          <button class="btn primary" id="wpGo" ${wallpaperBusy ? "disabled" : ""}>${wallpaperBusy ? "Downloading…" : "Use link"}</button></div></div>`, "stack") : "",
        v === "image" ? row(`${lbl("Colors from the picture", "The accent color follows the picture, and the clock and text stay readable on it")}
          ${get("appearance.wallpaper_accent") && get("appearance.adaptive_accent") ? `<span class="swatch on" style="background:${esc(get("appearance.wallpaper_accent"))}" title="${esc(get("appearance.wallpaper_accent"))}"></span>` : ""}
          <label class="switch"><input type="checkbox" data-path="appearance.adaptive_accent" ${get("appearance.adaptive_accent") ? "checked" : ""} aria-label="Colors from the picture"><span></span></label>`) : "",
        sw("appearance.newtab_show_clock", "Clock", ""),
        sw("appearance.newtab_show_shortcuts", "Shortcuts to your most visited sites", ""),
      ]);
    } },
    { id: "startup", icon: "home", title: "On startup", body: () => card([
      seg("startup.open", "When CottonCandy starts", "", [["newtab", "New tab"], ["restore", "Continue where you left off"], ["homepage", "Home page"]]),
      text("startup.homepage", "Home page", "Also where the Home button goes", "https://example.org"),
    ]) },
    { id: "search", icon: "search", title: "Search", body: () => card([
      sel("search.engine", "Search engine", "Used by the address bar", [...state.options.engines.map(e => [e.id, e.label]), ["custom", "Custom…"]]),
      get("search.engine") === "custom" ? text("search.custom_url", "Custom search address", "An https address with {} where the search goes", "https://kagi.com/search?q={}") : "",
      sw("search.local_suggestions", "Suggestions from history and bookmarks", "Worked out on this computer; nothing you type is sent anywhere until you press Enter"),
    ]) },
    { id: "grace", icon: "spark", title: "Grace", lead: "Grace runs a language model on this computer through CherryCola. Nothing is sent to a cloud service.", body: () => card([
      sw("grace.enabled", "Grace", "Enhance search and Grace Chat"),
      sw("grace.web_results", "Web results in Grace Chat", "Grace searches the web and cites what it found"),
      sw("grace.unload_after_enhance", "Unload the model after Enhance search", "Frees its memory as soon as the search is fixed"),
      text("grace.command", "CherryCola command", "Leave as cherrycola unless it's installed somewhere else", "cherrycola"),
    ]) + `<h3>Model</h3>` + graceModels() },
    { id: "performance", icon: "bolt", title: "Performance", lead: "Memory and graphics. Changes to graphics take effect after a restart.", body: () => card([
      row(`${lbl("Free up memory", "Tabs you aren't looking at go to sleep and every page drops its caches. No tab closes: a sleeping tab loads again when you open it. Pages with unsaved work stay awake.")}<button class="btn" id="freeMem">Free up memory</button>`),
      sw("performance.hardware_acceleration", "Use the graphics card", "Draws pages, video and WebGL on the GPU. Only turn off if pages flicker or show garbage."),
      sw("performance.ignore_gpu_blocklist", "Use the graphics card even on drivers Chromium distrusts", "Chromium draws with the processor on drivers it has had trouble with, which makes WebGL, video and scrolling slow. Try this if the graphics report says hardware acceleration is off."),
      sw("performance.smooth_scrolling", "Smooth scrolling", "The wheel and arrow keys glide instead of jumping"),
      row(`${lbl("Graphics report", "Shows whether the graphics card is in use for each kind of drawing (chrome://gpu)")}<button class="btn" id="gpuReport">Open</button>`),
    ]) },
    { id: "shields", icon: "shield", title: "Shields", lead: "Protections that apply to every site. You can turn them off for one site from the shield in the address bar.", body: () => {
      const lists = state.options.lists, on = get("shields.filter_lists") || [];
      return card([
        sw("shields.block_ads", "Block ads", ""),
        sw("shields.block_trackers", "Block trackers", "Analytics, fingerprinting and cross-site tracking scripts"),
        sw("shields.hide_ad_boxes", "Hide empty ad boxes", ""),
        sw("shields.block_scripts", "Block JavaScript", "Most sites break. Allow the ones you trust from the shield."),
        sw("shields.block_third_party_cookies", "Block third-party cookies", ""),
        sw("shields.https_only", "Always use secure connections", "Upgrades http to https and warns before a page that can't"),
        sw("shields.block_popups", "Block pop-ups", "Unless you clicked something to open them"),
        sw("shields.send_gpc", "Send Global Privacy Control", "Tells sites not to sell or share your data"),
      ]) + `<h3>Filter lists</h3>` + card([row(`<div class="checks" style="width:100%">${lists.map(l =>
        `<label><input type="checkbox" data-list="${esc(l.id)}" ${on.includes(l.id) ? "checked" : ""}> ${esc(l.label)}</label>`).join("")}</div>`, "stack"),
        area("shields.custom_rules", "Your own filter rules", "Adblock Plus / uBlock syntax, one per line", "||ads.example.com^\nexample.org##.banner")]);
    } },
    { id: "sites", icon: "globe", title: "Site settings", lead: "Sites where you've changed the Shields or zoom.", body: () => {
      const sites = Object.entries(state.settings.sites || {}).filter(([, v]) => Object.values(v).some(x => x !== null));
      if (!sites.length) return card([`<div class="empty">${cc.icon("globe")}No site has its own settings yet</div>`]);
      return card(sites.map(([host, v]) => `<div class="site"><img src="/favicon?host=${encodeURIComponent(host)}" alt=""><div class="grow"><div>${esc(host)}</div><div class="muted small">${[
        v.shields_up === false ? "Shields down" : v.shields_up ? "Shields up" : "",
        v.scripts === true ? "JavaScript allowed" : v.scripts === false ? "JavaScript blocked" : "",
        v.ads === false ? "Ads allowed" : "", v.zoom ? `Zoom ${v.zoom}%` : ""].filter(Boolean).join(" · ")}</div></div>
        <button class="btn" data-forget="${esc(host)}">Reset</button></div>`)) + `<div class="row-actions" style="margin-top:10px"><button class="btn" data-reset="sites">Reset all sites</button></div>`;
    } },
    { id: "privacy", icon: "lock", title: "Privacy and security", body: () => card([
      row(`${lbl("Clear browsing data", "History, cookies, cache and the download list")}<button class="btn" id="openClear">Clear…</button>`),
      sw("privacy.clear_on_exit", "Clear history when CottonCandy closes", ""),
      sw("privacy.save_history", "Save browsing history", "Private windows never save it"),
      sw("privacy.save_passwords", "Offer to save passwords", ""),
      sw("privacy.strip_tracking_params", "Remove tracking parts of links", "utm_source, fbclid, gclid and similar"),
      sel("privacy.webrtc_ip_policy", "WebRTC IP handling", "WebRTC (video calls) can reveal your real IP address behind a VPN or proxy", [
        ["default", "Default (fastest calls)"], ["default_public_interface_only", "Hide local addresses (recommended)"], ["disable_non_proxied_udp", "Only through the proxy (most private)"]]),
    ]) },
    { id: "network", icon: "vpn", title: "Proxy, VPN and secure DNS", lead: "A VPN app on your computer works with System or Direct. A VPN or tool that gives you a proxy (Tor, Mullvad SOCKS5, an SSH tunnel) goes under Proxy server.", body: () => {
      const p = state.settings.network.proxy, d = state.settings.network.dns;
      const rows = [
        sw("network.proxy.enabled", "Use the settings below", p.enabled ? "On: CottonCandy connects as set here" : "Off: CottonCandy uses your computer's proxy settings"),
        seg("network.proxy.mode", "Connection", "", [["system", "System"], ["direct", "Direct"], ["fixed", "Proxy server"], ["pac", "PAC script"]]),
      ];
      if (p.mode === "fixed") {
        rows.push(row(`${lbl("Quick setup", "Fills in the fields below")}<div class="presets">${state.options.presets.map(x =>
          `<button class="chip ${p.preset === x.id ? "on" : ""}" data-preset="${esc(x.id)}">${esc(x.label)}</button>`).join("")}</div>`, "stack"));
        rows.push(sel("network.proxy.scheme", "Type", "", [["socks5", "SOCKS5"], ["socks4", "SOCKS4"], ["http", "HTTP"], ["https", "HTTPS"]]));
        rows.push(text("network.proxy.host", "Server", "", "127.0.0.1"));
        rows.push(num("network.proxy.port", "Port", "", 1, 65535));
        rows.push(text("network.proxy.bypass", "Don't use the proxy for", "Comma separated: localhost, *.lan, 192.168.0.0/16", "localhost"));
      }
      if (p.mode === "pac") rows.push(text("network.proxy.pac_url", "PAC script address", "https:// or file://", "https://example.org/proxy.pac"));
      const dnsRows = [
        seg("network.dns.mode", "Secure DNS", d.mode === "secure" ? "Every lookup is encrypted; sites fail to load rather than leak" : d.mode === "automatic" ? "Encrypted when the network allows it" : "Using the network's DNS", [["off", "Off"], ["automatic", "Automatic"], ["secure", "Always"]]),
        sel("network.dns.provider", "Provider", "", [...state.options.dns.map(x => [x.id, x.label]), ["nextdns", "NextDNS (your profile)"], ["custom", "Custom…"]]),
      ];
      if (d.provider === "custom" || d.provider === "nextdns") dnsRows.push(text("network.dns.custom_template", "DNS over HTTPS address", d.provider === "nextdns" ? "https://dns.nextdns.io/<your id>" : "https://dns.example/dns-query{?dns}", "https://"));
      return (state.proxy_error ? `<div class="note err" role="alert">${esc(state.proxy_error)} CottonCandy uses the system settings until it's fixed.</div>` : "")
        + card(rows) + `<h3>DNS</h3>` + card(dnsRows)
        + (d.mode !== "off" && !state.dns_template ? `<div class="note err">That DNS address isn't a valid https:// address, so secure DNS is off.</div>` : "");
    } },
    { id: "downloads", icon: "download", title: "Downloads", body: () => card([
      row(`${lbl("Save files to", get("downloads.folder") || state.options.downloads_default)}<div class="row-actions"><button class="btn" id="pickFolder">Change…</button>${get("downloads.folder") ? `<button class="btn" data-path="downloads.folder" data-v="">Use Downloads</button>` : ""}</div>`),
      sw("downloads.ask_where", "Ask where to save each file", ""),
      sw("downloads.warn_executables", "Warn before files that can run programs", ".exe, .msi, .bat, .ps1, .sh, .AppImage and others"),
    ]) },
    { id: "extensions", icon: "puzzle", title: "Extensions", lead: "Install from the Chrome Web Store, or from Firefox Add-ons (best effort: add-ons that need Firefox-only features won't work). Only Manifest V3 extensions run.", body: () => {
      const items = state.options.extensions;
      return card([
        sw("extensions.enabled", "Run extensions", "Off keeps them installed but stops them all (after a restart)"),
        row(`<div class="lbl" style="width:100%"><div>Install an extension</div><div class="sub">Paste its page from chromewebstore.google.com or addons.mozilla.org</div>
          <div style="display:flex;gap:8px;margin-top:10px"><input id="extUrl" class="field" style="flex:1;width:auto" placeholder="https://chromewebstore.google.com/detail/…" aria-label="Extension page address" spellcheck="false"><button class="btn primary" id="extGo">Install</button></div></div>`, "stack"),
      ]) + (items.length ? `<h3>Installed</h3><div class="card">${items.map(x => `<div class="ext"><div class="icon">${cc.icon("puzzle")}</div>
          <div class="lbl" style="flex:1"><div>${esc(x.name)}</div><div class="sub muted small">${esc(x.version)} · ${x.source === "firefox" ? "Firefox Add-ons" : x.source === "chrome" ? "Chrome Web Store" : "Unpacked"}</div></div>
          <button class="btn danger" data-ext-remove="${esc(x.id)}">Remove</button>
          <label class="switch"><input type="checkbox" data-ext="${esc(x.id)}" ${x.enabled ? "checked" : ""} aria-label="Turn ${esc(x.name)} on or off"><span></span></label></div>`).join("")}</div>` : "");
    } },
    { id: "import", icon: "import", title: "Import from another browser", lead: "Bring your bookmarks, history, open tabs and extensions over. The other browser's files are only read, never changed.", body: () => {
      if (!sources) return card([row(lbl("Looking for other browsers…", ""))]);
      if (!sources.length) return card([`<div class="empty">${cc.icon("import")}No other browser found on this computer</div>`]);
      const box = (id, label, sub, on = true) => row(`${lbl(label, sub)}<label class="switch"><input type="checkbox" id="imp-${id}" ${on ? "checked" : ""} aria-label="${esc(label)}"><span></span></label>`);
      return card([
        row(`${lbl("Browser", "Close it first to bring over its very latest tabs")}<select class="field" id="impFrom" aria-label="Browser to import from">${
          sources.map((s, i) => `<option value="${i}">${esc(s.browser)}${sources.filter(o => o.browser === s.browser).length > 1 ? " · " + esc(s.name) : ""}</option>`).join("")}</select>`),
        box("bookmarks", "Bookmarks", "Into a folder named after the browser"),
        box("history", "History", "Up to 100,000 pages, newest first"),
        box("tabs", "Open tabs", "Opened in a new window and kept in a bookmark folder"),
        box("extensions", "Extensions", "Installed again from the Chrome Web Store or Firefox Add-ons", false),
        row(`${lbl("Passwords and cookies", "These stay behind: each browser locks them with its own key")}<button class="btn primary" id="impGo" ${importing ? "disabled" : ""}>${importing ? "Importing…" : "Import"}</button>`),
      ]);
    } },
    { id: "bookmarks", icon: "star", title: "Bookmarks", body: () => card([
      row(`${lbl("Import bookmarks", "From a bookmarks HTML file exported by Chrome, Edge, Brave, Firefox or Safari")}<button class="btn" id="importBm">Choose file…</button>`),
      row(`${lbl("Export bookmarks", "Saves a bookmarks HTML file to your download folder")}<button class="btn" id="exportBm">Export</button>`),
      row(`${lbl("Manage bookmarks", "")}<a class="btn" style="display:inline-flex;align-items:center" href="cottoncandy://bookmarks/">Open</a>`),
    ]) },
    { id: "about", icon: "info", title: "About CottonCandy", body: () => card([
      row(`${lbl("CottonCandy " + (about?.version || ""), about ? `Chromium ${about.facts.Chromium}` : "")}<a class="btn" style="display:inline-flex;align-items:center" href="cottoncandy://about/">Details</a>`),
      row(`${lbl("Reset settings", "Puts every setting back to its default. Installed extensions, bookmarks and history stay.")}<button class="btn danger" data-reset="all">Reset all</button>`),
    ]) },
  ];

  function graceModels() {
    if (installing) return card([row(`${lbl("Installing CherryColaLLM", installing.step || "")}${bar(installing.percent)}`)]);
    if (!grace) return card([row(lbl("Asking CherryCola…", ""))]);
    if (!grace.installed) return card([row(`${lbl("CherryColaLLM isn't installed", "Grace needs it to run a model on this computer. It installs for you only, without an administrator.")}<button class="btn primary" id="ccInstall">Install CherryColaLLM</button>`)]);
    const st = grace.status || {}, rec = grace.recommend || {};
    const rows = [];
    if (grace.message) rows.push(row(lbl("CherryCola says", grace.message)));
    const models = st.models || [];
    if (models.length) {
      const opts = models.includes(st.model) ? models : [st.model, ...models];
      rows.push(row(`${lbl("Model Grace uses", st.running ? `Loaded now: ${st.running}` : "Loads when Grace first answers")}<select class="field" id="ccModel" aria-label="Model Grace uses">${
        opts.map(m => `<option value="${esc(m)}" ${m === st.model ? "selected" : ""}>${esc(m)}</option>`).join("")}</select>`));
    } else if (st.reachable === false) {
      rows.push(row(lbl(`The model engine (${st.backend || "Ollama"}) isn't running`, rec.engine && !rec.engine_installed ? `Install it first: ${rec.engine}. Then download a model below.` : "Start it, then come back here.")));
    } else {
      rows.push(row(lbl("No model downloaded yet", "Download one below. Grace uses it right away.")));
    }
    if (pulling) rows.push(row(`${lbl(pulling.file ? "Adding " + fileName(pulling.model) : "Downloading " + pulling.model,
      pulling.status || (pulling.file ? "Nothing is downloaded. Ollama copies the file into its own models once." : "Starting…"))}${bar(pulling.percent)}`));
    const fits = (rec.models || []).filter(m => m.fits);
    if (fits.length) {
      const where = rec.runs_on === "gpu" ? `your graphics card (${rec.vram_gb} GB)` : `the processor (${rec.ram_gb} GB of memory)`;
      rows.push(row(`<div class="lbl" style="width:100%"><div>Models that suit this computer</div><div class="sub">They run on ${esc(where)}. Recommended: ${esc(rec.recommended || "")}</div>
        <div class="models">${fits.map(m => `<div class="model"><div class="grow"><div>${esc(m.model)}${m.model === rec.recommended ? ' <span class="tag">Recommended</span>' : ""}${models.includes(m.model) ? ' <span class="tag">Downloaded</span>' : ""}</div>
          <div class="sub">${esc(m.size_gb)} GB · ${esc(m.note)}${m.slow ? " · slow here" : ""}</div></div>
          ${models.includes(m.model) ? (m.model === st.model ? '<span class="muted small">In use</span>' : `<button class="btn" data-use="${esc(m.model)}">Use</button>`)
            : `<button class="btn" data-pull="${esc(m.model)}" ${pulling ? "disabled" : ""}>Download</button>`}</div>`).join("")}</div></div>`, "stack"));
    }
    rows.push(row(`<div class="lbl" style="width:100%"><div>Another model</div><div class="sub">An Ollama name (qwen3:8b), a Hugging Face GGUF (Qwen/Qwen3-4B-GGUF:Q4_K_M) or an https link to a .gguf</div>
      <div style="display:flex;gap:8px;margin-top:10px"><input id="ccOther" class="field" style="flex:1;width:auto" placeholder="qwen3:8b" aria-label="Model to download" spellcheck="false"><button class="btn" id="ccPull" ${pulling ? "disabled" : ""}>Download</button></div></div>`, "stack"));
    rows.push(row(`${lbl("A model file on this computer", "A .gguf you already have. Grace uses it without downloading anything.")}<button class="btn" id="ccFile" ${pulling ? "disabled" : ""}>Choose file…</button>`));
    return card(rows);
  }
  const fileName = p => String(p).split(/[\\/]/).pop();
  const bar = pct => `<div class="pbar" role="progressbar" ${pct != null ? `aria-valuenow="${Math.round(pct)}"` : ""}><i style="width:${pct != null ? Math.max(2, pct) : 30}%" class="${pct == null ? "busy" : ""}"></i></div>`;

  // The picture's color for the accent: the most common lively color, made bright enough to read on.
  async function pictureAccent() {
    const img = new Image();
    img.src = "/wallpaper?v=" + Date.now();
    await img.decode();
    const c = document.createElement("canvas");
    c.width = c.height = 48;
    const g = c.getContext("2d", { willReadFrequently: true });
    g.drawImage(img, 0, 0, 48, 48);
    const px = g.getImageData(0, 0, 48, 48).data;
    const buckets = new Map();
    for (let i = 0; i < px.length; i += 4) {
      const [r, gr, b] = [px[i] / 255, px[i + 1] / 255, px[i + 2] / 255];
      const max = Math.max(r, gr, b), min = Math.min(r, gr, b), d = max - min;
      const sat = max ? d / max : 0;
      if (sat < .25 || max < .2) continue; // grays and near black say nothing about the picture's color
      let h = d === 0 ? 0 : max === r ? ((gr - b) / d) % 6 : max === gr ? (b - r) / d + 2 : (r - gr) / d + 4;
      h = (h * 60 + 360) % 360;
      const key = Math.round(h / 20) % 18, w = sat * max;
      const e = buckets.get(key) || { w: 0, r: 0, g: 0, b: 0 };
      e.w += w; e.r += r * w; e.g += gr * w; e.b += b * w;
      buckets.set(key, e);
    }
    const best = [...buckets.values()].sort((a, b) => b.w - a.w)[0];
    if (!best) return null;
    let [r, gr, b] = [best.r / best.w, best.g / best.w, best.b / best.w];
    // Into a lightness that reads on both the dark and the light theme.
    const max = Math.max(r, gr, b), min = Math.min(r, gr, b), l = (max + min) / 2;
    const want = Math.min(.62, Math.max(.48, l)), scale = l ? want / l : 1;
    [r, gr, b] = [r, gr, b].map(v => Math.min(1, v * scale));
    return "#" + [r, gr, b].map(v => Math.round(v * 255).toString(16).padStart(2, "0")).join("");
  }
  async function adaptToPicture() {
    try {
      const color = await pictureAccent();
      if (color) await set("appearance.wallpaper_accent", color);
    } catch (e) { /* not a picture the browser can draw: the accent stays */ }
  }

  // ---- drawing -------------------------------------------------------------------------------------
  function render() {
    if (!state) return;
    const y = window.scrollY, focused = document.activeElement && document.activeElement.id;
    $("content").innerHTML = SECTIONS.map(s => `<section id="${s.id}" aria-labelledby="h-${s.id}"><h2 id="h-${s.id}">${esc(s.title)}</h2>
      ${s.lead ? `<p class="lead">${esc(s.lead)}</p>` : ""}${note(s.id)}${s.body()}</section>`).join("");
    $("restart").classList.toggle("hidden", !state.restart_needed);
    applyFilter();
    window.scrollTo(0, y);
    if (focused && $(focused)) $(focused).focus();
    wire();
    document.querySelectorAll("section").forEach(s => spy.observe(s));
  }

  function nav() {
    $("nav").innerHTML = SECTIONS.map(s => `<a href="#${s.id}" data-sec="${s.id}">${cc.icon(s.icon)}<span>${esc(s.title)}</span></a>`).join("");
  }

  function applyFilter() {
    const q = $("find").value.trim().toLowerCase();
    document.querySelectorAll("section").forEach(sec => {
      if (!q) { sec.classList.remove("hidden"); sec.querySelectorAll(".set").forEach(r => r.classList.remove("hidden")); return; }
      const title = sec.querySelector("h2").textContent.toLowerCase().includes(q);
      let any = title;
      sec.querySelectorAll(".set").forEach(r => { const hit = title || r.dataset.text.includes(q); r.classList.toggle("hidden", !hit); any = any || hit; });
      sec.classList.toggle("hidden", !any);
    });
  }

  function wire() {
    const main = $("content");
    main.querySelectorAll("input[type=checkbox][data-path]").forEach(el => el.addEventListener("change", () => set(el.dataset.path, el.checked)));
    main.querySelectorAll("select[data-path]").forEach(el => el.addEventListener("change", () =>
      set(el.dataset.path, typeof get(el.dataset.path) === "number" ? Number(el.value) : el.value)));
    main.querySelectorAll("button[data-path]").forEach(el => el.addEventListener("click", () => set(el.dataset.path, el.dataset.v)));
    main.querySelectorAll("input[type=color][data-path]").forEach(el => el.addEventListener("change", () => set(el.dataset.path, el.value)));
    main.querySelectorAll("input[type=range][data-path]").forEach(el => {
      el.addEventListener("input", () => { el.nextElementSibling.textContent = el.value + "px"; });
      el.addEventListener("change", () => set(el.dataset.path, Number(el.value)));
    });
    main.querySelectorAll("input.field[data-path]").forEach(el => {
      const commit = () => {
        const v = el.dataset.num ? Number(el.value) : el.value.trim();
        if (v !== get(el.dataset.path)) set(el.dataset.path, v);
      };
      el.addEventListener("change", commit);
      el.addEventListener("keydown", e => { if (e.key === "Enter") el.blur(); });
    });
    main.querySelectorAll("[data-save]").forEach(el => el.addEventListener("click", () => {
      const ta = main.querySelector(`textarea[data-path="${el.dataset.save}"]`);
      set(el.dataset.save, ta.value);
      flash(el.dataset.save.split(".")[0], "ok", "Saved.");
    }));
    main.querySelectorAll("[data-list]").forEach(el => el.addEventListener("change", () => {
      const on = new Set(get("shields.filter_lists"));
      el.checked ? on.add(el.dataset.list) : on.delete(el.dataset.list);
      set("shields.filter_lists", [...on]);
    }));
    main.querySelectorAll("[data-preset]").forEach(el => el.addEventListener("click", async () => {
      const p = state.options.presets.find(x => x.id === el.dataset.preset);
      for (const [k, v] of [["preset", p.id], ["scheme", p.scheme], ["host", p.host], ["port", p.port], ["enabled", true]]) {
        try { state = await cc.call("settings_set", { path: "network.proxy." + k, value: v }); } catch (e) { flash("network", "err", e.message); }
      }
      flash("network", "ok", `Using ${p.label}. Make sure it's running.`);
    }));
    main.querySelectorAll("[data-forget]").forEach(el => el.addEventListener("click", async () => {
      for (const k of ["shields_up", "scripts", "ads", "zoom"]) await cc.call("settings_set", { path: `sites.${el.dataset.forget}.${k}`, value: null }).then(r => { state = r; }).catch(() => {});
      render();
    }));
    main.querySelectorAll("[data-reset]").forEach(el => el.addEventListener("click", async () => {
      if (!confirm(el.dataset.reset === "all" ? "Put every setting back to its default?" : "Forget the settings of every site?")) return;
      state = await cc.call("settings_reset", { section: el.dataset.reset });
      flash(el.dataset.reset === "all" ? "about" : "sites", "ok", "Done.");
    }));
    main.querySelectorAll("[data-ext]").forEach(el => el.addEventListener("change", async () => {
      state = await cc.call("ext_toggle", { id: el.dataset.ext, enabled: el.checked }); render();
    }));
    main.querySelectorAll("[data-ext-remove]").forEach(el => el.addEventListener("click", async () => {
      state = await cc.call("ext_remove", { id: el.dataset.extRemove });
      flash("extensions", "info", "Removed. It stops running after a restart.");
    }));
    const click = (id, fn) => { const el = $(id); if (el) el.addEventListener("click", fn); };
    click("extGo", installExt);
    const extUrl = $("extUrl");
    if (extUrl) extUrl.addEventListener("keydown", e => { if (e.key === "Enter") installExt(); });
    click("pickFolder", () => cc.call("pick_folder", {}));
    click("importBm", () => cc.call("bookmarks_import", {}));
    click("impGo", async () => {
      const s = sources[+$("impFrom").value];
      const args = { key: s.key };
      for (const k of ["bookmarks", "history", "tabs", "extensions"]) args[k] = $("imp-" + k).checked;
      importing = true;
      try { await cc.call("import_run", args); flash("import", "info", `Reading ${s.browser}…`); }
      catch (e) { importing = false; flash("import", "err", e.message); }
    });
    click("exportBm", async () => {
      try { const r = await cc.call("bookmarks_export", {}); flash("bookmarks", "ok", `Exported ${r.count} bookmark${r.count === 1 ? "" : "s"} to ${r.path}`); }
      catch (e) { flash("bookmarks", "err", e.message); }
    });
    click("openClear", () => $("clear").showModal());
    click("wpPick", () => cc.call("pick_wallpaper", {}));
    const wpGo = async () => {
      const url = $("wpUrl").value.trim();
      if (!url) return;
      wallpaperBusy = true; render();
      try { await cc.call("wallpaper_from_link", { url }); }
      catch (e) { wallpaperBusy = false; flash("newtab", "err", e.message); }
    };
    click("wpGo", wpGo);
    if ($("wpUrl")) $("wpUrl").addEventListener("keydown", e => { if (e.key === "Enter") wpGo(); });
    click("freeMem", async () => {
      try { const r = await cc.call("free_memory", {}); flash("performance", "ok", r.text); }
      catch (e) { flash("performance", "err", e.message); }
    });
    click("gpuReport", () => cc.call("open_gpu_report", {}));
    click("ccInstall", () => {
      installing = { step: "Starting" }; render();
      cc.call("grace_install", {}).catch(e => { installing = null; flash("grace", "err", e.message); });
    });
    const model = $("ccModel");
    if (model) model.addEventListener("change", () => cc.call("grace_use", { model: model.value }).catch(e => flash("grace", "err", e.message)));
    const pull = m => { if (!m) return; pulling = { model: m }; render(); cc.call("grace_pull", { model: m }).catch(e => { pulling = null; flash("grace", "err", e.message); }); };
    main.querySelectorAll("[data-pull]").forEach(el => el.addEventListener("click", () => pull(el.dataset.pull)));
    main.querySelectorAll("[data-use]").forEach(el => el.addEventListener("click", () => cc.call("grace_use", { model: el.dataset.use }).catch(e => flash("grace", "err", e.message))));
    click("ccPull", () => pull($("ccOther").value.trim()));
    click("ccFile", () => cc.call("pick_model_file", {}).catch(e => flash("grace", "err", e.message)));
  }

  async function installExt() {
    const url = $("extUrl").value.trim();
    if (!url) return;
    try { await cc.call("ext_install", { url }); flash("extensions", "info", "Downloading and checking the extension…"); }
    catch (e) { flash("extensions", "err", e.message); }
  }

  // ---- events from the browser -------------------------------------------------------------------
  cc.on("grace_models", d => { grace = d; render(); });
  cc.on("grace_pull", d => {
    if (d.done) { pulling = null; flash("grace", "ok", d.file ? `Added ${fileName(d.model)}. Grace uses it now.` : `Downloaded ${d.model}. Grace uses it now.`); return; }
    if (d.error) { pulling = null; flash("grace", "err", d.error); return; }
    pulling = d; render();
  });
  cc.on("grace_install", d => {
    if (d.done) { installing = null; flash("grace", "ok", "CherryColaLLM is installed."); cc.call("grace_models", {}); return; }
    if (d.error) { installing = null; flash("grace", "err", d.error); return; }
    installing = d; render();
  });
  cc.on("wallpaper", async r => {
    wallpaperBusy = false;
    if (r.error) { flash("newtab", "err", r.error); return; }
    state = await cc.call("settings_get", {});
    flash("newtab", "ok", "New picture set.");
    adaptToPicture();
  });
  cc.on("ext_result", async r => {
    state = await cc.call("settings_get", {});
    flash("extensions", r.error ? "err" : "ok", r.error || `Installed ${r.installed}. Restart to start it.`);
  });
  cc.on("import", r => flash("bookmarks", r.error ? "err" : "ok", r.error || `Imported ${r.count} bookmark${r.count === 1 ? "" : "s"}.`));
  cc.on("folder", r => set("downloads.folder", r.folder));
  cc.on("import_done", r => {
    importing = false;
    const n = (k, one, many) => r[k] != null ? `${r[k].toLocaleString()} ${r[k] === 1 ? one : many}` : null;
    const parts = [n("bookmarks", "bookmark", "bookmarks"), n("history", "page of history", "pages of history"), n("tabs", "open tab", "open tabs")].filter(Boolean);
    if (r.extensions && r.extensions.length) parts.push(`${r.extensions.length} extension${r.extensions.length === 1 ? "" : "s"} (installing: ${r.extensions.join(", ")})`);
    const text = parts.length ? `From ${r.from}: ${parts.join(", ")}.` : `Nothing to bring over from ${r.from}.`;
    flash("import", r.errors.length ? "err" : "ok", text + (r.errors.length ? " " + r.errors.join(" ") : ""));
  });

  $("clearCancel").addEventListener("click", () => $("clear").close());
  $("clearGo").addEventListener("click", async () => {
    const args = { range: $("range").value };
    document.querySelectorAll("[data-c]").forEach(c => { args[c.dataset.c] = c.checked; });
    $("clear").close();
    try { const r = await cc.call("clear_data", args); flash("privacy", "ok", `Cleared: ${r.cleared.join(", ") || "nothing"}.`); }
    catch (e) { flash("privacy", "err", e.message); }
  });
  $("restartBtn").addEventListener("click", () => cc.call("restart", {}));
  $("find").addEventListener("input", applyFilter);

  // Highlight the section in view, and jump to #section links.
  const spy = new IntersectionObserver(entries => {
    for (const e of entries) if (e.isIntersecting) {
      document.querySelectorAll("nav a").forEach(a => a.classList.toggle("on", a.dataset.sec === e.target.id));
    }
  }, { rootMargin: "-10% 0px -80% 0px" });
  function jump() {
    const sec = location.hash && document.getElementById(location.hash.slice(1));
    if (sec) { sec.scrollIntoView(); sec.classList.add("hit"); setTimeout(() => sec.classList.remove("hit"), 1300); }
  }
  window.addEventListener("hashchange", jump);

  (async () => {
    nav();
    [state, about] = await Promise.all([cc.call("settings_get", {}), cc.call("about", {}).catch(() => null)]);
    render();
    jump();
    cc.call("import_sources", {}).then(r => { sources = r.sources; }).catch(() => { sources = []; }).finally(render);
    cc.call("grace_models", {}).catch(() => {});
    // A picture chosen before colors could follow it (or typed in by path) gets its color now.
    if (get("appearance.newtab_background") === "image" && get("appearance.wallpaper_path") && !get("appearance.wallpaper_accent")) adaptToPicture();
  })();
})();
