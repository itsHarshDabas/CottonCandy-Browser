(() => {
  const $ = id => document.getElementById(id);
  function render(items) {
    if (!items.length) { $("list").innerHTML = `<div class="empty">${cc.icon("download")}Files you download show up here</div>`; return; }
    $("list").innerHTML = items.map(x => `<div class="row" data-id="${x.id}">
      <div class="main"><div class="title">${cc.esc(x.name)}</div><div class="sub">${cc.esc(x.status)} · ${cc.esc(x.url)}</div>
      ${x.state === "progress" ? `<div class="bar"><i style="width:${x.percent}%"></i></div>` : ""}</div>
      ${x.warning ? `<span class="tag" style="color:var(--warn)">${cc.esc(x.warning)}</span>` : ""}
      ${x.state === "progress" ? `<button class="btn" data-a="${x.paused ? "resume" : "pause"}">${x.paused ? "Resume" : "Pause"}</button><button class="btn" data-a="cancel">Cancel</button>` : ""}
      ${x.state === "done" ? `<button class="btn" data-a="open">Open</button><button class="btn" data-a="show">Show in folder</button>` : ""}
      ${x.state !== "progress" ? `<button class="icon-btn act" data-a="remove" aria-label="Remove from list">${cc.icon("x")}</button>` : ""}</div>`).join("");
    $("list").querySelectorAll("[data-a]").forEach(b => b.addEventListener("click", () =>
      cc.call("download_action", { id: Number(b.closest(".row").dataset.id), action: b.dataset.a }).then(load)));
  }
  const load = () => cc.call("downloads", {}).then(r => render(r.items));
  cc.on("downloads", render);
  $("folder").addEventListener("click", () => cc.call("download_action", { id: 0, action: "folder" }));
  $("clear").addEventListener("click", () => cc.call("download_action", { id: 0, action: "clear" }).then(load));
  load();
  setInterval(load, 1000);
})();
