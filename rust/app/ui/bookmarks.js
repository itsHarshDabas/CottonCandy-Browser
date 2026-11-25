(() => {
  const $ = id => document.getElementById(id);
  let timer = 0;
  async function load() {
    const r = await cc.call("bookmarks", { query: $("q").value });
    if (!r.items.length) { $("list").innerHTML = `<div class="empty">${cc.icon("star")}${$("q").value ? "Nothing matches" : "Press the star in the address bar (Ctrl+D) to bookmark a page"}</div>`; return; }
    $("list").innerHTML = r.items.map(e => `<div class="row"><img class="fav" src="/favicon?url=${encodeURIComponent(e.url)}" alt="">
      <a class="main" href="${cc.esc(e.url)}"><div class="title">${cc.esc(e.title || e.url)}</div><div class="sub">${cc.esc(e.url)}</div></a>
      <button class="icon-btn act" aria-label="Delete bookmark" data-url="${cc.esc(e.url)}">${cc.icon("x")}</button></div>`).join("");
    $("list").querySelectorAll("[data-url]").forEach(b => b.addEventListener("click", async () => {
      await cc.call("bookmark_remove", { url: b.dataset.url });
      load();
    }));
  }
  const note = text => { $("note").textContent = text; };
  $("q").addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(load, 120); });
  $("import").addEventListener("click", () => cc.call("bookmarks_import", {}).then(r => { note(r.count ? `Imported ${r.count} bookmarks` : ""); load(); }).catch(e => note(e.message)));
  $("export").addEventListener("click", () => cc.call("bookmarks_export", {}).then(r => note(r.path ? `Saved to ${r.path}` : "")).catch(e => note(e.message)));
  load();
})();
