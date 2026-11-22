import { agEdit } from "./agent-editor.js";
import { agentModelName, agents, avHTML, mk } from "./agent-form.js";
import { renderHandoffPrefs } from "./handoff.js";
import { renderSkillLibrary, skills } from "./skills.js";
import { reload } from "./startup.js";
import { $, invoke, mutable, toast } from "./state.js";

export let memoryPrefsPending;

export function renderAgents() {
  renderMemoryPrefs();
  renderHandoffPrefs();
  renderSkillLibrary();
  $("paneAgEd").hidden = true;
  $("paneAgents").hidden = false;
  const ul = $("agList");
  ul.innerHTML = "";
  agents().forEach((a) => {
    const li = mk("li", "x-agc");
    li.innerHTML =
      avHTML(a, true) +
      '<div><b></b><p></p><div class="x-caps"><span class="x-cap"></span><span class="x-cap"></span><span class="x-cap"></span></div></div><button type="button" class="x-sb">编辑</button>';
    li.querySelector("b").textContent = a.name;
    li.querySelector("p").textContent = a.role || "还没写职责";
    const [c1, c2, c3] = li.querySelectorAll(".x-cap");
    c1.textContent = agentModelName(a);
    c2.textContent = `工具 ${a.tools.length}`;
    c3.textContent = `技能 ${skills().filter((s) => a.skills?.includes(s.id)).length}`;
    const b = li.querySelector("button");
    b.dataset.ag = a.id;
    b.setAttribute("aria-label", "编辑 " + a.name);
    b.addEventListener("click", () => agEdit(a));
    ul.appendChild(li);
  });
  $("agTpl").hidden = true;
  $("agAdd").setAttribute("aria-expanded", "false");
}

export function renderMemoryPrefs() {
  const b = $("memoryAsk");
  if (!memoryPrefsPending) b.setAttribute("aria-checked", mutable.cfg.memory_ask === true);
  b.disabled = memoryPrefsPending;
}

export function initAgents() {
  [
    ["bTok", "tokens"],
    ["bStep", "steps"],
    ["bPar", "parallel"],
  ].forEach(([id]) => {
    $(id).addEventListener("change", async () => {
      try {
        await invoke("save_limits", {
          limits: { tokens: +$("bTok").value, steps: +$("bStep").value, parallel: +$("bPar").value },
        });
        await reload();
        toast("已保存");
      } catch (e) {
        toast(String(e));
      }
    });
  });
  // 全部 Agent 共用的记忆确认偏好，放在 Agents 列表旁的「记忆」块。
  memoryPrefsPending = false;
  $("memoryAsk").onclick = async () => {
    if (!mutable.cfg || memoryPrefsPending) return;
    const b = $("memoryAsk"),
      memoryAsk = b.getAttribute("aria-checked") !== "true";
    memoryPrefsPending = true;
    b.setAttribute("aria-checked", memoryAsk);
    b.disabled = true;
    try {
      await invoke("save_memory_prefs", { memoryAsk });
      mutable.cfg.memory_ask = memoryAsk;
      toast("已保存");
    } catch (e) {
      toast(String(e));
    } finally {
      memoryPrefsPending = false;
      renderMemoryPrefs();
    }
  };
}
