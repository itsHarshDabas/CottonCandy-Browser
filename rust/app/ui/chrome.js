// The tab strip and toolbar. The browser owns the state and pushes it with cc.emit("state", ...);
// this page only draws it and sends what the user does back with cc.call(...).
(() => {
  const $ = id => document.getElementById(id);
  const vertical = document.body.dataset.role === "vtabs";
  let state = null;
  let editing = false;
  let suggestions = [];
  let selected = -1;
  let typedText = "";
  let suggestTimer = 0;

  const call = (name, args) => cc.call(name, args).catch(e => console.warn(name, e.message));
  const rectOf = el => { const r = el.getBoundingClientRect(); return { x: Math.round(r.left), y: Math.round(r.top), width: Math.round(r.width), height: Math.round(r.height) }; };

  function favicon(tab) {
    if (tab.loading) return '<span class="spin"></span>';
    if (tab.favicon) return `<img src="${cc.esc(tab.favicon)}" width="16" height="16" alt="">`;
    return cc.icon(tab.internal ? "spark" : "globe");
  }

  function renderTabs() {
    const box = $("tabs");
    if (!box || !state) return;
    const known = new Map([...box.children].map(el => [el.dataset.id, el]));
    const order = [];
    for (const tab of state.tabs) {
      const id = String(tab.id);
      let el = known.get(id);
      if (!el) {
        el = document.createElement("div");
        el.className = "tab";
        el.dataset.id = id;
        el.setAttribute("role", "tab");
        el.innerHTML = '<span class="fav"></span><span class="snd hidden"></span><span class="title"></span><button class="x" aria-label="Close tab"></button>';
        el.querySelector(".x").innerHTML = cc.icon("x");
        wireTab(el);
      }
      known.delete(id);
      el.classList.toggle("active", tab.id === state.active);
      el.classList.toggle("pinned", !!tab.pinned);
      el.setAttribute("aria-selected", tab.id === state.active);
      el.title = tab.title + (tab.url ? "\n" + tab.url : "");
      const fav = favicon(tab);
      const favEl = el.querySelector(".fav");
      if (favEl.dataset.html !== fav) { favEl.innerHTML = fav; favEl.dataset.html = fav; }
      el.querySelector(".title").textContent = tab.title || tab.url || "New tab";
      const snd = el.querySelector(".snd");
      snd.classList.toggle("hidden", !(tab.audible || tab.muted));
      snd.innerHTML = cc.icon(tab.muted ? "muted" : "audio");
      order.push(el);
    }
    for (const el of known.values()) el.remove();
    order.forEach((el, i) => { if (box.children[i] !== el) box.insertBefore(el, box.children[i] || null); });
  }

  let drag = null;
  function wireTab(el) {
    const id = () => Number(el.dataset.id);
    el.addEventListener("mousedown", e => {
      if (e.button === 1) { e.preventDefault(); return; }
      if (e.button !== 0 || e.target.closest(".x, .snd")) return;
      call("tab_activate", { id: id() });
      drag = { el, startX: e.clientX, startY: e.clientY, moved: false };
    });
    el.addEventListener("auxclick", e => { if (e.button === 1) call("tab_close", { id: id() }); });
    el.querySelector(".x").addEventListener("click", e => { e.stopPropagation(); call("tab_close", { id: id() }); });
    el.querySelector(".snd").addEventListener("click", e => { e.stopPropagation(); call("tab_mute", { id: id() }); });
    el.addEventListener("contextmenu", e => {
      e.preventDefault();
      const tab = state.tabs.find(t => t.id === id());
      popup("menu", { x: e.clientX, y: e.clientY, width: 1, height: 1 }, {
        items: [
          { id: "tab_new_right", label: "New tab to the right", arg: id() },
          { id: "tab_reload", label: "Reload", arg: id() },
          { id: "tab_duplicate", label: "Duplicate", arg: id() },
          { id: "tab_pin", label: tab && tab.pinned ? "Unpin" : "Pin", arg: id() },
          { id: "tab_mute", label: tab && tab.muted ? "Unmute site" : "Mute site", arg: id() },
          { sep: true },
          { id: "tab_close", label: "Close", shortcut: "Ctrl+W", arg: id() },
          { id: "tab_close_others", label: "Close other tabs", arg: id() },
          { id: "tab_close_right", label: "Close tabs to the right", arg: id() },
          { id: "tab_reopen", label: "Reopen closed tab", shortcut: "Ctrl+Shift+T" },
        ],
      });
    });
  }

  window.addEventListener("mousemove", e => {
    if (!drag) return;
    const axis = vertical ? e.clientY - drag.startY : e.clientX - drag.startX;
    if (!drag.moved && Math.abs(axis) < 6) return;
    drag.moved = true;
    const box = $("tabs");
    const over = document.elementFromPoint(e.clientX, e.clientY)?.closest(".tab");
    if (over && over !== drag.el && over.parentNode === box) {
      const r = over.getBoundingClientRect();
      const after = vertical ? e.clientY > r.top + r.height / 2 : e.clientX > r.left + r.width / 2;
      box.insertBefore(drag.el, after ? over.nextSibling : over);
    }
  });
  window.addEventListener("mouseup", () => {
    if (drag && drag.moved) {
      const index = [...$("tabs").children].indexOf(drag.el);
      call("tab_move", { id: Number(drag.el.dataset.id), index });
    }
    drag = null;
  });

  function renderToolbar() {
    if (vertical || !state) return;
    const nav = state.nav || {};
    $("back").disabled = !nav.can_back;
    $("forward").disabled = !nav.can_forward;
    $("reload").innerHTML = cc.icon(nav.loading ? "stop" : "reload");
    $("reload").title = nav.loading ? "Stop (Esc)" : "Reload (Ctrl+R)";
    $("home").classList.toggle("hidden", !state.ui.home_button);
    const parts = nav.parts || { kind: "empty", host: "", rest: "" };
    const site = $("site");
    site.className = "site " + parts.kind;
    site.innerHTML = cc.icon({ secure: "lock", insecure: "alert", internal: "spark", file: "copy" }[parts.kind] || "search");
    site.title = { secure: "Connection is secure", insecure: "Not secure: anyone on the network can see this page", internal: "CottonCandy page", file: "Local file" }[parts.kind] || "Site information";
    if (!editing) {
      $("url").value = nav.url || "";
      drawShown(parts);
    }
    $("star").innerHTML = cc.icon("star");
    $("star").classList.toggle("on", !!nav.bookmarked);
    $("star").classList.toggle("hidden", !nav.url || parts.kind === "internal" || parts.kind === "empty");
    const sh = state.shields || {};
    $("shields").classList.toggle("on", !!sh.on);
    $("shields").innerHTML = cc.icon("shield") + `<span class="count">${sh.on ? (sh.blocked || 0) : "Off"}</span>`;
    $("shields").classList.toggle("hidden", !sh.applies);
    const px = state.proxy || {};
    $("proxy").classList.toggle("on", !!px.on);
    $("proxy").innerHTML = cc.icon("vpn") + `<span>${cc.esc(px.label || "Direct")}</span>`;
    $("ext").classList.toggle("hidden", !state.ui.extensions);
    const dl = state.downloads || {};
    $("dlring").classList.toggle("active", !!dl.active);
    $("dlring").classList.toggle("hidden", !dl.count);
    $("dlring").style.setProperty("--p", dl.percent || 0);
    $("grace").classList.toggle("on", !!state.ui.grace_open);
    $("grace").classList.toggle("hidden", !state.ui.grace);
    document.body.classList.toggle("private", !!state.private);
    document.body.classList.toggle("vertical", state.ui.tab_layout === "vertical");
    document.body.classList.toggle("bookmarks-bar", !!state.ui.bookmarks_bar);
    renderBookmarks();
    renderControls();
  }

  function drawShown(parts) {
    const shown = $("shown");
    if (editing || !parts.host) { shown.innerHTML = ""; $("url").style.color = ""; return; }
    const warn = parts.kind === "insecure" ? '<span class="warn">Not secure</span>' : "";
    shown.innerHTML = `${warn}<span class="host">${cc.esc(parts.host)}</span><span class="rest">${cc.esc(parts.rest)}</span>`;
    $("url").style.color = "transparent";
  }

  function renderBookmarks() {
    const bar = $("bookmarks");
    const list = state.bookmarks || [];
    const key = JSON.stringify(list);
    if (bar.dataset.key === key) return;
    bar.dataset.key = key;
    bar.innerHTML = list.length ? "" : '<span class="muted small" style="padding:0 8px">Bookmarks you add with the star show up here</span>';
    for (const b of list) {
      const btn = document.createElement("button");
      btn.textContent = b.title || b.url;
      btn.title = b.url;
      btn.addEventListener("click", e => call("navigate", { text: b.url, new_tab: e.ctrlKey }));
      btn.addEventListener("auxclick", e => { if (e.button === 1) call("navigate", { text: b.url, new_tab: true }); });
      bar.appendChild(btn);
    }
  }

  function renderControls() {
    const box = $("wincontrols");
    const side = state.ui.controls || "right";
    box.classList.toggle("left", side === "left");
    box.classList.toggle("hidden", side === "none");
    const key = side + state.ui.maximized;
    if (box.dataset.key === key) return;
    box.dataset.key = key;
    if (side === "left") {
      box.innerHTML = '<button class="light close" aria-label="Close" data-w="close"></button><button class="light min" aria-label="Minimize" data-w="minimize"></button><button class="light max" aria-label="Maximize" data-w="maximize"></button>';
    } else {
      box.innerHTML = `<button class="icon-btn" aria-label="Minimize" data-w="minimize">${cc.icon("min")}</button><button class="icon-btn" aria-label="${state.ui.maximized ? "Restore" : "Maximize"}" data-w="maximize">${cc.icon(state.ui.maximized ? "copy" : "max")}</button><button class="icon-btn winclose" aria-label="Close" data-w="close">${cc.icon("x")}</button>`;
    }
    box.querySelectorAll("[data-w]").forEach(b => b.addEventListener("click", () => call("window", { action: b.dataset.w })));
  }

  // Address bar: typing asks for suggestions, which a popup under the bar shows.
  function popup(kind, rect, data = {}) { return call("popup", { kind, rect, ...data }); }
  function hideSuggestions() { suggestions = []; selected = -1; call("popup_hide", { kind: "suggest" }); }
  function showSuggestions() {
    if (!suggestions.length) return hideSuggestions();
    const r = rectOf($("address"));
    popup("suggest", { x: r.x, y: r.y + r.height + 4, width: r.width, height: Math.min(8, suggestions.length) * 36 + 12 }, { items: suggestions, selected });
  }
  async function suggest() {
    enhancing = null;
    const text = $("url").value;
    typedText = text;
    if (!text.trim()) return hideSuggestions();
    const result = await cc.call("suggest", { text }).catch(() => null);
    if (!result || $("url").value !== text) return;
    suggestions = result.items || [];
    selected = suggestions.length && result.autocomplete ? 0 : -1;
    showSuggestions();
  }

  // Grace rows: Enhance search asks Grace for better words and shows them first; Ask Grace opens Grace Chat.
  function choose(choice, newTab) {
    if (choice && (choice.kind === "enhancing" || choice.kind === "note")) return;
    if (choice && choice.kind === "enhance") return enhance(choice.query);
    hideSuggestions();
    $("url").blur();
    if (choice && choice.kind === "ask") return call("grace_ask", { question: choice.query });
    call("navigate", { text: choice ? choice.url : $("url").value, new_tab: newTab, typed: true });
  }
  function enhance(query) {
    const asTyped = suggestions.find(s => s.kind === "search");
    suggestions = [{ kind: "enhancing", title: "Grace is improving your search…", url: "" }].concat(asTyped ? [asTyped] : []);
    selected = -1;
    showSuggestions();
    call("grace_enhance", { query }).catch(e => cc.emit("enhanced", { query, error: e.message }));
    enhancing = { query, asTyped };
  }
  let enhancing = null;
  cc.on("enhanced", d => {
    if (!enhancing || enhancing.query !== d.query || !editing) return;
    const asTyped = enhancing.asTyped;
    enhancing = null;
    suggestions = d.error ? [{ kind: "note", title: d.error, url: "" }].concat(asTyped ? [asTyped] : []) : d.items;
    selected = d.error ? (asTyped ? 1 : -1) : 0;
    if (!d.error && suggestions[0]) $("url").value = suggestions[0].fill;
    showSuggestions();
  });

  function initToolbar() {
    for (const [id, icon] of [["back", "back"], ["forward", "forward"], ["reload", "reload"], ["home", "home"], ["newtab", "plus"],
      ["ext", "puzzle"], ["dlring", "download"], ["grace", "spark"], ["menu", "menu"]]) {
      const el = $(id);
      if (el) el.innerHTML = cc.icon(icon);
    }
    $("newtab").addEventListener("click", () => call("tab_new", {}));
    $("back").addEventListener("click", e => call("nav", { action: "back", new_tab: e.ctrlKey }));
    $("forward").addEventListener("click", e => call("nav", { action: "forward", new_tab: e.ctrlKey }));
    $("reload").addEventListener("click", e => call("nav", { action: state && state.nav.loading ? "stop" : (e.shiftKey ? "reload_hard" : "reload") }));
    $("home").addEventListener("click", () => call("nav", { action: "home" }));
    $("star").addEventListener("click", () => call("bookmark_toggle", {}));
    $("grace").addEventListener("click", () => call("grace_toggle", {}));
    $("shields").addEventListener("click", () => popup("shields", anchor($("shields"), 320, 360)));
    $("proxy").addEventListener("click", () => popup("proxy", anchor($("proxy"), 320, 380)));
    $("ext").addEventListener("click", () => popup("extensions", anchor($("ext"), 300, 320)));
    $("dlring").addEventListener("click", () => popup("downloads", anchor($("dlring"), 360, 360)));
    $("site").addEventListener("click", () => popup("site", { ...rectOf($("site")), width: 340, height: 300, y: rectOf($("address")).y + 40 }));
    $("menu").addEventListener("click", () => popup("menu", anchor($("menu"), 280, 0), { items: mainMenu() }));
    const url = $("url");
    url.addEventListener("focus", () => { editing = true; drawShown({}); url.style.color = ""; if (url.value) url.select(); });
    url.addEventListener("blur", () => { editing = false; setTimeout(hideSuggestions, 150); if (state) { url.value = state.nav.url || ""; drawShown(state.nav.parts || {}); } });
    url.addEventListener("input", () => { clearTimeout(suggestTimer); suggestTimer = setTimeout(suggest, 60); });
    url.addEventListener("keydown", e => {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        if (!suggestions.length) return;
        e.preventDefault();
        const n = suggestions.length;
        if (e.key === "ArrowDown") selected = selected + 1 >= n ? -1 : selected + 1;
        else selected = selected - 1 < -1 ? n - 1 : selected - 1;
        url.value = selected >= 0 ? (suggestions[selected].fill || suggestions[selected].url) : typedText;
        showSuggestions();
      } else if (e.key === "Enter") {
        e.preventDefault();
        choose(selected >= 0 ? suggestions[selected] : null, e.altKey);
      } else if (e.key === "Escape") {
        if (suggestions.length) hideSuggestions();
        else { url.value = state ? state.nav.url || "" : ""; url.blur(); call("focus_page", {}); }
      } else if (e.key === "Delete" && e.shiftKey && selected >= 0 && suggestions[selected].kind === "history") {
        e.preventDefault();
        call("history_delete_url", { url: suggestions[selected].url }).then(suggest);
      }
    });
    $("tabrow").addEventListener("dblclick", e => { if (e.target.classList.contains("drag")) call("window", { action: "maximize" }); });
  }

  function anchor(el, width, height) {
    const r = rectOf(el);
    return { x: Math.max(4, r.x + r.width - width), y: r.y + r.height + 4, width, height };
  }

  function mainMenu() {
    return [
      { id: "tab_new", label: "New tab", shortcut: "Ctrl+T" },
      { id: "window_new", label: "New window", shortcut: "Ctrl+N" },
      { id: "window_private", label: "New private window", shortcut: "Ctrl+Shift+N" },
      { sep: true },
      { id: "open_history", label: "History", shortcut: "Ctrl+H" },
      { id: "open_downloads", label: "Downloads", shortcut: "Ctrl+J" },
      { id: "open_bookmarks", label: "Bookmarks", shortcut: "Ctrl+Shift+O" },
      { id: "open_extensions", label: "Extensions" },
      { sep: true },
      { zoom: true },
      { id: "find", label: "Find in page", shortcut: "Ctrl+F" },
      { id: "print", label: "Print", shortcut: "Ctrl+P" },
      { id: "save_page", label: "Save page as…", shortcut: "Ctrl+S" },
      { id: "devtools", label: "Developer tools", shortcut: "F12" },
      { sep: true },
      { id: "toggle_bookmarks_bar", label: "Show bookmarks bar", check: !!(state && state.ui.bookmarks_bar), shortcut: "Ctrl+Shift+B" },
      { id: "clear_data", label: "Clear browsing data", shortcut: "Ctrl+Shift+Del" },
      { id: "open_settings", label: "Settings", shortcut: "Ctrl+," },
      { id: "open_about", label: "About CottonCandy" },
      { sep: true },
      { id: "quit", label: "Quit", shortcut: "Ctrl+Shift+Q" },
    ];
  }

  // The page's keyboard shortcuts reach the browser through its keyboard handler; these are for when the
  // toolbar itself has focus.
  window.addEventListener("keydown", e => {
    if (e.ctrlKey && e.key.toLowerCase() === "l") { e.preventDefault(); $("url")?.focus(); }
  });

  cc.on("state", s => {
    state = s;
    document.documentElement.dataset.theme = s.ui.theme || "dark";
    if (vertical) document.body.classList.toggle("folded", !!s.ui.tabs_collapsed);
    renderTabs();
    renderToolbar();
  });
  cc.on("focus_address", text => {
    const url = $("url");
    if (!url) return;
    url.focus();
    if (typeof text === "string") { url.value = text; suggest(); }
  });
  cc.on("suggest_pick", index => {
    const choice = suggestions[index];
    if (choice) choose(choice, false);
  });

  if (vertical) {
    $("newtab").innerHTML = cc.icon("plus") + "<span>New tab</span>";
    $("newtab").addEventListener("click", () => call("tab_new", {}));
    // Folded side tabs: open after the pointer rests on them a moment, fold again soon after it leaves.
    let foldTimer = 0;
    const want = open => {
      clearTimeout(foldTimer);
      if (!state || !state.ui.collapse_tabs || drag) return;
      foldTimer = setTimeout(() => call("vtabs_expand", { open }), open ? 220 : 450);
    };
    document.documentElement.addEventListener("mouseenter", () => want(true));
    document.documentElement.addEventListener("mouseleave", () => want(false));
  } else {
    initToolbar();
  }
  // The browser may not have filed this page under its window yet; ask again until it has.
  (function ready() { cc.call("ready", { role: vertical ? "vtabs" : "chrome" }).catch(() => setTimeout(ready, 50)); })();
})();
