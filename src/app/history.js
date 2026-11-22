import { addAiBubble, addUserBubble, Conv, liveById, newChat, scrollEnd, setConvId, showConv } from "./conversation.js";
import { md } from "./markdown.js";
import { setMeta } from "./receipt.js";
import { $, invoke, S } from "./state.js";
import { toast } from "./toast.js";
import { showView } from "./views.js";

/* ───────── 历史 ───────── */
export function fmtTime(ms) {
  const d = new Date(ms),
    now = new Date();
  if (d.toDateString() === now.toDateString()) return d.toTimeString().slice(0, 5);
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}

export async function loadHistory() {
  const ul = $("#histList");
  ul.innerHTML = "";
  let list = [];
  try {
    list = await invoke("history_list");
  } catch (e) {
    toast(String(e));
  }
  $("#histEmpty").hidden = list.length > 0;
  $("#clearHist").hidden = list.length === 0;
  for (const c of list) {
    const li = document.createElement("li");
    li.tabIndex = 0;
    li.innerHTML =
      '<span class="t"></span><span class="d"></span><button class="x" aria-label="删除"><svg viewBox="0 0 24 24"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></svg></button>';
    li.querySelector(".t").textContent = c.title;
    li.querySelector(".d").textContent = fmtTime(c.updated);
    li.onclick = () => openConv(c);
    li.onkeydown = (e) => {
      if (e.key === "Enter") openConv(c);
    };
    li.querySelector(".x").onclick = async (e) => {
      e.stopPropagation();
      await invoke("history_delete", { convId: c.id });
      const live = liveById.get(c.id);
      if (live) {
        setConvId(live, null);
        if (live === S.conv && !S.busy) newChat();
      }
      loadHistory();
    };
    ul.appendChild(li);
  }
}

export async function openConv(c) {
  let live = liveById.get(c.id);
  if (live) {
    showConv(live);
    showView("chat");
    return;
  }
  const rows = await invoke("history_load", { convId: c.id });
  live = liveById.get(c.id);
  if (live) {
    showConv(live);
    showView("chat");
    return;
  }
  if (!rows.length) return;
  const conv = Conv(null, c.title);
  setConvId(conv, c.id);
  showConv(conv);
  for (const r of rows) {
    if (r.role === "user") {
      addUserBubble(r.content);
      S.msgs.push({ role: "user", content: r.content });
    } else {
      const el = addAiBubble();
      const t = el.querySelector(".txt");
      t.classList.remove("cursor");
      t.innerHTML = md(r.content);
      setMeta(el, "", { copy: r.content });
      S.msgs.push({ role: "assistant", content: r.content });
    }
  }
  showView("chat");
  scrollEnd();
}

export function initHistory() {
  $("#clearHist").onclick = async () => {
    const b = $("#clearHist");
    if (b.dataset.sure !== "1") {
      b.dataset.sure = "1";
      b.textContent = "再点一次确认删除";
      setTimeout(() => {
        b.dataset.sure = "";
        b.textContent = "全部删除";
      }, 3000);
      return;
    }
    b.dataset.sure = "";
    b.textContent = "全部删除";
    await invoke("history_clear");
    for (const c of liveById.values()) setConvId(c, null);
    newChat();
    loadHistory();
  };
}
