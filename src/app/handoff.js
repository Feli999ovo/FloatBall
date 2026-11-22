import { goConv } from "./conversation.js";
import { moreBtn, moreDd, moreOpen, setPin } from "./peek.js";
import { $, invoke, jslog, S } from "./state.js";
import { toast, toastErr } from "./toast.js";
import { showView } from "./views.js";
import { focusAsk } from "./window.js";

export let hoS, hoH, skillButton, hoClose;

/* ───────── 交给终端（B4，照原型 #ho）───────── */
// 交接说明：从这次对话整理（要做什么、讨论过什么、最后的结论、下一步），用户可以先改再交
export function hoMd() {
  const ms = S.msgs.filter((m) => typeof m.content === "string" && m.content.trim());
  const u = ms.filter((m) => m.role === "user"),
    a = ms.filter((m) => m.role === "assistant");
  const cut = (s, n) => {
    s = s.replace(/\s+/g, " ").trim();
    return s.length > n ? s.slice(0, n) + "…" : s;
  };
  const lines = [
    "# 交接说明",
    "",
    "## 要做什么",
    u.length ? cut(u[u.length - 1].content, 600) : "（还没说要做什么）",
    "",
  ];
  if (ms.length > 1) {
    lines.push("## 讨论过什么");
    ms.slice(-10, -1).forEach((m) => lines.push(`- ${m.role === "user" ? "我" : "悬浮球"}：${cut(m.content, 200)}`));
    lines.push("");
  }
  if (a.length) lines.push("## 最后的结论", cut(a[a.length - 1].content, 1200), "");
  lines.push("## 下一步", "在这个项目里接着做上面「要做什么」。动手前先看一遍代码，有不清楚的先问我。");
  return lines.join("\n");
}

export function hoOpt(sel, list) {
  sel.replaceChildren(...list.map(([v, t]) => new Option(t, v)));
}

export async function hoScan() {
  const to = $("#hoTo");
  hoOpt(to, [["", "正在找本机的工具"]]);
  $("#hoToN").textContent = "";
  $("#hoGo").disabled = true;
  try {
    hoS = await invoke("handoff_scan");
  } catch (e) {
    hoS = { tools: [], terms: [] };
    jslog("handoff_scan 失败：" + e);
  }
  const h = hoH();
  hoOpt(to, [
    ...hoS.tools.map((t) => [t.id, t.name + (t.direct ? "" : "（打开后粘贴一次）")]),
    ["custom", "自定义命令"],
  ]);
  to.value =
    hoS.tools.some((t) => t.id === h.tool) || h.tool === "custom" ? h.tool : (hoS.tools[0] || { id: "custom" }).id;
  const tm = $("#hoTm");
  hoOpt(
    tm,
    hoS.terms.map((t) => [t.id, t.name]),
  );
  if (hoS.terms.some((t) => t.id === h.term)) tm.value = h.term;
  hoTool();
}

// 选了哪个工具：没装任何工具时说一声；要粘贴的先说清楚；自定义的出输入框
export function hoTool() {
  const id = $("#hoTo").value,
    t = hoS && hoS.tools.find((x) => x.id === id);
  $("#hoCustom").hidden = id !== "custom";
  $("#hoToN").textContent =
    id === "custom"
      ? hoS && !hoS.tools.length
        ? "本机没找到常见的编程工具（Claude Code、Codex、Gemini CLI 等），可以填一条自己的命令"
        : "在项目里运行这条命令，{file} 换成交接说明文件"
      : t && !t.direct
        ? "终端打开后按 Ctrl+V（Mac 上 ⌘V）再回车"
        : "打开时直接带上交接说明";
  hoReady();
}

