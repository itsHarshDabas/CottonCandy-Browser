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
  // Readable on any picture: a light picture gets dark text and light glass, a dark one the reverse,
  // judged from the middle of the picture, where the clock and search box sit.
  async function adapt(src) {
    try {
      const img = new Image();
      img.src = src;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = c.height = 24;
      const g = c.getContext("2d", { willReadFrequently: true });
      const w = img.naturalWidth, h = img.naturalHeight;
      g.drawImage(img, w * .2, h * .15, w * .6, h * .7, 0, 0, 24, 24);
      const px = g.getImageData(0, 0, 24, 24).data;
      let lum = 0;
      for (let i = 0; i < px.length; i += 4) lum += (.2126 * px[i] + .7152 * px[i + 1] + .0722 * px[i + 2]) / 255;
      lum /= px.length / 4;
      document.documentElement.dataset.picture = lum > .58 ? "light" : "dark";
    } catch (e) { /* the picture didn't load: keep the theme's colors */ }
  }
  cc.call("newtab_info", {}).then(info => {
    $("bg").className = info.background;
    if (info.background === "image") {
      const src = "/wallpaper?v=" + encodeURIComponent(info.wallpaper_version || "");
      $("bg").style.backgroundImage = `url("${src}")`;
      if (info.adaptive) adapt(src);
    }
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
