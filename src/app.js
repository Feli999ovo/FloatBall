/* 悬浮球前端。系统能力都通过 window.__TAURI__ 调到 Rust（src-tauri/src/lib.rs）。 */
"use strict";

const T = window.__TAURI__;
const { invoke, Channel } = T.core;
const { listen } = T.event;
const appWin = T.window.getCurrentWindow();
const { LogicalSize, LogicalPosition } = T.dpi;

// 前端出错时打到终端
const jslog = (m) => invoke("js_log", { msg: String(m) }).catch(() => {});
window.addEventListener("error", (e) => jslog(e.message + " @" + e.lineno));
window.addEventListener("unhandledrejection", (e) => jslog("promise: " + (e.reason && e.reason.message ? e.reason.message + " " : "") + ((e.reason && e.reason.stack) || e.reason)));

const $ = (s) => document.querySelector(s);
const body = document.body;
const orb = $("#orb");
const ask = $("#ask");

// 尺寸（逻辑像素）
const PAD = 12, ORB = 72;
const SMALL = ORB + PAD * 2; // 收起时窗口 96×96
const BIG_W = 420, BIG_H = 600;

const S = {
  open: false,
  pinned: false,
  busy: null,          // 正在生成的请求 id
  convId: null,        // 历史里的对话 id
  msgs: [],            // 发给模型的消息 {role, content}
  attach: [],          // 待发送的附件 {kind:'image'|'file', url|name,text}
  anchor: null,        // 收起时小球窗口左上角（逻辑坐标）
  cfg: null,
  lastActive: Date.now(),
};

/* ───────── 小球表情 ───────── */
let doneTimer = 0;
function mood(name, ms) {
  orb.className = orb.className.replace(/\bs-\w+/g, "").trim() + " s-" + name;
  clearTimeout(doneTimer);
  if (ms) doneTimer = setTimeout(() => mood(S.open ? (document.activeElement === ask ? "listen" : "idle") : "idle"), ms);
}
function poke() {
  orb.classList.remove("poke"); void orb.offsetWidth; orb.classList.add("poke");
}
// 眼睛跟着鼠标（只在窗口范围内能拿到鼠标）
document.addEventListener("mousemove", (e) => {
  S.lastActive = Date.now();
  if (orb.className.includes("s-sleep")) mood("idle");
  const r = orb.getBoundingClientRect();
  const dx = Math.max(-1, Math.min(1, (e.clientX - (r.left + r.width / 2)) / 160));
  const dy = Math.max(-1, Math.min(1, (e.clientY - (r.top + r.height / 2)) / 160));
  orb.style.setProperty("--lx", (dx * 3).toFixed(1) + "px");
  orb.style.setProperty("--ly", (dy * 2).toFixed(1) + "px");
});
orb.addEventListener("mouseenter", () => { if (!S.open && !S.busy && /s-(idle|sleep)/.test(orb.className)) mood("curious"); });
orb.addEventListener("mouseleave", () => { if (!S.open && orb.className.includes("s-curious")) mood("idle"); });
// 10 分钟没动静就睡觉
setInterval(() => {
  if (!S.open && !S.busy && Date.now() - S.lastActive > 10 * 60 * 1000) mood("sleep");
}, 30000);

/* ───────── 窗口：收起 / 展开 / 拖动 / 贴边 ───────── */
async function monitorArea() {
  // 拿不到显示器信息时（个别 Linux 环境）退回到网页的 screen 对象
  const fallback = () => ({ x: screen.availLeft || 0, y: screen.availTop || 0, w: Math.max(screen.availWidth || 0, 1280), h: Math.max(screen.availHeight || 0, 720) });
  try {
    const m = await T.window.currentMonitor();
    if (!m) return fallback();
    const s = m.scaleFactor || 1;
    const a = m.workArea && m.workArea.size && m.workArea.size.width ? m.workArea : { position: m.position, size: m.size };
    return { x: a.position.x / s, y: a.position.y / s, w: a.size.width / s, h: a.size.height / s, mon: { x: m.position.x, y: m.position.y, s } };
  } catch (e) {
    jslog("currentMonitor 失败，改用 screen：" + (e && e.message ? e.message : e));
    return fallback();
  }
}
async function winPos() {
  const p = await appWin.outerPosition();
  const s = await appWin.scaleFactor();
  return { x: p.x / s, y: p.y / s };
}

