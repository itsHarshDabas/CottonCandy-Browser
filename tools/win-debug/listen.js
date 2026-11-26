// Records key events in every page, so a later read shows which page the keyboard reached.
const { chromium } = require("playwright-core");
const mode = process.argv[2];
(async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
  for (const p of browser.contexts().flatMap(c => c.pages())) {
    if (mode === "start") {
      await p.evaluate(() => { window.__keys = []; for (const t of ["keydown", "keypress", "beforeinput", "input"]) window.addEventListener(t, e => window.__keys.push(t + ":" + (e.key || e.data || "") + "@" + (e.target.id || e.target.tagName)), true); }).catch(e => console.log(p.url(), String(e)));
    } else {
      console.log(p.url(), JSON.stringify(await p.evaluate(() => ({ keys: (window.__keys || []).join(" "), focus: document.activeElement && (document.activeElement.id || document.activeElement.tagName), vis: document.visibilityState }))));
    }
  }
  process.exit(0);
})().catch(e => { console.error(e); process.exit(1); });
