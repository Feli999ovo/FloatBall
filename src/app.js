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
const PAD = 12;
let SMALL = 72 + PAD * 2; // 收起时的窗口边长 = 小球直径 + 两边留白；小球大小在设置里能改（见 applyLook）
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
  tasks: [],           // 这次打开以来发出的请求 {title, st: run|done|err|stop, t0, t1}
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
  if (hide) { body.classList.add("resizing"); await frame(); await frame(); } // 等两帧：只等一帧时隐藏还没画出来，小球会在新窗口顶上闪一下（真机截图里见过）
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
  if (peekBox) await hidePeek();
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
    showView(view || "chat");
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
  setTimeout(() => { if (!document.hasFocus()) collapse(); }, 150);
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
  $("#vTask").hidden = v !== "task";
  $("#tabChat").setAttribute("aria-selected", v !== "task"); $("#tabTask").setAttribute("aria-selected", v === "task");
  $("#tabChat").tabIndex = v === "task" ? -1 : 0; $("#tabTask").tabIndex = v === "task" ? 0 : -1;
  $("#title").hidden = v === "task";
  $("#vHist").hidden = v !== "hist";
  $("#histBtn").setAttribute("aria-pressed", v === "hist");
  if (v === "hist") loadHistory();
  if (v === "task") renderTasks();
  if (v === "chat") updateEmpty();
}
$("#closeBtn").onclick = collapse;
$("#tabChat").onclick = () => { showView("chat"); focusAsk(); };
$("#tabTask").onclick = () => showView("task");
$(".seg").addEventListener("keydown", (e) => {
  if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
  e.preventDefault();
  const toTask = $("#vTask").hidden;
  showView(toTask ? "task" : "chat");
  $(toTask ? "#tabTask" : "#tabChat").focus();
});

/* ───────── 任务：这次打开以来发出的请求，正在回答的可以停（取自参考版本） ───────── */
function renderTasks() {
  const run = S.tasks.filter((t) => t.st === "run").length;
  $("#tBadge").hidden = !run; $("#tBadge").textContent = run;
  if ($("#vTask").hidden) return;
  const box = $("#tasks");
  box.textContent = "";
  if (!S.tasks.length) { const p = document.createElement("p"); p.className = "none"; p.textContent = "还没有任务"; box.appendChild(p); return; }
  for (const t of S.tasks.slice(0, 30)) {
    const d = document.createElement("div"); d.className = "tk"; d.dataset.st = t.st === "run" ? "run" : "done";
    const row = document.createElement("div"); row.className = "row";
    const name = document.createElement("span"); name.className = "name"; name.textContent = t.title;
    const when = document.createElement("span"); when.className = "when";
    const sec = Math.round(((t.t1 || Date.now()) - t.t0) / 1000);
    if (t.st === "done") { const ok = document.createElement("span"); ok.className = "ok"; ok.textContent = "✓ "; when.appendChild(ok); }
    when.appendChild(document.createTextNode({ run: "回答中", done: "完成", err: "出错", stop: "已停止" }[t.st] + " · " + sec + " 秒"));
    row.append(name, when);
    d.appendChild(row);
    if (t.st === "run") {
      const b = document.createElement("button"); b.type = "button"; b.textContent = "停止"; b.onclick = stop; row.appendChild(b);
      const bar = document.createElement("div"); bar.className = "bar"; bar.appendChild(document.createElement("i")); d.appendChild(bar);
    } else d.onclick = () => { showView("chat"); focusAsk(); };
    box.appendChild(d);
  }
}
setInterval(() => { if (S.busy && !$("#vTask").hidden) renderTasks(); }, 1000);

/* ───────── 气泡：卡片收起时回复完成，在小球旁冒一句（取自参考版本） ─────────
   收起时窗口只有小球那么大，放不下气泡：冒泡时临时把窗口扩大，小球留在原地，消失后再缩回。 */
