/* 悬浮球前端。系统能力都通过 window.__TAURI__ 调到 Rust（src-tauri/src/lib.rs）。 */
"use strict";

const T = window.__TAURI__;
const { invoke, Channel } = T.core;
const { listen } = T.event;
const appWin = T.window.getCurrentWindow();
const { LogicalSize, LogicalPosition, PhysicalPosition } = T.dpi;

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
// 展开后照原型：卡片 340×260，胶囊 288 长；卡片在胶囊上居中，所以小球那一侧多出 OVER
const CARD_W = 340, CARD_H = 260, CAP_W = 288, OVER = (CARD_W - CAP_W) / 2;
const BIG_W = CARD_W + PAD * 2;
const bigH = () => PAD * 2 + (SMALL - PAD * 2) + 8 + CARD_H; // 留白 + 胶囊（小球高）+ 间距 + 卡片

const S = {
  open: false,
  pinned: false,
  busy: null,          // 当前对话里正在生成的请求 id（别的对话在后台跑的不算）
  convId: null,        // 历史里的对话 id
  msgs: [],            // 发给模型的消息 {role, content}
  conv: null,          // 当前对话（见 Conv，启动时建一个空的）。切走时它的气泡收进 conv.dom，在后台接着生成
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
// 没动静一段时间就睡觉（设置 → 外观「空闲多久后睡觉」，0 = 从不）
setInterval(() => {
  const ms = S.cfg ? S.cfg.idle_ms : 600000;
  if (ms && !S.open && !S.busy && !orb.className.includes("s-sleep") && Date.now() - S.lastActive > ms) mood("sleep");
}, 5000);

/* ───────── 窗口：收起 / 展开 / 拖动 / 贴边 ───────── */
// 贴边：side -1 左、1 右、0 不贴。贴着时窗口有一截在屏幕外，小球外侧被裁平；指针移上去时滑出来露出整颗球
let docked = 0;
function setDock(side) {
  docked = side;
  orb.dataset.edge = side < 0 ? "l" : side > 0 ? "r" : "";
  orb.classList.toggle("docked", !!side);
}
// 从贴边位置换算出完整露出来的位置（展开、指针移上来时用）
const tuckShift = () => (SMALL - PAD * 2) * 0.1 + 2;
async function peel(out) {
  if (!docked || S.open || morphing || peekBox || hv.shown) return;
  const p = await winPos();
  morphing = true;
  orb.classList.toggle("peel", out);
  try { await appWin.setPosition(new LogicalPosition(p.x - docked * tuckShift() * (out ? 1 : -1), p.y)); }
  finally { setTimeout(() => (morphing = false), 80); }
}
orb.addEventListener("pointerenter", () => { if (docked && !orb.classList.contains("peel")) peel(true); });
orb.addEventListener("pointerleave", () => { if (docked && orb.classList.contains("peel") && !press) peel(false); });
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
  if (hv.shown || hv.hiding) await hvHide();
  if (peekBox) await hidePeek();
  // 贴边的球刚滑出来（或别的短动画）时点下去：等它停稳再展开，不要吞掉这次点击
  for (let i = 0; morphing && i < 20; i++) await new Promise((r) => setTimeout(r, 25));
  if (morphing) { jslog("展开被动画挡住"); return; }
  if (S.open) { showView(view || "chat"); focusAsk(); return; }
  morphing = true;
  try {
    S.anchor = await winPos();
    // 贴着边：先换算成完整露出来的位置，收起时再贴回去
    if (docked) {
      if (!orb.classList.contains("peel")) S.anchor.x -= docked * tuckShift();
      S.redock = docked; orb.classList.remove("peel"); setDock(0);
    }
    const a = await monitorArea();
    const cx = S.anchor.x + SMALL / 2;
    const flipX = cx > a.x + a.w / 2;
    // 卡片放在胶囊上方（照原型）；只有贴着屏幕顶、上面放不下时才翻到下面。两边都放不下就挪窗口
    const h = bigH(), above = S.anchor.y + SMALL - a.y - 4, below = a.y + a.h - S.anchor.y - 4;
    const flipY = above >= h || above > below;
    let x = flipX ? S.anchor.x + SMALL + OVER - BIG_W : S.anchor.x - OVER;
    let y = flipY ? S.anchor.y + SMALL - h : S.anchor.y;
    x = Math.max(a.x + 4, Math.min(x, a.x + a.w - BIG_W - 4));
    y = Math.max(a.y + 4, Math.min(y, a.y + a.h - h - 4));
    const card = $("#card");
    card.classList.remove("glassy");
    const bd = await grabBackdrop({ x, y, w: BIG_W, h }, a);
    await reshape(BIG_W, h, x, y, flipX || flipY, () => {
      body.classList.toggle("flip-x", flipX);
      body.classList.toggle("flip-y", flipY);
      body.classList.add("big");
    });
    void body.offsetWidth; // 让小球先在新版面里排好，胶囊才是从小球的位置开始拉长
    S.open = true;
    body.classList.remove("collapsed");
    showView(view || "chat");
    measureCap(bd, BIG_W);
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
    await reshape(SMALL, SMALL, p.x, p.y, flipped, () => body.classList.remove("flip-x", "flip-y", "big"));
    if (!S.busy) mood("idle");
  } finally { morphing = false; }
  if (S.redock) { S.redock = 0; settle(); } // 展开前贴着边：缩回后贴回去
}
function toggle() { S.open ? collapse() : expand(); }
function focusAsk() { if (!$("#vChat").hidden) setTimeout(() => ask.focus(), 30); }

// 点住小球：移动超过 4 像素算拖动，否则算点击
let press = null;
orb.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  press = { x: e.screenX, y: e.screenY, dragged: false, voice: false, lt: 0 };
  try { orb.setPointerCapture(e.pointerId); } catch (_) {}
  // 长按（收起时）：按住说话
  if (!S.open && !vc) press.lt = setTimeout(() => { if (press && !press.dragged) { press.voice = true; vcStart("press"); } }, VC_HOLD);
});
window.addEventListener("pointermove", (e) => {
  if (press && press.voice) return vcPressMove(e);
  if (!press || press.dragged) return;
  if (Math.hypot(e.screenX - press.x, e.screenY - press.y) > 4) {
    press.dragged = true; clearTimeout(press.lt);
    S.userMoved = true;
    if (docked) { orb.classList.remove("peel"); setDock(0); }
    orb.classList.add("dragging");
    appWin.startDragging().catch(() => {});
  }
});
window.addEventListener("pointercancel", () => { if (press && press.voice && vc && vc.mode === "press") vcCancel(); if (press) clearTimeout(press.lt); press = null; });
window.addEventListener("pointerup", () => {
  if (!press) return;
  const wasDrag = press.dragged, wasVoice = press.voice;
  clearTimeout(press.lt);
  press = null;
  if (wasVoice) { orb.classList.remove("dragging"); if (vc && vc.mode === "press") vcRelease(); return; }
  orb.classList.remove("dragging");
  if (!wasDrag) { poke(); toggle(); }
});
orb.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggle(); } });
orb.addEventListener("contextmenu", (e) => { e.preventDefault(); invoke("show_context_menu").catch(toastErr); });

// 拖完停下 350ms 后：贴边吸附 + 记住位置
let moveTimer = 0;
appWin.onMoved(() => {
  if (morphing || hv.shown || orb.classList.contains("peel")) return; // 贴边滑出来露脸、弹出小泡不算拖动
  orb.classList.remove("dragging");
  if (S.open) $("#card").classList.remove("glassy"); // 窗口挪了，底图对不上了
  clearTimeout(moveTimer);
  moveTimer = setTimeout(settle, 350);
});
async function settle() {
  if (morphing || hv.shown) return;
  const p = await winPos();
  if (S.open) {
    // 只有用户在展开状态下亲手拖动时才更新收起位置；展开时程序自己挪窗口（贴着屏幕边缩回来）不算
    if (!S.userMoved) return;
    S.userMoved = false;
    // 展开时拖动了胶囊上的小球：把收起后的位置也跟着挪
    const flipX = body.classList.contains("flip-x"), flipY = body.classList.contains("flip-y");
    const size = await appWin.outerSize(), s = await appWin.scaleFactor();
    const w = size.width / s, h = size.height / s;
    S.anchor = { x: flipX ? p.x + w - SMALL - OVER : p.x + OVER, y: flipY ? p.y + h - SMALL : p.y };
    invoke("save_position", S.anchor).catch(() => {});
    return;
  }
  S.userMoved = false;
  const a = await monitorArea();
  let { x, y } = p;
  const SNAP = 24, ORB = SMALL - PAD * 2;
  // 贴边（原型的 dock）：开了「贴边时自动收起」时，小球的一侧压平、平边贴住屏幕边；没开就只是吸到边上
  let side = 0;
  if (x - a.x < SNAP) { side = -1; x = S.cfg && S.cfg.auto_dock ? a.x - PAD - ORB * 0.1 : a.x - PAD + 2; }
  else if (a.x + a.w - (x + SMALL) < SNAP) { side = 1; x = S.cfg && S.cfg.auto_dock ? a.x + a.w - PAD - ORB * 0.9 : a.x + a.w - SMALL + PAD - 2; }
  setDock(S.cfg && S.cfg.auto_dock ? side : 0);
  y = Math.max(a.y - PAD + 2, Math.min(y, a.y + a.h - SMALL + PAD - 2));
  if (x !== p.x || y !== p.y) {
    morphing = true;
    await appWin.setPosition(new LogicalPosition(x, y));
    setTimeout(() => (morphing = false), 60);
  }
  invoke("save_position", { x, y }).catch(() => {});
  setTimeout(() => measureOrb({ x, y }), 400); // 等吸边动画停下再量身后
}

// 自适应玻璃：原型的玻璃把身后的桌面直接透出来（×0.9），放到白色的文档、网页上，奶白的眼睛和占位字就看不见了。
// 像系统的液态玻璃那样按身后的亮度加深：胶囊用展开前截的那张底图量它那一条，小球收起后量窗口四角（四角在球外，是桌面本身）。
// 只有 Windows 有底图；拿不到时用一个中等的深度，保证白底上也看得清。
function lumOf(url, pick) {
  return new Promise((res) => {
    const im = new Image();
    im.onload = () => {
      try {
        const c = document.createElement("canvas"); c.width = im.width; c.height = im.height;
        const g = c.getContext("2d"); g.drawImage(im, 0, 0);
        let t = 0, n = 0;
        for (const [x, y, w, h] of pick(im.width, im.height)) {
          const d = g.getImageData(Math.round(x), Math.round(y), Math.max(1, Math.round(w)), Math.max(1, Math.round(h))).data;
          for (let i = 0; i < d.length; i += 4) { t += (0.2126 * d[i] + 0.7152 * d[i + 1] + 0.0722 * d[i + 2]) / 255; n++; }
        }
        res(n ? t / n : null);
      } catch (_) { res(null); }
    };
    im.onerror = () => res(null);
    im.src = url;
  });
}
const clamp01 = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
async function measureCap(bd, w) {
  const r = $("#capsule").getBoundingClientRect();
  const L = bd ? await lumOf(bd, (iw) => { const k = iw / w; return [[r.left * k, r.top * k, r.width * k, r.height * k]]; }) : null;
  document.documentElement.style.setProperty("--capk", String(L == null ? 0.3 : +clamp01(0.1 + (L - 0.45) * 1.1, 0.1, 0.55).toFixed(3)));
}
async function measureOrb(p) {
  if (S.open || morphing) return;
  const a = await monitorArea();
  const bd = await grabBackdrop({ x: p.x, y: p.y, w: SMALL, h: SMALL }, a);
  const L = bd ? await lumOf(bd, (iw, ih) => { const q = Math.max(1, iw * 0.15); return [[0, 0, q, q], [iw - q, 0, q, q], [0, ih - q, q, q], [iw - q, ih - q, q, q]]; }) : null;
  document.documentElement.style.setProperty("--orbk", String(L == null ? 0.12 : +clamp01((L - 0.5) * 0.8, 0, 0.35).toFixed(3)));
}

