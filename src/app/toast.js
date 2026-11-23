import { errorMessage } from "../error-message.js";
import { mood } from "./orb.js";
import { $, invoke } from "./state.js";

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
  toast(errorMessage(e));
  mood("error", 2400);
  const action = {
    permission: ["去设置", () => invoke("open_settings", { agent: null })],
    conflict: ["重新加载", () => window.location.reload()],
  }[e?.kind];
  if (!action) return;
  const el = $("#toast");
  const button = document.createElement("button");
  button.type = "button";
  button.className = "toast-action";
  button.textContent = action[0];
  button.addEventListener("click", async () => {
    try {
      await action[1]();
    } catch (error) {
      toast(errorMessage(error));
    }
  });
  el.append(" ", button);
}

export function initToast() {
  toastTimer = 0;
}
