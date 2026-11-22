import { needOn } from "./agents.js";
import { msgsEl, needsSetup } from "./appearance.js";
import { syncShare } from "./attach.js";
import { moreBtn, moreDd } from "./peek.js";
import { closeChange, historyText } from "./receipt.js";
import { $, ask, invoke, mutable, orb, S } from "./state.js";
import { pendingCards } from "./tasks.js";
import { toast, toastErr } from "./toast.js";
import { showView } from "./views.js";
import { expand, focusAsk } from "./window.js";

export let convs, liveById, ASK_PH, TMP_PH, PLAN_PH, planToggle, askHistory, tmpQ, tmpOk, tmpNo, tmpTarget, unseen;

export function updateEmpty() {
  $("#empty").hidden = msgsEl.children.length > 0;
  // 还没设置模型时，空白页改成提示去设置（不再一启动就强行打开设置页）
  $("#empty").classList.toggle("setup", needsSetup());
  msgsEl.hidden = msgsEl.children.length === 0;
}

export function scrollEnd() {
  msgsEl.scrollTop = msgsEl.scrollHeight;
}

export function Conv(id, title) {
  return {
    id,
    title: title || "新对话",
    msgs: [],
    dom: document.createDocumentFragment(),
    busy: null,
    stop: null,
    task: null,
    tmp: false,
    tmpChanging: false,
    plan: false,
    queue: [],
    history: Promise.resolve(),
    historyEpoch: 0,
  };
}

export function setConvId(c, id) {
  if (c.id != null) liveById.delete(c.id);
  c.id = id;
  if (id != null) liveById.set(id, c);
  if (c === S.conv) S.convId = id;
}

// 每个对话串行写入；转临时成功后，旧队列即使迟到也不能重新创建历史。
export function saveMsg(c, role, content) {
  if (c.tmp) return Promise.resolve();
  const epoch = c.historyEpoch;
  c.history = c.history
    .catch(() => {})
    .then(async () => {
      if (c.tmp || epoch !== c.historyEpoch) return;
      setConvId(c, await invoke("history_append", { convId: c.id, role, content }));
    });
  return c.history;
}

export function syncTmp() {
  const c = S.conv;
  $("#card").classList.toggle("tmp", c.tmp);
  orb.classList.toggle("tmp", c.tmp);
  $("#tmpBanner").hidden = !c.tmp || $("#vChat").hidden;
  const b = moreDd.querySelector("[data-cm=tmp]");
  b.setAttribute("aria-checked", c.tmp);
  b.disabled = c.tmpChanging || (c.busy && !c.tmp);
  $("#planBanner").hidden = !c.plan || $("#vChat").hidden;
  planToggle.textContent = c.plan ? "只规划" : "执行";
  planToggle.setAttribute("aria-pressed", c.plan);
  const ph = c.plan ? PLAN_PH : c.tmp ? TMP_PH : ASK_PH;
  if ([ASK_PH, TMP_PH, PLAN_PH].includes(ask.placeholder) && ask.placeholder !== ph) ask.placeholder = ph;
}

export function closeTmp() {
  tmpQ.hidden = true;
  tmpTarget = null;
  tmpOk.disabled = tmpNo.disabled = false;
}

export function askTmp(on) {
  const c = S.conv;
  if (c.tmpChanging || c.tmp === on) return;
  if (on && c.busy) {
    toast("正在回答，等回答完再转为临时对话");
    return;
  }
  if (!c.msgs.length) {
    setTmp(c, on);
    return;
  }
  tmpTarget = { conv: c, on };
  $("#tmpQT").textContent = on ? "转为临时对话？之前的消息会从历史里删掉。" : "改回普通对话？这段对话会存进历史记录。";
  tmpOk.textContent = on ? "转成临时" : "改回普通";
  tmpQ.hidden = false;
  tmpOk.focus({ preventScroll: true });
}

export async function setTmp(c, on) {
  if (c.tmpChanging || c.tmp === on) return;
  if (on && c.busy) {
    toast("正在回答，等回答完再转为临时对话");
    return;
  }
  c.tmpChanging = true;
  if (c === S.conv) syncTmp();
  try {
    if (on) {
      const change = c.history
        .catch(() => {})
        .then(async () => {
          if (c.id != null) await invoke("history_delete", { convId: c.id });
          setConvId(c, null);
          c.tmp = true;
          c.historyEpoch++;
        });
      c.history = change;
      await change;
    } else {
      c.tmp = false;
      await Promise.all(
        c.msgs.map((m) =>
          saveMsg(
            c,
            m.role,
            typeof m.content === "string"
              ? m.content
              : historyText(
                  m.content[0].text,
                  m.content.slice(1).map(() => ({ kind: "image" })),
                ),
          ),
        ),
      );
    }
    if (c === S.conv) {
      syncTmp();
      toast(on ? "已切换为临时对话 · 不会保存" : "已改回普通对话");
    }
  } catch (e) {
    toastErr((on ? "无法转为临时对话，历史未删除：" : "普通对话保存失败：") + e);
  } finally {
    c.tmpChanging = false;
    if (c === S.conv) syncTmp();
  }
}