// 点别处：没在生成、没固定、输入框是空的 → 收起
// 右键菜单是另一个小窗口，打开时焦点在它那里：这时别把卡片收起来
T.event.listen("ctx", (e) => { S.ctx = !!e.payload; });
appWin.onFocusChanged(({ payload: focused }) => {
  if (focused || !S.open || S.pinned || S.busy || S.ctx) return;
  if (ask.value.trim() || S.attach.length) return;
  setTimeout(() => { if (!document.hasFocus() && !S.ctx) collapse(); }, 150);
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
  $("#vHo").hidden = v !== "ho";
  $("#card").classList.toggle("ho", v === "ho"); // 交给终端打开时把「对话 | 任务」和标题栏让给面板：卡片只有 260 高
  $("#histBtn").setAttribute("aria-pressed", v === "hist");
  if (v === "hist") loadHistory();
  if (v === "task") { renderTasks(); rtRefresh().then(renderTasks); }
  if (v === "chat") updateEmpty();
  syncTmp();
}
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
  if (agList().length > 1) { box.appendChild(agRow()); agSync(); }
  // 照原型分组：进行中、定时任务、已完成
  const head = (t) => { const h = document.createElement("p"); h.className = "hg"; h.textContent = t; box.appendChild(h); return h; };
  const rts = (S.cfg && S.cfg.routines) || [];
  const act = S.tasks.filter((t) => t.st === "run"), fin = S.tasks.filter((t) => t.st !== "run");
  if (act.length) { head("进行中"); act.forEach((t) => box.appendChild(taskRow(t))); }
  head("定时任务").id = "routineHead";
  rts.forEach((r) => box.appendChild(rtRow(r)));
  if (rtNew) box.appendChild(rtForm(rtNew));
  box.appendChild(rtAddBtn());
  const n = document.createElement("p"); n.className = "rnote"; n.textContent = "电脑睡眠或关机时不运行，开机后只补跑最近一次"; box.appendChild(n);
  if (fin.length) { head("已完成"); fin.slice(0, 30).forEach((t) => box.appendChild(taskRow(t))); }
  if (rtFocus) { const el = document.getElementById(rtFocus); rtFocus = ""; if (el) el.focus({ preventScroll: true }); }
  // 卡片只有 260 高：展开的详情 / 表单滚到看得见
  const open = box.querySelector(".rd"); if (open) open.scrollIntoView({ block: "nearest" });
}
function taskRow(t) {
  {
    const d = document.createElement("div"); d.className = "tk"; d.dataset.st = t.st === "run" ? "run" : "done";
    if (t.rt) d.classList.add("fromrt");
    const row = document.createElement("div"); row.className = "row";
    const name = document.createElement("span"); name.className = "name"; name.textContent = t.title;
    const when = document.createElement("span"); when.className = "when";
    const sec = Math.round(((t.t1 || Date.now()) - t.t0) / 1000);
    if (t.st === "done") { const ok = document.createElement("span"); ok.className = "ok"; ok.textContent = "✓ "; when.appendChild(ok); }
    if (t.st === "run" && t.why) { d.dataset.st = "pause"; const bd = document.createElement("span"); bd.className = "bud"; bd.textContent = "已暂停 · " + t.why; when.appendChild(bd); }
    else when.appendChild(document.createTextNode({ run: "回答中", done: "完成", err: "出错", stop: "已停止" }[t.st] + " · " + sec + " 秒"));
    row.append(name, when);
    d.appendChild(row);
    if (t.st === "run") {
      const b = document.createElement("button"); b.type = "button"; b.textContent = "停止";
      b.onclick = (e) => { e.stopPropagation(); if (t.conv && t.conv.stop) t.conv.stop(); }; row.appendChild(b);
      const bar = document.createElement("div"); bar.className = "bar"; bar.appendChild(document.createElement("i")); d.appendChild(bar);
    }
    d.onclick = () => (t.conv ? goConv(t.conv) : (showView("chat"), focusAsk()));
    return d;
  }
}
setInterval(() => { if (convs.size && !$("#vTask").hidden) renderTasks(); }, 1000);

/* ───────── 定时任务（B5，照原型 rtRow / rtDetail）：开关、每天 / 每周 / 每小时、下次时间；到点在后台跑 ───────── */
const WK = "日一二三四五六";
let rtNext = {}, rtOpen = "", rtNew = null, rtFocus = "";
const hm2 = (d) => String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0");
function rtSched(r) { return r.every === "hour" ? `每小时 :${r.time.slice(3)}` : (r.every === "week" ? "每周" + WK[r.weekday] : "每天") + " " + r.time; }
function rtWhen(ts) {
  if (!ts) return "";
  const d = new Date(ts * 1000), day = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const k = Math.round((day(d) - day(new Date())) / 864e5);
  return (["今天", "明天", "后天"][k] || (k < 0 ? `${d.getMonth() + 1}月${d.getDate()}日` : "周" + WK[d.getDay()])) + " " + hm2(d);
}
async function rtRefresh() { try { rtNext = await invoke("routine_next"); } catch (e) { rtNext = {}; jslog("routine_next 失败：" + e); } }
function rtAddBtn() {
  const b = document.createElement("button"); b.type = "button"; b.className = "rtadd"; b.textContent = "+ 新建定时任务";
  b.onclick = () => { rtNew = { id: "", name: "", prompt: "", every: "day", weekday: 1, time: "09:00", on: true }; rtFocus = "rtEdprompt"; renderTasks(); };
  return b;
}
function rtRow(r) {
  const d = document.createElement("div"); d.className = "tk rt" + (r.on ? "" : " off"); d.dataset.rid = r.id;
  const row = document.createElement("div"); row.className = "row";
  const name = document.createElement("button"); name.type = "button"; name.className = "name"; name.textContent = r.name; name.id = "rtN" + r.id;
  name.setAttribute("aria-expanded", rtOpen === r.id);
  const sw = document.createElement("button"); sw.type = "button"; sw.className = "x-sw"; sw.setAttribute("role", "switch");
  sw.setAttribute("aria-checked", r.on); sw.setAttribute("aria-label", `定时任务「${r.name}」开关`);
  sw.onclick = (e) => { e.stopPropagation(); rtSave({ ...r, on: !r.on }, `${r.on ? "已关闭" : "已开启"}：${r.name}`); };
  row.append(name, sw);
  const sub = document.createElement("p"); sub.className = "rts";
  sub.textContent = rtSched(r) + " · " + (r.on ? (rtNext[r.id] ? "下次 " + rtWhen(rtNext[r.id]) : "") : "已关闭");
  d.append(row, sub);
  if (rtOpen === r.id) d.appendChild(r._edit ? rtForm(r._edit) : rtDetail(r));
  name.onclick = () => { rtOpen = rtOpen === r.id ? "" : r.id; rtFocus = "rtN" + r.id; renderTasks(); };
  return d;
}
function rtBox() { const d = document.createElement("div"); d.className = "rd"; d.onclick = (e) => e.stopPropagation(); return d; }
function rtBtns(d, list) {
  const bar = document.createElement("div"); bar.className = "rda";
  for (const [t, f, cls] of list) { const b = document.createElement("button"); b.type = "button"; b.className = "x-sb" + (cls ? " " + cls : ""); b.textContent = t; b.onclick = f; bar.appendChild(b); }
  d.appendChild(bar); return bar;
}
function rtDl(d) {
  const dl = document.createElement("dl"); d.appendChild(dl);
  return (k, v) => { const dt = document.createElement("dt"), dd = document.createElement("dd"); dt.textContent = k; if (typeof v === "string") dd.textContent = v; else dd.appendChild(v); dl.append(dt, dd); return v; };
}
function rtDetail(r) {
  const d = rtBox(), row = rtDl(d);
  row("指令", r.prompt);
  row("何时运行", rtSched(r));
  row("下次运行", r.on ? rtWhen(rtNext[r.id]) || "—" : "已暂停");
  const ul = document.createElement("ul");
  const runs = r.runs || [];
  if (!runs.length) { const li = document.createElement("li"); li.textContent = "还没有运行过"; ul.appendChild(li); }
  runs.slice(0, 3).forEach((x) => {
    const li = document.createElement("li"), st = document.createElement("span"), sm = document.createElement("small");
    st.className = x.ok ? "rok" : "rno"; st.textContent = x.ok ? "✓ 成功" : "✕ 失败";
    sm.textContent = ` ${rtWhen(x.at)} · ${x.note}`; li.append(st, sm); ul.appendChild(li);
  });
  row("运行记录", ul);
  if (r._del) {
    const bar = rtBtns(d, [["确认删除", () => { rtOpen = ""; invoke("delete_routine", { id: r.id }).then(() => toast(`已删除：${r.name}`)).catch(toastErr); }, "danger"], ["取消", () => { r._del = false; renderTasks(); }]]);
    const p = document.createElement("p"); p.textContent = "删除后不再运行"; bar.prepend(p);
  } else {
    rtBtns(d, [["立即测试", () => rtRun(r, "手动测试")], ["编辑", () => { r._edit = { ...r }; rtFocus = "rtEdprompt"; renderTasks(); }],
      [r.on ? "暂停" : "恢复", () => rtSave({ ...r, on: !r.on }, `${r.on ? "已暂停" : "已恢复"}：${r.name}`)], ["删除", () => { r._del = true; renderTasks(); }, "danger"]]);
  }
  return d;
}
// 编辑 / 新建：指令、名字、每天 / 每周几 / 每小时、时间
function rtForm(dr) {
  const d = rtBox(); d.classList.add("ed");
  if (!dr.id) { const h = document.createElement("p"); h.className = "rth"; h.textContent = "新建定时任务"; d.appendChild(h); }
  const row = rtDl(d);
  const inp = (k, ph, label) => { const x = document.createElement("input"); x.className = "x-in"; x.id = "rtEd" + k; x.value = dr[k]; x.placeholder = ph; x.spellcheck = false; x.setAttribute("aria-label", label); x.oninput = () => (dr[k] = x.value); return x; };
  row("指令", inp("prompt", "每次运行时让它做什么", "指令"));
  row("名字", inp("name", "不填就用指令开头", "名字"));
  const wrap = document.createElement("div"); wrap.className = "rtw";
  const sel = (id, label, opts, v) => { const s = document.createElement("select"); s.className = "x-in"; s.id = id; s.setAttribute("aria-label", label); opts.forEach(([val, t]) => s.add(new Option(t, val))); s.value = v; return s; };
  const ev = sel("rtEdevery", "多久一次", [["day", "每天"], ["week", "每周"], ["hour", "每小时"]], dr.every);
  const wk = sel("rtEdweekday", "星期几", [1, 2, 3, 4, 5, 6, 0].map((i) => [i, "周" + WK[i]]), dr.weekday);
  const tm = document.createElement("input"); tm.type = "time"; tm.className = "x-in"; tm.id = "rtEdtime"; tm.value = dr.time; tm.setAttribute("aria-label", "时间");
  const sync = () => { wk.hidden = dr.every !== "week"; };
  ev.onchange = () => { dr.every = ev.value; sync(); };
  wk.onchange = () => (dr.weekday = +wk.value);
  tm.oninput = () => (dr.time = tm.value || "09:00");
  sync(); wrap.append(ev, wk, tm); row("何时运行", wrap);
  const close = () => { if (dr === rtNew) rtNew = null; else { const r = ((S.cfg && S.cfg.routines) || []).find((x) => x.id === dr.id); if (r) r._edit = null; } };
  const save = async () => {
    if (!dr.prompt.trim()) { toast("要写上每次让它做什么"); document.getElementById("rtEdprompt").focus(); return; }
    const isNew = !dr.id, { _edit, _del, ...r } = dr;
    const label = r.name.trim() || r.prompt.trim().slice(0, 12);
    if (await rtSave(r, isNew ? `已创建：${label}` : `已保存：${label}`)) close();
  };
  rtBtns(d, [["保存", save, "x-pri"], ["取消", () => { close(); renderTasks(); }]]);
  d.addEventListener("keydown", (e) => { if (e.key === "Escape") { e.stopPropagation(); close(); renderTasks(); } });
  return d;
}
async function rtSave(r, msg) {
  try { await invoke("save_routine", { routine: r }); toast(msg); return true; } catch (e) { toastErr(e); return false; }
}
// 跑一次定时任务：开一个新对话在后台跑（和普通提问一样，审批卡、预算、历史都照常），不打扰当前对话
function rtRun(r, note) {
  const keep = S.conv, c = Conv(null, "定时 · " + r.name);
  showConv(c);
  S.msgs.push({ role: "user", content: r.prompt });
  addUserBubble(r.prompt, []);
  run();
  const task = c.task;
  if (task) { task.title = "定时 · " + r.name; task.rt = { id: r.id, note }; }
  saveMsg(c, "user", r.prompt).catch(() => {});
  if (keep !== c) showConv(keep);
  renderTasks();
  if (!S.open) showPeek(`定时任务开始：${r.name}`, false, () => goConv(c));
  else toast(`已开始：${r.name}`);
}
T.event.listen("routine", (e) => { if (e.payload && e.payload.id) rtRun(e.payload, "按时运行"); });