const PEEK_W = 260, PEEK_H = 60;
let peekTimer = 0, peekBox = null;
async function showPeek(text) {
  const t = String(text).replace(/\s+/g, " ").trim();
  if (!t || S.open || morphing || peekBox) return;
  morphing = true;
  try {
    const p = await winPos(), a = await monitorArea();
    const toL = p.x + SMALL / 2 > a.x + a.w / 2, below = p.y - PEEK_H < a.y + 8;
    const w = SMALL + PEEK_W, h = SMALL + PEEK_H;
    const x = toL ? p.x - PEEK_W : p.x, y = below ? p.y : p.y - PEEK_H;
    peekBox = { p, toL, below };
    const el = $("#peek");
    el.textContent = t.length <= 20 ? t : t.slice(0, 20) + "…";
    await reshape(w, h, x, y, toL || !below, () => {
      // 小球留在原来的位置：窗口往哪边长，小球就靠哪边
      body.classList.add("peeking");
      $("#app").style.padding = `${below ? PAD : PAD + PEEK_H}px ${toL ? PAD : PAD + PEEK_W}px ${below ? PAD + PEEK_H : PAD}px ${toL ? PAD + PEEK_W : PAD}px`;
      el.style.left = toL ? "auto" : SMALL - 4 + "px"; el.style.right = toL ? SMALL - 4 + "px" : "auto";
      el.style.top = below ? SMALL - 4 + "px" : "auto"; el.style.bottom = below ? "auto" : SMALL - 4 + "px";
      el.style.borderRadius = below ? (toL ? "14px 4px 14px 14px" : "4px 14px 14px 14px") : (toL ? "14px 14px 4px 14px" : "14px 14px 14px 4px");
      el.style.transformOrigin = (toL ? "100% " : "0 ") + (below ? "0" : "100%");
    });
    el.classList.add("on");
  } finally { morphing = false; }
  clearTimeout(peekTimer);
  peekTimer = setTimeout(hidePeek, 6000);
}
async function hidePeek() {
  clearTimeout(peekTimer);
  const box = peekBox;
  if (!box) return;
  $("#peek").classList.remove("on");
  await new Promise((r) => setTimeout(r, 220));
  if (S.open || morphing) { peekBox = null; return; } // 已经点开了：展开会自己排版
  morphing = true;
  try {
    await reshape(SMALL, SMALL, box.p.x, box.p.y, box.toL || !box.below, () => { body.classList.remove("peeking"); $("#app").style.padding = ""; });
  } finally { morphing = false; peekBox = null; }
}
$("#peek").onclick = async () => {
  const box = peekBox;
  clearTimeout(peekTimer);
  $("#peek").classList.remove("on");
  if (box) {
    morphing = true;
    try { await reshape(SMALL, SMALL, box.p.x, box.p.y, false, () => { body.classList.remove("peeking"); $("#app").style.padding = ""; }); }
    finally { morphing = false; peekBox = null; }
  }
  expand("chat");
};
$("#newBtn").onclick = () => { newChat(); showView("chat"); focusAsk(); };
$("#histBtn").onclick = () => showView($("#vHist").hidden ? "hist" : "chat");
$("#setBtn").onclick = openSettings;
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
function needsSetup() { return !!S.cfg && !S.cfg.ready; }
// 设置是单独的窗口。小球窗口这时会失去焦点而收起，正好不挡着设置窗口。
function openSettings() { invoke("open_settings").catch(toastErr); }
// 小球大小、眼睛颜色、卡片透明度
function applyLook() {
  const c = S.cfg, root = document.documentElement.style;
  if (!c) return;
  const orbPx = c.orb_size || 72, small = orbPx + PAD * 2;
  root.setProperty("--orb", orbPx + "px");
  root.setProperty("--k", String(orbPx / 72));
  const eye = c.eye || "#FFF1DC", warm = eye.toUpperCase() === "#FFF1DC";
  root.setProperty("--eye", eye);
  // 眼睛周围的光晕：默认的暖白用样式表里调好的暖色，换了颜色就跟着眼睛的颜色
  if (warm) { root.removeProperty("--eye-near"); root.removeProperty("--eye-glow"); }
  else { root.setProperty("--eye-near", eye); root.setProperty("--eye-glow", eye + "BF"); }
  root.setProperty("--tint", String(1 - (c.glass == null ? 25 : c.glass) / 100));
  if (small !== SMALL) {
    SMALL = small;
    // 收起着的时候窗口就是小球那么大，跟着改
    if (!S.open) appWin.setSize(new LogicalSize(SMALL, SMALL)).catch(() => {});
  }
}
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
  if (opts.settings) { const b = document.createElement("button"); b.textContent = "去设置"; b.onclick = openSettings; meta.appendChild(b); }
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
  const task = { title: (S.msgs[S.msgs.length - 1] && historyText(lastText(), [])) || "提问", st: "run", t0: Date.now() };
  S.tasks.unshift(task); renderTasks();
  const el = addAiBubble();
  const think = el.querySelector(".think");
  let txt = el.querySelector(".txt");
  // out：整条回复（存历史、复制）；seg：当前这段文字。工具药丸把回复分成几段，药丸后面的字另起一段
  let out = "", seg = "", reasoning = "", usage = null, raf = 0;
  const paint = () => { raf = 0; const live = S.busy === id; txt.innerHTML = live ? md(seg + "\uE000").replace("\uE000", '<span class="cur"></span>') : md(seg); scrollEnd(); };
  const ch = new Channel();
  ch.onmessage = (ev) => {
    if (S.busy !== id) return;
    if (ev.type === "delta") {
      if (!out) mood("work");
      out += ev.text; seg += ev.text;
      if (!raf) raf = requestAnimationFrame(paint);
    } else if (ev.type === "tool") {
      toolPill(el, ev);
      if (seg) { txt.classList.remove("cursor"); txt.innerHTML = md(seg); seg = ""; out += "\n\n"; const t2 = document.createElement("div"); t2.className = "txt cursor"; txt = t2; }
      el.insertBefore(txt, el.querySelector(".meta"));
      if (ev.status === "run") mood("work");
      scrollEnd();
    } else if (ev.type === "ask") {
      askCard(el, ev);
      mood("curious");
      if (!S.open) showPeek("需要你确认：" + ev.desc);
      scrollEnd();
    } else if (ev.type === "usage") {
      usage = ev;
    } else if (ev.type === "done") {
      finish();
      txt.classList.remove("cursor"); paint();
      out = out.replace(/\n\n$/, "");
      if (!out) { txt.textContent = "（模型没有返回内容）"; }
      S.msgs.push({ role: "assistant", content: out });
      invoke("history_append", { convId: S.convId, role: "assistant", content: out }).catch(() => {});
      setMeta(el, usage && usage.prompt + usage.completion ? `${leadModelName()} · ${fmtTok(usage.prompt + usage.completion)} tokens` : leadModelName(), { copy: out });
      mood("done", 1800);
      if (!S.open) { poke(); showPeek(out || "回复好了"); }
    } else if (ev.type === "error") {
      finish("err");
      txt.classList.remove("cursor");
      el.classList.add("err");
      txt.textContent = ev.message;
      const noKey = /API Key|设置/.test(ev.message);
      setMeta(el, "", { retry: () => { el.remove(); run(); }, settings: noKey });
      mood("error", 4000);
    }
  };
  function finish(st = "done") {
    if (S.busy === id) S.busy = null;
    $("#sendBtn").disabled = false; $("#stopRow").hidden = true;
    if (task.st === "run") { task.st = st; task.t1 = Date.now(); renderTasks(); }
  }
  el._stop = () => {
    invoke("cancel_chat", { id }).catch(() => {});
    finish("stop"); txt.classList.remove("cursor");
    el.querySelectorAll(".x-ap button, .x-ap input").forEach((b) => (b.disabled = true));
    if (out) { txt.innerHTML = md(seg); S.msgs.push({ role: "assistant", content: out }); invoke("history_append", { convId: S.convId, role: "assistant", content: out + "\n\n（已停止）" }).catch(() => {}); }
    else txt.textContent = "（已停止）";
    setMeta(el, "已停止", out ? { copy: out } : {});
    mood("idle");
  };
  S.stopFn = el._stop;
  invoke("chat", { id, messages: S.msgs, onEvent: ch }).catch((e) => ch.onmessage({ type: "error", message: String(e) }));
}
function stop() { if (S.busy && S.stopFn) S.stopFn(); }

