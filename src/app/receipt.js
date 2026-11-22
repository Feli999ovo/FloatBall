import { openSettings } from "./appearance.js";
import { fmtTok } from "./approval.js";
import { run } from "./chat.js";
import { addUserBubble, saveMsg } from "./conversation.js";
import { $, invoke, S } from "./state.js";
import { agList, fmtCny } from "./team.js";
import { toast, toastErr } from "./toast.js";

export let changeQ, changeOk, changeNo, changeTarget;

export function setMeta(el, text, opts = {}) {
  const meta = el.querySelector(".meta");
  meta.hidden = false;
  meta.innerHTML = "";
  if (opts.copy) {
    const b = document.createElement("button");
    b.textContent = "复制";
    b.onclick = async () => {
      await navigator.clipboard.writeText(opts.copy);
      b.textContent = "已复制";
      setTimeout(() => (b.textContent = "复制"), 1500);
    };
    meta.appendChild(b);
  }
  if (opts.retry) {
    const b = document.createElement("button");
    b.textContent = "重试";
    b.onclick = opts.retry;
    meta.appendChild(b);
  }
  if (opts.settings) {
    const b = document.createElement("button");
    b.textContent = "去设置";
    b.onclick = openSettings;
    meta.appendChild(b);
  }
  if (text) {
    const s = document.createElement("span");
    s.textContent = text;
    meta.appendChild(s);
  }
}

export function renderReceipt(el, tools, confirmations, task, usage, model, changes) {
  if (!tools.size) return;
  const counts = new Map(),
    reads = new Set(),
    writes = new Set(),
    deletes = new Set(),
    effects = [];
  let read = 0,
    write = 0,
    allowed = 0,
    denied = 0;
  for (const { name, arg, status, path } of tools.values()) {
    counts.set(name, (counts.get(name) || 0) + 1);
    const reading = name === "读取文件" || name === "查看文件夹";
    if (reading) read++;
    if (name === "写入文件") write++;
    if (status !== "done") continue;
    if (path && reading) reads.add(path);
    if (path && name === "写入文件") writes.add(path);
    if (path && name === "删除文件") deletes.add(path);
    if (name === "发送邮件" || name === "打开网页" || name.startsWith("已记住："))
      effects.push([name, arg].filter(Boolean).join(" · "));
  }
  for (const decision of confirmations.values()) {
    if (decision === true) allowed++;
    else if (decision === false) denied++;
  }
  const elapsed = `${Math.round((task.t1 - task.t0) / 1000)} 秒`;
  const cost = Number.isFinite(usage?.cost)
    ? `约 ${fmtCny(usage.cost)}${Number(S.cfg?.usd_cny) === 7.2 ? "（默认汇率）" : ""}`
    : "未知";
  const tokens = (usage?.prompt || 0) + (usage?.completion || 0);
  const summary = document.createElement("span");
  summary.className = "receipt-summary";
  summary.textContent = `${tools.size} 步 · 读 ${read} · 写 ${write} · 确认 ${confirmations.size} · ${elapsed} · ${cost === "未知" ? "花费未知" : cost}`;
  const meta = el.querySelector(".meta"),
    button = document.createElement("button"),
    detail = document.createElement("dl");
  button.type = "button";
  button.textContent = "明细";
  button.setAttribute("aria-expanded", "false");
  detail.className = "receipt";
  detail.hidden = true;
  const rows = [
    ["结果", { done: "完成", stop: "已停止", err: "出错" }[task.st]],
    ["用时", elapsed],
    ["工具", [...counts].map(([name, n]) => `${name} × ${n}`).join("、")],
    ["读过", [...reads].join("\n")],
    ["写过", [...writes].join("\n")],
    ["移到回收站", [...deletes].join("\n")],
    ["确认", `允许 ${allowed} 次 · 拒绝 ${denied} 次`],
    ["其他操作", effects.join("\n")],
    ["改动", changes.map(({ kind, path }) => `${kind} · ${path}`).join("\n")],
    ["模型", model],
    ["用量", tokens ? `${fmtTok(tokens)} tokens` : "未知"],
    ["花费", cost],
  ];
  for (const [title, text] of rows) {
    if (!text) continue;
    const dt = document.createElement("dt"),
      dd = document.createElement("dd");
    dt.textContent = title;
    dd.textContent = text;
    detail.append(dt, dd);
  }
  button.onclick = () => {
    detail.hidden = !detail.hidden;
    button.setAttribute("aria-expanded", !detail.hidden);
    button.textContent = detail.hidden ? "明细" : "收起明细";
    if (!detail.hidden) detail.scrollIntoView({ block: "nearest" });
  };
  meta.append(summary, button);
  meta.after(detail);
}

export function closeChange() {
  changeQ.hidden = true;
  changeTarget = null;
  changeOk.disabled = changeNo.disabled = false;
}

export function agentModel(agent) {
  const c = S.cfg || {};
  return c.models?.find((m) => m.id === agent?.model) || c.models?.find((m) => m.id === c.default_model);
}

