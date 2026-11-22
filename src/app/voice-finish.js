import { scrollEnd } from "./conversation.js";
import { autosize } from "./input.js";
import { mood } from "./orb.js";
import { showPeek } from "./peek.js";
import { hasZh } from "./selection.js";
import { send } from "./send.js";
import { ask, invoke, listen, mutable, S } from "./state.js";
import { toast, toastErr } from "./toast.js";
import {
  VC_CANCEL,
  VC_LEAD,
  VC_MIN,
  vcAsk0,
  vcBub,
  vcCancel,
  vcCfg,
  vcCloseBox,
  vcHideBub,
  vcMic,
  vcPlaceBub,
  vcRecognize,
  vcRip,
  vcSay,
  vcStart,
  vcStopRec,
  vcSys,
} from "./voice.js";
import { expand, press } from "./window.js";

export let VC_SPK;

// 松开：录音交给服务商识别，识别完按「松开后」的设置直接发送或放进输入框
export async function vcRelease() {
  const me = mutable.vc;
  if (!me || me.mode === "dict" || me.stopped) return;
  if (me.cancel) return vcCancel();
  const el = performance.now() - me.t0;
  if (el < VC_MIN) return vcCancel("说话时间太短");
  const blob = await vcStopRec(me);
  vcSay("识别中…", true);
  vcPlaceBub();
  mood("think");
  let text = "",
    err = "";
  try {
    text = (await vcRecognize(blob)).trim();
  } catch (e) {
    err = String(e);
  }
  if (mutable.vc !== me) return; // 识别途中按了 Esc
  mutable.vc = null;
  if (!err && text) {
    vcSay(text);
    vcPlaceBub();
    await new Promise((r) => setTimeout(r, 450));
  }
  await vcHideBub();
  await vcCloseBox();
  if (err || !text) {
    mood(err ? "error" : "idle", 2400);
    const m = err || "没听清，再说一次";
    toast(m);
    if (!S.open) showPeek(m, true);
    return;
  }
  mood("idle");
  await expand("chat");
  if (vcCfg().after === "box") {
    ask.value = text;
    autosize();
    ask.focus();
    ask.setSelectionRange(text.length, text.length);
  } else send(text);
}

// 胶囊麦克风：听写进输入框；点第二下停止，回车停止并发送
export async function vcDictStop(thenSend) {
  const me = mutable.vc;
  if (!me || me.mode !== "dict" || me.stopped) return;
  const blob = await vcStopRec(me);
  vcMic.classList.remove("vc-on");
  vcMic.setAttribute("aria-pressed", "false");
  vcMic.setAttribute("aria-label", "语音输入：听写到输入框");
  vcMic.classList.add("vc-busy");
  ask.placeholder = "识别中…";
  let text = "",
    err = "";
  try {
    if (performance.now() - me.t0 >= VC_MIN) text = (await vcRecognize(blob)).trim();
    else if (vcSys()) invoke("stt_cancel").catch(() => {});
  } catch (e) {
    err = String(e);
  }
  vcMic.classList.remove("vc-busy");
  ask.placeholder = vcAsk0 || "有什么可以帮你？";
  if (mutable.vc === me) mutable.vc = null;
  if (err) {
    toastErr(err);
    return;
  }
  if (text) {
    const base = ask.value.replace(/\s+$/, "");
    ask.value = base + (base && /[A-Za-z0-9]$/.test(base) && /^[A-Za-z0-9]/.test(text) ? " " : "") + text;
    autosize();
  } else if (!thenSend) toast("没听清，再说一次");
  ask.focus();
  ask.setSelectionRange(ask.value.length, ask.value.length);
  if (thenSend) send();
}

// 长按小球：按住 350ms 不动开始说话；拖开 48 像素「松开取消」，拖回来恢复
export function vcPressMove(e) {
  if (!mutable.vc || mutable.vc.mode !== "press" || !press) return;
  const far = Math.hypot(e.screenX - press.x, e.screenY - press.y) >= VC_CANCEL;
  if (far === mutable.vc.cancel) return;
  mutable.vc.cancel = far;
  vcBub.classList.toggle("vc-x", far);
  vcRip.classList.toggle("vc-x", far);
}

export function vcSpeak(el, text) {
  if (!vcCfg().tts || !window.speechSynthesis || !text) return;
  const plain = text
    .replace(/```[\s\S]*?```/g, "（代码）")
    .replace(/[#*_`>|]/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .trim();
  if (!plain) return;
  speechSynthesis.cancel();
  document.querySelectorAll(".vc-tts:not(.vc-end)").forEach((c) => c._end("已停止"));
  const c = document.createElement("div"),
    lb = document.createElement("span"),
    b = document.createElement("button");
  c.className = "vc-tts";
  b.type = "button";
  b.textContent = "停止";
  b.setAttribute("aria-label", "停止朗读");
  const set = (t) => {
    lb.innerHTML = VC_SPK;
    lb.append(t);
  };
  set("朗读中");
  c._end = (t) => {
    if (c.classList.contains("vc-end")) return;
    set(t);
    c.classList.add("vc-end");
  };
  b.onclick = () => {
    speechSynthesis.cancel();
    c._end("已停止");
  };
  c.append(lb, b);
  el.appendChild(c);
  scrollEnd();
  const u = new SpeechSynthesisUtterance(plain.slice(0, 4000));
  const lang = vcCfg().lang;
  u.lang = lang === "en" ? "en-US" : lang === "yue" ? "zh-HK" : hasZh(plain) ? "zh-CN" : "en-US";
  u.onend = () => c._end("已读完");
  u.onerror = () => c._end("已停止");
  speechSynthesis.speak(u);
}

export function initVoiceFinish() {
  vcMic.addEventListener("mousedown", (e) => e.preventDefault());
  // 不抢输入框焦点
  vcMic.addEventListener("click", () => {
    if (mutable.vc && mutable.vc.mode === "dict") vcDictStop();
    else if (!mutable.vc) vcStart("dict");
  });
  // 听写时回车：先停听写、识别完再发送
  ask.addEventListener(
    "keydown",
    (e) => {
      if (
        mutable.vc &&
        mutable.vc.mode === "dict" &&
        !mutable.vc.stopped &&
        e.key === "Enter" &&
        !e.shiftKey &&
        !e.isComposing
      ) {
        e.preventDefault();
        e.stopImmediatePropagation();
        vcDictStop(true);
      }
    },
    true,
  );
  // Esc：取消录音 / 停止听写（先于其他 Esc 逻辑）
  addEventListener(
    "keydown",
    (e) => {
      if (e.key !== "Escape" || !mutable.vc) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (mutable.vc.mode === "dict") vcDictStop();
      else vcCancel();
    },
    true,
  );
  // 快捷键「按住说话」：按下超过 300ms 才开始（单独按一下右 Alt 不算），松开结束
  listen("voice:key", ({ payload: down }) => {
    if (down) {
      if (mutable.vc || mutable.vcKeyT) return;
      mutable.vcKeyT = setTimeout(() => {
        mutable.vcKeyT = 0;
        vcStart("key");
      }, VC_LEAD);
    } else {
      clearTimeout(mutable.vcKeyT);
      mutable.vcKeyT = 0;
      if (mutable.vc && mutable.vc.mode === "key") vcRelease();
    }
  });
  // 朗读回复：开着时每条新回复读完后用系统语音读出来，回复下面出现「朗读中 · 停止」
  VC_SPK =
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M15.5 9a4.2 4.2 0 0 1 0 6M18 6.5a7.6 7.6 0 0 1 0 11"/></svg>';
}
