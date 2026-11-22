import { activeShare } from "./attach.js";
import { hv, hvHide } from "./hover.js";
import { mood, poke } from "./orb.js";
import { hidePeek, peekBox } from "./peek.js";
import {
  $,
  appWin,
  ask,
  BIG_W,
  bigH,
  body,
  invoke,
  jslog,
  LogicalPosition,
  LogicalSize,
  mutable,
  OVER,
  orb,
  PAD,
  S,
  T,
} from "./state.js";
import { toastErr } from "./toast.js";
import { showView } from "./views.js";
import { VC_HOLD, vcCancel, vcStart } from "./voice.js";
import { vcPressMove, vcRelease } from "./voice-finish.js";
import { measureCap, measureOrb } from "./window-glass.js";

export let docked, tuckShift, frame, press, moveTimer;

export function setDock(side) {
  docked = side;
  orb.dataset.edge = side < 0 ? "l" : side > 0 ? "r" : "";
  orb.classList.toggle("docked", !!side);
}

export async function peel(out) {
  if (!docked || S.open || mutable.morphing || peekBox || hv.shown) return;
  const p = await winPos();
  mutable.morphing = true;
  orb.classList.toggle("peel", out);
  try {
    await appWin.setPosition(new LogicalPosition(p.x - docked * tuckShift() * (out ? 1 : -1), p.y));
  } finally {
    setTimeout(() => (mutable.morphing = false), 80);
  }
}

export async function monitorArea() {
  // 拿不到显示器信息时（个别 Linux 环境）退回到网页的 screen 对象
  const fallback = () => ({
    x: screen.availLeft || 0,
    y: screen.availTop || 0,
    w: Math.max(screen.availWidth || 0, 1280),
    h: Math.max(screen.availHeight || 0, 720),
  });
  try {
    const m = await T.window.currentMonitor();
    if (!m) return fallback();
    const s = m.scaleFactor || 1;
    const a =
      m.workArea && m.workArea.size && m.workArea.size.width ? m.workArea : { position: m.position, size: m.size };
    return {
      x: a.position.x / s,
      y: a.position.y / s,
      w: a.size.width / s,
      h: a.size.height / s,
      mon: { x: m.position.x, y: m.position.y, s },
    };
  } catch (e) {
    jslog("currentMonitor 失败，改用 screen：" + (e && e.message ? e.message : e));
    return fallback();
  }
}

export async function winPos() {
  const p = await appWin.outerPosition();
  const s = await appWin.scaleFactor();
  return { x: p.x / s, y: p.y / s };
}

// 改窗口的大小和位置。窗口往左或往上长（缩）的时候，有一两帧画的还是旧版面，小球会闪到别处；
// 所以这种情况先把内容藏起来，等新版面排好再显示。
export async function reshape(w, h, x, y, hide, relayout) {
  if (hide) {
    body.classList.add("resizing");
    await frame();
    await frame();
  } // 等两帧：只等一帧时隐藏还没画出来，小球会在新窗口顶上闪一下（真机截图里见过）
  await appWin.setSize(new LogicalSize(w, h));
  await appWin.setPosition(new LogicalPosition(x, y));
  await new Promise((r) => {
    // 等网页拿到新的窗口尺寸，最多 300 毫秒
    const ok = () => Math.abs(innerWidth - w) < 2 && Math.abs(innerHeight - h) < 2;
    if (ok()) return r();
    const done = () => {
      removeEventListener("resize", f);
      clearTimeout(t);
      r();
    };
    const f = () => {
      if (ok()) done();
    };
    const t = setTimeout(done, 300);
    addEventListener("resize", f);
  });
  relayout();
  if (hide) {
    await frame();
    body.classList.remove("resizing");
  }
}

// 卡片的毛玻璃底：网页画不出窗口后面的桌面，所以在展开之前让程序截下窗口将要盖住的那一块、模糊后垫在卡片下面。
// 只有 Windows 有；拿不到（macOS、超时、出错）就用不透明的底。图只在内存里，不保存、不外传。
// shortcut: 底图是展开那一刻截的，之后后面的窗口变了、或者拖动了悬浮球，就退回不透明的底；要实时的话得换成系统原生的毛玻璃。
export function grabBackdrop(r, a) {
  if (!a.mon) return Promise.resolve(null);
  const s = a.mon.s,
    P = (v) => Math.round(v * s);
  const shot = invoke("backdrop", {
    px: P(r.x + r.w / 2),
    py: P(r.y + r.h / 2),
    x: P(r.x) - a.mon.x,
    y: P(r.y) - a.mon.y,
    w: P(r.w),
    h: P(r.h),
  }).catch(() => null);
  return Promise.race([shot, new Promise((res) => setTimeout(() => res(null), 250))]);
}

