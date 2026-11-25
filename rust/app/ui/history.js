(() => {
  const $ = id => document.getElementById(id);
  let timer = 0;
  const dayName = t => {
    const d = new Date(t * 1000), today = new Date();
    const y = new Date(); y.setDate(today.getDate() - 1);
    if (d.toDateString() === today.toDateString()) return "Today";
    if (d.toDateString() === y.toDateString()) return "Yesterday";
    return d.toLocaleDateString([], { weekday: "long", month: "long", day: "numeric", year: d.getFullYear() === today.getFullYear() ? undefined : "numeric" });
  };
  async function load() {
    const r = await cc.call("history", { query: $("q").value, limit: 500 });
    if (r.private) { $("note").textContent = "This private window doesn't save history."; }
    if (!r.items.length) { $("list").innerHTML = `<div class="empty">${cc.icon("clock")}${$("q").value ? "Nothing matches" : "Pages you visit show up here"}</div>`; return; }
    let html = "", last = "";
    for (const e of r.items) {
      const day = dayName(e.time);
      if (day !== last) { if (last) html += "</div>"; html += `<div class="day">${cc.esc(day)}</div><div class="card">`; last = day; }
      html += `<div class="row" data-id="${e.id}"><span class="time">${new Date(e.time * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span>
        <img class="fav" src="/favicon?url=${encodeURIComponent(e.url)}" alt="">
        <a class="main" href="${cc.esc(e.url)}"><div class="title">${cc.esc(e.title || e.url)}</div><div class="sub">${cc.esc(e.url)}</div></a>
        <button class="icon-btn act" aria-label="Remove from history" data-del="${e.id}">${cc.icon("x")}</button></div>`;
    }
    $("list").innerHTML = html + "</div>";
    $("list").querySelectorAll("[data-del]").forEach(b => b.addEventListener("click", async () => {
      await cc.call("history_delete", { id: Number(b.dataset.del) });
      b.closest(".row").remove();
    }));
  }
  $("q").addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(load, 120); });
  $("clear").addEventListener("click", () => cc.call("command", { id: "clear_data" }));
  load();
})();
