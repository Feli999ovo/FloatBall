import { scrollEnd } from "./conversation.js";
import { mood } from "./orb.js";
import { hidePeek } from "./peek.js";
import { agentModel } from "./receipt.js";
import { ask, invoke, S } from "./state.js";
import { renderTasks } from "./tasks.js";
import { agList } from "./team.js";

export let TOOL_ICO, ICO_PATH, icoSvg, fmtTok, wan;

export function leadModelName() {
  const m = agentModel(agList()[0]);
  return m ? m.name || m.model : "";
}

export function toolPill(el, ev) {
  let w = el.querySelector(`.x-tw[data-call="${ev.call}"]`);
  if (!w) {
    w = document.createElement("div");
    w.className = "x-tw";
    w.dataset.call = ev.call;
    w.innerHTML =
      '<button type="button" class="x-tool" aria-expanded="false"><span aria-hidden="true"></span><span class="x-tl"></span><span class="x-td" aria-hidden="true"><i></i><i></i><i></i></span><span class="x-tc" aria-hidden="true">✓</span><span class="x-tp">' +
      icoSvg("pause", "x-ico--in") +
      '等你确认</span></button><p class="x-tres" hidden><b>结果</b><span></span></p>';
    const b = w.firstChild,
      r = w.lastChild;
    b.addEventListener("click", () => {
      if (!/\b(done|err)\b/.test(w.className)) return;
      r.hidden = !r.hidden;
      b.setAttribute("aria-expanded", !r.hidden);
    });
    el.insertBefore(w, el.querySelector(".meta"));
  }
  const lead = ((S.cfg && S.cfg.agents) || [])[0];
  const who = lead && ev.agent && ev.agent !== lead.name ? ev.agent + " · " : "";
  w.firstChild.firstChild.innerHTML = icoSvg(ev.name.startsWith("交给") ? "upload" : TOOL_ICO[ev.name] || "pages");
  const preAuthorized = ev.result === "已预授权" || ev.result?.startsWith("已预授权 · ");
  w.querySelector(".x-tl").textContent =
    who + ev.name + (ev.arg ? " · " + ev.arg : "") + (preAuthorized ? " · 已预授权" : "");
  w.className = "x-tw" + ({ wait: " wait", done: " done", no: " no", err: " err" }[ev.status] || "");
  if (ev.status === "no") w.querySelector(".x-tp").textContent = "✕ 已拒绝";
  if (ev.status === "err") w.querySelector(".x-tc").textContent = "✕";
  w.lastChild.querySelector("span").textContent = ev.result || "";
  w.lastChild.querySelector("b").textContent = ev.status === "err" ? "出错" : "结果";
  if (w.memoryUndo || (ev.status === "done" && ev.memory_id != null && ev.agent_id)) memoryUndo(w, ev);
}

// showConv 会连同药丸 DOM 一起保留此状态；切换会话、后续工具更新都不会重新启用已撤销的按钮。
export function memoryUndo(w, ev) {
  w.classList.add("memory");
  if (w.memoryUndo) {
    w.memoryUndo.render();
    return;
  }
  const b = document.createElement("button"),
    note = document.createElement("p");
  b.type = "button";
  b.className = "x-sb x-memundo";
  note.className = "x-apn x-memnote";
  note.hidden = true;
  note.setAttribute("role", "status");
  w.insertBefore(b, w.lastChild);
  w.insertBefore(note, w.lastChild);
  const state = (w.memoryUndo = {
    pending: false,
    done: false,
    failed: false,
    render: () => {
      b.disabled = state.pending || state.done;
      b.hidden = state.done;
      b.setAttribute("aria-busy", state.pending);
      b.textContent = state.pending ? "正在撤销…" : state.failed ? "重试撤销" : "撤销";
      if (state.done) {
        w.querySelector(".x-tl").textContent = "已撤销";
        w.querySelector(".x-tres span").textContent = "已撤销";
      }
    },
  });
  w.querySelector(".x-tl").setAttribute("aria-live", "polite");
  b.onclick = async () => {
    if (state.pending || state.done) return;
    state.pending = true;
    note.hidden = true;
    state.render();
    try {
      await invoke("forget_memory", { agent: ev.agent_id, id: ev.memory_id });
      state.done = true;
    } catch (e) {
      state.failed = true;
      note.textContent = "撤销失败：" + e;
      note.hidden = false;
    } finally {
      state.pending = false;
      state.render();
    }
  };
  state.render();
}

