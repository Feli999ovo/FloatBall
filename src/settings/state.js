export const mutable = {};

export let T, invoke, win, $, jslog, toastT;

export function toast(t) {
  const el = $("toast");
  el.textContent = t;
  el.classList.add("on");
  clearTimeout(toastT);
  toastT = setTimeout(() => el.classList.remove("on"), 1800);
}

export function initState() {
  T = window.__TAURI__;
  ({ invoke } = T.core);
  win = T.window.getCurrentWindow();
  $ = (id) => document.getElementById(id);
  jslog = (m) => invoke("js_log", { msg: "[设置] " + String(m) }).catch(() => {});
  window.addEventListener("error", (e) => jslog(e.message + " @" + e.lineno));
  mutable.cfg = null;
  toastT = 0;
}