let morphing = false;
const frame = () => new Promise((r) => { const t = setTimeout(r, 60); requestAnimationFrame(() => { clearTimeout(t); r(); }); });
// 改窗口的大小和位置。窗口往左或往上长（缩）的时候，有一两帧画的还是旧版面，小球会闪到别处；
// 所以这种情况先把内容藏起来，等新版面排好再显示。
async function reshape(w, h, x, y, hide, relayout) {
  if (hide) { body.classList.add("resizing"); await frame(); }
  await appWin.setSize(new LogicalSize(w, h));
  await appWin.setPosition(new LogicalPosition(x, y));
  await new Promise((r) => { // 等网页拿到新的窗口尺寸，最多 300 毫秒
    const ok = () => Math.abs(innerWidth - w) < 2 && Math.abs(innerHeight - h) < 2;
    if (ok()) return r();
    const done = () => { removeEventListener("resize", f); clearTimeout(t); r(); };
    const f = () => { if (ok()) done(); };
    const t = setTimeout(done, 300);
    addEventListener("resize", f);
  });
  relayout();
  if (hide) { await frame(); body.classList.remove("resizing"); }
}
// 卡片的毛玻璃底：网页画不出窗口后面的桌面，所以在展开之前让程序截下窗口将要盖住的那一块、模糊后垫在卡片下面。
// 只有 Windows 有；拿不到（macOS、超时、出错）就用不透明的底。图只在内存里，不保存、不外传。
// shortcut: 底图是展开那一刻截的，之后后面的窗口变了、或者拖动了悬浮球，就退回不透明的底；要实时的话得换成系统原生的毛玻璃。
function grabBackdrop(r, a) {
  if (!a.mon) return Promise.resolve(null);
  const s = a.mon.s, P = (v) => Math.round(v * s);
  const shot = invoke("backdrop", { px: P(r.x + r.w / 2), py: P(r.y + r.h / 2), x: P(r.x) - a.mon.x, y: P(r.y) - a.mon.y, w: P(r.w), h: P(r.h) }).catch(() => null);
  return Promise.race([shot, new Promise((res) => setTimeout(() => res(null), 250))]);
}
async function expand(view) {
  if (morphing) return;
  if (S.open) { showView(view || "chat"); focusAsk(); return; }
  morphing = true;
  try {
    S.anchor = await winPos();
    const a = await monitorArea();
    const cx = S.anchor.x + SMALL / 2;
    const flipX = cx > a.x + a.w / 2;
    // 卡片优先放在胶囊上方（和设计稿一样）；上面放不下时，哪边地方大放哪边。
    // 两边都放不下完整的卡片就把卡片缩短，小球留在原地不动；实在太挤（不到 360）才挪窗口。
    const above = S.anchor.y + SMALL - a.y - 4, below = a.y + a.h - S.anchor.y - 4;
    const flipY = above >= BIG_H || above > below;
    const h = Math.max(360, Math.min(BIG_H, flipY ? above : below));
    let x = flipX ? S.anchor.x + SMALL - BIG_W : S.anchor.x;
    let y = flipY ? S.anchor.y + SMALL - h : S.anchor.y;
    x = Math.max(a.x + 4, Math.min(x, a.x + a.w - BIG_W - 4));
    y = Math.max(a.y + 4, Math.min(y, a.y + a.h - h - 4));
    const card = $("#card");
    card.classList.remove("glassy");
    const bd = await grabBackdrop({ x, y, w: BIG_W, h }, a);
    await reshape(BIG_W, h, x, y, flipX || flipY, () => {
      body.classList.toggle("flip-x", flipX);
      body.classList.toggle("flip-y", flipY);
    });
    void body.offsetWidth; // 让小球先在新版面里排好，胶囊才是从小球的位置开始拉长
    S.open = true;
    body.classList.remove("collapsed");
    // 设置填到一半点了别处被收起：再点开时接着填，不清空
    const resume = S.resumeSet && !view;
    S.resumeSet = false;
    if (!resume) showView(view || "chat");
    if (bd) {
      card.style.setProperty("--bd", 'url("' + bd + '")');
      card.style.setProperty("--bd-size", BIG_W + "px " + h + "px");
      card.style.setProperty("--bd-pos", -card.offsetLeft + "px " + -card.offsetTop + "px");
      card.classList.add("glassy");
    }
    await appWin.setFocus();
    focusAsk();
    mood("listen");
  } finally { morphing = false; }
}
async function collapse() {
  if (!S.open || morphing) return;
  morphing = true;
  try {
    S.open = false;
    body.classList.add("collapsed");
    await new Promise((r) => setTimeout(r, 340)); // 等胶囊缩回小球
    const flipped = body.classList.contains("flip-x") || body.classList.contains("flip-y");
    const p = S.anchor || (await winPos());
    await reshape(SMALL, SMALL, p.x, p.y, flipped, () => body.classList.remove("flip-x", "flip-y"));
    if (!S.busy) mood("idle");
  } finally { morphing = false; }
}
function toggle() { S.open ? collapse() : expand(); }
function focusAsk() { if (!$("#vChat").hidden) setTimeout(() => ask.focus(), 30); }

