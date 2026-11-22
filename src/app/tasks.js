import { agRow, agSync, needOn } from "./agents.js";
import { convs, goConv } from "./conversation.js";
import { rtForm } from "./routine-form.js";
import { rtAddBtn, rtRow } from "./routines.js";
import { $, mutable, S } from "./state.js";
import { agList } from "./team.js";
import { showView } from "./views.js";
import { focusAsk } from "./window.js";

export let pendingCards;

export function syncPending() {
  needOn(pendingCards.size > 0);
  renderTasks();
}

export function renderTasks() {
  const badgeCount = S.tasks.filter((t) => t.st === "run").length + pendingCards.size;
  $("#tBadge").hidden = !badgeCount;
  $("#tBadge").textContent = badgeCount;
  if ($("#vTask").hidden) return;
  const box = $("#tasks");
  box.textContent = "";
  const head = (t) => {
    const h = document.createElement("p");
    h.className = "hg";
    h.textContent = t;
    box.appendChild(h);
    return h;
  };
  if (pendingCards.size) {
    head("等你处理").id = "pendingHead";
    for (const { conv, card, desc } of pendingCards.values()) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "tk pending-row";
      row.textContent = `${conv.title} · ${desc}`;
      row.title = row.textContent;
      row.onclick = () => {
        goConv(conv, false);
        card.scrollIntoView({ block: "nearest" });
        card.tabIndex = -1;
        card.focus({ preventScroll: true });
      };
      box.appendChild(row);
    }
  }
  if (agList().length > 1) {
    box.appendChild(agRow());
    agSync();
  }
  const rts = (S.cfg && S.cfg.routines) || [];
  const act = S.tasks.filter((t) => t.st === "run"),
    fin = S.tasks.filter((t) => t.st !== "run");
  if (act.length) {
    head("进行中");
    act.forEach((t) => box.appendChild(taskRow(t)));
  }
  head("定时任务").id = "routineHead";
  rts.forEach((r) => box.appendChild(rtRow(r)));
  if (mutable.rtNew) box.appendChild(rtForm(mutable.rtNew));
  box.appendChild(rtAddBtn());
  const n = document.createElement("p");
  n.className = "rnote";
  n.textContent = "电脑睡眠或关机时不运行，开机后只补跑最近一次";
  box.appendChild(n);
  if (fin.length) {
    head("已完成");
    fin.slice(0, 30).forEach((t) => box.appendChild(taskRow(t)));
  }
  if (mutable.rtFocus) {
    const el = document.getElementById(mutable.rtFocus);
    mutable.rtFocus = "";
    if (el) el.focus({ preventScroll: true });
  }
  // 卡片只有 260 高：展开的详情 / 表单滚到看得见
  const open = box.querySelector(".rd");
  if (open) open.scrollIntoView({ block: "nearest" });
}

export function taskRow(t) {
  {
    const d = document.createElement("div");
    d.className = "tk";
    d.dataset.st = t.st === "run" ? "run" : "done";
    if (t.rt) d.classList.add("fromrt");
    const row = document.createElement("div");
    row.className = "row";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = t.title;
    const when = document.createElement("span");
    when.className = "when";
    const sec = Math.round(((t.t1 || Date.now()) - t.t0) / 1000);
    if (t.st === "done") {
      const ok = document.createElement("span");
      ok.className = "ok";
      ok.textContent = "✓ ";
      when.appendChild(ok);
    }
    if (t.st === "run" && t.why) {
      d.dataset.st = "pause";
      const bd = document.createElement("span");
      bd.className = "bud";
      bd.textContent = "已暂停 · " + t.why;
      when.appendChild(bd);
    } else
      when.appendChild(
        document.createTextNode(
          { run: "回答中", done: "完成", err: "出错", stop: "已停止" }[t.st] + " · " + sec + " 秒",
        ),
      );
    row.append(name, when);
    d.appendChild(row);
    if (t.st === "run") {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = "停止";
      b.onclick = (e) => {
        e.stopPropagation();
        if (t.conv && t.conv.stop) t.conv.stop();
      };
      row.appendChild(b);
      const bar = document.createElement("div");
      bar.className = "bar";
      bar.appendChild(document.createElement("i"));
      d.appendChild(bar);
    }
    d.onclick = () => (t.conv ? goConv(t.conv) : (showView("chat"), focusAsk()));
    return d;
  }
}

export function initTasks() {
  /* ───────── 任务：这次打开以来发出的请求，正在回答的可以停（取自参考版本） ───────── */
  pendingCards = new Map();
  setInterval(() => {
    if (convs.size && !$("#vTask").hidden) renderTasks();
  }, 1000);
}