/* ───────── 悬停快捷入口（原型的 hoverEntries）：指针停在球上，旁边沿弧线弹出几个小泡 ─────────
   收起时窗口只有小球那么大，放不下小泡：弹出时把窗口扩成以小球为中心的方块，小球留在原地；收起时缩回。 */
const HV_ACT = {
  shot: ["截屏提问", '<path d="M4 8V5.5A1.5 1.5 0 0 1 5.5 4H8M16 4h2.5A1.5 1.5 0 0 1 20 5.5V8M20 16v2.5a1.5 1.5 0 0 1-1.5 1.5H16M8 20H5.5A1.5 1.5 0 0 1 4 18.5V16"/><circle cx="12" cy="12" r="2.6"/>', () => captureAsk()],
  new: ["新对话", '<path d="M5 4.5h14a2 2 0 0 1 2 2v8.5a2 2 0 0 1-2 2h-7l-4.5 3.5V17H5a2 2 0 0 1-2-2V6.5a2 2 0 0 1 2-2z"/><path d="M12 8v5.5M9.25 10.75h5.5"/>', () => { newChat(); expand("chat"); }],
  hist: ["历史记录", '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>', () => expand("hist")],
  tasks: ["任务列表", '<path d="M10 6.5h10M10 12h10M10 17.5h10M3.8 6.5l1.2 1.2 2.2-2.4M3.8 12l1.2 1.2 2.2-2.4M3.8 17.5l1.2 1.2 2.2-2.4"/>', () => expand("task")],
};
const hv = { shown: false, t: 0, hideT: 0, box: null };
const hvQ = $("#hvq"), hvTip = $("#hvTip");
const hvIds = () => { const h = S.cfg && S.cfg.hover; return h && h.on ? h.order.filter((o) => o.show && HV_ACT[o.id]).slice(0, 4).map((o) => o.id) : []; };
const hvOk = () => hvIds().length && !vc && !vcBox && !S.open && !morphing && !peekBox && !press && !orb.className.includes("s-sleep");
orb.addEventListener("pointerenter", () => {
  clearTimeout(hv.hideT);
  if (hv.shown || hv.t || !hvOk()) return;
  hv.t = setTimeout(() => { hv.t = 0; if (hvOk() && orb.matches(":hover")) hvShow(); }, (S.cfg.hover.delay || 350) + (docked ? 120 : 0));
});
orb.addEventListener("pointerleave", () => { clearTimeout(hv.t); hv.t = 0; });
orb.addEventListener("pointerdown", () => { clearTimeout(hv.t); hv.t = 0; if (hv.shown) hvHide(); }, true);
// 指针离开窗口（小球和小泡以外的地方都在窗口外）就收起
document.documentElement.addEventListener("pointerleave", () => { if (hv.shown) { clearTimeout(hv.hideT); hv.hideT = setTimeout(hvHide, 400); } });
document.documentElement.addEventListener("pointerenter", () => clearTimeout(hv.hideT));
addEventListener("keydown", (e) => { if (e.key === "Escape" && hv.shown) hvHide(); });
async function hvShow() {
  const ids = hvIds();
  morphing = true;
  try {
    const p = await winPos(), a = await monitorArea();
    const ORB = SMALL - PAD * 2, BS = 34, ring = ORB / 2 + 12 + BS / 2, E = Math.ceil(ring + BS / 2 + 6);
    const side = E * 2, cxs = p.x + SMALL / 2, cys = p.y + SMALL / 2; // 小球中心（屏幕坐标）
    hv.box = { p };
    // 弧线朝屏幕中间；上下放不下就转一点
    const right = cxs < a.x + a.w / 2, base = right ? 0 : Math.PI, sg = right ? 1 : -1, n = ids.length;
    const step = 2 * Math.asin(Math.min(1, (BS + 8) / 2 / ring));
    const tilts = [0]; for (let k = 1; k <= 7; k++) { const t = k * 0.18; tilts.push(cys < a.y + a.h / 2 ? t : -t, cys < a.y + a.h / 2 ? -t : t); }
    let pts;
    for (const t of tilts) {
      pts = []; let fit = true;
      for (let i = 0; i < n; i++) {
        const ang = base + sg * (t + (i - (n - 1) / 2) * step), bx = Math.cos(ang) * ring, by = Math.sin(ang) * ring;
        if (cys + by < a.y + 20 || cys + by > a.y + a.h - 20 || cxs + bx < a.x + 20 || cxs + bx > a.x + a.w - 20) fit = false;
        pts.push({ x: bx, y: by });
      }
      if (fit) break;
    }
    await reshape(side, side, cxs - E, cys - E, true, () => {
      body.classList.add("hovering");
      $("#app").style.padding = `${E - ORB / 2}px 0 0 ${E - ORB / 2}px`;
      hvQ.innerHTML = "";
      ids.forEach((id, i) => {
        const [label, path, run] = HV_ACT[id], b = document.createElement("button");
        b.type = "button"; b.className = "x-hvb"; b.dataset.id = id; b.setAttribute("aria-label", label);
        b.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true">${path}</svg>`;
        b.style.left = E + "px"; b.style.top = E + "px";
        b.style.setProperty("--dx", pts[i].x.toFixed(1) + "px"); b.style.setProperty("--dy", pts[i].y.toFixed(1) + "px"); b.style.setProperty("--dl", i * 40 + "ms");
        b.addEventListener("click", async () => { await hvHide(); run(); });
        const tip = () => {
          hvTip.textContent = label; const left = pts[i].x < 0, tw = hvTip.offsetWidth, th = hvTip.offsetHeight;
          hvTip.style.left = Math.max(2, Math.min(side - tw - 2, left ? E + pts[i].x - 23 - tw : E + pts[i].x + 23)) + "px";
          hvTip.style.top = Math.max(2, Math.min(side - th - 2, E + pts[i].y - th / 2)) + "px"; hvTip.classList.add("on");
        };
        b.addEventListener("pointerenter", tip); b.addEventListener("focus", tip);
        b.addEventListener("pointerleave", () => hvTip.classList.remove("on")); b.addEventListener("blur", () => hvTip.classList.remove("on"));
        hvQ.appendChild(b);
      });
    });
    void hvQ.offsetWidth;
    hvQ.classList.add("on"); hvQ.setAttribute("aria-hidden", "false");
    hv.shown = true;
  } finally { morphing = false; }
}
// 正在收起时再调用，返回同一个收起过程：点小球展开时要等小泡收完，
// 否则收起晚 200ms 才把窗口缩回小球大小，会把刚展开的卡片截掉（Windows 真机截图里见过）
function hvHide() {
  if (!hv.shown) return hv.hiding || Promise.resolve();
  hv.shown = false; clearTimeout(hv.hideT);
  hvQ.classList.remove("on"); hvQ.setAttribute("aria-hidden", "true"); hvTip.classList.remove("on");
  hv.hiding = (async () => {
    await new Promise((r) => setTimeout(r, 200));
    morphing = true;
    try {
      await reshape(SMALL, SMALL, hv.box.p.x, hv.box.p.y, true, () => { body.classList.remove("hovering"); $("#app").style.padding = ""; hvQ.innerHTML = ""; });
    } finally { morphing = false; }
    // 贴边时露出来的球：指针已经不在球上就贴回去
    if (docked && orb.classList.contains("peel") && !orb.matches(":hover")) peel(false);
  })().finally(() => { hv.hiding = null; });
  return hv.hiding;
}

/* ───────── 气泡：卡片收起时回复完成，在小球旁冒一句（取自参考版本） ─────────
   收起时窗口只有小球那么大，放不下气泡：冒泡时临时把窗口扩大，小球留在原地，消失后再缩回。 */
const PEEK_W = 260, PEEK_H = 60;
let peekTimer = 0, peekBox = null;
// err：出错提示。小泡只放得下 20 个字，点开后把整句（去哪设置）完整显示出来
let peekFull = "";
let peekGo = null; // 点小泡时要回到的对话（后台任务的提示）
async function showPeek(text, err, go) {
  const t = String(text).replace(/\s+/g, " ").trim();
  if (!t || S.open || morphing || peekBox || hv.shown) return;
  peekFull = err && t.length > 20 ? t : "";
  peekGo = go || null;
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
  const full = peekFull, go = peekGo; peekFull = ""; peekGo = null;
  if (go) go(); else await expand("chat");
  if (full) toast(full);
};
$("#newBtn").onclick = () => { newChat(); showView("chat"); focusAsk(); };
$("#histBtn").onclick = () => showView($("#vHist").hidden ? "hist" : "chat");
// 「⋯」下拉（照原型 chatMoreDd）：钉住。钉住后标题旁出现实心图钉，点它取消
const moreDd = $("#moreDd"), moreBtn = $("#moreBtn");
function setPin(v) {
  S.pinned = v;
  $("#pinMark").hidden = !v;
  moreDd.querySelector("[data-cm=pin]").setAttribute("aria-checked", v);
  toast(v ? "已钉住，点别处不会收起" : "取消钉住");
}
function moreOpen(v) {
  moreDd.classList.toggle("on", v); moreBtn.setAttribute("aria-expanded", v);
  if (v) {
    const c = $("#card").getBoundingClientRect(), b = moreBtn.getBoundingClientRect();
    moreDd.style.left = Math.max(6, b.right - c.left - moreDd.offsetWidth) + "px";
    moreDd.style.top = b.bottom - c.top + 4 + "px";
    moreDd.querySelector("button").focus();
  }
}
moreBtn.onclick = (e) => { e.stopPropagation(); moreOpen(!moreDd.classList.contains("on")); };
moreDd.addEventListener("click", (e) => {
  const b = e.target.closest("[data-cm]"); if (!b) return;
  moreOpen(false);
  if (b.dataset.cm === "pin") setPin(!S.pinned);
  if (b.dataset.cm === "tmp") askTmp(!S.conv.tmp);
  if (b.dataset.cm === "ho") hoOpen();
});

