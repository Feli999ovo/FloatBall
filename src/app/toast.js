import { mood } from "./orb.js";
import { $ } from "./state.js";

export let toastTimer;

export function toast(t) {
  const el = $("#toast");
  el.textContent = t;
  el.classList.add("on");
  // 长的提示（比如告诉你去哪设置）多留一会儿，够读完
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove("on"), Math.max(2600, String(t).length * 150));
}

export function toastErr(e) {
  toast(String(e));
  mood("error", 2400);
}

export function initToast() {
  toastTimer = 0;
}