export function renderReview(list, changes, conv) {
  const agents = agList(),
    authors = new Set(changes.map(({ agent_id }) => agent_id));
  const reviewers = agents.filter((agent) => !authors.has(agent.id));
  if (!reviewers.length) return;
  const usedModels = new Set(agents.filter((agent) => authors.has(agent.id)).map((agent) => agentModel(agent)?.id));
  const recommended = reviewers.find((agent) => !usedModels.has(agentModel(agent)?.id)) || reviewers[0];
  const button = document.createElement("button"),
    form = document.createElement("form"),
    select = document.createElement("select"),
    start = document.createElement("button");
  button.type = "button";
  button.className = start.className = "x-sb";
  button.textContent = "请别的 Agent 审查";
  button.setAttribute("aria-expanded", "false");
  select.className = "x-in";
  select.setAttribute("aria-label", "审查 Agent");
  for (const agent of reviewers) select.add(new Option(agent.name, agent.id));
  select.value = recommended.id;
  start.type = "submit";
  start.textContent = "开始审查";
  form.hidden = true;
  form.append(select, start);
  button.onclick = () => {
    form.hidden = !form.hidden;
    button.setAttribute("aria-expanded", !form.hidden);
    if (!form.hidden) select.focus();
  };
  form.onsubmit = (e) => {
    e.preventDefault();
    if (conv.busy) {
      toast("请等当前回复结束再开始审查");
      return;
    }
    const agent = reviewers.find(({ id }) => id === select.value);
    if (!agent) return;
    const files = changes.map(
      ({ kind, path, backup }) =>
        `${kind} · 原位置：${path} · 新：${kind === "删除" ? "无" : path} · 旧：${backup || "无"}`,
    );
    const prompt = `请审查这一轮的改动，只读，不要修改。改动：\n${files.join("\n")}\n找出错误、遗漏和风险，按严重程度列出。`;
    const title = `审查 · ${agent.name}`;
    form.hidden = true;
    button.setAttribute("aria-expanded", "false");
    addUserBubble(title, [], conv);
    conv.msgs.push({ role: "user", content: prompt });
    saveMsg(conv, "user", title).catch(() => {});
    run(undefined, conv, true, [], agent);
  };
  list.append(button, form);
}

export function renderChanges(el, changes, conv) {
  if (!changes.length) return;
  const button = document.createElement("button"),
    list = document.createElement("div");
  button.type = "button";
  const total = new Set(changes.map(({ path }) => path)).size;
  button.textContent = `这一轮改了 ${total} 个文件 · 查看`;
  button.setAttribute("aria-expanded", "false");
  list.className = "file-changes";
  list.hidden = true;
  renderReview(list, changes, conv);
  for (const { id, path, kind } of [...changes].reverse()) {
    const row = document.createElement("div"),
      text = document.createElement("span"),
      undo = document.createElement("button");
    const mark = { 新建: "新", 修改: "改", 删除: "删" }[kind];
    text.textContent = `${mark} · ${path}`;
    undo.type = "button";
    undo.className = "x-sb";
    undo.textContent = "撤回";
    undo.onclick = () => {
      changeTarget = { id, button: undo, confirmChanged: false };
      $("#changeQT").textContent = `撤回对这个文件的${kind}？\n${path}`;
      changeOk.textContent = "撤回";
      changeQ.hidden = false;
      changeNo.focus();
    };
    row.append(text, undo);
    list.appendChild(row);
  }
  button.onclick = () => {
    list.hidden = !list.hidden;
    button.setAttribute("aria-expanded", !list.hidden);
    button.textContent = `这一轮改了 ${total} 个文件 · ${list.hidden ? "查看" : "收起"}`;
    if (!list.hidden) list.scrollIntoView({ block: "nearest" });
  };
  el.querySelector(".meta").appendChild(button);
  el.appendChild(list);
}

export function buildContent(text, attach) {
  const files = attach.filter((a) => a.kind === "file");
  const imgs = attach.filter((a) => a.kind === "image");
  let t = text;
  for (const f of files) t = `文件「${f.name}」的内容：\n\`\`\`\n${f.text}\n\`\`\`\n\n` + t;
  if (!imgs.length) return t;
  return [
    { type: "text", text: t || "看看这张截图" },
    ...imgs.map((i) => ({ type: "image_url", image_url: { url: i.url } })),
  ];
}

export function historyText(text, attach) {
  const tags = attach.map((a) => (a.kind === "image" ? "[截图]" : `[文件：${a.name}]`)).join(" ");
  return (tags ? tags + " " : "") + text;
}

export function initReceipt() {
  changeQ = $("#changeQ");
  changeOk = $("#changeOk");
  changeNo = $("#changeNo");
  changeTarget = null;
  changeNo.onclick = () => {
    const button = changeTarget?.button;
    closeChange();
    button?.focus();
  };
  changeQ.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !changeNo.disabled) {
      e.stopPropagation();
      changeNo.click();
    }
  });
  changeOk.onclick = async () => {
    const target = changeTarget;
    if (!target) return;
    changeOk.disabled = changeNo.disabled = true;
    try {
      const undone = await invoke("undo_change", { id: target.id, confirmChanged: target.confirmChanged });
      if (undone) {
        target.button.textContent = "已撤回";
        target.button.disabled = true;
      }
      if (changeTarget !== target) return;
      if (undone) {
        closeChange();
        return;
      }
      target.confirmChanged = true;
      $("#changeQT").textContent = "这个文件之后又被改过，撤回会丢掉之后的改动";
      changeOk.textContent = "仍要撤回";
    } catch (e) {
      if (changeTarget === target) {
        closeChange();
        toastErr(e);
        target.button.focus();
      }
    } finally {
      if (changeTarget === target) {
        changeOk.disabled = changeNo.disabled = false;
        changeNo.focus();
      }
    }
  };
}