/* ───────── 交给终端（B4，照原型 #ho）───────── */
// 交接说明：从这次对话整理（要做什么、讨论过什么、最后的结论、下一步），用户可以先改再交
function hoMd() {
  const ms = S.msgs.filter((m) => typeof m.content === "string" && m.content.trim());
  const u = ms.filter((m) => m.role === "user"), a = ms.filter((m) => m.role === "assistant");
  const cut = (s, n) => { s = s.replace(/\s+/g, " ").trim(); return s.length > n ? s.slice(0, n) + "…" : s; };
  const lines = ["# 交接说明", "", "## 要做什么", u.length ? cut(u[u.length - 1].content, 600) : "（还没说要做什么）", ""];
  if (ms.length > 1) { lines.push("## 讨论过什么"); ms.slice(-10, -1).forEach((m) => lines.push(`- ${m.role === "user" ? "我" : "悬浮球"}：${cut(m.content, 200)}`)); lines.push(""); }
  if (a.length) lines.push("## 最后的结论", cut(a[a.length - 1].content, 1200), "");
  lines.push("## 下一步", "在这个项目里接着做上面「要做什么」。动手前先看一遍代码，有不清楚的先问我。");
  return lines.join("\n");
}
let hoS = null; // 扫描结果 {tools, terms}
const hoH = () => (S.cfg && S.cfg.handoff) || { tool: "", term: "", dirs: [], save: true, custom: "" };
function hoOpt(sel, list) { sel.replaceChildren(...list.map(([v, t]) => new Option(t, v))); }
async function hoScan() {
  const to = $("#hoTo");
  hoOpt(to, [["", "正在找本机的工具"]]); $("#hoToN").textContent = ""; $("#hoGo").disabled = true;
  try { hoS = await invoke("handoff_scan"); } catch (e) { hoS = { tools: [], terms: [] }; jslog("handoff_scan 失败：" + e); }
  const h = hoH();
  hoOpt(to, [...hoS.tools.map((t) => [t.id, t.name + (t.direct ? "" : "（打开后粘贴一次）")]), ["custom", "自定义命令"]]);
  to.value = hoS.tools.some((t) => t.id === h.tool) || h.tool === "custom" ? h.tool : (hoS.tools[0] || { id: "custom" }).id;
  const tm = $("#hoTm");
  hoOpt(tm, hoS.terms.map((t) => [t.id, t.name]));
  if (hoS.terms.some((t) => t.id === h.term)) tm.value = h.term;
  hoTool();
}
// 选了哪个工具：没装任何工具时说一声；要粘贴的先说清楚；自定义的出输入框
function hoTool() {
  const id = $("#hoTo").value, t = hoS && hoS.tools.find((x) => x.id === id);
  $("#hoCustom").hidden = id !== "custom";
  $("#hoToN").textContent = id === "custom"
    ? (hoS && !hoS.tools.length ? "本机没找到常见的编程工具（Claude Code、Codex、Gemini CLI 等），可以填一条自己的命令" : "在项目里运行这条命令，{file} 换成交接说明文件")
    : t && !t.direct ? "终端打开后按 Ctrl+V（Mac 上 ⌘V）再回车" : "打开时直接带上交接说明";
  hoReady();
}
function hoReady() {
  const id = $("#hoTo").value, md = $("#hoMd").value;
  // 收起时只显示一句摘要（「要做什么」那一行）；卡片只有 260 高，放不下整段
  $("#hoMdS").textContent = (md.split("\n").find((_l, i, a) => a[i - 1] === "## 要做什么") || md.replace(/^#.*$/gm, "").trim().split("\n")[0] || "（空）").trim();
  $("#hoGo").disabled = !id || !$("#hoDir").value || !md.trim() || !$("#hoTm").value || (id === "custom" && !$("#hoCustom").value.trim());
}
function hoDirs(first) {
  const ds = [...new Set([first, ...hoH().dirs].filter(Boolean))];
  hoOpt($("#hoDir"), ds.length ? ds.map((d) => [d, d]) : [["", "先选一个项目文件夹"]]);
}
function hoEdit(on) {
  $("#hoMd").hidden = !on; $("#hoMdS").hidden = on;
  $("#hoEdit").setAttribute("aria-expanded", on); $("#hoEdit").textContent = on ? "收起" : "查看和修改";
}
function hoOpen() {
  const h = hoH();
  $("#hoMd").value = hoMd();
  $("#hoCustom").value = h.custom || "";
  $("#hoSave").setAttribute("aria-checked", h.save !== false);
  hoDirs(); hoEdit(false);
  showView("ho");
  hoScan().then(() => $("#hoTo").focus());
}
$("#hoTo").onchange = hoTool;
$("#hoTm").onchange = $("#hoDir").onchange = hoReady;
$("#hoMd").oninput = $("#hoCustom").oninput = hoReady;
$("#hoReScan").onclick = hoScan;
$("#hoEdit").onclick = () => { const on = $("#hoMd").hidden; hoEdit(on); if (on) $("#hoMd").focus(); };
$("#hoSave").onclick = () => { const b = $("#hoSave"); b.setAttribute("aria-checked", b.getAttribute("aria-checked") !== "true"); };
$("#hoPick").onclick = async () => {
  try { const d = await invoke("handoff_pick", { start: $("#hoDir").value || "" }); if (d) { hoDirs(d); hoReady(); } } catch (e) { toastErr(e); }
};
const hoClose = () => { showView("chat"); focusAsk(); };
$("#hoCancel").onclick = hoClose;
$("#vHo").addEventListener("keydown", (e) => { if (e.key === "Escape") { e.stopPropagation(); hoClose(); } });
$("#hoGo").onclick = async () => {
  const b = $("#hoGo"); b.disabled = true;
  try {
    const [file, paste] = await invoke("handoff_go", { md: $("#hoMd").value, tool: $("#hoTo").value, term: $("#hoTm").value, dir: $("#hoDir").value,
      save: $("#hoSave").getAttribute("aria-checked") === "true", custom: $("#hoCustom").value });
    jslog("交接说明：" + file);
    if (!paste) toast("终端已打开，由它接手");
    else {
      try { await navigator.clipboard.writeText(paste); toast("已复制，在终端里按 Ctrl+V（Mac 上 ⌘V）再回车"); }
      catch (e) { jslog("剪贴板写不进去：" + e); toast("请在终端里输入：" + paste); }
    }
    hoClose();
  } catch (e) { toastErr(e); b.disabled = false; }
};
moreDd.addEventListener("keydown", (e) => { if (e.key === "Escape") { e.stopPropagation(); moreOpen(false); moreBtn.focus(); } });
addEventListener("pointerdown", (e) => { if (moreDd.classList.contains("on") && !e.target.closest("#moreDd,#moreBtn")) moreOpen(false); });
$("#pinMark").onclick = () => setPin(false);

let toastTimer = 0;
function toast(t) {
  const el = $("#toast");
  el.textContent = t; el.classList.add("on");
  // 长的提示（比如告诉你去哪设置）多留一会儿，够读完
  clearTimeout(toastTimer); toastTimer = setTimeout(() => el.classList.remove("on"), Math.max(2600, String(t).length * 150));
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
  root.setProperty("--tint", String(+(0.58 * (1 - Math.min(40, c.glass || 0) / 100)).toFixed(3))); // 原型 .x-glassy 的墨层 0.58，「更通透」按比例减
  // 减少动态效果；跟随系统深浅色（系统为深色时卡片玻璃更深）
  const html = document.documentElement.classList;
  html.toggle("calm", !!c.calm);
  html.toggle("dark", !!c.follow_system && darkMQ.matches);
  if (small !== SMALL) {
    SMALL = small;
    // 收起着的时候窗口就是小球那么大，跟着改
    if (!S.open) appWin.setSize(new LogicalSize(SMALL, SMALL)).catch(() => {});
  }
}
const darkMQ = matchMedia("(prefers-color-scheme: dark)");
darkMQ.addEventListener("change", applyLook);
const msgsEl = $("#msgs");
function updateEmpty() {
  $("#empty").hidden = msgsEl.children.length > 0;
  // 还没设置模型时，空白页改成提示去设置（不再一启动就强行打开设置页）
  $("#empty").classList.toggle("setup", needsSetup());
  msgsEl.hidden = msgsEl.children.length === 0;
}
function scrollEnd() { msgsEl.scrollTop = msgsEl.scrollHeight; }
/* 后台任务（B3）：每个对话一份气泡 + 消息 + 正在跑的请求。新对话、打开历史只是换一个显示，
   原来那个对话的请求在后台接着跑，回复写进它自己的气泡（收在 conv.dom 里，不在屏幕上）。 */
const convs = new Set(); // 有请求在跑的对话（跑完就删掉）
function Conv(id, title) { return { id, title: title || "新对话", msgs: [], dom: document.createDocumentFragment(), busy: null, stop: null, task: null, tmp: false, tmpChanging: false, history: Promise.resolve(), historyEpoch: 0 }; }
S.conv = Conv(null);
const ASK_PH = "有什么可以帮你？", TMP_PH = "临时对话…";
const tmpQ = $("#tmpQ"), tmpOk = $("#tmpOk"), tmpNo = $("#tmpNo");
let tmpTarget = null;
// 每个对话串行写入；转临时成功后，旧队列即使迟到也不能重新创建历史。
function saveMsg(c, role, content) {
  if (c.tmp) return Promise.resolve();
  const epoch = c.historyEpoch;
  c.history = c.history.catch(() => {}).then(async () => {
    if (c.tmp || epoch !== c.historyEpoch) return;
    c.id = await invoke("history_append", { convId: c.id, role, content });
    if (c === S.conv) S.convId = c.id;
  });
  return c.history;
}
function syncTmp() {
  const c = S.conv;
  $("#card").classList.toggle("tmp", c.tmp);
  orb.classList.toggle("tmp", c.tmp);
  $("#tmpBanner").hidden = !c.tmp || $("#vChat").hidden;
  const b = moreDd.querySelector("[data-cm=tmp]");
  b.setAttribute("aria-checked", c.tmp); b.disabled = c.tmpChanging;
  const ph = c.tmp ? TMP_PH : ASK_PH;
  if ((ask.placeholder === ASK_PH || ask.placeholder === TMP_PH) && ask.placeholder !== ph) ask.placeholder = ph;
}
// 截屏、附件或听写还原占位文字时，也要保留当前会话的临时标记。
new MutationObserver(syncTmp).observe(ask, { attributes: true, attributeFilter: ["placeholder"] });
function closeTmp() { tmpQ.hidden = true; tmpTarget = null; tmpOk.disabled = tmpNo.disabled = false; }
function askTmp(on) {
  const c = S.conv;
  if (c.tmpChanging || c.tmp === on) return;
  if (!c.msgs.length) { setTmp(c, on); return; }
  tmpTarget = { conv: c, on };
  $("#tmpQT").textContent = on ? "转为临时对话？之前的消息会从历史里删掉。" : "改回普通对话？这段对话会存进历史记录。";
  tmpOk.textContent = on ? "转成临时" : "改回普通";
  tmpQ.hidden = false; tmpOk.focus({ preventScroll: true });
}
async function setTmp(c, on) {
  if (c.tmpChanging || c.tmp === on) return;
  c.tmpChanging = true;
  if (c === S.conv) syncTmp();
  try {
    if (on) {
      const change = c.history.catch(() => {}).then(async () => {
        if (c.id != null) await invoke("history_delete", { convId: c.id });
        c.id = null; c.tmp = true; c.historyEpoch++;
        if (c === S.conv) S.convId = null;
      });
      c.history = change;
      await change;
    } else {
      c.tmp = false;
      await Promise.all(c.msgs.map((m) => saveMsg(c, m.role, typeof m.content === "string" ? m.content : historyText(m.content[0].text, m.content.slice(1).map(() => ({ kind: "image" }))))));
    }
    if (c === S.conv) { syncTmp(); toast(on ? "已切换为临时对话 · 不会保存" : "已改回普通对话"); }
  } catch (e) { toastErr((on ? "无法转为临时对话，历史未删除：" : "普通对话保存失败：") + e); }
  finally { c.tmpChanging = false; if (c === S.conv) syncTmp(); }
}
tmpOk.onclick = async () => {
  const target = tmpTarget; if (!target) return;
  tmpOk.disabled = tmpNo.disabled = true;
  await setTmp(target.conv, target.on);
  if (tmpTarget === target) { closeTmp(); moreBtn.focus({ preventScroll: true }); }
};
tmpNo.onclick = () => { closeTmp(); moreBtn.focus({ preventScroll: true }); };
tmpQ.addEventListener("keydown", (e) => {
  if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); tmpNo.click(); }
  if (e.key === "Tab") { e.preventDefault(); (document.activeElement === tmpOk ? tmpNo : tmpOk).focus(); }
});
// 把当前对话的气泡收起来，换上 c 的
function showConv(c) {
  const o = S.conv;
  if (o === c) return;
  closeTmp();
  if (o) { o.id = S.convId; o.msgs = S.msgs; o.title = $("#title").textContent; o.dom.append(...msgsEl.childNodes); }
  S.conv = c; S.convId = c.id; S.msgs = c.msgs; S.busy = c.busy; S.stopFn = c.stop;
  msgsEl.innerHTML = ""; msgsEl.append(...c.dom.childNodes);
  $("#title").textContent = c.title;
  $("#stopRow").hidden = !c.busy;
  needOn([...convs].some((x) => x.need));
  seen(c);
  syncTmp(); updateEmpty(); scrollEnd();
}
function newChat() {
  showConv(Conv(null));
}
function newTmpChat() {
  const c = Conv(null); c.tmp = true;
  showConv(c); expand("chat");
}
// 在后台做完、还没看的对话：球角上的数字（照原型），点开那条对话就清掉
const unseen = new Set();
function doneMark(c) { unseen.add(c); badge(); }
function seen(c) { unseen.delete(c); badge(); }
function badge() { const n = unseen.size; orb.querySelector(".dn").dataset.n = n; if (n) orb.dataset.done = n; else delete orb.dataset.done; }
// 回到某条对话（任务页点一行、点后台任务的小泡）
function goConv(c) {
  showConv(c); seen(c);
  if (S.open) showView("chat"); else expand("chat");
  focusAsk();
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
  const c = S.conv;
  saveMsg(c, "user", historyText(text, attach)).catch(() => {});
  if (c.msgs.length === 1) { c.title = text.slice(0, 24) || "截图提问"; $("#title").textContent = c.title; }
  run();
}