// ───────── 工具药丸、确认卡片（照搬原型的 toolChip / askAppr） ─────────
const TOOL_ICO = { 搜索网页: "search", 读取网页: "window", 读取文件: "doc", 查看文件夹: "pages", 写入文件: "write", 删除文件: "trash", 发送邮件: "mail", 打开网页: "window" };
const ICO_PATH = {
  doc: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/><path d="M9 13h6M9 16.5h4"/>',
  search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m20 20-4.9-4.9"/>',
  trash: '<path d="M4 7h16M9.5 7V4.5h5V7"/><path d="M6.5 7l.9 12.2a2 2 0 0 0 2 1.8h5.2a2 2 0 0 0 2-1.8L17.5 7"/><path d="M10 11v6M14 11v6"/>',
  mail: '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="m3.6 6.6 8.4 6.4 8.4-6.4"/>',
  write: '<path d="M15 4.5a2.1 2.1 0 0 1 3 3L8.5 17l-4 1 1-4z"/><path d="M13 20h7M16.5 16h3.5"/>',
  pages: '<path d="M8 7V5a2 2 0 0 1 2-2h6l4 4v10a2 2 0 0 1-2 2h-2"/><rect x="4" y="7" width="12" height="14" rx="2"/><path d="M7.5 12h5M7.5 15.5h3"/>',
  window: '<rect x="3" y="4.5" width="18" height="15" rx="2"/><path d="M3 9h18"/><circle cx="6" cy="6.8" r=".6" fill="currentColor"/><circle cx="8.6" cy="6.8" r=".6" fill="currentColor"/>',
  pause: '<rect x="7" y="5.5" width="3" height="13" rx="1"/><rect x="14" y="5.5" width="3" height="13" rx="1"/>',
  upload: '<path d="M12 15V4M7.5 8.5 12 4l4.5 4.5"/><path d="M5 14v4a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4"/>',
};
const icoSvg = (n, cls) => `<svg class="x-ico${cls ? " " + cls : ""}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICO_PATH[n] || ""}</svg>`;
const fmtTok = (n) => (n < 1000 ? String(n) : (n / 1000).toFixed(1) + "K");
function leadModelName() {
  const c = S.cfg || {}, lead = (c.agents || [])[0] || {}, ms = c.models || [];
  const m = ms.find((x) => x.id === lead.model) || ms.find((x) => x.id === c.default_model);
  return m ? m.name || m.model : "";
}
function toolPill(el, ev) {
  let w = el.querySelector(`.x-tw[data-call="${ev.call}"]`);
  if (!w) {
    w = document.createElement("div"); w.className = "x-tw"; w.dataset.call = ev.call;
    w.innerHTML = '<button type="button" class="x-tool" aria-expanded="false"><span aria-hidden="true"></span><span class="x-tl"></span><span class="x-td" aria-hidden="true"><i></i><i></i><i></i></span><span class="x-tc" aria-hidden="true">✓</span><span class="x-tp">' + icoSvg("pause", "x-ico--in") + '等你确认</span></button><p class="x-tres" hidden><b>结果</b><span></span></p>';
    const b = w.firstChild, r = w.lastChild;
    b.addEventListener("click", () => {
      if (!/\b(done|err)\b/.test(w.className)) return;
      r.hidden = !r.hidden; b.setAttribute("aria-expanded", !r.hidden);
    });
    el.insertBefore(w, el.querySelector(".meta"));
  }
  const lead = ((S.cfg && S.cfg.agents) || [])[0];
  const who = lead && ev.agent && ev.agent !== lead.name ? ev.agent + " · " : "";
  w.firstChild.firstChild.innerHTML = icoSvg(ev.name.startsWith("交给") ? "upload" : TOOL_ICO[ev.name] || "pages");
  w.querySelector(".x-tl").textContent = who + ev.name + (ev.arg ? " · " + ev.arg : "");
  w.className = "x-tw" + ({ wait: " wait", done: " done", no: " no", err: " err" }[ev.status] || "");
  if (ev.status === "no") w.querySelector(".x-tp").textContent = "✕ 已拒绝";
  if (ev.status === "err") w.querySelector(".x-tc").textContent = "✕";
  w.lastChild.querySelector("span").textContent = ev.result || "";
  w.lastChild.querySelector("b").textContent = ev.status === "err" ? "出错" : "结果";
}
function askCard(el, ev) {
  const w = el.querySelector(`.x-tw[data-call="${ev.call}"]`);
  const c = document.createElement("div");
  c.className = "x-ap" + (ev.hi ? " hi" : ""); c.setAttribute("role", "group"); c.setAttribute("aria-label", "需要你确认");
  c.innerHTML = '<p class="x-aph"><span class="x-risk"></span>需要你确认</p><p class="x-apd"></p>' +
    '<div class="x-ape" hidden><input class="x-in" spellcheck="false" aria-label="修改操作内容" /><button type="button" class="x-sb x-pri">确认</button></div>' +
    '<div class="x-apb x-ap0"><button type="button" class="x-sb x-pri">允许一次</button><button type="button" class="x-sb">总是允许</button>' +
    '<button type="button" class="x-sb">拒绝</button><button type="button" class="x-sb x-apx" aria-label="更多选项" aria-expanded="false">…</button></div>' +
    '<div class="x-apb x-apm" hidden><button type="button" class="x-sb">修改后允许</button><button type="button" class="x-sb">拒绝并说明</button></div>' +
    '<div class="x-apr x-ape" hidden><input class="x-in" placeholder="原因（会告诉 Agent）" aria-label="原因（会告诉 Agent）" /><button type="button" class="x-sb x-pri">发送</button></div>';
  const apd = c.querySelector(".x-apd");
  c.querySelector(".x-risk").textContent = ev.hi ? "高风险" : "中风险"; apd.textContent = ev.desc;
  if (w && w.nextSibling) el.insertBefore(c, w.nextSibling); else el.insertBefore(c, el.querySelector(".meta"));
  const row0 = c.querySelector(".x-ap0"), more = c.querySelector(".x-apm"), [edBox, rsBox] = c.querySelectorAll(".x-ape");
  const edIn = edBox.querySelector("input"), rsIn = rsBox.querySelector("input");
  const note = (t) => { const n = document.createElement("p"); n.className = "x-apn"; n.textContent = t; c.appendChild(n); };
  let done = false;
  const fin = (b, answer) => {
    if (done) return; done = true;
    c.querySelectorAll("button,input").forEach((x) => (x.disabled = true)); b.classList.add("on");
    invoke("answer_ask", { ask: ev.ask, answer: { allow: false, always: false, reason: "", edit: "", ...answer } }).then((ok) => { if (!ok) note("这一步已经结束了"); }).catch(() => {});
    if (answer.always) note(`以后「${ev.kind} · ${ev.scope}」不再询问`);
    mood(answer.allow ? "work" : "think");
    ask.focus({ preventScroll: true });
  };
  const [bOnce, bAlways, bNo, bMore] = row0.querySelectorAll("button");
  bOnce.addEventListener("click", () => fin(bOnce, { allow: true }));
  bAlways.addEventListener("click", () => fin(bAlways, { allow: true, always: true }));
  bNo.addEventListener("click", () => fin(bNo, { allow: false }));
  bMore.addEventListener("click", () => { more.hidden = !more.hidden; bMore.setAttribute("aria-expanded", !more.hidden); if (!more.hidden) more.querySelector("button").focus({ preventScroll: true }); scrollEnd(); });
  const [bEdit, bWhy] = more.querySelectorAll("button");
  bEdit.addEventListener("click", () => { edIn.value = ""; edIn.placeholder = ev.kind === "发送" ? "改过的邮件正文" : ev.kind === "写入" ? "改过的文件内容" : "改过的内容"; apd.hidden = false; edBox.hidden = false; row0.hidden = more.hidden = rsBox.hidden = true; edIn.focus(); });
  bWhy.addEventListener("click", () => { rsBox.hidden = false; row0.hidden = more.hidden = true; rsIn.focus(); scrollEnd(); });
  const edOk = edBox.querySelector("button"), rsOk = rsBox.querySelector("button");
  edOk.addEventListener("click", () => { const v = edIn.value.trim(); if (!v) { edIn.focus(); return; } note("已按修改后的内容执行"); fin(edOk, { allow: true, edit: v }); });
  rsOk.addEventListener("click", () => { const v = rsIn.value.trim(); if (!v) { rsIn.focus(); return; } fin(rsOk, { allow: false, reason: v }); });
  edIn.addEventListener("keydown", (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); edOk.click(); } });
  rsIn.addEventListener("keydown", (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); rsOk.click(); } });
  bOnce.focus({ preventScroll: true });
}
// 最近一次提问的文字（任务列表里显示）
function lastText() {
  const c = S.msgs[S.msgs.length - 1].content;
  const s = typeof c === "string" ? c : (c.find((p) => p.type === "text") || {}).text || "截图提问";
  return s.replace(/^文件「[^」]*」的内容：[\s\S]*?```\n\n/, "").slice(0, 40) || "提问";
}
$("#stopBtn").onclick = stop;

$("#capsule").addEventListener("submit", (e) => { e.preventDefault(); send(); });
ask.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); }
});
function autosize() { ask.style.height = "auto"; ask.style.height = Math.min(40, ask.scrollHeight) + "px"; }
ask.addEventListener("input", () => { autosize(); S.lastActive = Date.now(); });
ask.addEventListener("focus", () => { if (!S.busy) mood("listen"); });
ask.addEventListener("blur", () => { if (!S.busy && orb.className.includes("s-listen")) mood("idle"); });
$("#setupBtn").onclick = openSettings;
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

/* ───────── 菜单、快捷键 ───────── */
// 设置窗口里改了东西
// 设置窗口里「测试连接」成功时，小球开心一下（原型里的行为）
listen("ball-state", ({ payload }) => mood(String(payload), 1800));
listen("config", async () => {
  try { S.cfg = await invoke("get_config"); } catch (_) { return; }
  applyLook();
  updateEmpty();
});
listen("menu", async ({ payload }) => {
  switch (payload) {
    case "toggle": toggle(); break;
    case "open": expand(); break;
    case "new_chat": newChat(); expand("chat"); break;
    case "capture": captureAsk(); break;
    case "history": expand("hist"); break;
  }
});

/* ───────── 启动 ───────── */
(async () => {
  try { S.cfg = await invoke("get_config"); } catch (_) {}
  applyLook();
  updateEmpty();
  mood("idle");
  // 还没设置模型：小球露出好奇的表情提醒一下，不自动展开，免得挡住屏幕
  if (needsSetup()) mood("curious", 4000);
})();
