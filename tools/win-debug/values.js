// Prints what the address bar and the new tab's search box hold, and which element has focus.
const { chromium } = require("playwright-core");
(async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
  for (const p of browser.contexts().flatMap(c => c.pages())) {
    const v = await p.evaluate(() => ({ focus: document.activeElement && (document.activeElement.id || document.activeElement.tagName), hasFocus: document.hasFocus(),
      inputs: [...document.querySelectorAll("input")].map(i => `${i.id || i.name || "input"}=${JSON.stringify(i.value)}`) })).catch(e => String(e));
    console.log(p.url(), JSON.stringify(v));
  }
  process.exit(0);
})().catch(e => { console.error(e); process.exit(1); });