const HO_RE = /改代码|写代码|重构|脚本|bug|报错/i;
function run() {
  const id = "c" + Date.now(), conv = S.conv;
  const prompt = S.msgs[S.msgs.length - 1].content;
  const codeTask = HO_RE.test(typeof prompt === "string" ? prompt : prompt[0].text);
  const here = () => conv === S.conv; // 这个对话是不是正显示在卡片里
  conv.busy = S.busy = id; convs.add(conv);
  $("#stopRow").hidden = false;
  mood("think");
  const task = { title: (S.msgs[S.msgs.length - 1] && historyText(lastText(), [])) || "提问", st: "run", t0: Date.now(), conv };
  conv.task = task;
  S.tasks.unshift(task); renderTasks();
  const el = addAiBubble();
  const scrollEnd = () => { if (here()) msgsEl.scrollTop = msgsEl.scrollHeight; };
  let txt = el.querySelector(".txt");
  // out：整条回复（存历史、复制）；seg：当前这段文字。工具药丸把回复分成几段，药丸后面的字另起一段
  let out = "", seg = "", usage = null, raf = 0;
  const paint = () => { raf = 0; const live = conv.busy === id; txt.innerHTML = live && !seg ? "" : live ? md(seg + "\uE000").replace("\uE000", '<span class="cur"></span>') : md(seg); scrollEnd(); };
  const ch = new Channel();
  ch.onmessage = (ev) => {
    if (conv.busy !== id) return;
    if (ev.type === "delta") {
      out += ev.text; seg += ev.text; // 说话时保持思考的光点（照原型：流式输出时是 think）
      if (!raf) raf = requestAnimationFrame(paint);
    } else if (ev.type === "tool") {
      toolPill(el, ev);
      if (seg) { txt.classList.remove("cursor"); txt.innerHTML = md(seg); seg = ""; out += "\n\n"; const t2 = document.createElement("div"); t2.className = "txt cursor"; txt = t2; }
      el.insertBefore(txt, el.querySelector(".meta"));
      if (ev.status === "run" && here()) mood("work");
      scrollEnd();
    } else if (ev.type === "team") {
      teamRow(el, ev);
      if (seg) { txt.classList.remove("cursor"); txt.innerHTML = md(seg); seg = ""; out += "\n\n"; const t2 = document.createElement("div"); t2.className = "txt cursor"; txt = t2; }
      el.insertBefore(txt, el.querySelector(".meta"));
      if (here()) mood("work");
      scrollEnd();
    } else if (ev.type === "budget") {
      budgetCard(el, ev, task);
      conv.need = true; needOn(true); mood("wait");
      if (!S.open || !here()) showPeek(`预算用完，已暂停：${task.title}`, false, () => goConv(conv));
      scrollEnd();
    } else if (ev.type === "ask") {
      askCard(el, ev);
      conv.need = true; needOn(true); mood("wait"); // 原型：等你确认 → 「等你」表情，收起时球角上亮「!」
      if (!S.open || !here()) showPeek("需要你确认：" + ev.desc, false, () => goConv(conv));
      scrollEnd();
    } else if (ev.type === "usage") {
      usage = ev;
    } else if (ev.type === "done") {
      finish();
      txt.classList.remove("cursor"); paint();
      out = out.replace(/\n\n$/, "");
      if (!out) { txt.textContent = "（模型没有返回内容）"; }
      conv.msgs.push({ role: "assistant", content: out });
      saveMsg(conv, "assistant", out).catch(() => {});
      setMeta(el, usage && usage.prompt + usage.completion ? `${leadModelName()} · ${fmtTok(usage.prompt + usage.completion)} tokens` : leadModelName(), { copy: out });
      if (codeTask && (S.cfg.handoff || {}).hint !== false) {
        const b = document.createElement("button"); b.type = "button"; b.className = "ho-hint"; b.textContent = "交给终端";
        b.onclick = () => { goConv(conv); hoOpen(); };
        const meta = el.querySelector(".meta"); meta.classList.add("with-ho"); meta.appendChild(b);
      }
      mood("done", 1500);
      if (here()) vcSpeak(el, out);
      // 在后台做完的：球角上记一个数，点开回到那条对话（照原型：完成时小球开心 + 角标）
      if (!S.open || !here()) { if (!here()) doneMark(conv); poke(); showPeek(out || "回复好了", false, here() ? null : () => goConv(conv)); }
    } else if (ev.type === "error") {
      finish("err");
      txt.classList.remove("cursor");
      el.classList.add("err");
      txt.textContent = ev.message;
      const noKey = /API Key|设置/.test(ev.message);
      setMeta(el, "", { retry: () => { el.remove(); goConv(conv); run(); }, settings: noKey });
      mood("error", 4000);
      if (!here()) doneMark(conv);
    }
  };
  function finish(st = "done") {
    if (conv.busy === id) { conv.busy = conv.stop = null; conv.need = false; convs.delete(conv); }
    if (here()) { S.busy = S.stopFn = null; $("#stopRow").hidden = true; }
    if (Object.keys(S.agBusy).length && !convs.size) { S.agBusy = {}; agSync(); }
    needOn([...convs].some((x) => x.need));
    if (task.st === "run") {
      task.st = st; task.t1 = Date.now(); renderTasks();
      if (task.rt) invoke("routine_done", { id: task.rt.id, ok: st === "done", note: { done: task.rt.note, err: "运行失败，下次按时重试", stop: "已停止" }[st] || "" }).catch(() => {});
    }
  }
  el._stop = () => {
    invoke("cancel_chat", { id }).catch(() => {});
    finish("stop"); txt.classList.remove("cursor");
    el.querySelectorAll(".x-ap button, .x-ap input").forEach((b) => (b.disabled = true));
    if (out) { txt.innerHTML = md(seg); conv.msgs.push({ role: "assistant", content: out }); saveMsg(conv, "assistant", out + "\n\n（已停止）").catch(() => {}); }
    else txt.textContent = "（已停止）";
    setMeta(el, "已停止", out ? { copy: out } : {});
    if (here() || !convs.size) mood("idle");
  };
  conv.stop = el._stop; if (here()) S.stopFn = el._stop;
  invoke("chat", { id, messages: S.msgs, onEvent: ch }).catch((e) => ch.onmessage({ type: "error", message: String(e) }));
}
// ───────── 多 Agent 计划卡（照搬原型 runTeam 的 .x-plan）：主控每交给一个 Agent 一步，卡里多一行 ─────────
S.agBusy = {}; // Agent id → 正在做的那一步（任务页 Agents 头像条的状态）
const agList = () => ((S.cfg && S.cfg.agents) || []);
const agOf = (id) => agList().find((a) => a.id === id);
const avHTML = (a, big) => `<span class="x-av${big ? " x-avl" : ""}${a.emoji ? " x-ave" : ""}" style="--t:${/^#[0-9a-f]{6}$/i.test(a.tint || "") ? a.tint : "#E6D49A"}" data-agv="${String(a.id || "").replace(/[^\w-]/g, "")}" aria-hidden="true">${a.emoji ? String(a.emoji).replace(/[&<>"]/g, "") : ""}</span>`;
function agentModelName(a) {
  const ms = (S.cfg && S.cfg.models) || [], mn = (m) => m.name || m.model;
  const m = ms.find((x) => x.id === a.model), d = ms.find((x) => x.id === S.cfg.default_model);
  return m ? mn(m) : d ? mn(d) + "（默认）" : "未设置";
}
function teamRow(el, ev) {
  let c = el.querySelector(".x-plan");
  if (!c) {
    c = document.createElement("div"); c.className = "x-plan"; c.setAttribute("role", "group");
    c.innerHTML = '<p class="x-plh"></p><ol></ol><p class="x-plf"></p>';
    el.insertBefore(c, el.querySelector(".meta"));
  }
  const ol = c.children[1];
  let r = ol.querySelector(`.x-ps[data-call="${ev.call}"]`);
  const a = agOf(ev.agent_id) || { id: ev.agent_id, name: ev.agent, tint: "#E6D49A" };
  if (!r) {
    r = document.createElement("li"); r.className = "x-ps"; r.dataset.call = ev.call;
    r.innerHTML = avHTML(a) + '<span class="x-psn"><b></b><span class="x-pm"></span></span><span class="x-pss"></span><span class="x-ptk"></span><span class="x-pst"></span>';
    r.querySelector("b").textContent = ev.agent; r.querySelector(".x-pm").textContent = ev.model || agentModelName(a); r.querySelector(".x-pss").textContent = ev.task;
    // 点开一步：一行「为什么交给它」（它的职责和会用的工具）
    const wy = document.createElement("li"); wy.className = "x-pwhy"; wy.hidden = true;
    r.tabIndex = 0; r.setAttribute("role", "button"); r.setAttribute("aria-expanded", "false");
    const tog = () => { const on = wy.hidden; if (on) wy.textContent = whyLine(a); wy.hidden = !on; r.setAttribute("aria-expanded", on); };
    r.addEventListener("click", tog);
    r.addEventListener("keydown", (e) => { if (e.target === r && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); tog(); } });
    ol.append(r, wy);
  }
  // 用户在预算卡上点了「停止」：这一步之后报的错就是这个，仍显示「已停止」
  const userStop = ev.status === "err" && r.dataset.st === "stop";
  if (!userStop) {
    r.dataset.st = ev.status === "err" ? "stop" : ev.status;
    r.querySelector(".x-pst").textContent = { run: "进行中", done: "✓", err: "出错" }[ev.status] || "等待";
  }
  r.dataset.tok = ev.tokens || 0;
  r.querySelector(".x-ptk").textContent = ev.tokens ? fmtTok(ev.tokens) : "";
  const rows = [...ol.querySelectorAll(".x-ps")], n = rows.length, tot = rows.reduce((s, x) => s + +x.dataset.tok, 0);
  c.firstChild.textContent = `主控 · ${n} 步计划 · 点一步看为什么交给它`;
  c.setAttribute("aria-label", `主控的 ${n} 步计划`);
  c.lastChild.textContent = `共 ${fmtTok(tot)} tokens`;
  if (ev.status === "run") S.agBusy[ev.agent_id] = ev.task; else delete S.agBusy[ev.agent_id];
  agSync();
}
function whyLine(a) {
  const tools = (a.tools || []).join("、");
  return `${a.name}的职责是「${a.role || "没写职责"}」` + (tools ? `，会用 ${tools}` : "") + "，所以主控把这一步交给它。";
}
// ───────── 任务页顶上的 Agents 头像条 + 弹卡（照搬原型 agRow / pop） ─────────
let agOpen = null;
function agRow() {
  const w = document.createElement("div"); w.className = "x-ags";
  w.innerHTML = '<p class="x-hg">Agents</p><div class="x-agl"></div><div class="x-agp" role="status" hidden></div>';
  agList().slice(1).forEach((a) => {
    const b = document.createElement("button"); b.type = "button"; b.className = "x-ag"; b.dataset.ag = a.id;
    b.innerHTML = `<span class="x-avw">${avHTML(a)}<i class="x-agd"></i></span><span></span>`; b.lastChild.textContent = a.name;
    b.addEventListener("click", (e) => { e.stopPropagation(); agOpen = agOpen === a.id ? null : a.id; agSync(); });
    w.children[1].appendChild(b);
  });
  return w;
}
function agSync() {
  const w = $("#tasks") && $("#tasks").querySelector(".x-ags");
  if (!w) return;
  w.querySelectorAll(".x-ag").forEach((b) => {
    const a = agOf(b.dataset.ag), busy = S.agBusy[b.dataset.ag] != null;
    b.dataset.st = busy ? "busy" : "idle"; b.setAttribute("aria-expanded", agOpen === b.dataset.ag);
    b.setAttribute("aria-label", `${a ? a.name : ""}，${busy ? "忙碌" : "空闲"}`);
  });
  const pop = w.querySelector(".x-agp"), a = agOpen && agOf(agOpen), b = a && w.querySelector(`.x-ag[data-ag="${a.id}"]`);
  pop.hidden = !b;
  if (!b) return;
  pop.innerHTML = '<b></b><p></p><p class="x-agm"><span></span><button type="button" class="x-agx">更换</button></p><p></p>';
  const [nm, p1, pm, p3] = pop.children;
  nm.textContent = a.name; p1.textContent = a.role || "还没写职责";
  pm.firstChild.textContent = "模型 · " + agentModelName(a);
  pm.lastChild.setAttribute("aria-label", `更换${a.name}的模型`);
  pm.lastChild.addEventListener("click", () => { agOpen = null; agSync(); invoke("open_settings", { agent: a.id }).catch(toastErr); });
  p3.textContent = S.agBusy[a.id] != null ? `忙碌 · ${S.agBusy[a.id]}` : "空闲";
  // 靠右的 Agent 弹卡往左让，不被卡片右边挤窄
  pop.style.left = Math.max(4, Math.min(b.offsetLeft, pop.parentElement.clientWidth - pop.offsetWidth - 4)) + "px";
}
document.addEventListener("pointerdown", (e) => { if (agOpen && !e.target.closest(".x-ag,.x-agp")) { agOpen = null; agSync(); } });
addEventListener("keydown", (e) => { if (e.key === "Escape" && agOpen) { agOpen = null; agSync(); e.stopImmediatePropagation(); } }, true);