// 点住小球：移动超过 4 像素算拖动，否则算点击
let press = null;
orb.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  press = { x: e.screenX, y: e.screenY, dragged: false };
});
window.addEventListener("pointermove", (e) => {
  if (!press || press.dragged) return;
  if (Math.hypot(e.screenX - press.x, e.screenY - press.y) > 4) {
    press.dragged = true;
    S.userMoved = true;
    orb.classList.add("dragging");
    appWin.startDragging().catch(() => {});
  }
});
window.addEventListener("pointerup", () => {
  if (!press) return;
  const wasDrag = press.dragged;
  press = null;
  orb.classList.remove("dragging");
  if (!wasDrag) { poke(); toggle(); }
});
orb.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggle(); } });
orb.addEventListener("contextmenu", (e) => { e.preventDefault(); invoke("show_context_menu").catch(toastErr); });

// 拖完停下 350ms 后：贴边吸附 + 记住位置
let moveTimer = 0;
appWin.onMoved(() => {
  if (morphing) return;
  orb.classList.remove("dragging");
  if (S.open) $("#card").classList.remove("glassy"); // 窗口挪了，底图对不上了
  clearTimeout(moveTimer);
  moveTimer = setTimeout(settle, 350);
});
async function settle() {
  if (morphing) return;
  const p = await winPos();
  if (S.open) {
    // 只有用户在展开状态下亲手拖动时才更新收起位置；展开时程序自己挪窗口（贴着屏幕边缩回来）不算
    if (!S.userMoved) return;
    S.userMoved = false;
    // 展开时拖动了胶囊上的小球：把收起后的位置也跟着挪
    const flipX = body.classList.contains("flip-x"), flipY = body.classList.contains("flip-y");
    const size = await appWin.outerSize(), s = await appWin.scaleFactor();
    const w = size.width / s, h = size.height / s;
    S.anchor = { x: flipX ? p.x + w - SMALL : p.x, y: flipY ? p.y + h - SMALL : p.y };
    invoke("save_position", S.anchor).catch(() => {});
    return;
  }
  S.userMoved = false;
  const a = await monitorArea();
  let { x, y } = p;
  const SNAP = 24;
  if (x - a.x < SNAP) x = a.x - PAD + 2;
  if (a.x + a.w - (x + SMALL) < SNAP) x = a.x + a.w - SMALL + PAD - 2;
  y = Math.max(a.y - PAD + 2, Math.min(y, a.y + a.h - SMALL + PAD - 2));
  if (x !== p.x || y !== p.y) {
    morphing = true;
    await appWin.setPosition(new LogicalPosition(x, y));
    setTimeout(() => (morphing = false), 60);
  }
  invoke("save_position", { x, y }).catch(() => {});
}