export async function expand(view) {
  if (hv.shown || hv.hiding) await hvHide();
  if (peekBox) await hidePeek();
  // 贴边的球刚滑出来（或别的短动画）时点下去：等它停稳再展开，不要吞掉这次点击
  for (let i = 0; mutable.morphing && i < 20; i++) await new Promise((r) => setTimeout(r, 25));
  if (mutable.morphing) {
    jslog("展开被动画挡住");
    return;
  }
  if (S.open) {
    showView(view || "chat");
    focusAsk();
    return;
  }
  mutable.morphing = true;
  try {
    S.anchor = await winPos();
    // 贴着边：先换算成完整露出来的位置，收起时再贴回去
    if (docked) {
      if (!orb.classList.contains("peel")) S.anchor.x -= docked * tuckShift();
      S.redock = docked;
      orb.classList.remove("peel");
      setDock(0);
    }
    const a = await monitorArea();
    const cx = S.anchor.x + mutable.SMALL / 2;
    const flipX = cx > a.x + a.w / 2;
    // 卡片放在胶囊上方（照原型）；只有贴着屏幕顶、上面放不下时才翻到下面。两边都放不下就挪窗口
    const h = bigH(),
      above = S.anchor.y + mutable.SMALL - a.y - 4,
      below = a.y + a.h - S.anchor.y - 4;
    const flipY = above >= h || above > below;
    let x = flipX ? S.anchor.x + mutable.SMALL + OVER - BIG_W : S.anchor.x - OVER;
    let y = flipY ? S.anchor.y + mutable.SMALL - h : S.anchor.y;
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
  } finally {
    mutable.morphing = false;
  }
}

export async function collapse() {
  if (!S.open || mutable.morphing) return;
  mutable.morphing = true;
  try {
    S.open = false;
    body.classList.add("collapsed");
    await new Promise((r) => setTimeout(r, 340)); // 等胶囊缩回小球
    const flipped = body.classList.contains("flip-x") || body.classList.contains("flip-y");
    const p = S.anchor || (await winPos());
    await reshape(mutable.SMALL, mutable.SMALL, p.x, p.y, flipped, () =>
      body.classList.remove("flip-x", "flip-y", "big"),
    );
    if (!S.busy) mood("idle");
  } finally {
    mutable.morphing = false;
  }
  if (S.redock) {
    S.redock = 0;
    settle();
  } // 展开前贴着边：缩回后贴回去
}

export function toggle() {
  S.open ? collapse() : expand();
}

export function focusAsk() {
  if (!$("#vChat").hidden) setTimeout(() => ask.focus(), 30);
}

export async function settle() {
  if (mutable.morphing || hv.shown) return;
  const p = await winPos();
  if (S.open) {
    // 只有用户在展开状态下亲手拖动时才更新收起位置；展开时程序自己挪窗口（贴着屏幕边缩回来）不算
    if (!S.userMoved) return;
    S.userMoved = false;
    // 展开时拖动了胶囊上的小球：把收起后的位置也跟着挪
    const flipX = body.classList.contains("flip-x"),
      flipY = body.classList.contains("flip-y");
    const size = await appWin.outerSize(),
      s = await appWin.scaleFactor();
    const w = size.width / s,
      h = size.height / s;
    S.anchor = { x: flipX ? p.x + w - mutable.SMALL - OVER : p.x + OVER, y: flipY ? p.y + h - mutable.SMALL : p.y };
    invoke("save_position", S.anchor).catch(() => {});
    return;
  }
  S.userMoved = false;
  const a = await monitorArea();
  let { x, y } = p;
  const SNAP = 24,
    ORB = mutable.SMALL - PAD * 2;
  // 贴边（原型的 dock）：开了「贴边时自动收起」时，小球的一侧压平、平边贴住屏幕边；没开就只是吸到边上
  let side = 0;
  if (x - a.x < SNAP) {
    side = -1;
    x = S.cfg && S.cfg.auto_dock ? a.x - PAD - ORB * 0.1 : a.x - PAD + 2;
  } else if (a.x + a.w - (x + mutable.SMALL) < SNAP) {
    side = 1;
    x = S.cfg && S.cfg.auto_dock ? a.x + a.w - PAD - ORB * 0.9 : a.x + a.w - mutable.SMALL + PAD - 2;
  }
  setDock(S.cfg && S.cfg.auto_dock ? side : 0);
  y = Math.max(a.y - PAD + 2, Math.min(y, a.y + a.h - mutable.SMALL + PAD - 2));
  if (x !== p.x || y !== p.y) {
    mutable.morphing = true;
    await appWin.setPosition(new LogicalPosition(x, y));
    setTimeout(() => (mutable.morphing = false), 60);
  }
  invoke("save_position", { x, y }).catch(() => {});
  setTimeout(() => measureOrb({ x, y }), 400); // 等吸边动画停下再量身后
}

