// Grace, in the side panel: summarize or ask about the page in the active tab, or chat (optionally answered
// from web results, with sources). Everything runs through CherryCola on this computer. Answers are shown as
// plain text: only the source links, which open in a new tab, are clickable.
(() => {
  const $ = id => document.getElementById(id);
  const esc = cc.esc;
  let status = {};
  let page = {};
  let history = []; // Grace Chat turns: {role, content, sources?}
  let busy = { page: false, chat: false };

  $("spark").innerHTML = cc.icon("spark");
  $("close").innerHTML = cc.icon("x");
  $("summarize").innerHTML = cc.icon("spark") + "<span>Summarize this page</span>";
  $("explain").innerHTML = cc.icon("alert") + "<span>Explain this error</span>";

  function setMode(mode) {
    document.querySelectorAll(".seg button").forEach(b => b.classList.toggle("on", b.dataset.mode === mode));
    $("page").classList.toggle("hidden", mode !== "page");
    $("chat").classList.toggle("hidden", mode !== "chat");
    $("foot").classList.toggle("hidden", mode !== "page");
    if (mode === "chat") $("input").focus();
  }
  document.querySelectorAll(".seg button").forEach(b => b.addEventListener("click", () => setMode(b.dataset.mode)));
  $("close").addEventListener("click", () => cc.call("grace_close"));

  // ---- This page ----
  function drawPage() {
    const web = page.kind === "web", error = page.kind === "error";
    $("about").textContent = web ? (page.title || page.url) : error ? "This page couldn't be opened: " + (page.code || "") : "Open a web page to summarize it or ask about it.";
    $("summarize").classList.toggle("hidden", !web);
    $("question").classList.toggle("hidden", !web);
    $("explain").classList.toggle("hidden", !error);
  }
  function showAnswer(text, waiting, plain) {
    const box = $("answer");
    box.classList.remove("hidden");
    box.classList.toggle("wait", !!waiting);
    box.innerHTML = waiting ? `<span class="dots">${esc(text)}</span>` : esc(text);
    $("pageCopy").classList.toggle("hidden", !!waiting || !text || !!plain);
  }
  async function runPage(action) {
    if (busy.page) return;
    const question = $("question").value.trim();
    if (action === "ask" && !question) return;
    busy.page = true;
    $("pageStop").classList.remove("hidden");
    showAnswer(action === "summarize" ? "Reading the page" : "Thinking", true);
    try { await cc.call("grace_page", { action, question }); }
    catch (e) { finishPage(); showAnswer(e.message, false, true); }
  }
  function finishPage() { busy.page = false; $("pageStop").classList.add("hidden"); }
  $("summarize").addEventListener("click", () => runPage("summarize"));
  $("explain").addEventListener("click", () => runPage("error"));
  $("question").addEventListener("keydown", e => { if (e.key === "Enter") runPage("ask"); });
  $("pageStop").addEventListener("click", () => cc.call("grace_stop", { slot: "page" }));
  $("pageCopy").addEventListener("click", () => navigator.clipboard.writeText($("answer").textContent).catch(() => {}));

  // ---- Chat ----
  function drawChat(pending) {
    const log = $("log");
    if (!history.length && !pending) {
      log.innerHTML = `<div class="empty-chat">${cc.icon("spark")}Ask Grace anything.<br>${status.web ? "Turn on Web results for answers with sources." : ""}</div>`;
      return;
    }
    log.innerHTML = history.map((t, i) => {
      const sources = (t.sources || []).map(s => `<a data-url="${esc(s[1])}" title="${esc(s[1])}">[${s[0]}] ${esc(s[1])}</a>`).join("");
      return `<div class="msg ${t.role}" data-i="${i}">${esc(t.content)}${sources ? `<div class="sources">${sources}</div>` : ""}</div>`;
    }).join("") + (pending ? `<div class="msg assistant ${pending.text ? "" : "wait"}">${pending.text ? esc(pending.text) : '<span class="dots">Thinking</span>'}</div>` : "");
    log.querySelectorAll("a[data-url]").forEach(a => a.addEventListener("click", () => cc.call("navigate", { text: a.dataset.url, new_tab: true })));
    log.scrollTop = log.scrollHeight;
  }
  async function send(text) {
    text = (text ?? $("input").value).trim();
    if (!text || busy.chat) return;
    $("input").value = "";
    const prior = history.filter(t => t.role === "user" || t.role === "assistant").map(t => ({ role: t.role, content: t.content }));
    history.push({ role: "user", content: text });
    busy.chat = true;
    $("chatStop").classList.remove("hidden");
    drawChat({ text: "" });
    try { await cc.call("grace_chat", { history: prior, question: text, web: $("web").checked }); }
    catch (e) { finishChat(); history.push({ role: "err", content: e.message }); drawChat(); }
  }
  function finishChat() { busy.chat = false; $("chatStop").classList.add("hidden"); }
  $("send").addEventListener("click", () => send());
  $("input").addEventListener("keydown", e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); } });
  $("input").addEventListener("input", e => { e.target.style.height = "40px"; e.target.style.height = Math.min(140, e.target.scrollHeight + 2) + "px"; });
  $("chatStop").addEventListener("click", () => cc.call("grace_stop", { slot: "chat" }));
  $("newChat").addEventListener("click", () => { if (busy.chat) cc.call("grace_stop", { slot: "chat" }); history = []; drawChat(); $("input").focus(); });

  // ---- What the browser tells us ----
  cc.on("grace_partial", d => {
    if (d.slot === "page" && busy.page) showAnswer(d.text, false);
    if (d.slot === "chat" && busy.chat) drawChat({ text: d.text });
  });
  cc.on("grace_done", d => {
    if (d.slot === "page") { finishPage(); showAnswer(d.text || "Grace didn't answer."); }
    if (d.slot === "chat") {
      finishChat();
      history.push({ role: "assistant", content: d.text || "Grace didn't answer.", sources: d.sources || [] });
      if (d.note) history.push({ role: "note", content: d.note });
      drawChat();
    }
  });
  cc.on("grace_error", d => {
    if (d.slot === "page") { finishPage(); showAnswer(d.message, false, true); }
    if (d.slot === "chat") { finishChat(); history.push({ role: "err", content: d.message }); drawChat(); }
  });
  cc.on("grace_page", p => { page = p || {}; drawPage(); });
  cc.on("grace_ask", q => { setMode("chat"); send(q); });

  (async () => {
    for (let i = 0; i < 40 && !status.page; i++) {
      // The browser may not have filed this panel under its window yet; ask again until it has.
      status = await cc.call("grace_ready").catch(() => ({}));
      if (!status.page) await new Promise(r => setTimeout(r, 50));
    }
    page = status.page || {};
    $("web").checked = !!status.web_default && !!status.web;
    $("web").disabled = !status.web;
    if (!status.web) $("web").parentElement.title = "This CherryCola can't search the web yet. Update the cherrycolallm package.";
    drawPage();
    drawChat();
    if (status.message) { history.push({ role: "note", content: status.message }); drawChat(); showAnswer(status.message, false, true); }
    for (const q of status.pending || []) { setMode("chat"); send(q); }
  })();
})();