// 有确认在等你：球角上的「!」（收起时才显示）
function needOn(v) { S.pend = !!v; orb.classList.toggle("need", !!v); }
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
  play: '<path d="M8 5.5v13l10.5-6.5z"/>',
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
// 预算用满（照原型 capCard / capGo）：正在做的步骤停住，对话里一张小卡「再给 2万 / 停止」
const wan = (n) => Math.round(n / 1000) / 10 + "万";
function budgetCard(el, ev, task) {
  task.why = "预算"; renderTasks();
  const tok = ev.kind === "tokens";
  const c = document.createElement("div");
  c.className = "x-ap"; c.setAttribute("role", "group"); c.setAttribute("aria-label", "预算已用完");
  c.innerHTML = `<p class="x-aph"><span class="x-risk">预算</span>${tok ? "单个任务上限" : "最多步数"}</p><p class="x-apd"></p><div class="x-apb"><button type="button" class="x-sb x-pri"></button><button type="button" class="x-sb">停止</button></div>`;
  c.querySelector(".x-apd").textContent = tok ? `已用 ${(ev.used / 1e4).toFixed(1)}万 / ${wan(ev.limit)} tokens，已暂停` : `已走 ${ev.used} / ${ev.limit} 步，已暂停`;
  const [more, stop] = c.querySelectorAll("button");
  more.textContent = tok ? `再给 ${wan(ev.more)}` : `再给 ${ev.more} 步`;
  el.insertBefore(c, el.querySelector(".meta"));
  // 计划卡里正在做的那几步显示「已暂停」
  const paused = [...el.querySelectorAll('.x-ps[data-st="run"]')];
  paused.forEach((r) => { r.dataset.st = "pause"; r.querySelector(".x-pst").innerHTML = icoSvg("pause", "x-ico--in") + "已暂停"; });
  let done = false;
  const go = (b, allow) => {
    if (done) return; done = true;
    needOn(false); hidePeek();
    task.why = ""; renderTasks();
    c.querySelectorAll("button").forEach((x) => (x.disabled = true)); b.classList.add("on");
    const n = document.createElement("p"); n.className = "x-apn"; c.appendChild(n);
    n.textContent = allow ? (tok ? `上限加到 ${wan(ev.used + ev.more)} tokens，继续` : `最多步数加到 ${ev.used + ev.more} 步，继续`) : "已停止";
    paused.forEach((r) => { if (r.dataset.st !== "pause") return; r.dataset.st = allow ? "run" : "stop"; r.querySelector(".x-pst").textContent = allow ? "进行中" : "已停止"; });
    invoke("answer_ask", { ask: ev.ask, answer: { allow, always: false, reason: "", edit: "" } }).then((ok) => { if (!ok) n.textContent = "这个任务已经结束了"; }).catch(() => {});
    mood(allow ? "work" : "think");
  };
  more.addEventListener("click", () => go(more, true));
  stop.addEventListener("click", () => go(stop, false));
  more.focus({ preventScroll: true });
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
    needOn(false);
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
      if (S.convId === c.id && !S.busy) newChat();
      loadHistory();
    };
    ul.appendChild(li);
  }
}
async function openConv(c) {
  const live = [...convs].find((x) => x.id === c.id);
  if (live) { showConv(live); showView("chat"); return; }
  const rows = await invoke("history_load", { convId: c.id });
  showConv(Conv(c.id, c.title));
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
/* ───────── 多显示器（原型的 mdsSet）─────────
   mouse：小球跟着鼠标去它所在的屏幕；main：固定在主屏；last：待在上次的位置（拖到哪就是哪）。
   换屏时按小球在原屏幕上的相对位置放到新屏幕，开了「滑过去」就几帧滑过去，减少动态时直接跳。全程用物理像素，免得两块屏缩放不同时算错。 */
const inScr = (m, x, y) => x >= m.x && x < m.x + m.w && y >= m.y && y < m.y + m.h;
let scrBusy = false;
async function screenCheck() {
  const mode = S.cfg && S.cfg.screen;
  if (!mode || mode === "last" || scrBusy || S.open || morphing || hv.shown || peekBox || press || orb.classList.contains("dragging") || orb.classList.contains("peel")) return;
  scrBusy = true;
  try {
    const sc = await invoke("screens");
    if (!sc || !sc.list || sc.list.length < 2) return;
    const p = await appWin.outerPosition(), z = await appWin.outerSize();
    const cx = p.x + z.width / 2, cy = p.y + z.height / 2;
    const from = sc.list.find((m) => inScr(m, cx, cy)) || sc.list.find((m) => m.primary) || sc.list[0];
    const to = mode === "main" ? sc.list.find((m) => m.primary) : sc.cursor && sc.list.find((m) => inScr(m, sc.cursor[0], sc.cursor[1]));
    if (!to || (to.x === from.x && to.y === from.y)) return;
    // 在原屏的相对位置 → 新屏；窗口在新屏上的物理尺寸按新屏的缩放算
    const fx = (cx - from.x) / from.w, fy = (cy - from.y) / from.h, half = (SMALL * to.scale) / 2;
    const tx = Math.round(Math.max(to.x, Math.min(to.x + fx * to.w - half, to.x + to.w - half * 2)));
    const ty = Math.round(Math.max(to.y, Math.min(to.y + fy * to.h - half, to.y + to.h - half * 2)));
    morphing = true; orb.classList.remove("peel"); setDock(0);
    try {
      const slide = S.cfg.screen_slide && !S.cfg.calm && !matchMedia("(prefers-reduced-motion: reduce)").matches;
      const n = slide ? 8 : 1;
      for (let i = 1; i <= n; i++) {
        const k = n === 1 ? 1 : 1 - Math.pow(1 - i / n, 3); // 缓出
        await appWin.setPosition(new PhysicalPosition(Math.round(p.x + (tx - p.x) * k), Math.round(p.y + (ty - p.y) * k)));
        if (i < n) await new Promise((r) => setTimeout(r, 30));
      }
    } finally { morphing = false; }
    await settle(); // 新屏上照常吸边/贴边，并记住位置
  } catch (e) { jslog("换屏失败：" + (e && e.message ? e.message : e)); }
  finally { scrBusy = false; }
}
setInterval(() => { if (S.cfg && S.cfg.screen === "mouse") screenCheck(); }, 700);

listen("config", async () => {
  const was = S.cfg ? S.cfg.auto_dock + "|" + S.cfg.orb_size : "";
  try { S.cfg = await invoke("get_config"); } catch (_) { return; }
  applyLook();
  updateEmpty();
  if (!$("#vTask").hidden) rtRefresh().then(renderTasks);
  // 改了贴边开关或小球大小：重新贴一次
  if (was !== S.cfg.auto_dock + "|" + S.cfg.orb_size && !S.open) setTimeout(settle, 60);
  if (S.cfg.screen === "main") setTimeout(() => screenCheck(), 120);
});
listen("menu", async ({ payload }) => {
  switch (payload) {
    case "toggle": toggle(); break;
    case "open": expand(); break;
    case "new_chat": newChat(); expand("chat"); break;
    case "new_tmp": newTmpChat(); break;
    case "capture": captureAsk(); break;
    case "history": expand("hist"); break;
    case "tasks": expand("task"); break;
    case "handoff": await expand("chat"); hoOpen(); break;
    case "routines": await expand("task"); $("#routineHead").scrollIntoView({ block: "start" }); break;
    case "sleep": {
      const wake = orb.classList.contains("s-sleep");
      await collapse();
      S.lastActive = Date.now();
      mood(wake ? "idle" : "sleep");
      break;
    }
  }
});

/* ───────── 划词：工具栏上点了某个动作 ───────── */
const SEL_LABEL = { ask: "问问它", explain: "解释", tr: "翻译", sum: "总结", rew: "改写", read: "朗读", expand: "扩写", shrink: "缩写" };
const hasZh = (t) => /[\u3400-\u9fff]/.test(t);
function selPrompt(act, t) {
  const tr = (S.cfg && S.cfg.sel && S.cfg.sel.tr) || "auto";
  const to = { en: "英文", zh: "中文", ja: "日文" }[tr === "auto" ? (hasZh(t) ? "en" : "zh") : tr];
  const P = {
    explain: "解释下面这段文字在说什么，简洁清楚：",
    tr: `把下面这段文字翻译成${to}，只给译文：`,
    sum: "用一句话总结下面这段文字：",
    rew: "改写下面这段文字，让它更通顺清楚，意思不变，只给改写后的文字：",
    expand: "把下面这段文字扩写成更完整的一段，意思不变：",
    shrink: "把下面这段文字缩写得更短，保留要点：",
  };
  return P[act] + "\n\n" + t;
}
// 用户消息：引用块 + 动作名（原型的 selBubble）
function addSelBubble(t, label) {
  const el = document.createElement("div");
  el.className = "m user";
  const q = document.createElement("span"); q.className = "x-sel-q"; q.textContent = t;
  const a = document.createElement("span"); a.className = "x-sel-qa"; a.textContent = label;
  el.append(q, a); msgsEl.appendChild(el);
  updateEmpty(); scrollEnd();
}
async function selAct({ act, text }) {
  const t = String(text || "").trim(); if (!t || !SEL_LABEL[act]) return;
  if (act === "read") {
    // 朗读：用系统语音读出来，不开对话
    if (!window.speechSynthesis) return;
    speechSynthesis.cancel();
    const u = new SpeechSynthesisUtterance(t.slice(0, 4000)); u.lang = hasZh(t) ? "zh-CN" : "en-US";
    mood("work"); u.onend = u.onerror = () => mood("idle");
    speechSynthesis.speak(u); return;
  }
  await expand("chat");
  if (act === "ask") {
    // 问问它：引用放进输入框，不自动发送
    const flat = t.replace(/\s+/g, " ");
    ask.value = "「" + (flat.length > 200 ? flat.slice(0, 200) + "…" : flat) + "」 ";
    autosize(); ask.focus(); ask.setSelectionRange(ask.value.length, ask.value.length);
    return;
  }
  if (S.busy) return;
  showView("chat");
  const label = SEL_LABEL[act];
  addSelBubble(t, label);
  S.msgs.push({ role: "user", content: selPrompt(act, t) });
  const c = S.conv;
  saveMsg(c, "user", label + " · " + t).catch(() => {});
  if (c.msgs.length === 1) { c.title = (label + " · " + t.replace(/\s+/g, " ")).slice(0, 24); $("#title").textContent = c.title; }
  run();
}
listen("sel:act", ({ payload }) => selAct(payload));

/* ───────── 语音（原型的 vcStart / vcRelease / vcDict） ─────────
   三种开始方式：长按小球（按住说话，松开结束）· 「按住说话」快捷键（默认单独按住右 Alt）· 胶囊里的麦克风（听写进输入框）。
   录音用网页的麦克风接口，松开后把整段录音交给语音设置里选的服务商识别（/audio/transcriptions）。
   和原型不同：识别接口不是流式的，说话时气泡里显示「在听…」和时长，松开后显示「识别中…」，没有逐字出现。 */
const VC_LEAD = 300, VC_MIN = 600, VC_CANCEL = 48, VC_HOLD = 350, VC_MAXMS = 120000;
const vcBub = $("#vcBub"), vcRip = $("#vcRip"), vcMic = $("#vcMic");
const vcTxt = vcBub.querySelector(".vc-txt"), vcTime = vcBub.querySelector(".vc-time");
const vcRings = [...vcRip.children];
let vc = null, vcBox = null, vcKeyT = 0, vcAsk0 = "";
const vcCfg = () => (S.cfg && S.cfg.voice) || { lang: "auto", after: "send", tts: false, engine: "system" };
const vcSys = () => vcCfg().engine !== "provider";
// 按住说话之前先看识别能不能用：系统识别要 Windows / macOS；服务商识别要有模型、有 Key。不能用就说清楚去哪改
function vcNotReady() {
  if (vcSys()) return S.cfg && ["windows", "macos"].includes(S.cfg.platform) ? "" : "这个系统没有自带语音识别，请在设置 → 语音里改用服务商识别";
  const ms = (S.cfg && S.cfg.models) || [];
  const v = vcCfg(), m = ms.find((x) => x.id === v.model) || ms.find((x) => x.id === S.cfg.default_model) || ms[0];
  if (!m) return "语音识别选的是服务商，但还没有添加模型：设置 → 模型里添加，或在设置 → 语音里改用系统识别";
  if (!m.no_key && !m.has_key) return "语音识别选的服务商还没填 API Key：设置 → 模型里补上，或在设置 → 语音里改用系统识别";
  return "";
}
const vcFmt = (ms) => { const s = Math.floor(ms / 1000); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; };
// macOS 系统识别读不了 webm，录成 mp4（m4a）
const vcMime = () => (vcSys() && S.cfg.platform === "macos" ? ["audio/mp4", "audio/webm;codecs=opus", "audio/webm"] : ["audio/webm;codecs=opus", "audio/webm", "audio/mp4", "audio/ogg;codecs=opus"]).find((t) => window.MediaRecorder && MediaRecorder.isTypeSupported && MediaRecorder.isTypeSupported(t)) || "";
function vcSay(t, dim) {
  vcTxt.textContent = "";
  if (dim) { const i = document.createElement("i"); i.textContent = t; vcTxt.appendChild(i); } else vcTxt.textContent = t;
}
// 收起时窗口只有小球那么大：录音时扩成「小球 + 涟漪 + 一侧的气泡」，小球留在原地（同悬停小泡、回复气泡的做法）
async function vcOpenBox() {
  if (vcBox) return;
  const p = await winPos(), a = await monitorArea();
  const ORB = SMALL - PAD * 2, E = Math.ceil(ORB / 2 + 62), BW = 16 + 260, H = Math.max(E * 2, 132);
  const cxs = p.x + SMALL / 2, cys = p.y + SMALL / 2, right = cxs < a.x + a.w / 2;
  const w = E * 2 + BW, h = H;
  let x = right ? cxs - E : cxs - E - BW, y = cys - h / 2;
  x = Math.max(a.x, Math.min(x, a.x + a.w - w)); y = Math.max(a.y, Math.min(y, a.y + a.h - h));
  const lx = cxs - x, ly = cys - y; // 小球中心在新窗口里的位置
  vcBox = { p, right, lx, ly, R: ORB / 2 };
  morphing = true;
  try {
    // 气泡和对话卡片用同一种毛玻璃：扩开前截下窗口将要盖住的那块桌面（见 grabBackdrop）
    vcBub.classList.remove("glassy");
    const bd = await grabBackdrop({ x, y, w, h }, a);
    await reshape(w, h, x, y, true, () => {
      body.classList.add("voicing");
      $("#app").style.padding = `${ly - ORB / 2}px 0 0 ${lx - ORB / 2}px`;
      vcBub.style.transformOrigin = right ? "0 50%" : "100% 50%";
      vcBub.style.borderRadius = right ? "4px 16px 16px 16px" : "16px 4px 16px 16px";
      vcBub.style.left = right ? lx + ORB / 2 + 16 + "px" : "auto";
      vcBub.style.right = right ? "auto" : w - lx + ORB / 2 + 16 + "px";
      if (bd) {
        vcBub.style.setProperty("--bd", 'url("' + bd + '")');
        vcBub.style.setProperty("--bd-size", w + "px " + h + "px");
        vcBub.classList.add("glassy");
      }
    });
  } finally { morphing = false; }
}
async function vcCloseBox() {
  const b = vcBox; if (!b) return;
  vcBox = null;
  vcBub.classList.remove("glassy");
  if (S.open) { body.classList.remove("voicing"); return; } // 已经展开：展开时会自己排版
  morphing = true;
  try {
    await reshape(SMALL, SMALL, b.p.x, b.p.y, true, () => { body.classList.remove("voicing"); $("#app").style.padding = ""; });
  } finally { morphing = false; }
}
function vcPlaceBub() {
  if (!vcBox) return;
  const bh = vcBub.offsetHeight;
  vcBub.style.top = Math.max(4, Math.min(innerHeight - bh - 4, vcBox.ly - bh / 2)) + "px";
  vcBub.style.setProperty("--bd-pos", -vcBub.offsetLeft + "px " + -vcBub.offsetTop + "px"); // 底图按窗口截的，挪到气泡自己的位置
}
function vcTick() {
  if (!vc || vc.stopped) return;
  const now = performance.now(), el = now - vc.t0;
  let L = 0;
  if (vc.an) {
    vc.an.getByteTimeDomainData(vc.buf);
    let sum = 0; for (const v of vc.buf) { const d = (v - 128) / 128; sum += d * d; }
    L = Math.min(1, Math.sqrt(sum / vc.buf.length) * 5);
  }
  vc.lv += (L - vc.lv) * 0.35;
  if (vc.mode === "dict") { vcMic.style.setProperty("--vc-l", vc.lv.toFixed(3)); }
  else {
    body.style.setProperty("--vc-l", (0.8 + 0.45 * vc.lv).toFixed(3));
    vcTime.textContent = vcFmt(el);
    if (vcBox) {
      const { lx, ly, R } = vcBox;
      vcRings.forEach((g, i) => {
        const d = R + 8 + i * 13 + vc.lv * (5 + i * 4) + Math.sin(now / 260 - i * 0.9) * 1.5;
        g.style.width = g.style.height = d * 2 + "px";
        g.style.transform = `translate(${lx - d}px,${ly - d}px)`;
        g.style.opacity = ((0.7 - i * 0.2) * (0.35 + 0.65 * vc.lv)).toFixed(3);
      });
      vcPlaceBub();
    }
  }
  if (el > VC_MAXMS) return vc.mode === "dict" ? vcDictStop() : vcRelease(); // 最长 2 分钟
  vc.raf = requestAnimationFrame(vcTick);
}
// 开始录音。mode：press 长按小球 / key 快捷键 / dict 胶囊麦克风
async function vcStart(mode) {
  if (vc || S.busy || morphing) return false;
  const nr = vcNotReady();
  if (nr) { toast(nr); if (!S.open) showPeek(nr, true); mood("error", 2400); return false; }
  const me = { mode, t0: performance.now(), cancel: false, stopped: false, lv: 0, chunks: [], raf: 0 };
  vc = me;
  if (hv.shown || hv.hiding) await hvHide();
  if (peekBox) await hidePeek();
  if (mode === "key" && S.open) await collapse();
  try {
    me.stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true } });
  } catch (e) {
    vc = null;
    const msg = e && e.name === "NotAllowedError" ? "没有麦克风权限。macOS：系统设置 → 隐私与安全性 → 麦克风；Windows：设置 → 隐私 → 麦克风" : e && e.name === "NotFoundError" ? "没找到麦克风" : "打不开麦克风：" + (e && e.message || e);
    toast(msg); if (!S.open) showPeek(msg, true); mood("error", 2400);
    return false;
  }
  if (vc !== me) { me.stream.getTracks().forEach((t) => t.stop()); return false; } // 等权限时已经松手、取消了
  // 系统识别：Windows 从这里开始听（macOS 松开后识别录音文件）
  if (vcSys()) {
    try { await invoke("stt_begin"); } catch (e) {
      me.stream.getTracks().forEach((t) => t.stop());
      if (vc === me) vc = null;
      const msg = String(e); toast(msg); if (!S.open) showPeek(msg, true); mood("error", 2400);
      return false;
    }
    if (vc !== me) { me.stream.getTracks().forEach((t) => t.stop()); invoke("stt_cancel").catch(() => {}); return false; }
  }
  me.mime = vcMime();
  me.rec = new MediaRecorder(me.stream, me.mime ? { mimeType: me.mime } : undefined);
  me.rec.ondataavailable = (e) => { if (e.data && e.data.size) me.chunks.push(e.data); };
  me.rec.start(250);
  try {
    me.ctx = new AudioContext(); me.an = me.ctx.createAnalyser(); me.an.fftSize = 512;
    me.ctx.createMediaStreamSource(me.stream).connect(me.an); me.buf = new Uint8Array(me.an.fftSize);
  } catch (_) { me.an = null; }
  me.t0 = performance.now();
  if (mode === "dict") {
    vcAsk0 = ask.placeholder; ask.placeholder = "在听…再点一下麦克风结束";
    vcMic.classList.add("vc-on"); vcMic.setAttribute("aria-pressed", "true"); vcMic.setAttribute("aria-label", "停止听写");
    ask.focus();
  } else {
    if (!S.open) await vcOpenBox();
    if (vc !== me) return true;
    mood("listen");
    body.classList.add("vc-rec");
    vcBub.classList.remove("vc-x", "vc-out"); vcRip.classList.remove("vc-x");
    vcSay(S.cfg.voice && S.cfg.voice.lang === "en" ? "Listening…" : "在听…", true); vcTime.textContent = "0:00";
    vcBub.classList.add("on"); vcRip.classList.add("on");
  }
  vcTick();
  return true;
}
// 停止录音，返回录到的音频（取消时不要）
function vcStopRec(me) {
  me.stopped = true; cancelAnimationFrame(me.raf);
  body.classList.remove("vc-rec"); body.style.removeProperty("--vc-l"); vcRip.classList.remove("on");
  const done = new Promise((r) => {
    if (!me.rec || me.rec.state === "inactive") return r();
    me.rec.onstop = () => r(); me.rec.stop();
  });
  return done.then(() => {
    me.stream && me.stream.getTracks().forEach((t) => t.stop());
    me.ctx && me.ctx.close().catch(() => {});
    return new Blob(me.chunks, { type: (me.mime || "audio/webm").split(";")[0] });
  });
}
function vcHideBub() {
  vcBub.classList.add("vc-out");
  return new Promise((r) => setTimeout(() => { vcBub.classList.remove("on", "vc-out", "vc-x"); vcRip.classList.remove("vc-x"); r(); }, 220));
}
// 取消：气泡缩小淡出，小球「嗯？」眯一下眼
async function vcCancel(msg) {
  const me = vc; if (!me || me.mode === "dict") return false;
  vc = null;
  vcStopRec(me);
  if (vcSys()) invoke("stt_cancel").catch(() => {});
  body.classList.add("vc-eh"); setTimeout(() => body.classList.remove("vc-eh"), 450);
  mood("idle");
  await vcHideBub(); await vcCloseBox();
  if (msg) { toast(msg); if (!S.open) showPeek(msg, true); }
  return true;
}
function blobB64(b) {
  return new Promise((res, rej) => { const r = new FileReader(); r.onload = () => res(String(r.result).split(",")[1] || ""); r.onerror = rej; r.readAsDataURL(b); });
}
async function vcRecognize(blob) {
  const audio = await blobB64(blob);
  return invoke("transcribe", { audio, mime: blob.type || "audio/webm" });
}
// 松开：录音交给服务商识别，识别完按「松开后」的设置直接发送或放进输入框
async function vcRelease() {
  const me = vc; if (!me || me.mode === "dict" || me.stopped) return;
  if (me.cancel) return vcCancel();
  const el = performance.now() - me.t0;
  if (el < VC_MIN) return vcCancel("说话时间太短");
  const blob = await vcStopRec(me);
  vcSay("识别中…", true); vcPlaceBub();
  mood("think");
  let text = "", err = "";
  try { text = (await vcRecognize(blob)).trim(); } catch (e) { err = String(e); }
  if (vc !== me) return; // 识别途中按了 Esc
  vc = null;
  if (!err && text) { vcSay(text); vcPlaceBub(); await new Promise((r) => setTimeout(r, 450)); }
  await vcHideBub(); await vcCloseBox();
  if (err || !text) { mood(err ? "error" : "idle", 2400); const m = err || "没听清，再说一次"; toast(m); if (!S.open) showPeek(m, true); return; }
  mood("idle");
  await expand("chat");
  if (vcCfg().after === "box") {
    ask.value = text; autosize(); ask.focus(); ask.setSelectionRange(text.length, text.length);
  } else send(text);
}
// 胶囊麦克风：听写进输入框；点第二下停止，回车停止并发送
async function vcDictStop(thenSend) {
  const me = vc; if (!me || me.mode !== "dict" || me.stopped) return;
  const blob = await vcStopRec(me);
  vcMic.classList.remove("vc-on"); vcMic.setAttribute("aria-pressed", "false"); vcMic.setAttribute("aria-label", "语音输入：听写到输入框");
  vcMic.classList.add("vc-busy"); ask.placeholder = "识别中…";
  let text = "", err = "";
  try { if (performance.now() - me.t0 >= VC_MIN) text = (await vcRecognize(blob)).trim(); else if (vcSys()) invoke("stt_cancel").catch(() => {}); } catch (e) { err = String(e); }
  vcMic.classList.remove("vc-busy"); ask.placeholder = vcAsk0 || "有什么可以帮你？";
  if (vc === me) vc = null;
  if (err) { toastErr(err); return; }
  if (text) {
    const base = ask.value.replace(/\s+$/, "");
    ask.value = base + (base && /[A-Za-z0-9]$/.test(base) && /^[A-Za-z0-9]/.test(text) ? " " : "") + text;
    autosize();
  } else if (!thenSend) toast("没听清，再说一次");
  ask.focus(); ask.setSelectionRange(ask.value.length, ask.value.length);
  if (thenSend) send();
}
vcMic.addEventListener("mousedown", (e) => e.preventDefault()); // 不抢输入框焦点
vcMic.addEventListener("click", () => { if (vc && vc.mode === "dict") vcDictStop(); else if (!vc) vcStart("dict"); });
// 听写时回车：先停听写、识别完再发送
ask.addEventListener("keydown", (e) => {
  if (vc && vc.mode === "dict" && !vc.stopped && e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); e.stopImmediatePropagation(); vcDictStop(true); }
}, true);
// Esc：取消录音 / 停止听写（先于其他 Esc 逻辑）
addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || !vc) return;
  e.preventDefault(); e.stopImmediatePropagation();
  if (vc.mode === "dict") vcDictStop(); else vcCancel();
}, true);
// 长按小球：按住 350ms 不动开始说话；拖开 48 像素「松开取消」，拖回来恢复
function vcPressMove(e) {
  if (!vc || vc.mode !== "press" || !press) return;
  const far = Math.hypot(e.screenX - press.x, e.screenY - press.y) >= VC_CANCEL;
  if (far === vc.cancel) return;
  vc.cancel = far;
  vcBub.classList.toggle("vc-x", far); vcRip.classList.toggle("vc-x", far);
}
// 快捷键「按住说话」：按下超过 300ms 才开始（单独按一下右 Alt 不算），松开结束
listen("voice:key", ({ payload: down }) => {
  if (down) {
    if (vc || vcKeyT) return;
    vcKeyT = setTimeout(() => { vcKeyT = 0; vcStart("key"); }, VC_LEAD);
  } else {
    clearTimeout(vcKeyT); vcKeyT = 0;
    if (vc && vc.mode === "key") vcRelease();
  }
});

