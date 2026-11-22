import { CTX_STEPS, fBase, fModel, fProv, showCaps } from "./providers.js";
import { $, invoke, jslog, mutable } from "./state.js";

export let capT, capSeq;

export function lookupCaps(delay = 250) {
  clearTimeout(capT);
  const model = fModel.value.trim();
  if (!model) {
    mutable.detected = null;
    showCaps();
    return;
  }
  const seq = ++capSeq;
  capT = setTimeout(async () => {
    try {
      const c = await invoke("model_caps", { provider: fProv.value, baseUrl: fBase.value.trim(), model });
      if (seq !== capSeq) return;
      mutable.detected = c;
      showCaps();
    } catch (e) {
      jslog(e);
    }
  }, delay);
}

// 有 models.dev 的 logo 就用；没有就用服务商名字的第一个字代替（不放来源说不清的图）
export function logoOf(id) {
  return id && window.FB_LOGOS && window.FB_LOGOS.has(id) ? id : "";
}

export function setLogo(el, m) {
  const id = logoOf(m.logo);
  if (id) {
    el.style.setProperty("--logo", `url("assets/logos/${id}.svg")`);
    return;
  }
  el.classList.add("x-logo-t");
  el.textContent = ((m.provider || m.name || m.model || "?").trim()[0] || "?").toUpperCase();
}

export function initCapabilities() {
  $("fCapList").addEventListener("click", (e) => {
    const b = e.target.closest("button[data-k]");
    if (!b || !mutable.detected) return;
    const k = b.dataset.k;
    if (k === "ctx") {
      const i = CTX_STEPS.findIndex((v) => v === (mutable.detected.ctx || 0));
      mutable.detected.ctx = CTX_STEPS[(i + 1) % CTX_STEPS.length] || null;
    } else mutable.detected[k] = mutable.detected[k] === true ? false : true;
    mutable.detected.src = "manual";
    showCaps();
    const nb = $("fCapList").querySelector(`[data-k="${k}"]`);
    if (nb) nb.focus({ preventScroll: true });
  });
  // 输入模型名 / 换服务商 / 改地址后查资料（不联网）
  capT = 0;
  capSeq = 0;
}
