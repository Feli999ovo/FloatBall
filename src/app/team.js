import { agSync } from "./agents.js";
import { fmtTok } from "./approval.js";
import { S } from "./state.js";

export let agList, agOf, avHTML;

export function agentModelName(a) {
  const ms = (S.cfg && S.cfg.models) || [],
    mn = (m) => m.name || m.model;
  const m = ms.find((x) => x.id === a.model),
    d = ms.find((x) => x.id === S.cfg.default_model);
  return m ? mn(m) : d ? mn(d) + "（默认）" : "未设置";
}

export function fmtCny(n) {
  if (!Number.isFinite(Number(n))) return "";
  const v = Number(n);
  return "¥" + (v < 0.01 ? v.toFixed(4) : v.toFixed(2));
}

export function handoffRow(el, ev) {
  const ol = el.querySelector(".x-plan ol");
  const target = ol?.querySelector(`.x-ps[data-call="${CSS.escape(ev.to_call)}"]`);
  if (!target) {
    (el._pendingHandoffs ||= []).push(ev);
    return;
  }
  const key = `${ev.from_call}-${ev.to_call}`;
  if (ol.querySelector(`[data-handoff="${CSS.escape(key)}"]`)) return;
  const row = document.createElement("li");
  row.className = "x-ph";
  row.dataset.handoff = key;
  row.textContent = `${ev.from} → ${ev.to}：已交接 ${ev.count || 0} ${ev.unit || "段结果"}${ev.cost != null ? ` · ${fmtCny(ev.cost)}` : ""}`;
  ol.insertBefore(row, target);
}

export function teamRow(el, ev) {
  let c = el.querySelector(".x-plan");
  if (!c) {
    c = document.createElement("div");
    c.className = "x-plan";
    c.setAttribute("role", "group");
    c.innerHTML = '<p class="x-plh"></p><ol></ol><p class="x-plf"></p>';
    el.insertBefore(c, el.querySelector(".meta"));
  }
  const ol = c.children[1];
  let r = ol.querySelector(`.x-ps[data-call="${ev.call}"]`);
  const a = agOf(ev.agent_id) || { id: ev.agent_id, name: ev.agent, tint: "#E6D49A" };
  if (!r) {
    r = document.createElement("li");
    r.className = "x-ps";
    r.dataset.call = ev.call;
    r.dataset.agent = ev.agent;
    r.innerHTML =
      avHTML(a) +
      '<span class="x-psn"><b></b><span class="x-pm"></span></span><span class="x-pss"></span><span class="x-ptk"></span><span class="x-pst"></span>';
    r.querySelector("b").textContent = ev.agent;
    r.querySelector(".x-pm").textContent = ev.model || agentModelName(a);
    r.querySelector(".x-pss").textContent = ev.task;
    // 点开一步：一行「为什么交给它」（它的职责和会用的工具）
    const wy = document.createElement("li");
    wy.className = "x-pwhy";
    wy.hidden = true;
    r.tabIndex = 0;
    r.setAttribute("role", "button");
    r.setAttribute("aria-expanded", "false");
    const tog = () => {
      const on = wy.hidden;
      if (on) wy.textContent = whyLine(a);
      wy.hidden = !on;
      r.setAttribute("aria-expanded", on);
    };
    r.addEventListener("click", tog);
    r.addEventListener("keydown", (e) => {
      if (e.target === r && (e.key === "Enter" || e.key === " ")) {
        e.preventDefault();
        tog();
      }
    });
    ol.append(r, wy);
    if (el._pendingHandoffs) {
      const pending = el._pendingHandoffs;
      el._pendingHandoffs = [];
      for (const handoff of pending) handoffRow(el, handoff);
    }
  }
  // 用户在预算卡上点了「停止」：这一步之后报的错就是这个，仍显示「已停止」
  const userStop = ev.status === "err" && r.dataset.st === "stop";
  if (!userStop) {
    r.dataset.st = ev.status === "err" ? "stop" : ev.status;
    r.querySelector(".x-pst").textContent = { run: "进行中", done: "✓", err: "出错" }[ev.status] || "等待";
  }
  r.dataset.tok = ev.tokens || 0;
  if (ev.status === "done") r.dataset.cost = ev.cost == null ? "" : String(ev.cost);
  r.querySelector(".x-ptk").textContent = ev.tokens
    ? fmtTok(ev.tokens) + (ev.cost != null ? ` · ${fmtCny(ev.cost)}` : "")
    : "";
  const rows = [...ol.querySelectorAll(".x-ps")],
    n = rows.length,
    tot = rows.reduce((s, x) => s + +x.dataset.tok, 0);
  const priced = rows.filter((x) => x.dataset.st === "done");
  const complete = rows.length > 0 && rows.every((x) => x.dataset.st === "done");
  c.firstChild.textContent = `主控 · ${n} 步计划 · 点一步看为什么交给它`;
  c.setAttribute("aria-label", `主控的 ${n} 步计划`);
  const hasUnknown = priced.some((x) => x.dataset.cost === "");
  const actualTotal = priced.reduce((sum, item) => sum + Number(item.dataset.cost || 0), 0);
  const totalLabel =
    complete && priced.length && !hasUnknown && Number.isFinite(actualTotal)
      ? ` · 约 ${fmtCny(actualTotal)}${Number(S.cfg?.usd_cny) === 7.2 ? "（默认汇率）" : ""}`
      : "";
  c.lastChild.textContent = `共 ${fmtTok(tot)} tokens${totalLabel}`;
  if (ev.status === "run") S.agBusy[ev.agent_id] = ev.task;
  else delete S.agBusy[ev.agent_id];
  agSync();
}

export function whyLine(a) {
  const tools = (a.tools || []).join("、");
  return (
    `${a.name}的职责是「${a.role || "没写职责"}」` + (tools ? `，会用 ${tools}` : "") + "，所以主控把这一步交给它。"
  );
}

export function initTeam() {
  // ───────── 多 Agent 计划卡（照搬原型 runTeam 的 .x-plan）：主控每交给一个 Agent 一步，卡里多一行 ─────────
  S.agBusy = {};
  // Agent id → 正在做的那一步（任务页 Agents 头像条的状态）
  agList = () => (S.cfg && S.cfg.agents) || [];
  agOf = (id) => agList().find((a) => a.id === id);
  avHTML = (a, big) =>
    `<span class="x-av${big ? " x-avl" : ""}${a.emoji ? " x-ave" : ""}" style="--t:${/^#[0-9a-f]{6}$/i.test(a.tint || "") ? a.tint : "#E6D49A"}" data-agv="${String(a.id || "").replace(/[^\w-]/g, "")}" aria-hidden="true">${a.emoji ? String(a.emoji).replace(/[&<>"]/g, "") : ""}</span>`;
}