// 点别处：没在生成、没固定、输入框是空的 → 收起
appWin.onFocusChanged(({ payload: focused }) => {
  if (focused || !S.open || S.pinned || S.busy) return;
  if (ask.value.trim() || S.attach.length) return;
  setTimeout(() => { if (!document.hasFocus()) { S.resumeSet = !$("#vSet").hidden; collapse(); } }, 150);
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && S.open) {
    if (S.busy) stop();
    else if (!$("#vChat").hidden) collapse();
    else showView("chat");
  }
});

/* ───────── 视图 ───────── */
function showView(v) {
  $("#vChat").hidden = v !== "chat";
  $("#vHist").hidden = v !== "hist";
  $("#vSet").hidden = v !== "set";
  $("#histBtn").setAttribute("aria-pressed", v === "hist");
  $("#setBtn").setAttribute("aria-pressed", v === "set");
  if (v === "hist") loadHistory();
  if (v === "set") loadSettings();
  if (v === "chat") updateEmpty();
}
$("#closeBtn").onclick = collapse;
$("#newBtn").onclick = () => { newChat(); showView("chat"); focusAsk(); };
$("#histBtn").onclick = () => showView($("#vHist").hidden ? "hist" : "chat");
$("#setBtn").onclick = () => showView($("#vSet").hidden ? "set" : "chat");
$("#pinBtn").onclick = () => {
  S.pinned = !S.pinned;
  $("#pinBtn").setAttribute("aria-pressed", S.pinned);
  toast(S.pinned ? "已固定，点别处不会收起" : "取消固定");
};

let toastTimer = 0;
function toast(t) {
  const el = $("#toast");
  el.textContent = t; el.classList.add("on");
  clearTimeout(toastTimer); toastTimer = setTimeout(() => el.classList.remove("on"), 2600);
}
function toastErr(e) { toast(String(e)); mood("error", 2400); }