// 把当前对话的气泡收起来，换上 c 的
export function showConv(c) {
  const o = S.conv;
  if (o === c) return;
  closeTmp();
  closeChange();
  mutable.historyIndex = -1;
  if (o) {
    o.msgs = S.msgs;
    o.title = $("#title").textContent;
    o.dom.append(...msgsEl.childNodes);
  }
  S.conv = c;
  S.convId = c.id;
  S.msgs = c.msgs;
  S.busy = c.busy;
  S.stopFn = c.stop;
  msgsEl.innerHTML = "";
  msgsEl.append(...c.dom.childNodes);
  $("#title").textContent = c.title;
  $("#stopRow").hidden = !c.busy;
  needOn(pendingCards.size > 0);
  seen(c);
  syncTmp();
  syncShare();
  updateEmpty();
  scrollEnd();
}

export function newChat() {
  showConv(Conv(null));
}

export function newTmpChat() {
  const c = Conv(null);
  c.tmp = true;
  showConv(c);
  expand("chat");
}

export function doneMark(c) {
  unseen.add(c);
  badge();
}

export function seen(c) {
  unseen.delete(c);
  badge();
}

export function badge() {
  const n = unseen.size;
  orb.querySelector(".dn").dataset.n = n;
  if (n) orb.dataset.done = n;
  else delete orb.dataset.done;
}

// 回到某条对话（任务页点一行、点后台任务的小泡）
export function goConv(c, focus = true) {
  showConv(c);
  seen(c);
  if (S.open) showView("chat");
  else expand("chat");
  if (focus) focusAsk();
}

export function addUserBubble(text, attach, conv = S.conv) {
  const el = document.createElement("div");
  el.className = "m user";
  for (const a of attach || []) {
    if (a.kind === "image") {
      const i = document.createElement("img");
      i.src = a.url;
      i.alt = "截图";
      el.appendChild(i);
    }
    if (a.kind === "file") {
      const f = document.createElement("div");
      f.className = "file";
      f.textContent = "文件：" + a.name;
      el.appendChild(f);
    }
  }
  el.appendChild(document.createTextNode(text));
  (conv === S.conv ? msgsEl : conv.dom).appendChild(el);
  if (conv === S.conv) {
    updateEmpty();
    scrollEnd();
  }
  return el;
}

export function addAiBubble(conv = S.conv) {
  const el = document.createElement("div");
  el.className = "m ai";
  el.innerHTML = '<div class="think" hidden></div><div class="txt cursor"></div><div class="meta" hidden></div>';
  (conv === S.conv ? msgsEl : conv.dom).appendChild(el);
  if (conv === S.conv) {
    updateEmpty();
    scrollEnd();
  }
  return el;
}

export function initConversation() {
  /* 后台任务（B3）：每个对话一份气泡 + 消息 + 正在跑的请求。新对话、打开历史只是换一个显示，
     原来那个对话的请求在后台接着跑，回复写进它自己的气泡（收在 conv.dom 里，不在屏幕上）。 */
  convs = new Set();
  // 有请求在跑的对话（跑完就删掉）
  liveById = new Map();
  S.conv = Conv(null);
  ASK_PH = "有什么可以帮你？";
  TMP_PH = "临时对话…";
  PLAN_PH = "只规划：先出方案…";
  planToggle = $("#planToggle");
  planToggle.onclick = () => {
    S.conv.plan = !S.conv.plan;
    syncTmp();
    ask.focus();
  };
  askHistory = [];
  mutable.historyDraft = "";
  try {
    const saved = JSON.parse(localStorage.getItem("fb.askHistory") || "[]");
    if (Array.isArray(saved)) askHistory = saved.filter((text) => typeof text === "string").slice(0, 50);
  } catch {}
  mutable.historyIndex = -1;
  tmpQ = $("#tmpQ");
  tmpOk = $("#tmpOk");
  tmpNo = $("#tmpNo");
  tmpTarget = null;
  // 截屏、附件或听写还原占位文字时，也要保留当前会话的临时标记。
  new MutationObserver(syncTmp).observe(ask, { attributes: true, attributeFilter: ["placeholder"] });
  tmpOk.onclick = async () => {
    const target = tmpTarget;
    if (!target) return;
    tmpOk.disabled = tmpNo.disabled = true;
    await setTmp(target.conv, target.on);
    if (tmpTarget === target) {
      closeTmp();
      moreBtn.focus({ preventScroll: true });
    }
  };
  tmpNo.onclick = () => {
    closeTmp();
    moreBtn.focus({ preventScroll: true });
  };
  tmpQ.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      tmpNo.click();
    }
    if (e.key === "Tab") {
      e.preventDefault();
      (document.activeElement === tmpOk ? tmpNo : tmpOk).focus();
    }
  });
  // 在后台做完、还没看的对话：球角上的数字（照原型），点开那条对话就清掉
  unseen = new Set();
}
