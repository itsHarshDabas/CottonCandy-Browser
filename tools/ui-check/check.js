// Checks a running CottonCandy over the DevTools protocol (it must run with CC_DEBUG_PORT=9222).
//   node check.js loaded                   the toolbar and new tab page loaded with their styles and scripts
//   node check.js value <page> <id> <text> the input #id on the page whose address contains <page> holds <text>
// Exits 1 with a reason when the check fails. Used by release.yml on Windows.
const { chromium } = require("playwright-core");
const [mode, ...args] = process.argv.slice(2);
const fail = why => { console.error("FAILED: " + why); process.exit(1); };

(async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
  const pages = browser.contexts().flatMap(c => c.pages());
  const find = part => pages.find(p => p.url().includes(part));
  if (mode === "loaded") {
    for (const part of ["cottoncandy://ui/chrome.html", "cottoncandy://newtab/"]) {
      const page = find(part);
      if (!page) fail(`no page at ${part}`);
      const state = await page.evaluate(() => ({
        bridge: typeof cc,
        styles: [...document.styleSheets].filter(s => s.href).map(s => { try { return s.cssRules.length > 0; } catch { return false; } }),
        text: document.body.innerText,
      }));
      console.log(part, JSON.stringify({ bridge: state.bridge, styles: state.styles, text: state.text.slice(0, 60) }));
      if (state.bridge !== "object") fail(`${part}: its script didn't load`);
      if (!state.styles.length || state.styles.includes(false)) fail(`${part}: a style sheet didn't load`);
    }
    const chrome = find("cottoncandy://ui/chrome.html");
    if (!(await chrome.evaluate(() => document.body.innerText)).includes("New tab")) fail("the tab strip shows no tab");
  } else if (mode === "value") {
    const [part, id, want] = args;
    const values = [];
    for (const page of pages.filter(p => p.url().includes(part))) {
      values.push(await page.evaluate(id => document.getElementById(id)?.value ?? null, id));
    }
    console.log(`${part} #${id}: ${JSON.stringify(values)}`);
    if (!values.includes(want)) fail(`#${id} should hold ${JSON.stringify(want)}`);
  } else {
    fail("unknown check " + mode);
  }
  process.exit(0);
})().catch(e => fail(String(e)));
