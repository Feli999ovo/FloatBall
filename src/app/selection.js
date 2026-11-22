import { msgsEl } from "./appearance.js";
import { run } from "./chat.js";
import { saveMsg, scrollEnd, updateEmpty } from "./conversation.js";
import { autosize } from "./input.js";
import { mood } from "./orb.js";
import { $, appWin, ask, invoke, listen, S } from "./state.js";
import { toast, toastErr } from "./toast.js";
import { showView } from "./views.js";
import { expand, focusAsk } from "./window.js";

export let SEL_LABEL, hasZh;

export function selPrompt(act, t) {
  const tr = (S.cfg && S.cfg.sel && S.cfg.sel.tr) || "auto";
  const to = { en: "英文", zh: "中文", ja: "日文" }[tr === "auto" ? (hasZh(t) ? "en" : "zh") : tr];
  const P = {
    explain: "解释下面这段文字在说什么，简洁清楚：",
    tr: `把下面这段文字翻译成${to}，只给译文：`,
    sum: "用一句话总结下面这段文字：",
    rew: "改写下面这段文字，让它更通顺清楚，意思不变，只给改写后的文字：",
    expand: "把下面这段文字扩写成更完整的一段，意思不变：",
    shrink: "把下面这段文字缩写得更短，保留要点：",
  };
  return P[act] + "\n\n" + t;
}

// 用户消息：引用块 + 动作名（原型的 selBubble）
export function addSelBubble(t, label) {
  const el = document.createElement("div");
  el.className = "m user";
  const q = document.createElement("span");
  q.className = "x-sel-q";
  q.textContent = t;
  const a = document.createElement("span");
  a.className = "x-sel-qa";
  a.textContent = label;
  el.append(q, a);
  msgsEl.appendChild(el);
  updateEmpty();
  scrollEnd();
}

export async function selAct({ act, text }) {
  const t = String(text || "").trim();
  if (!t || !SEL_LABEL[act]) return;
  if (act === "read") {
    // 朗读：用系统语音读出来，不开对话
    if (!window.speechSynthesis) return;
    speechSynthesis.cancel();
    const u = new SpeechSynthesisUtterance(t.slice(0, 4000));
    u.lang = hasZh(t) ? "zh-CN" : "en-US";
    mood("work");
    u.onend = u.onerror = () => mood("idle");
    speechSynthesis.speak(u);
    return;
  }
  await expand("chat");
  if (act === "ask") {
    // 问问它：引用放进输入框，不自动发送
    const flat = t.replace(/\s+/g, " ");
    ask.value = "「" + (flat.length > 200 ? flat.slice(0, 200) + "…" : flat) + "」 ";
    autosize();
    ask.focus();
    ask.setSelectionRange(ask.value.length, ask.value.length);
    return;
  }
  if (S.busy) return;
  showView("chat");
  const label = SEL_LABEL[act];
  addSelBubble(t, label);
  S.msgs.push({ role: "user", content: selPrompt(act, t) });
  const c = S.conv;
  saveMsg(c, "user", label + " · " + t).catch(() => {});
  if (c.msgs.length === 1) {
    c.title = (label + " · " + t.replace(/\s+/g, " ")).slice(0, 24);
    $("#title").textContent = c.title;
  }
  run();
}

export function closeFeedPermission() {
  $("#feedPermission").hidden = true;
  focusAsk();
}

export function initSelection() {
  /* ───────── 划词：工具栏上点了某个动作 ───────── */
  SEL_LABEL = {
    ask: "问问它",
    explain: "解释",
    tr: "翻译",
    sum: "总结",
    rew: "改写",
    read: "朗读",
    expand: "扩写",
    shrink: "缩写",
  };
  hasZh = (t) => /[\u3400-\u9fff]/.test(t);
  listen("sel:act", ({ payload }) => selAct(payload));
  $("#feedClose").onclick = closeFeedPermission;
  $("#feedSettings").onclick = () => invoke("sel_open_privacy").then(closeFeedPermission).catch(toastErr);
  listen("sel:feed", async ({ payload }) => {
    const conv = S.conv;
    await expand("chat");
    if (conv !== S.conv) return;
    await appWin.setFocus();
    if (payload.Err === "permission") {
      $("#feedPermission").hidden = false;
      $("#feedSettings").focus();
      return;
    }
    if (!payload.Ok) {
      toast("没读到选中的文字");
      return;
    }
    ask.value = ask.value ? ask.value + "\n" + payload.Ok : payload.Ok;
    autosize();
    ask.focus();
    ask.setSelectionRange(ask.value.length, ask.value.length);
  });
}
