import { isScreenPermission } from "../error-message.js";
import { activeShare, renderAttach, sharePermission } from "./attach.js";
import { run } from "./chat.js";
import { addUserBubble, askHistory, saveMsg, scrollEnd } from "./conversation.js";
import { autosize } from "./input.js";
import { buildContent, historyText } from "./receipt.js";
import { $, ask, invoke, mutable, S } from "./state.js";
import { toast, toastErr } from "./toast.js";
import { showView } from "./views.js";

export let HO_RE;

export function renderQueue(conv) {
  for (const item of conv.queue) {
    const { meta } = item;
    meta.textContent = item.paused ? "没发出" : "等它说完再发";
    const remove = () => {
      conv.queue.splice(conv.queue.indexOf(item), 1);
      item.el.remove();
    };
    if (item.paused) {
      const retry = document.createElement("button");
      retry.textContent = "重新发送";
      retry.disabled = !!conv.busy;
      retry.onclick = () => {
        if (!conv.busy) {
          remove();
          submitPrompt(conv, item);
        }
      };
      meta.appendChild(retry);
    }
    const cancel = document.createElement("button");
    cancel.textContent = item.paused ? "删除" : "×";
    cancel.setAttribute("aria-label", "删除待发消息");
    cancel.onclick = remove;
    meta.appendChild(cancel);
  }
}

export function submitPrompt(conv, { text, attach, shared, plan, temporary }) {
  const shown = shared ? [...attach, shared] : attach;
  addUserBubble(text, shown, conv);
  conv.msgs.push({ role: "user", content: buildContent(text, attach) });
  saveMsg(conv, "user", historyText(text, shown)).catch(() => {});
  if (!temporary && !conv.tmp && !conv.tmpChanging && text) {
    askHistory.unshift(text);
    askHistory.length = Math.min(askHistory.length, 50);
    mutable.historyIndex = -1;
    try {
      localStorage.setItem("fb.askHistory", JSON.stringify(askHistory));
    } catch {}
  }
  if (conv.msgs.length === 1) {
    conv.title = text.slice(0, 24) || "截图提问";
    if (conv === S.conv) $("#title").textContent = conv.title;
  }
  run(shared ? buildContent(text, shown) : undefined, conv, plan);
}

export async function send(textArg) {
  const text = (textArg ?? ask.value).trim();
  if ((!text && !S.attach.length) || S.sending) return;
  const c = S.conv,
    plan = c.plan,
    temporary = c.tmp || c.tmpChanging;
  if (c.busy && c.queue.length >= 3) {
    toast("最多排队 3 条消息");
    return;
  }
  const share = c.busy ? null : activeShare(c);
  const draft = ask.value;
  let shared;
  if (share) {
    if (S.cfg?.lead_vision !== true) toast("当前模型不支持图片，这次不会截图");
    else {
      S.sending = true;
      try {
        const url = await invoke("capture_shared", { target: share.target });
        if (S.conv !== c || ask.value !== draft || c.busy) return;
        if (!url) toast("当前模型不支持图片，这次不会截图");
        else if (activeShare(c) === share) shared = { kind: "image", url };
      } catch (e) {
        if (S.conv !== c || ask.value !== draft || c.busy) return;
        if (isScreenPermission(e)) {
          await sharePermission();
          return;
        }
        toastErr(e);
      } finally {
        S.sending = false;
      }
    }
  }
  const attach = S.attach.splice(0);
  renderAttach();
  ask.value = "";
  autosize();
  showView("chat");
  mutable.historyIndex = -1;
  if (c.busy) {
    const el = addUserBubble(text, attach, c);
    el.classList.add("queued");
    const meta = document.createElement("div");
    meta.className = "meta";
    el.appendChild(meta);
    c.queue.push({ text, attach, plan, temporary, el, meta, paused: false });
    renderQueue(c);
    scrollEnd();
  } else {
    submitPrompt(c, { text, attach, shared, plan, temporary });
  }
}

export function initSend() {
  HO_RE = /改代码|写代码|重构|脚本|bug|报错/i;
}
