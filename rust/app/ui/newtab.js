(() => {
  const $ = id => document.getElementById(id);
  $("f").insertAdjacentHTML("afterbegin", cc.icon("search"));
  const tick = () => {
    const now = new Date();
    $("clock").textContent = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    $("date").textContent = now.toLocaleDateString([], { weekday: "long", month: "long", day: "numeric" });
  };
  $("f").addEventListener("submit", e => {
    e.preventDefault();
    const text = $("q").value.trim();
    if (text) cc.call("navigate", { text, typed: true, here: true });
  });
  // Typing here moves to the address bar, like other browsers, so suggestions work the same way.
  $("q").addEventListener("input", () => {
    const text = $("q").value;
    $("q").value = "";
    cc.call("focus_address", { text });
  });
  cc.call("newtab_info", {}).then(info => {
    $("bg").className = info.background;
    if (info.private) { $("private").classList.remove("hidden"); $("tiles").classList.add("hidden"); $("bg").className = "plain"; }
    if (info.show_clock) { tick(); setInterval(tick, 10000); } else $("clockbox").classList.add("hidden");
    if (info.show_shortcuts && !info.private) {
      $("tiles").innerHTML = info.tiles.map(t => {
        const letter = (t.host || "?").replace(/^www\./, "")[0].toUpperCase();
        return `<a class="tile" href="${cc.esc(t.url)}" title="${cc.esc(t.title)}\n${cc.esc(t.url)}">
          <span class="ic">${t.icon ? `<img src="${cc.esc(t.icon)}" alt="">` : cc.esc(letter)}</span><span class="nm">${cc.esc(t.title || t.host)}</span></a>`;
      }).join("");
    }
    if (info.blocked_total) $("stats").textContent = `CottonCandy has blocked ${info.blocked_total.toLocaleString()} ads and trackers`;
    setTimeout(() => $("q").focus(), 0);
  }).catch(() => {});
})();