export function hoReady() {
  const id = $("#hoTo").value,
    md = $("#hoMd").value;
  // 收起时只显示一句摘要（「要做什么」那一行）；卡片只有 260 高，放不下整段
  $("#hoMdS").textContent = (
    md.split("\n").find((_l, i, a) => a[i - 1] === "## 要做什么") ||
    md.replace(/^#.*$/gm, "").trim().split("\n")[0] ||
    "（空）"
  ).trim();
  $("#hoGo").disabled =
    !id || !$("#hoDir").value || !md.trim() || !$("#hoTm").value || (id === "custom" && !$("#hoCustom").value.trim());
}

export function hoDirs(first) {
  const ds = [...new Set([first, ...hoH().dirs].filter(Boolean))];
  hoOpt($("#hoDir"), ds.length ? ds.map((d) => [d, d]) : [["", "先选一个项目文件夹"]]);
}

export function hoEdit(on) {
  $("#hoMd").hidden = !on;
  $("#hoMdS").hidden = on;
  $("#hoEdit").setAttribute("aria-expanded", on);
  $("#hoEdit").textContent = on ? "收起" : "查看和修改";
}

export function hoOpen() {
  const h = hoH();
  $("#hoMd").value = hoMd();
  $("#hoCustom").value = h.custom || "";
  $("#hoSave").setAttribute("aria-checked", h.save !== false);
  hoDirs();
  hoEdit(false);
  showView("ho");
  hoScan().then(() => $("#hoTo").focus());
}

export function draftSkill(conv, button, question, answer) {
  goConv(conv);
  skillButton = button;
  const request = (
    typeof question === "string" ? question : question.find((x) => x.type === "text")?.text || ""
  ).trim();
  const title = request.replace(/\s+/g, " ").slice(0, 40);
  $("#skillName").value = title || "未命名技能";
  $("#skillWhen").value = title ? "用户提出「" + title + "」这类请求时" : "";
  // 沿用这条回复中的步骤与 Markdown，让用户保存前修改。
  $("#skillSteps").value = answer.trim().slice(0, 6000);
  showView("skill");
  $("#skillName").focus();
}

export function closeSkillDraft() {
  showView("chat");
  skillButton?.focus({ preventScroll: true });
  skillButton = null;
}

export function initHandoff() {
  hoS = null;
  // 扫描结果 {tools, terms}
  hoH = () => (S.cfg && S.cfg.handoff) || { tool: "", term: "", dirs: [], save: true, custom: "" };
  $("#hoTo").onchange = hoTool;
  $("#hoTm").onchange = $("#hoDir").onchange = hoReady;
  $("#hoMd").oninput = $("#hoCustom").oninput = hoReady;
  $("#hoReScan").onclick = hoScan;
  $("#hoEdit").onclick = () => {
    const on = $("#hoMd").hidden;
    hoEdit(on);
    if (on) $("#hoMd").focus();
  };
  $("#hoSave").onclick = () => {
    const b = $("#hoSave");
    b.setAttribute("aria-checked", b.getAttribute("aria-checked") !== "true");
  };
  $("#hoPick").onclick = async () => {
    try {
      const d = await invoke("handoff_pick", { start: $("#hoDir").value || "" });
      if (d) {
        hoDirs(d);
        hoReady();
      }
    } catch (e) {
      toastErr(e);
    }
  };
  skillButton = null;
  $("#skillCancel").onclick = closeSkillDraft;
  $("#vSkill").onsubmit = async (e) => {
    e.preventDefault();
    const save = $("#skillSave"),
      button = skillButton;
    if (save.disabled) return;
    save.disabled = true;
    try {
      const skill = { name: $("#skillName").value, when: $("#skillWhen").value, steps: $("#skillSteps").value };
      await invoke("save_skill", { skill });
      if (skillButton === button && !$("#vSkill").hidden) closeSkillDraft();
      toast("已存为技能");
    } catch (err) {
      toastErr(err);
    } finally {
      save.disabled = false;
    }
  };
  hoClose = () => {
    showView("chat");
    focusAsk();
  };
  $("#hoCancel").onclick = hoClose;
  $("#vHo").addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      hoClose();
    }
  });
  $("#hoGo").onclick = async () => {
    const b = $("#hoGo");
    b.disabled = true;
    try {
      const [file, paste] = await invoke("handoff_go", {
        md: $("#hoMd").value,
        tool: $("#hoTo").value,
        term: $("#hoTm").value,
        dir: $("#hoDir").value,
        save: $("#hoSave").getAttribute("aria-checked") === "true",
        custom: $("#hoCustom").value,
      });
      jslog("交接说明：" + file);
      if (!paste) toast("终端已打开，由它接手");
      else {
        try {
          await navigator.clipboard.writeText(paste);
          toast("已复制，在终端里按 Ctrl+V（Mac 上 ⌘V）再回车");
        } catch (e) {
          jslog("剪贴板写不进去：" + e);
          toast("请在终端里输入：" + paste);
        }
      }
      hoClose();
    } catch (e) {
      toastErr(e);
      b.disabled = false;
    }
  };
  moreDd.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      moreOpen(false);
      moreBtn.focus();
    }
  });
  addEventListener("pointerdown", (e) => {
    if (moreDd.classList.contains("on") && !e.target.closest("#moreDd,#moreBtn")) moreOpen(false);
  });
  $("#pinMark").onclick = () => setPin(false);
}
