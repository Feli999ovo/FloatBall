import { applyLook } from "./appearance.js";
import { captureAsk, toggleShare } from "./attach.js";
import { newChat, newTmpChat, updateEmpty } from "./conversation.js";
import { hoOpen } from "./handoff.js";
import { hv } from "./hover.js";
import { mood } from "./orb.js";
import { peekBox } from "./peek.js";
import { rtRefresh } from "./routines.js";
import { $, appWin, invoke, jslog, listen, mutable, orb, PhysicalPosition, S } from "./state.js";
import { renderTasks } from "./tasks.js";
import { collapse, expand, press, setDock, settle, toggle } from "./window.js";

export let inScr, scrBusy;

export async function screenCheck() {
  const mode = S.cfg && S.cfg.screen;
  if (
    !mode ||
    mode === "last" ||
    scrBusy ||
    S.open ||
    mutable.morphing ||
    hv.shown ||
    peekBox ||
    press ||
    orb.classList.contains("dragging") ||
    orb.classList.contains("peel")
  )
    return;
  scrBusy = true;
  try {
    const sc = await invoke("screens");
    if (!sc || !sc.list || sc.list.length < 2) return;
    const p = await appWin.outerPosition(),
      z = await appWin.outerSize();
    const cx = p.x + z.width / 2,
      cy = p.y + z.height / 2;
    const from = sc.list.find((m) => inScr(m, cx, cy)) || sc.list.find((m) => m.primary) || sc.list[0];
    const to =
      mode === "main"
        ? sc.list.find((m) => m.primary)
        : sc.cursor && sc.list.find((m) => inScr(m, sc.cursor[0], sc.cursor[1]));
    if (!to || (to.x === from.x && to.y === from.y)) return;
    // 在原屏的相对位置 → 新屏；窗口在新屏上的物理尺寸按新屏的缩放算
    const fx = (cx - from.x) / from.w,
      fy = (cy - from.y) / from.h,
      half = (mutable.SMALL * to.scale) / 2;
    const tx = Math.round(Math.max(to.x, Math.min(to.x + fx * to.w - half, to.x + to.w - half * 2)));
    const ty = Math.round(Math.max(to.y, Math.min(to.y + fy * to.h - half, to.y + to.h - half * 2)));
    mutable.morphing = true;
    orb.classList.remove("peel");
    setDock(0);
    try {
      const slide = S.cfg.screen_slide && !S.cfg.calm && !matchMedia("(prefers-reduced-motion: reduce)").matches;
      const n = slide ? 8 : 1;
      for (let i = 1; i <= n; i++) {
        const k = n === 1 ? 1 : 1 - (1 - i / n) ** 3; // 缓出
        await appWin.setPosition(
          new PhysicalPosition(Math.round(p.x + (tx - p.x) * k), Math.round(p.y + (ty - p.y) * k)),
        );
        if (i < n) await new Promise((r) => setTimeout(r, 30));
      }
    } finally {
      mutable.morphing = false;
    }
    await settle(); // 新屏上照常吸边/贴边，并记住位置
  } catch (e) {
    jslog("换屏失败：" + (e && e.message ? e.message : e));
  } finally {
    scrBusy = false;
  }
}

export function initMonitors() {
  /* ───────── 菜单、快捷键 ───────── */
  // 设置窗口里改了东西
  // 设置窗口里「测试连接」成功时，小球开心一下（原型里的行为）
  listen("ball-state", ({ payload }) => mood(String(payload), 1800));
  /* ───────── 多显示器（原型的 mdsSet）─────────
     mouse：小球跟着鼠标去它所在的屏幕；main：固定在主屏；last：待在上次的位置（拖到哪就是哪）。
     换屏时按小球在原屏幕上的相对位置放到新屏幕，开了「滑过去」就几帧滑过去，减少动态时直接跳。全程用物理像素，免得两块屏缩放不同时算错。 */
  inScr = (m, x, y) => x >= m.x && x < m.x + m.w && y >= m.y && y < m.y + m.h;
  scrBusy = false;
  setInterval(() => {
    if (S.cfg && S.cfg.screen === "mouse") screenCheck();
  }, 700);
  listen("config", async () => {
    const was = S.cfg ? S.cfg.auto_dock + "|" + S.cfg.orb_size : "";
    try {
      S.cfg = await invoke("get_config");
    } catch (_) {
      return;
    }
    applyLook();
    updateEmpty();
    if (!$("#vTask").hidden) rtRefresh().then(renderTasks);
    // 改了贴边开关或小球大小：重新贴一次
    if (was !== S.cfg.auto_dock + "|" + S.cfg.orb_size && !S.open) setTimeout(settle, 60);
    if (S.cfg.screen === "main") setTimeout(() => screenCheck(), 120);
  });
  listen("menu", async ({ payload }) => {
    switch (payload) {
      case "toggle":
        toggle();
        break;
      case "open":
        expand();
        break;
      case "new_chat":
        newChat();
        expand("chat");
        break;
      case "new_tmp":
        newTmpChat();
        break;
      case "capture":
        captureAsk();
        break;
      case "capture_window":
        captureAsk("capture_window");
        break;
      case "share":
        await toggleShare();
        break;
      case "history":
        expand("hist");
        break;
      case "tasks":
        expand("task");
        break;
      case "handoff":
        await expand("chat");
        hoOpen();
        break;
      case "routines":
        await expand("task");
        $("#routineHead").scrollIntoView({ block: "start" });
        break;
      case "sleep": {
        const wake = orb.classList.contains("s-sleep");
        await collapse();
        S.lastActive = Date.now();
        mood(wake ? "idle" : "sleep");
        break;
      }
    }
  });
}