export function budgetCard(el, ev, task, onDecision) {
  task.why = "预算";
  renderTasks();
  const tok = ev.kind === "tokens";
  const c = document.createElement("div");
  c.className = "x-ap";
  c.setAttribute("role", "group");
  c.setAttribute("aria-label", "预算已用完");
  c.innerHTML = `<p class="x-aph"><span class="x-risk">预算</span>${tok ? "单个任务上限" : "最多步数"}</p><p class="x-apd"></p><div class="x-apb"><button type="button" class="x-sb x-pri"></button><button type="button" class="x-sb">停止</button></div>`;
  c.querySelector(".x-apd").textContent = tok
    ? `已用 ${(ev.used / 1e4).toFixed(1)}万 / ${wan(ev.limit)} tokens，已暂停`
    : `已走 ${ev.used} / ${ev.limit} 步，已暂停`;
  const [more, stop] = c.querySelectorAll("button");
  more.textContent = tok ? `再给 ${wan(ev.more)}` : `再给 ${ev.more} 步`;
  el.insertBefore(c, el.querySelector(".meta"));
  // 计划卡里正在做的那几步显示「已暂停」
  const paused = [...el.querySelectorAll('.x-ps[data-st="run"]')];
  paused.forEach((r) => {
    r.dataset.st = "pause";
    r.querySelector(".x-pst").innerHTML = icoSvg("pause", "x-ico--in") + "已暂停";
  });
  let done = false;
  const go = (b, allow) => {
    if (done) return;
    done = true;
    onDecision();
    hidePeek();
    task.why = "";
    renderTasks();
    c.querySelectorAll("button").forEach((x) => (x.disabled = true));
    b.classList.add("on");
    const n = document.createElement("p");
    n.className = "x-apn";
    c.appendChild(n);
    n.textContent = allow
      ? tok
        ? `上限加到 ${wan(ev.used + ev.more)} tokens，继续`
        : `最多步数加到 ${ev.used + ev.more} 步，继续`
      : "已停止";
    paused.forEach((r) => {
      if (r.dataset.st !== "pause") return;
      r.dataset.st = allow ? "run" : "stop";
      r.querySelector(".x-pst").textContent = allow ? "进行中" : "已停止";
    });
    invoke("answer_ask", { ask: ev.ask, answer: { allow, always: false, reason: "", edit: "" } })
      .then((ok) => {
        if (!ok) n.textContent = "这个任务已经结束了";
      })
      .catch(() => {});
    mood(allow ? "work" : "think");
  };
  more.addEventListener("click", () => go(more, true));
  stop.addEventListener("click", () => go(stop, false));
  if (task.conv === S.conv) more.focus({ preventScroll: true });
  return c;
}

export function askCard(el, ev, onDecision) {
  const w = el.querySelector(`.x-tw[data-call="${ev.call}"]`);
  const c = document.createElement("div");
  c.className = "x-ap" + (ev.hi ? " hi" : "");
  c.setAttribute("role", "group");
  c.setAttribute("aria-label", "需要你确认");
  c.innerHTML =
    '<p class="x-aph"><span class="x-risk"></span>需要你确认</p><p class="x-apd"></p>' +
    '<div class="x-ape" hidden><input class="x-in" spellcheck="false" aria-label="修改操作内容" /><button type="button" class="x-sb x-pri">确认</button></div>' +
    '<div class="x-apb x-ap0"><button type="button" class="x-sb x-pri">允许一次</button><button type="button" class="x-sb">总是允许</button>' +
    '<button type="button" class="x-sb">拒绝</button><button type="button" class="x-sb x-apx" aria-label="更多选项" aria-expanded="false">…</button></div>' +
    '<div class="x-apb x-apm" hidden><button type="button" class="x-sb">修改后允许</button><button type="button" class="x-sb">拒绝并说明</button></div>' +
    '<div class="x-apr x-ape" hidden><input class="x-in" placeholder="原因（会告诉 Agent）" aria-label="原因（会告诉 Agent）" /><button type="button" class="x-sb x-pri">发送</button></div>';
  const apd = c.querySelector(".x-apd");
  c.querySelector(".x-risk").textContent = ev.hi ? "高风险" : "中风险";
  apd.textContent = ev.desc;
  if (w && w.nextSibling) el.insertBefore(c, w.nextSibling);
  else el.insertBefore(c, el.querySelector(".meta"));
  const row0 = c.querySelector(".x-ap0"),
    more = c.querySelector(".x-apm"),
    [edBox, rsBox] = c.querySelectorAll(".x-ape");
  const edIn = edBox.querySelector("input"),
    rsIn = rsBox.querySelector("input");
  const note = (t) => {
    const n = document.createElement("p");
    n.className = "x-apn";
    n.textContent = t;
    c.appendChild(n);
  };
  let done = false;
  const fin = (b, answer) => {
    if (done) return;
    done = true;
    onDecision(answer.allow);
    c.querySelectorAll("button,input").forEach((x) => (x.disabled = true));
    b.classList.add("on");
    invoke("answer_ask", { ask: ev.ask, answer: { allow: false, always: false, reason: "", edit: "", ...answer } })
      .then((ok) => {
        if (!ok) note("这一步已经结束了");
      })
      .catch(() => {});
    if (answer.always) note(`以后「${ev.kind} · ${ev.scope}」不再询问`);
    mood(answer.allow ? "work" : "think");
    ask.focus({ preventScroll: true });
  };
  const [bOnce, bAlways, bNo, bMore] = row0.querySelectorAll("button");
  bAlways.hidden = bAlways.disabled = ev.no_always === true;
  bOnce.addEventListener("click", () => fin(bOnce, { allow: true }));
  bAlways.addEventListener("click", () => {
    if (!ev.no_always) fin(bAlways, { allow: true, always: true });
  });
  bNo.addEventListener("click", () => fin(bNo, { allow: false }));
  bMore.addEventListener("click", () => {
    more.hidden = !more.hidden;
    bMore.setAttribute("aria-expanded", !more.hidden);
    if (!more.hidden) more.querySelector("button").focus({ preventScroll: true });
    scrollEnd();
  });
  const [bEdit, bWhy] = more.querySelectorAll("button");
  bEdit.addEventListener("click", () => {
    edIn.value = "";
    edIn.placeholder = ev.kind === "发送" ? "改过的邮件正文" : ev.kind === "写入" ? "改过的文件内容" : "改过的内容";
    apd.hidden = false;
    edBox.hidden = false;
    row0.hidden = more.hidden = rsBox.hidden = true;
    edIn.focus();
  });
  bWhy.addEventListener("click", () => {
    rsBox.hidden = false;
    row0.hidden = more.hidden = true;
    rsIn.focus();
    scrollEnd();
  });
  const edOk = edBox.querySelector("button"),
    rsOk = rsBox.querySelector("button");
  edOk.addEventListener("click", () => {
    const v = edIn.value.trim();
    if (!v) {
      edIn.focus();
      return;
    }
    note("已按修改后的内容执行");
    fin(edOk, { allow: true, edit: v });
  });
  rsOk.addEventListener("click", () => {
    const v = rsIn.value.trim();
    if (!v) {
      rsIn.focus();
      return;
    }
    fin(rsOk, { allow: false, reason: v });
  });
  edIn.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      edOk.click();
    }
  });
  rsIn.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      rsOk.click();
    }
  });
  if (el.isConnected) bOnce.focus({ preventScroll: true });
  return c;
}

