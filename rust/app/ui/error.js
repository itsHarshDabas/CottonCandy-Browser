(() => {
  const $ = id => document.getElementById(id);
  const q = new URLSearchParams(location.search);
  const url = q.get("url") || "", code = q.get("code") || "", kind = q.get("kind") || "";
  let host = url; try { host = new URL(url).host || url; } catch (e) { /* keep the text */ }
  $("ic").innerHTML = cc.icon(kind === "blocked" ? "shield" : "alert");
  const texts = {
    blocked: ["CottonCandy blocked this page", `${host} is on a filter list you use, so Shields stopped it. You can lower Shields for this site from the Shields button.`],
    https: ["This site doesn't offer a secure connection", `HTTPS-only mode is on, and ${host} can't be loaded over HTTPS. Anyone on the network could see or change this page.`],
    navigation: ["CottonCandy stopped this", "A page tried to open something it isn't allowed to (a local file or a CottonCandy page)."],
  };
  const [t, m] = texts[kind] || ["This page can't be reached", `${host} didn't answer. Check the address, your connection, or the proxy and DNS settings.`];
  $("t").textContent = t; $("m").textContent = m; $("code").textContent = code;
  document.title = t;
  $("retry").textContent = kind === "https" ? "Continue to the insecure site" : "Try again";
  $("retry").addEventListener("click", () => cc.call("error_retry", { url, kind }));
  $("back").addEventListener("click", () => history.length > 1 ? history.back() : cc.call("nav", { action: "home" }));
})();