/* ───────── Markdown（够用的子集，先转义再排版） ───────── */
function esc(s) { return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])); }
function inline(s) {
  return s
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
    .replace(/(^|[^*])\*([^*\s][^*]*)\*/g, "$1<i>$2</i>")
    .replace(/\[([^\]]+)\]\((https?:[^)\s]+)\)/g, '<u title="$2">$1</u>');
}
function md(src) {
  const out = [];
  const parts = src.split(/```/);
  parts.forEach((part, i) => {
    if (i % 2 === 1) {
      const code = part.replace(/^[\w+-]*\n/, "");
      out.push("<pre><code>" + esc(code.replace(/\n$/, "")) + "</code></pre>");
      return;
    }
    let list = null;
    const close = () => { if (list) { out.push("</" + list + ">"); list = null; } };
    let para = [];
    const flush = () => { if (para.length) { out.push("<p>" + para.map((l) => inline(esc(l))).join("<br>") + "</p>"); para = []; } };
    for (const line of part.split("\n")) {
      let m;
      if (!line.trim()) { flush(); close(); continue; }
      if ((m = line.match(/^(#{1,3})\s+(.*)/))) { flush(); close(); out.push(`<h${m[1].length}>` + inline(esc(m[2])) + `</h${m[1].length}>`); continue; }
      if ((m = line.match(/^\s*[-*•]\s+(.*)/)) || (m = line.match(/^\s*\d+[.)]\s+(.*)/))) {
        flush();
        const kind = /^\s*\d/.test(line) ? "ol" : "ul";
        if (list !== kind) { close(); out.push("<" + kind + ">"); list = kind; }
        out.push("<li>" + inline(esc(m[1])) + "</li>");
        continue;
      }
      close(); para.push(line);
    }
    flush(); close();
  });
  return out.join("");
}

/* ───────── 对话 ───────── */
function needsSetup() { return !!S.cfg && !S.cfg.has_key && !S.cfg.allow_no_key; }
const msgsEl = $("#msgs");
function updateEmpty() {
  $("#empty").hidden = msgsEl.children.length > 0;
  // 还没设置模型时，空白页改成提示去设置（不再一启动就强行打开设置页）
  $("#empty").classList.toggle("setup", needsSetup());
  msgsEl.hidden = msgsEl.children.length === 0;
}
function scrollEnd() { msgsEl.scrollTop = msgsEl.scrollHeight; }
function newChat() {
  if (S.busy) stop();
  S.convId = null; S.msgs = []; msgsEl.innerHTML = "";
  $("#title").textContent = "新对话";
  updateEmpty();
}
function addUserBubble(text, attach) {
  const el = document.createElement("div");
  el.className = "m user";
  for (const a of attach || []) {
    if (a.kind === "image") { const i = document.createElement("img"); i.src = a.url; i.alt = "截图"; el.appendChild(i); }
    if (a.kind === "file") { const f = document.createElement("div"); f.className = "file"; f.textContent = "文件：" + a.name; el.appendChild(f); }
  }
  el.appendChild(document.createTextNode(text));
  msgsEl.appendChild(el);
  updateEmpty(); scrollEnd();
}
function addAiBubble() {
  const el = document.createElement("div");
  el.className = "m ai";
  el.innerHTML = '<div class="think" hidden></div><div class="txt cursor"></div><div class="meta" hidden></div>';
  msgsEl.appendChild(el);
  updateEmpty(); scrollEnd();
  return el;
}
function setMeta(el, text, opts = {}) {
  const meta = el.querySelector(".meta");
  meta.hidden = false; meta.innerHTML = "";
  if (opts.copy) {
    const b = document.createElement("button"); b.textContent = "复制";
    b.onclick = async () => { await navigator.clipboard.writeText(opts.copy); b.textContent = "已复制"; setTimeout(() => (b.textContent = "复制"), 1500); };
    meta.appendChild(b);
  }
  if (opts.retry) { const b = document.createElement("button"); b.textContent = "重试"; b.onclick = opts.retry; meta.appendChild(b); }
  if (opts.settings) { const b = document.createElement("button"); b.textContent = "去设置"; b.onclick = () => showView("set"); meta.appendChild(b); }
  if (text) { const s = document.createElement("span"); s.textContent = text; meta.appendChild(s); }
}

function buildContent(text, attach) {
  const files = attach.filter((a) => a.kind === "file");
  const imgs = attach.filter((a) => a.kind === "image");
  let t = text;
  for (const f of files) t = `文件「${f.name}」的内容：\n\`\`\`\n${f.text}\n\`\`\`\n\n` + t;
  if (!imgs.length) return t;
  return [{ type: "text", text: t || "看看这张截图" }, ...imgs.map((i) => ({ type: "image_url", image_url: { url: i.url } }))];
}
function historyText(text, attach) {
  const tags = attach.map((a) => (a.kind === "image" ? "[截图]" : `[文件：${a.name}]`)).join(" ");
  return (tags ? tags + " " : "") + text;
}

async function send(textArg) {
  const text = (textArg ?? ask.value).trim();
  if ((!text && !S.attach.length) || S.busy) return;
  const attach = S.attach.splice(0);
  renderAttach();
  ask.value = ""; autosize();
  showView("chat");
  addUserBubble(text, attach);
  S.msgs.push({ role: "user", content: buildContent(text, attach) });
  try {
    S.convId = await invoke("history_append", { convId: S.convId, role: "user", content: historyText(text, attach) });
    if (S.msgs.length === 1) $("#title").textContent = text.slice(0, 24) || "截图提问";
  } catch (_) { /* 历史写不进去不影响对话 */ }
  run();
}

function run() {
  const id = "c" + Date.now();
  S.busy = id;
  $("#sendBtn").disabled = true;
  $("#stopRow").hidden = false;
  mood("think");
  const el = addAiBubble();
  const txt = el.querySelector(".txt"), think = el.querySelector(".think");
  let out = "", reasoning = "", usage = null, raf = 0;
  const paint = () => { raf = 0; const live = S.busy === id; txt.innerHTML = live ? md(out + "\uE000").replace("\uE000", '<span class="cur"></span>') : md(out); scrollEnd(); };
  const ch = new Channel();
  ch.onmessage = (ev) => {
    if (S.busy !== id) return;
    if (ev.type === "delta") {
      if (!out) mood("work");
      out += ev.text;
      if (!raf) raf = requestAnimationFrame(paint);
    } else if (ev.type === "reasoning") {
      reasoning += ev.text; think.hidden = false; think.textContent = reasoning; think.scrollTop = think.scrollHeight;
    } else if (ev.type === "usage") {
      usage = ev;
    } else if (ev.type === "done") {
      finish();
      txt.classList.remove("cursor"); paint();
      if (!out) { txt.textContent = "（模型没有返回内容）"; }
      S.msgs.push({ role: "assistant", content: out });
      invoke("history_append", { convId: S.convId, role: "assistant", content: out }).catch(() => {});
      setMeta(el, usage ? `${S.cfg?.model || ""} · ${usage.prompt + usage.completion} tokens` : S.cfg?.model || "", { copy: out });
      mood("done", 1800);
      if (!S.open) poke();
    } else if (ev.type === "error") {
      finish();
      txt.classList.remove("cursor");
      el.classList.add("err");
      txt.textContent = ev.message;
      const noKey = /API Key|设置/.test(ev.message);
      setMeta(el, "", { retry: () => { el.remove(); run(); }, settings: noKey });
      mood("error", 4000);
    }
  };
  function finish() { if (S.busy === id) S.busy = null; $("#sendBtn").disabled = false; $("#stopRow").hidden = true; }
  el._stop = () => {
    invoke("cancel_chat", { id }).catch(() => {});
    finish(); txt.classList.remove("cursor");
    if (out) { txt.innerHTML = md(out); S.msgs.push({ role: "assistant", content: out }); invoke("history_append", { convId: S.convId, role: "assistant", content: out + "\n\n（已停止）" }).catch(() => {}); }
    else txt.textContent = "（已停止）";
    setMeta(el, "已停止", out ? { copy: out } : {});
    mood("idle");
  };
  S.stopFn = el._stop;
  invoke("chat", { id, messages: S.msgs, onEvent: ch }).catch((e) => ch.onmessage({ type: "error", message: String(e) }));
}
function stop() { if (S.busy && S.stopFn) S.stopFn(); }
$("#stopBtn").onclick = stop;

$("#capsule").addEventListener("submit", (e) => { e.preventDefault(); send(); });
ask.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); }
});
function autosize() { ask.style.height = "auto"; ask.style.height = Math.min(40, ask.scrollHeight) + "px"; }
ask.addEventListener("input", () => { autosize(); S.lastActive = Date.now(); });
ask.addEventListener("focus", () => { if (!S.busy) mood("listen"); });
ask.addEventListener("blur", () => { if (!S.busy && orb.className.includes("s-listen")) mood("idle"); });
$("#setupBtn").onclick = () => showView("set");
document.querySelectorAll("#empty .chips button").forEach((b) => (b.onclick = () => { ask.value = b.dataset.q; autosize(); ask.focus(); }));