// 最近一次提问的文字（任务列表里显示）
export function lastText(conv = S.conv) {
  const c = conv.msgs.at(-1).content;
  const s = typeof c === "string" ? c : (c.find((p) => p.type === "text") || {}).text || "截图提问";
  return s.replace(/^文件「[^」]*」的内容：[\s\S]*?```\n\n/, "").slice(0, 40) || "提问";
}

export function initApproval() {
  // ───────── 工具药丸、确认卡片（照搬原型的 toolChip / askAppr） ─────────
  TOOL_ICO = {
    搜索网页: "search",
    读取网页: "window",
    读取文件: "doc",
    查看文件夹: "pages",
    写入文件: "write",
    删除文件: "trash",
    发送邮件: "mail",
    打开网页: "window",
  };
  ICO_PATH = {
    doc: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/><path d="M9 13h6M9 16.5h4"/>',
    search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m20 20-4.9-4.9"/>',
    trash:
      '<path d="M4 7h16M9.5 7V4.5h5V7"/><path d="M6.5 7l.9 12.2a2 2 0 0 0 2 1.8h5.2a2 2 0 0 0 2-1.8L17.5 7"/><path d="M10 11v6M14 11v6"/>',
    mail: '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="m3.6 6.6 8.4 6.4 8.4-6.4"/>',
    write: '<path d="M15 4.5a2.1 2.1 0 0 1 3 3L8.5 17l-4 1 1-4z"/><path d="M13 20h7M16.5 16h3.5"/>',
    pages:
      '<path d="M8 7V5a2 2 0 0 1 2-2h6l4 4v10a2 2 0 0 1-2 2h-2"/><rect x="4" y="7" width="12" height="14" rx="2"/><path d="M7.5 12h5M7.5 15.5h3"/>',
    window:
      '<rect x="3" y="4.5" width="18" height="15" rx="2"/><path d="M3 9h18"/><circle cx="6" cy="6.8" r=".6" fill="currentColor"/><circle cx="8.6" cy="6.8" r=".6" fill="currentColor"/>',
    pause: '<rect x="7" y="5.5" width="3" height="13" rx="1"/><rect x="14" y="5.5" width="3" height="13" rx="1"/>',
    upload: '<path d="M12 15V4M7.5 8.5 12 4l4.5 4.5"/><path d="M5 14v4a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4"/>',
    play: '<path d="M8 5.5v13l10.5-6.5z"/>',
  };
  icoSvg = (n, cls) =>
    `<svg class="x-ico${cls ? " " + cls : ""}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICO_PATH[n] || ""}</svg>`;
  fmtTok = (n) => (n < 1000 ? String(n) : (n / 1000).toFixed(1) + "K");
  // 预算用满（照原型 capCard / capGo）：正在做的步骤停住，对话里一张小卡「再给 2万 / 停止」
  wan = (n) => Math.round(n / 1000) / 10 + "万";
}