// 朗读回复：开着时每条新回复读完后用系统语音读出来，回复下面出现「朗读中 · 停止」
const VC_SPK = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M15.5 9a4.2 4.2 0 0 1 0 6M18 6.5a7.6 7.6 0 0 1 0 11"/></svg>';
function vcSpeak(el, text) {
  if (!vcCfg().tts || !window.speechSynthesis || !text) return;
  const plain = text.replace(/```[\s\S]*?```/g, "（代码）").replace(/[#*_`>|]/g, "").replace(/\[([^\]]*)\]\([^)]*\)/g, "$1").trim();
  if (!plain) return;
  speechSynthesis.cancel();
  document.querySelectorAll(".vc-tts:not(.vc-end)").forEach((c) => c._end("已停止"));
  const c = document.createElement("div"), lb = document.createElement("span"), b = document.createElement("button");
  c.className = "vc-tts"; b.type = "button"; b.textContent = "停止"; b.setAttribute("aria-label", "停止朗读");
  const set = (t) => { lb.innerHTML = VC_SPK; lb.append(t); };
  set("朗读中");
  c._end = (t) => { if (c.classList.contains("vc-end")) return; set(t); c.classList.add("vc-end"); };
  b.onclick = () => { speechSynthesis.cancel(); c._end("已停止"); };
  c.append(lb, b);
  el.appendChild(c); scrollEnd();
  const u = new SpeechSynthesisUtterance(plain.slice(0, 4000));
  const lang = vcCfg().lang;
  u.lang = lang === "en" ? "en-US" : lang === "yue" ? "zh-HK" : hasZh(plain) ? "zh-CN" : "en-US";
  u.onend = () => c._end("已读完"); u.onerror = () => c._end("已停止");
  speechSynthesis.speak(u);
}

/* ───────── 启动 ───────── */
(async () => {
  try { S.cfg = await invoke("get_config"); } catch (_) {}
  applyLook();
  updateEmpty();
  mood("idle");
  settle(); // 上次贴着边：重新压平
  if (S.cfg && S.cfg.screen === "main") setTimeout(screenCheck, 300); // 固定在主屏：上次在副屏也挪回来
  // 还没设置模型：小球露出好奇的表情提醒一下，不自动展开，免得挡住屏幕
  if (needsSetup()) mood("curious", 4000);
})();