/* ───────── 附件：截图、拖进来的文件 ───────── */
function renderAttach() {
  const box = $("#attach");
  box.innerHTML = "";
  S.attach.forEach((a, i) => {
    const c = document.createElement("div"); c.className = "chip";
    if (a.kind === "image") { const im = document.createElement("img"); im.src = a.url; im.alt = "截图"; c.appendChild(im); }
    const sp = document.createElement("span"); sp.textContent = a.kind === "image" ? "截图" : a.name; c.appendChild(sp);
    const x = document.createElement("button"); x.type = "button"; x.textContent = "×"; x.setAttribute("aria-label", "移除");
    x.onclick = () => { S.attach.splice(i, 1); renderAttach(); };
    c.appendChild(x); box.appendChild(c);
  });
  box.hidden = S.attach.length === 0;
  if (!S.attach.some((a) => a.kind === "file")) ask.placeholder = "有什么可以帮你？";
}
async function captureAsk() {
  try {
    mood("think");
    const url = await invoke("capture_screen");
    S.attach = S.attach.filter((a) => a.kind !== "image");
    S.attach.push({ kind: "image", url });
    renderAttach();
    await expand("chat");
    if (S.cfg && !S.cfg.vision) toast("当前模型设置为不支持图片，截图不会发给它（设置里可改）");
    mood("listen");
    ask.focus();
  } catch (e) { toastErr(e); }
}
$("#shotBtn").onclick = captureAsk;

