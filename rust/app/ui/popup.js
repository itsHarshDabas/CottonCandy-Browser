// Menus and panels that float over the page: address suggestions, the main menu, Shields, proxy and VPN,
// downloads, extensions and site information. The browser says what to show with cc.emit("show", ...).
(() => {
  const box = document.getElementById("box");
  let current = null;
  const call = (name, args) => cc.call(name, args).catch(e => console.warn(name, e.message));
  const hide = () => call("popup_hide", {});
  const esc = cc.esc;

  function fit() {
    requestAnimationFrame(() => call("popup_resize", { height: Math.ceil(box.scrollHeight) + 2 }));
  }

  function switchRow(label, sub, on, attrs) {
    return `<label class="row"><div class="grow"><div>${esc(label)}</div>${sub ? `<div class="sub">${esc(sub)}</div>` : ""}</div>
      <span class="switch"><input type="checkbox" ${on ? "checked" : ""} ${attrs}><span></span></span></label>`;
  }

  const views = {
    find() {
      box.innerHTML = `<div class="find"><input id="find" type="search" placeholder="Find in page" aria-label="Find in page" autocomplete="off">
        <span class="count muted small" id="findcount"></span>
        <button class="icon-btn" data-dir="back" aria-label="Previous match">${cc.icon("up")}</button>
        <button class="icon-btn" data-dir="next" aria-label="Next match">${cc.icon("down")}</button>
        <button class="icon-btn" data-dir="close" aria-label="Close find">${cc.icon("x")}</button></div>`;
      const input = box.querySelector("#find");
      const go = (next, back) => { if (input.value) call("find", { text: input.value, next, back }); else { call("find", { text: "" }); box.querySelector("#findcount").textContent = ""; } };
      input.addEventListener("input", () => go(false, false));
      input.addEventListener("keydown", e => { if (e.key === "Enter") { go(true, e.shiftKey); e.preventDefault(); } });
      box.querySelectorAll("[data-dir]").forEach(el => el.addEventListener("click", () => {
        if (el.dataset.dir === "close") { call("find", { text: "" }); hide(); } else go(true, el.dataset.dir === "back");
      }));
      setTimeout(() => input.focus(), 0);
    },
    suggest(d) {
      box.setAttribute("role", "listbox");
      box.innerHTML = d.items.map((s, i) => `<div class="item ${i === d.selected ? "sel" : ""}" data-i="${i}" data-kind="${esc(s.kind)}" role="option">
        ${cc.icon({ history: "clock", bookmark: "star", search: "search", url: "globe", internal: "spark", enhance: "spark", enhanced: "spark", enhancing: "spark", ask: "spark", note: "info" }[s.kind] || "globe")}
        <span class="t ${s.kind === "enhancing" ? "dots" : ""}">${esc(s.title || s.url)}</span>${s.group ? `<span class="u">— ${esc(s.group)}</span>` : ["enhance", "ask"].includes(s.kind) ? '<span class="u">— Local AI · on this device</span>' : s.title && s.url && s.kind !== "search" ? `<span class="u">— ${esc(s.url)}</span>` : ""}</div>`).join("");
      box.querySelectorAll(".item").forEach(el => el.addEventListener("mousedown", e => {
        e.preventDefault();
        call("suggest_pick", { index: Number(el.dataset.i) });
      }));
    },
    menu(d) {
      box.setAttribute("role", "menu");
      box.innerHTML = d.items.map((m, i) => {
        if (m.sep) return '<div class="sep"></div>';
        if (m.zoom) return `<div class="zoom"><span class="lbl">Zoom</span><button class="icon-btn" data-zoom="out" aria-label="Zoom out">${cc.icon("min")}</button>
          <span class="val">${(d.info && d.info.zoom) || 100}%</span><button class="icon-btn" data-zoom="in" aria-label="Zoom in">${cc.icon("plus")}</button>
          <button class="icon-btn" data-zoom="reset" aria-label="Reset zoom">${cc.icon("reload")}</button></div>`;
        return `<div class="item ${m.check ? "check" : ""}" data-i="${i}" role="menuitem" tabindex="-1"><span class="t">${esc(m.label)}</span>${m.shortcut ? `<span class="k">${esc(m.shortcut)}</span>` : ""}</div>`;
      }).join("");
      box.querySelectorAll(".item").forEach(el => el.addEventListener("click", () => {
        const m = d.items[Number(el.dataset.i)];
        hide();
        call("command", { id: m.id, arg: m.arg });
      }));
      box.querySelectorAll("[data-zoom]").forEach(el => el.addEventListener("click", async () => {
        const r = await cc.call("command", { id: "zoom_" + el.dataset.zoom }).catch(() => null);
        if (r && r.zoom) box.querySelector(".zoom .val").textContent = r.zoom + "%";
      }));
    },
    shields(d) {
      const s = d.info || {};
      box.innerHTML = `<div class="head">${cc.icon("shield")}<h2>${esc(s.host || "This page")}</h2></div>
        ${switchRow(s.on ? "Shields are up" : "Shields are down", s.on ? "Ads, trackers and the switches below are blocked here" : "Everything on this site loads", s.on, 'data-f="shields_up"')}
        <div class="stats"><div><div class="big">${s.ads_blocked || 0}</div><div class="sub muted small">Ads blocked</div></div>
          <div><div class="big">${s.trackers_blocked || 0}</div><div class="sub muted small">Trackers blocked</div></div></div>
        <div class="${s.on ? "" : "hidden"}">
        ${switchRow("Block ads", "", s.ads, 'data-f="ads"')}
        ${switchRow("Block scripts", "Pages may break; turn off for sites you trust", s.scripts_blocked, 'data-f="scripts"')}
        </div>
        <div class="foot"><button class="btn" data-open="cottoncandy://settings/#shields">Shields settings</button></div>`;
      box.querySelectorAll("input[data-f]").forEach(el => el.addEventListener("change", async () => {
        const r = await cc.call("site_shields", { host: s.host, field: el.dataset.f, value: el.checked }).catch(() => null);
        if (r) { current.info = r; render(current); }
      }));
    },
    proxy(d) {
      const p = d.info || {};
      const options = (p.presets || []).map(x => `<option value="${esc(x.id)}" ${x.id === p.preset ? "selected" : ""}>${esc(x.label)}</option>`).join("");
      const dns = (p.dns_providers || []).map(x => `<option value="${esc(x.id)}" ${x.id === p.dns_provider ? "selected" : ""}>${esc(x.label)}</option>`).join("");
      box.innerHTML = `<div class="head">${cc.icon("vpn")}<h2>Proxy and VPN</h2></div>
        ${switchRow("Route through a proxy or VPN", p.enabled ? p.label : "Connecting directly", p.enabled, 'data-s="network.proxy.enabled"')}
        <div class="row"><div class="grow">Route</div><select class="field" data-s="network.proxy.preset">${options}</select></div>
        <div class="sep"></div>
        ${switchRow("Secure DNS", p.dns_mode === "secure" ? "Every lookup is encrypted" : p.dns_mode === "automatic" ? "Encrypted when the network allows it" : "Using the network's DNS", p.dns_mode !== "off", 'data-s="network.dns.mode"')}
        <div class="row"><div class="grow">Provider</div><select class="field" data-s="network.dns.provider">${dns}</select></div>
        ${p.error ? `<div class="row" style="color:var(--danger)">${esc(p.error)}</div>` : ""}
        <div class="foot"><button class="btn" data-open="cottoncandy://settings/#network">Network settings</button></div>`;
      box.querySelectorAll("[data-s]").forEach(el => el.addEventListener("change", async () => {
        let value = el.type === "checkbox" ? el.checked : el.value;
        if (el.dataset.s === "network.dns.mode") value = el.checked ? "secure" : "off";
        const r = await cc.call("proxy_set", { path: el.dataset.s, value }).catch(e => ({ ...p, error: e.message }));
        current.info = r; render(current);
      }));
    },
    downloads(d) {
      const items = (d.info && d.info.items) || [];
      box.innerHTML = `<div class="head">${cc.icon("download")}<h2>Downloads</h2></div>` + (items.length ? items.slice(0, 6).map(x => `
        <div class="dl" data-id="${x.id}"><div class="grow" style="flex:1;min-width:0">
          <div class="t" style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(x.name)}</div>
          <div class="sub muted small">${esc(x.status)}</div>
          ${x.state === "progress" ? `<div class="bar"><i style="width:${x.percent}%"></i></div>` : ""}</div>
          ${x.state === "progress" ? `<button class="icon-btn" data-a="cancel" aria-label="Cancel">${cc.icon("x")}</button>` : ""}
          ${x.state === "done" ? `<button class="icon-btn" data-a="show" aria-label="Show in folder">${cc.icon("copy")}</button>` : ""}
        </div>`).join("") : '<div class="empty">No downloads yet</div>') +
        '<div class="foot"><button class="btn" data-open="cottoncandy://downloads/">All downloads</button></div>';
      box.querySelectorAll(".dl").forEach(el => {
        el.addEventListener("click", e => {
          const a = e.target.closest("[data-a]");
          call("download_action", { id: Number(el.dataset.id), action: a ? a.dataset.a : "open" });
          if (!a) hide();
        });
      });
    },
    extensions(d) {
      const items = (d.info && d.info.items) || [];
      box.innerHTML = `<div class="head">${cc.icon("puzzle")}<h2>Extensions</h2></div>` + (items.length ? items.map(x => `
        <div class="item" data-id="${esc(x.id)}"><span class="t">${esc(x.name)}</span><span class="k">${x.enabled ? "On" : "Off"}</span></div>`).join("")
        : '<div class="empty">Add extensions from the Chrome Web Store or Firefox Add-ons</div>') +
        '<div class="foot"><button class="btn" data-open="cottoncandy://settings/#extensions">Manage extensions</button></div>';
      box.querySelectorAll(".item").forEach(el => el.addEventListener("click", () => { hide(); call("command", { id: "open_extensions" }); }));
    },
    site(d) {
      const s = d.info || {};
      const line = { secure: "Connection is secure. What you send this site is encrypted.", insecure: "Not secure. Don't enter passwords or card numbers here.", internal: "This is a CottonCandy page.", file: "This is a file on this computer." }[s.kind] || "";
      box.innerHTML = `<div class="head">${cc.icon(s.kind === "secure" ? "lock" : s.kind === "insecure" ? "alert" : "info")}<h2>${esc(s.host || s.url || "")}</h2></div>
        <div class="row muted">${esc(line)}</div>
        ${s.host ? `${switchRow("JavaScript", "", !s.scripts_blocked, 'data-f="scripts" data-invert="1"')}
        <div class="item" data-act="site_clear">${cc.icon("x")}<span class="t">Delete cookies and site data</span></div>
        <div class="item" data-act="site_reset">${cc.icon("reload")}<span class="t">Reset permissions</span></div>` : ""}
        <div class="foot"><button class="btn" data-open="cottoncandy://settings/#sites">Site settings</button></div>`;
      box.querySelectorAll("input[data-f]").forEach(el => el.addEventListener("change", async () => {
        const r = await cc.call("site_shields", { host: s.host, field: "scripts", value: !el.checked }).catch(() => null);
        if (r) { current.info = { ...s, scripts_blocked: r.scripts_blocked }; render(current); }
      }));
      box.querySelectorAll("[data-act]").forEach(el => el.addEventListener("click", () => { call(el.dataset.act, { host: s.host }); hide(); }));
    },
  };

  function render(d) {
    current = d;
    box.setAttribute("role", "dialog");
    (views[d.kind] || (() => { box.innerHTML = ""; }))(d);
    box.querySelectorAll("[data-open]").forEach(el => el.addEventListener("click", () => { hide(); call("navigate", { text: el.dataset.open, new_tab: true }); }));
    fit();
  }

  cc.on("show", render);
  cc.on("find_result", r => {
    const el = document.getElementById("findcount");
    if (el) el.textContent = !r.count ? "No matches" : r.active ? `${r.active} of ${r.count}` : `${r.count} found`;
  });
  cc.on("theme", theme => { document.documentElement.dataset.theme = theme; });
  window.addEventListener("keydown", e => {
    if (e.key === "Escape") { if (current && current.kind === "find") call("find", { text: "" }); hide(); }
    if (current && current.kind === "menu" && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      const items = [...box.querySelectorAll(".item")];
      const at = items.indexOf(document.activeElement);
      const next = items[(at + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length];
      if (next) next.focus();
      e.preventDefault();
    }
    if (e.key === "Enter" && document.activeElement.classList.contains("item")) document.activeElement.click();
  });
  window.addEventListener("blur", () => { if (current && current.kind !== "suggest") hide(); });
  // The browser may not have filed this page under its window yet; ask again until it has.
  (function ready() { cc.call("ready", { role: "popup" }).catch(() => setTimeout(ready, 50)); })();
})();
