import { agBack, agDraw } from "./agent-editor.js";
import { mk } from "./agent-form.js";
import { reload } from "./startup.js";
import { $, invoke, mutable, T, toast } from "./state.js";

export let refreshAgentMemory;

export function memoryControls(agent) {
  const wrap = mk("div"),
    bar = mk("div", "x-mem"),
    summary = mk("span", "", agent ? "正在读取记忆" : "还没有记忆");
  const view = mk("button", "x-sb", "查看"),
    clear = mk("button", "x-sb", "清空");
  const list = mk("ul", "x-memlist");
  view.type = clear.type = "button";
  view.disabled = clear.disabled = true;
  view.setAttribute("aria-expanded", "false");
  list.hidden = true;
  summary.setAttribute("aria-live", "polite");
  bar.append(summary, view, clear);
  wrap.append(bar, list);
  if (!agent) {
    refreshAgentMemory = null;
    return wrap;
  }

  let confirming = false,
    busy = false,
    latest = 0;
  async function refresh() {
    const req = ++latest;
    try {
      const records = await invoke("agent_memory_list", { agent });
      if (!wrap.isConnected || req !== latest) return;
      summary.textContent = records.length ? `独立记忆 · ${records.length} 条` : "还没有记忆";
      view.disabled = clear.disabled = records.length === 0 || busy;
      if (!records.length) list.hidden = true;
      view.setAttribute("aria-expanded", String(!list.hidden));
      view.textContent = list.hidden ? "查看" : "收起";
      list.replaceChildren();
      for (const record of records) {
        const row = mk("li"),
          text = mk("span", "", record.text),
          remove = mk("button", "x-sb", "删除");
        remove.type = "button";
        remove.setAttribute("aria-label", "删除记忆：" + record.text);
        remove.onclick = () => erase(record.id);
        row.append(text, remove);
        list.append(row);
      }
    } catch (err) {
      if (wrap.isConnected && req === latest) summary.textContent = "无法读取记忆：" + err;
    }
  }
  async function erase(id) {
    if (busy) return;
    busy = true;
    clear.disabled = true;
    try {
      await invoke("agent_memory_delete", { agent, id });
    } catch (err) {
      toast(String(err));
    } finally {
      busy = confirming = false;
      clear.textContent = "清空";
      await refresh();
    }
  }
  view.onclick = () => {
    list.hidden = !list.hidden;
    view.setAttribute("aria-expanded", String(!list.hidden));
    view.textContent = list.hidden ? "查看" : "收起";
  };
  clear.onclick = () => {
    if (confirming) return erase(null);
    confirming = true;
    clear.textContent = "确认清空";
  };
  clear.onblur = () => {
    confirming = false;
    clear.textContent = "清空";
  };
  refreshAgentMemory = (id) => {
    if (id === agent && wrap.isConnected) refresh();
  };
  refresh();
  return wrap;
}

export function agRedraw(sel) {
  const w = $("paneAgEd"),
    y = w.scrollTop;
  agDraw();
  w.scrollTop = y;
  const el = sel && $("paneAgEd").querySelector(sel);
  if (el) el.focus({ preventScroll: true });
}

export async function agSave() {
  const { a, d } = mutable.agEd,
    name = d.name.trim() || "未命名",
    old = a && a.name;
  try {
    const id = await invoke("save_agent", {
      agent: {
        id: d.id || "",
        name,
        role: d.role,
        prompt: d.prompt,
        tint: d.tint,
        emoji: d.emoji,
        tools: d.tools,
        skills: d.skills,
        model: d.model || 0,
        fallback: d.fallback || 0,
        auto: !!d.auto,
      },
    });
    await reload();
    agBack(id);
    toast(!a ? `已新建 Agent：${name}` : old !== name ? `已改名：${old} → ${name}` : "已保存");
  } catch (e) {
    toast(String(e));
  }
}

export async function agDelete() {
  const a = mutable.agEd.a;
  if (!a || a.id === "lead") return;
  try {
    await invoke("delete_agent", { id: a.id });
    await reload();
    agBack();
    toast(`已删除 Agent：${a.name}`);
  } catch (e) {
    toast(String(e));
  }
}

export function initMemory() {
  refreshAgentMemory = null;
  T.event.listen("memory:changed", (e) => refreshAgentMemory?.(e.payload));
}
