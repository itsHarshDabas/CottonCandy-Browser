// The bridge between CottonCandy's own pages and the browser. Pages call `cc.call(name, args)`; the browser
// answers, and pushes changes with `cc.emit(event, data)`. Only CottonCandy's own pages can reach /api/:
// the browser checks the page's address and a per-session token before doing anything.
(() => {
  const token = document.querySelector('meta[name="cc-token"]')?.content || "";
  const handlers = new Map();
  window.cc = {
    async call(name, args = {}) {
      const response = await fetch("/api/" + encodeURIComponent(name), {
        method: "POST",
        headers: { "Content-Type": "application/json", "X-CC-Token": token },
        body: JSON.stringify(args),
      });
      const text = await response.text();
      const data = text ? JSON.parse(text) : null;
      if (!response.ok) throw new Error((data && data.error) || response.statusText);
      return data;
    },
    on(event, fn) {
      if (!handlers.has(event)) handlers.set(event, []);
      handlers.get(event).push(fn);
    },
    emit(event, data) {
      for (const fn of handlers.get(event) || []) {
        try { fn(data); } catch (e) { console.error(e); }
      }
    },
    esc(text) {
      return String(text ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
    },
    icon(name) { return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="${ICONS[name] || ""}"/></svg>`; },
  };
  const ICONS = {
    back: "M15 18l-6-6 6-6", up: "M6 15l6-6 6 6", down: "M6 9l6 6 6-6", forward: "M9 18l6-6-6-6", reload: "M20 11a8 8 0 1 0-2.3 5.6M20 4v7h-7",
    stop: "M6 6l12 12M18 6L6 18", plus: "M12 5v14M5 12h14", x: "M6 6l12 12M18 6L6 18",
    star: "M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z",
    shield: "M12 3l7 3v5c0 4.5-3 8.3-7 10-4-1.7-7-5.5-7-10V6z", spark: "M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8zM19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z",
    menu: "M4 7h16M4 12h16M4 17h16", download: "M12 4v11M7 10l5 5 5-5M5 20h14", lock: "M7 11V8a5 5 0 0 1 10 0v3M6 11h12v9H6z",
    info: "M12 8h.01M11 12h1v5h1M12 21a9 9 0 1 1 0-18 9 9 0 0 1 0 18z", alert: "M12 9v4M12 17h.01M10.3 3.9L2 18a2 2 0 0 0 1.7 3h16.6a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z",
    search: "M11 18a7 7 0 1 1 0-14 7 7 0 0 1 0 14zM20 20l-4-4", globe: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18",
    import: "M10 17l5-5-5-5M15 12H3M14 3h5a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-5",
    clock: "M12 7v5l3 2M12 21a9 9 0 1 1 0-18 9 9 0 0 1 0 18z", puzzle: "M10 4a2 2 0 0 1 4 0v2h4v4h-2a2 2 0 0 0 0 4h2v4h-4v-2a2 2 0 0 0-4 0v2H6v-4h2a2 2 0 0 0 0-4H6V6h4z",
    vpn: "M12 3l7 3v5c0 4.5-3 8.3-7 10-4-1.7-7-5.5-7-10V6zM9 12l2 2 4-4", min: "M6 12h12", max: "M6 6h12v12H6z", home: "M4 11l8-7 8 7v9h-5v-6H9v6H4z",
    audio: "M5 10v4h3l5 4V6L8 10zM16 9a4 4 0 0 1 0 6", muted: "M5 10v4h3l5 4V6L8 10zM17 9l4 6M21 9l-4 6", pin: "M15 4l5 5-3 1-4 4 1 5-2 1-3-4-5 5M9 15l-3-3 1-2 5 1 4-4 1-3",
    tabs: "M4 6h16v14H4zM4 10h16", sidebar: "M4 5h16v14H4zM15 5v14", copy: "M8 8h11v12H8zM5 16V4h11",
    code: "M9 7l-5 5 5 5M15 7l5 5-5 5", script: "M8 4h9l3 3v13H8zM4 8v12h4", eye: "M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
  };
  window.addEventListener("keydown", e => { if (e.key === "F5" || (e.ctrlKey && e.key === "r")) e.preventDefault(); }, true);
})();
