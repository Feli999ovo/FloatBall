import { mk } from "./agent-form.js";
import { radio, savePrefs, sw } from "./appearance.js";
import { $, mutable } from "./state.js";

export function renderVoice() {
  const a = document.activeElement && document.activeElement.id;
  $("vcLang").value = mutable.vcCfg.lang;
  radio("vcAfter", mutable.vcCfg.after);
  const prov = mutable.vcCfg.engine === "provider";
  radio("vcEng", prov ? "provider" : "system");
  $("vcSysNote").textContent = prov
    ? "录音会发给下面选的模型服务商识别。"
    : mutable.cfg.platform === "windows"
      ? "用 Windows 自带的听写，不用配模型。要先打开 Windows 设置 → 隐私和安全性 → 语音里的「联机语音识别」，录音可能经微软云端处理。"
      : mutable.cfg.platform === "macos"
        ? "用 macOS 自带的本机识别，不用配模型，录音不离开这台 Mac。第一次用会弹系统授权；没下载对应语言时去 系统设置 → 键盘 → 听写 下载。"
        : "这个系统没有自带语音识别，请改用模型服务商。";
  document.querySelector(".x-vcsvc").hidden = !prov;
  $("vcSvcNote").hidden = !prov;
  const sel = $("vcModel"),
    ms = mutable.cfg.models || [];
  sel.innerHTML = "";
  const def = ms.find((m) => m.id === mutable.cfg.default_model);
  const o0 = mk("option", "", "默认模型" + (def ? `（${def.name || def.model}）` : ""));
  o0.value = "0";
  sel.appendChild(o0);
  ms.forEach((m) => {
    const o = mk("option", "", m.name || m.model);
    o.value = String(m.id);
    sel.appendChild(o);
  });
  sel.value = String(ms.some((m) => m.id === mutable.vcCfg.model) ? mutable.vcCfg.model : 0);
  sel.disabled = !ms.length;
  if (a !== "vcStt") $("vcStt").value = mutable.vcCfg.stt;
  $("vcTts").setAttribute("aria-checked", !!mutable.vcCfg.tts);
  $("vcAltK").textContent = mutable.cfg.platform === "macos" ? "⌥" : "Alt";
  $("vcSvcNote").textContent = ms.length
    ? "用这个模型的接口地址和 Key，调用它的语音识别接口。DeepSeek 等没有语音识别的服务商用不了。"
    : "先在「模型」里添加一个模型，语音识别用它的接口地址和 Key。";
}

export function initVoice() {
  // 语音（原型的 vcCfg）：识别语言、松开后、识别用的服务商和模型、朗读回复
  mutable.vcCfg = null;
  $("vcLang").addEventListener("change", (e) => {
    mutable.vcCfg.lang = e.target.value;
    savePrefs();
  });
  $("vcEng").addEventListener("click", (e) => {
    const b = e.target.closest("[role=radio]");
    if (!b) return;
    radio("vcEng", b.dataset.v);
    mutable.vcCfg.engine = b.dataset.v;
    renderVoice();
    savePrefs();
  });
  $("vcEng").addEventListener("keydown", (e) => {
    const bs = [...$("vcEng").querySelectorAll("[role=radio]")],
      i = bs.indexOf(document.activeElement);
    const d =
      e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!d || i < 0) return;
    e.preventDefault();
    const n = bs[(i + d + bs.length) % bs.length];
    n.focus();
    n.click();
  });
  $("vcAfter").addEventListener("click", (e) => {
    const b = e.target.closest("[role=radio]");
    if (!b) return;
    radio("vcAfter", b.dataset.v);
    mutable.vcCfg.after = b.dataset.v;
    savePrefs();
  });
  $("vcAfter").addEventListener("keydown", (e) => {
    const bs = [...$("vcAfter").querySelectorAll("[role=radio]")],
      i = bs.indexOf(document.activeElement);
    const d =
      e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!d || i < 0) return;
    e.preventDefault();
    const n = bs[(i + d + bs.length) % bs.length];
    n.focus();
    n.click();
  });
  $("vcModel").addEventListener("change", (e) => {
    mutable.vcCfg.model = +e.target.value;
    savePrefs();
  });
  $("vcStt").addEventListener("change", (e) => {
    const v = e.target.value.trim();
    if (!v) e.target.value = mutable.vcCfg.stt;
    else {
      mutable.vcCfg.stt = v;
      savePrefs();
    }
  });
  $("vcStt").addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      e.target.blur();
    }
  });
  $("vcTts").addEventListener("click", () => {
    mutable.vcCfg.tts = !sw("vcTts");
    $("vcTts").setAttribute("aria-checked", mutable.vcCfg.tts);
    savePrefs();
  });
  $("usdCny").addEventListener("change", savePrefs);
}