T.webview.getCurrentWebview().onDragDropEvent(async (e) => {
  const p = e.payload;
  if (p.type === "enter" || p.type === "over") { if (!orb.className.includes("s-feed")) mood("feed"); return; }
  if (p.type === "leave") { mood("idle"); return; }
  if (p.type === "drop") {
    let n = 0;
    for (const path of p.paths || []) {
      try { const f = await invoke("read_text_file", { path }); S.attach.push({ kind: "file", name: f.name, text: f.text }); n++; }
      catch (err) { toast(String(err)); }
    }
    renderAttach();
    if (n) { mood("done", 1200); await expand("chat"); ask.placeholder = "想让我对这个文件做什么？"; }
    else mood("idle");
  }
});

/* ───────── 历史 ───────── */
function fmtTime(ms) {
  const d = new Date(ms), now = new Date();
  if (d.toDateString() === now.toDateString()) return d.toTimeString().slice(0, 5);
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}
async function loadHistory() {
  const ul = $("#histList");
  ul.innerHTML = "";
  let list = [];
  try { list = await invoke("history_list"); } catch (e) { toast(String(e)); }
  $("#histEmpty").hidden = list.length > 0;
  $("#clearHist").hidden = list.length === 0;
  for (const c of list) {
    const li = document.createElement("li"); li.tabIndex = 0;
    li.innerHTML = '<span class="t"></span><span class="d"></span><button class="x" aria-label="删除"><svg viewBox="0 0 24 24"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></svg></button>';
    li.querySelector(".t").textContent = c.title;
    li.querySelector(".d").textContent = fmtTime(c.updated);
    li.onclick = () => openConv(c);
    li.onkeydown = (e) => { if (e.key === "Enter") openConv(c); };
    li.querySelector(".x").onclick = async (e) => {
      e.stopPropagation();
      await invoke("history_delete", { convId: c.id });
      if (S.convId === c.id) newChat();
      loadHistory();
    };
    ul.appendChild(li);
  }
}
async function openConv(c) {
  newChat();
  const rows = await invoke("history_load", { convId: c.id });
  S.convId = c.id;
  $("#title").textContent = c.title;
  for (const r of rows) {
    if (r.role === "user") { addUserBubble(r.content); S.msgs.push({ role: "user", content: r.content }); }
    else {
      const el = addAiBubble(); const t = el.querySelector(".txt");
      t.classList.remove("cursor"); t.innerHTML = md(r.content);
      setMeta(el, "", { copy: r.content });
      S.msgs.push({ role: "assistant", content: r.content });
    }
  }
  showView("chat");
  scrollEnd();
}
$("#clearHist").onclick = async () => {
  const b = $("#clearHist");
  if (b.dataset.sure !== "1") { b.dataset.sure = "1"; b.textContent = "再点一次确认删除"; setTimeout(() => { b.dataset.sure = ""; b.textContent = "全部删除"; }, 3000); return; }
  b.dataset.sure = ""; b.textContent = "全部删除";
  await invoke("history_clear"); newChat(); loadHistory();
};

