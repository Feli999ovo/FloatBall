import { $, invoke, listen, orb, S } from "./state.js";
import { agentModelName, agList, agOf, avHTML } from "./team.js";
import { toastErr } from "./toast.js";

export let agOpen, memoryCounts, memoryRefresh;

export async function loadMemoryCounts() {
  const request = ++memoryRefresh;
  try {
    const counts = await invoke("agent_memory_counts");
    if (request !== memoryRefresh) return;
    memoryCounts = counts;
    agSync();
  } catch (e) {
    console.warn("无法读取记忆条数", e);
  }
}

export function agRow() {
  const w = document.createElement("div");
  w.className = "x-ags";
  w.innerHTML = '<p class="x-hg">Agents</p><div class="x-agl"></div><div class="x-agp" role="status" hidden></div>';
  agList()
    .slice(1)
    .forEach((a) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "x-ag";
      b.dataset.ag = a.id;
      b.innerHTML = `<span class="x-avw">${avHTML(a)}<i class="x-agd"></i></span><span></span>`;
      b.lastChild.textContent = a.name;
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        agOpen = agOpen === a.id ? null : a.id;
        agSync();
        if (agOpen) loadMemoryCounts();
      });
      w.children[1].appendChild(b);
    });
  return w;
}

export function agSync() {
  const w = $("#tasks") && $("#tasks").querySelector(".x-ags");
  if (!w) return;
  w.querySelectorAll(".x-ag").forEach((b) => {
    const a = agOf(b.dataset.ag),
      busy = S.agBusy[b.dataset.ag] != null;
    b.dataset.st = busy ? "busy" : "idle";
    b.setAttribute("aria-expanded", agOpen === b.dataset.ag);
    b.setAttribute("aria-label", `${a ? a.name : ""}，${busy ? "忙碌" : "空闲"}`);
  });
  const pop = w.querySelector(".x-agp"),
    a = agOpen && agOf(agOpen),
    b = a && w.querySelector(`.x-ag[data-ag="${a.id}"]`);
  pop.hidden = !b;
  if (!b) return;
  pop.innerHTML =
    '<b></b><p></p><p class="x-agm"><span></span><button type="button" class="x-agx">更换</button></p><p></p>';
  const [nm, p1, pm, p3] = pop.children;
  nm.textContent = a.name;
  p1.textContent = a.role || "还没写职责";
  pm.firstChild.textContent = "模型 · " + agentModelName(a);
  pm.lastChild.setAttribute("aria-label", `更换${a.name}的模型`);
  pm.lastChild.addEventListener("click", () => {
    agOpen = null;
    agSync();
    invoke("open_settings", { agent: a.id }).catch(toastErr);
  });
  p3.textContent = S.agBusy[a.id] != null ? `忙碌 · ${S.agBusy[a.id]}` : "空闲";
  const memory = document.createElement("p");
  memory.className = "memory-count";
  memory.textContent = `独立记忆 · ${memoryCounts[a.id] ?? 0} 条`;
  pop.appendChild(memory);
  // 靠右的 Agent 弹卡往左让，不被卡片右边挤窄
  pop.style.left = Math.max(4, Math.min(b.offsetLeft, pop.parentElement.clientWidth - pop.offsetWidth - 4)) + "px";
}

// 有确认在等你：球角上的「!」（收起时才显示）
export function needOn(v) {
  S.pend = !!v;
  orb.classList.toggle("need", !!v);
}

export function stop() {
  if (S.busy && S.stopFn) S.stopFn();
}

export function initAgents() {
  // ───────── 任务页顶上的 Agents 头像条 + 弹卡（照搬原型 agRow / pop） ─────────
  agOpen = null;
  memoryCounts = {};
  memoryRefresh = 0;
  listen("memory:changed", loadMemoryCounts);
  document.addEventListener("pointerdown", (e) => {
    if (agOpen && !e.target.closest(".x-ag,.x-agp")) {
      agOpen = null;
      agSync();
    }
  });
  addEventListener(
    "keydown",
    (e) => {
      if (e.key === "Escape" && agOpen) {
        agOpen = null;
        agSync();
        e.stopImmediatePropagation();
      }
    },
    true,
  );
}