export function initWindow() {
  /* ───────── 窗口：收起 / 展开 / 拖动 / 贴边 ───────── */
  // 贴边：side -1 左、1 右、0 不贴。贴着时窗口有一截在屏幕外，小球外侧被裁平；指针移上去时滑出来露出整颗球
  docked = 0;
  // 从贴边位置换算出完整露出来的位置（展开、指针移上来时用）
  tuckShift = () => (mutable.SMALL - PAD * 2) * 0.1 + 2;
  orb.addEventListener("pointerenter", () => {
    if (docked && !orb.classList.contains("peel")) peel(true);
  });
  orb.addEventListener("pointerleave", () => {
    if (docked && orb.classList.contains("peel") && !press) peel(false);
  });
  mutable.morphing = false;
  frame = () =>
    new Promise((r) => {
      const t = setTimeout(r, 60);
      requestAnimationFrame(() => {
        clearTimeout(t);
        r();
      });
    });
  // 点住小球：移动超过 4 像素算拖动，否则算点击
  press = null;
  orb.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    press = { x: e.screenX, y: e.screenY, dragged: false, voice: false, lt: 0 };
    try {
      orb.setPointerCapture(e.pointerId);
    } catch (_) {}
    // 长按（收起时）：按住说话
    if (!S.open && !mutable.vc)
      press.lt = setTimeout(() => {
        if (press && !press.dragged) {
          press.voice = true;
          vcStart("press");
        }
      }, VC_HOLD);
  });
  window.addEventListener("pointermove", (e) => {
    if (press && press.voice) return vcPressMove(e);
    if (!press || press.dragged) return;
    if (Math.hypot(e.screenX - press.x, e.screenY - press.y) > 4) {
      press.dragged = true;
      clearTimeout(press.lt);
      S.userMoved = true;
      if (docked) {
        orb.classList.remove("peel");
        setDock(0);
      }
      orb.classList.add("dragging");
      appWin.startDragging().catch(() => {});
    }
  });
  window.addEventListener("pointercancel", () => {
    if (press && press.voice && mutable.vc && mutable.vc.mode === "press") vcCancel();
    if (press) clearTimeout(press.lt);
    press = null;
  });
  window.addEventListener("pointerup", () => {
    if (!press) return;
    const wasDrag = press.dragged,
      wasVoice = press.voice;
    clearTimeout(press.lt);
    press = null;
    if (wasVoice) {
      orb.classList.remove("dragging");
      if (mutable.vc && mutable.vc.mode === "press") vcRelease();
      return;
    }
    orb.classList.remove("dragging");
    if (!wasDrag) {
      poke();
      toggle();
    }
  });
  orb.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      toggle();
    }
  });
  orb.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    invoke("show_context_menu", { sharing: !!activeShare(S.conv) }).catch(toastErr);
  });
  // 拖完停下 350ms 后：贴边吸附 + 记住位置
  moveTimer = 0;
  appWin.onMoved(() => {
    if (mutable.morphing || hv.shown || orb.classList.contains("peel")) return; // 贴边滑出来露脸、弹出小泡不算拖动
    orb.classList.remove("dragging");
    if (S.open) $("#card").classList.remove("glassy"); // 窗口挪了，底图对不上了
    clearTimeout(moveTimer);
    moveTimer = setTimeout(settle, 350);
  });
}
