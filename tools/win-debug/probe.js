// Connects to a running CottonCandy over DevTools and reports what each of its pages looks like.
const { chromium } = require("playwright-core");
const fs = require("fs");
const out = process.argv[2];
(async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
  const report = [];
  const pages = browser.contexts().flatMap(c => c.pages());
  let i = 0;
  for (const p of pages) {
    const logs = [];
    p.on("console", m => logs.push(`${m.type()}: ${m.text()}`));
    p.on("pageerror", e => logs.push(`pageerror: ${e.message}`));
    const info = { url: p.url() };
    try {
      info.viewport = await p.evaluate(() => ({ w: innerWidth, h: innerHeight, dpr: devicePixelRatio, ua: navigator.userAgent }));
      info.text = await p.evaluate(() => document.body ? document.body.innerText.slice(0, 400) : null);
      info.html = await p.evaluate(() => document.documentElement.outerHTML.length);
      info.styles = await p.evaluate(() => [...document.styleSheets].map(s => { try { return s.href + " rules=" + s.cssRules.length; } catch (e) { return s.href + " " + e.message; } }));
      info.fonts = await p.evaluate(() => getComputedStyle(document.body).fontFamily + " | color " + getComputedStyle(document.body).color);
      info.svg = await p.evaluate(() => { const s = document.querySelector("svg"); if (!s) return null; const r = s.getBoundingClientRect(); return { w: r.width, h: r.height, path: s.innerHTML.slice(0, 80) }; });
      await p.screenshot({ path: `${out}/page-${i}.png` });
    } catch (e) { info.error = String(e); }
    await p.waitForTimeout(1500);
    info.logs = logs;
    report.push(info);
    i++;
  }
  // Ask the toolbar page to call the browser and time the answer.
  const chrome = pages.find(p => p.url().includes("chrome.html"));
  if (chrome) {
    report.push({ ready: await chrome.evaluate(async () => {
      const t = Date.now();
      try { const r = await Promise.race([cc.call("ready", { role: "chrome" }), new Promise((_, no) => setTimeout(() => no(new Error("no answer in 5s")), 5000))]); return { ms: Date.now() - t, r: JSON.stringify(r).slice(0, 300) }; }
      catch (e) { return { ms: Date.now() - t, error: e.message }; }
    }) });
  }
  fs.writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  process.exit(0);
})().catch(e => { console.error(e); process.exit(1); });