/* ───────── 设置 ───────── */
async function loadSettings() {
  const c = (S.cfg = await invoke("get_config"));
  $("#fBase").value = c.base_url;
  $("#fModel").value = c.model;
  $("#fKey").value = "";
  $("#fKey").placeholder = c.has_key ? "已保存（留空不改，填新的会替换）" : "sk-…";
  $("#fNoKey").checked = c.allow_no_key;
  $("#fVision").checked = c.vision;
  $("#fSys").value = c.system_prompt;
  $("#fKeyMain").value = c.shortcut;
  $("#fKeyCap").value = c.capture_shortcut;
  $("#fAuto").checked = c.autostart;
  setMsg("");
}
function setMsg(t, kind) { const m = $("#setMsg"); m.textContent = t; m.className = "msg" + (kind ? " " + kind : ""); }
document.querySelectorAll("#presets button").forEach((b) => (b.onclick = () => {
  $("#fBase").value = b.dataset.b; $("#fModel").value = b.dataset.m;
  $("#fNoKey").checked = b.dataset.k === "1";
  $("#fVision").checked = b.dataset.v !== "0";
  setMsg("已填入，记得填 Key 后保存");
}));
function formCfg() {
  return {
    base_url: $("#fBase").value.trim(),
    model: $("#fModel").value.trim(),
    vision: $("#fVision").checked,
    system_prompt: $("#fSys").value,
    shortcut: $("#fKeyMain").value.trim(),
    capture_shortcut: $("#fKeyCap").value.trim(),
    allow_no_key: $("#fNoKey").checked,
    position: null,
  };
}
$("#testBtn").onclick = async () => {
  setMsg("连接中…");
  try {
    const r = await invoke("test_connection", { baseUrl: $("#fBase").value.trim(), model: $("#fModel").value.trim(), apiKey: $("#fKey").value.trim() || null });
    setMsg(r, "ok"); mood("done", 1500);
  } catch (e) { setMsg(String(e), "bad"); mood("error", 2500); }
};
$("#saveBtn").onclick = async () => {
  const cfg = formCfg();
  if (!/^https?:\/\//.test(cfg.base_url)) return setMsg("接口地址要以 http:// 或 https:// 开头", "bad");
  if (!cfg.model) return setMsg("模型名不能为空", "bad");
  const key = $("#fKey").value.trim();
  try {
    const busy = await invoke("save_config", { cfg, apiKey: key ? key : null });
    const want = $("#fAuto").checked;
    if (want !== S.cfg.autostart) await invoke("set_autostart", { on: want });
    S.cfg = await invoke("get_config");
    $("#fKey").value = "";
    $("#fKey").placeholder = S.cfg.has_key ? "已保存（留空不改，填新的会替换）" : "sk-…";
    if (busy) { setMsg("其余设置已保存。" + busy + "，换一个快捷键才能用", "bad"); mood("error", 2500); }
    else { setMsg("已保存", "ok"); mood("done", 1500); }
  } catch (e) { setMsg(String(e), "bad"); }
};

/* ───────── 菜单、快捷键 ───────── */
listen("menu", async ({ payload }) => {
  switch (payload) {
    case "toggle": toggle(); break;
    case "open": expand(); break;
    case "new_chat": newChat(); expand("chat"); break;
    case "capture": captureAsk(); break;
    case "history": expand("hist"); break;
    case "settings": expand("set"); break;
  }
});

/* ───────── 启动 ───────── */
(async () => {
  try { S.cfg = await invoke("get_config"); } catch (_) {}
  updateEmpty();
  mood("idle");
  // 还没设置模型：小球露出好奇的表情提醒一下，不自动展开，免得挡住屏幕
  if (needsSetup()) mood("curious", 4000);
})();
