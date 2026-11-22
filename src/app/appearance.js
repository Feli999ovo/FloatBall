import { $, appWin, invoke, LogicalSize, mutable, PAD, S } from "./state.js";
import { toastErr } from "./toast.js";

export let darkMQ, msgsEl;

/* ───────── 对话 ───────── */
export function needsSetup() {
  return !!S.cfg && !S.cfg.ready;
}

// 设置是单独的窗口。小球窗口这时会失去焦点而收起，正好不挡着设置窗口。
export function openSettings() {
  invoke("open_settings").catch(toastErr);
}

// 小球大小、眼睛颜色、卡片透明度
export function applyLook() {
  const c = S.cfg,
    root = document.documentElement.style;
  if (!c) return;
  const orbPx = c.orb_size || 72,
    small = orbPx + PAD * 2;
  root.setProperty("--orb", orbPx + "px");
  root.setProperty("--k", String(orbPx / 72));
  const eye = c.eye || "#FFF1DC",
    warm = eye.toUpperCase() === "#FFF1DC";
  root.setProperty("--eye", eye);
  // 眼睛周围的光晕：默认的暖白用样式表里调好的暖色，换了颜色就跟着眼睛的颜色
  if (warm) {
    root.removeProperty("--eye-near");
    root.removeProperty("--eye-glow");
  } else {
    root.setProperty("--eye-near", eye);
    root.setProperty("--eye-glow", eye + "BF");
  }
  root.setProperty("--tint", String(+(0.58 * (1 - Math.min(40, c.glass || 0) / 100)).toFixed(3))); // 原型 .x-glassy 的墨层 0.58，「更通透」按比例减
  // 减少动态效果；跟随系统深浅色（系统为深色时卡片玻璃更深）
  const html = document.documentElement.classList;
  html.toggle("calm", !!c.calm);
  html.toggle("dark", !!c.follow_system && darkMQ.matches);
  if (small !== mutable.SMALL) {
    mutable.SMALL = small;
    // 收起着的时候窗口就是小球那么大，跟着改
    if (!S.open) appWin.setSize(new LogicalSize(mutable.SMALL, mutable.SMALL)).catch(() => {});
  }
}

export function initAppearance() {
  darkMQ = matchMedia("(prefers-color-scheme: dark)");
  darkMQ.addEventListener("change", applyLook);
  msgsEl = $("#msgs");
}
