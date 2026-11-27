// Checks a running CottonCandy over the DevTools protocol (it must run with CC_DEBUG_PORT=9222).
//   node check.js loaded                   the toolbar and new tab page loaded with their styles and scripts
//   node check.js value <page> <id> <text> the input #id on the page whose address contains <page> holds <text>
//   node check.js windows                  says what Chromium draws with (the graphics card or the processor)
//   node check.js mp4 <folder url> <yes|no> sample.mp4 from that folder plays (H.264 picture, AAC sound), or doesn't
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
  } else if (mode === "windows") {
    // Without the manifest's supportedOS, Windows says it is 8 (NT 6.2) and Chromium keeps the GPU off.
    const page = find("cottoncandy://newtab/") || pages[0];
    const ua = await page.evaluate(async () => navigator.userAgent + " platformVersion=" +
      (await navigator.userAgentData.getHighEntropyValues(["platformVersion"])).platformVersion);
    const cdp = await browser.newBrowserCDPSession();
    const info = await cdp.send("SystemInfo.getInfo");
    console.log("userAgent", ua);
    console.log("gpu", JSON.stringify(info.gpu.featureStatus), JSON.stringify(info.gpu.devices.map(d => d.deviceString)));
    // GPU process start time: 0 when it never started (what Chromium shows on Windows 8).
    console.log("gpu process", JSON.stringify(info.gpu.auxAttributes?.initializationTime ?? null), info.gpu.driverBugWorkarounds?.length);
  } else if (mode === "mp4") {
    const [folder, want, file] = args;
    const page = find(folder) || find("cottoncandy://newtab/");
    if (!page) fail("no page to play the video in");
    if (page.url() !== folder) await page.goto(folder, { waitUntil: "load" });
    const got = await page.evaluate(async file => {
      const bytes = await (await fetch(file)).arrayBuffer();
      const picture = await new Promise(done => {
        const v = document.createElement("video");
        v.muted = true;
        v.onloadeddata = () => done(v.videoWidth > 0 ? `${v.videoWidth}x${v.videoHeight}` : "");
        v.onerror = () => done("");
        setTimeout(() => done(""), 10000);
        v.src = URL.createObjectURL(new Blob([bytes]));
      });
      let sound = "";
      try {
        const a = await new OfflineAudioContext(1, 44100, 44100).decodeAudioData(bytes.slice(0));
        sound = a.duration > 0.5 ? `${a.duration.toFixed(2)} s` : "";
      } catch {}
      return { picture, sound, canPlay: document.createElement("video").canPlayType('video/mp4; codecs="avc1.42E01E, mp4a.40.2"') };
    }, file || "sample.mp4");
    console.log("mp4", JSON.stringify(got));
    const plays = !!(got.picture && got.sound);
    if (want === "yes" && !plays) fail("the MP4 doesn't play (H.264 picture or AAC sound missing)");
    if (want === "no" && (got.picture || got.sound)) fail("the stock build shouldn't play MP4");
  } else {
    fail("unknown check " + mode);
  }
  process.exit(0);
})().catch(e => fail(String(e)));
