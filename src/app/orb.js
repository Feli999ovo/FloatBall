import { ask, orb, S } from "./state.js";

export let doneTimer;

export function mood(name, ms) {
  orb.className = orb.className.replace(/\bs-\w+/g, "").trim() + " s-" + name;
  clearTimeout(doneTimer);
  if (ms)
    doneTimer = setTimeout(() => mood(S.open ? (document.activeElement === ask ? "listen" : "idle") : "idle"), ms);
}

export function poke() {
  orb.classList.remove("poke");
  void orb.offsetWidth;
  orb.classList.add("poke");
}

export function initOrb() {
  /* ───────── 小球表情 ───────── */
  doneTimer = 0;
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
  orb.addEventListener("mouseenter", () => {
    if (!S.open && !S.busy && /s-(idle|sleep)/.test(orb.className)) mood("curious");
  });
  orb.addEventListener("mouseleave", () => {
    if (!S.open && orb.className.includes("s-curious")) mood("idle");
  });
  // 没动静一段时间就睡觉（设置 → 外观「空闲多久后睡觉」，0 = 从不）
  setInterval(() => {
    const ms = S.cfg ? S.cfg.idle_ms : 600000;
    if (ms && !S.open && !S.busy && !orb.className.includes("s-sleep") && Date.now() - S.lastActive > ms) mood("sleep");
  }, 5000);
}
