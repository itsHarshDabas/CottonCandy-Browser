cc.call("about", {}).then(a => {
  document.getElementById("v").textContent = `Version ${a.version}`;
  document.getElementById("facts").innerHTML = Object.entries(a.facts).map(([k, v]) =>
    `<div class="row"><div class="main"><div class="sub">${cc.esc(k)}</div><div class="title" style="white-space:normal;word-break:break-all">${cc.esc(v)}</div></div></div>`).join("");
});
