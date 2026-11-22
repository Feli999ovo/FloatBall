import { models } from "./providers.js";
import { mutable } from "./state.js";

export let mk, TOOLS, AG_TINTS, AG_TPL, CONCEPTS, reqWord, TRY_RE, agents, modelOf, mName, esc, avHTML;

export function matchRole(role, text) {
  if (!role || !text) return null;
  for (const [rr, qr] of CONCEPTS) {
    const r = role.match(rr),
      q = text.match(qr);
    if (r && q) return { r: r[0], q: q[0], at: r.index };
  }
  for (let i = 0; i < role.length - 1; i++) {
    const w = role.slice(i, i + 2);
    if (/^[\u4e00-\u9fa5A-Za-z]{2}$/.test(w) && text.includes(w)) return { r: w, q: w, at: i };
  }
  return null;
}

export function agentModelName(a) {
  const m = modelOf(a.model),
    d = modelOf(mutable.cfg.default_model);
  return m ? mName(m) : d ? mName(d) + "（默认）" : "未设置";
}

export function initAgentForm() {
  /* ───────── Agents（原型的 renderAgents / agEdit / agDraw / agSave / agDelete） ─────────
     数据是真的：存在配置里（Rust：save_agent / delete_agent / save_limits），主控的指令和模型就是提问时用的。
     原型里的技能库、独立记忆程序里还没有，这两栏显示原型自己的空状态。 */
  mk = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  };
  TOOLS = [
    ["搜索网页", false],
    ["读文件", false],
    ["写文件", false],
    ["删除", true],
    ["发邮件", true],
    ["浏览器", true],
  ];
  AG_TINTS = ["#C9B8F2", "#A9BEDB", "#AECDB6", "#DCC0A8", "#F2B8C6", "#E6D49A"];
  AG_TPL = [
    {
      k: "审查员",
      d: "检查事实、错别字和逻辑漏洞，只读",
      a: {
        name: "审查员",
        role: "检查事实、错别字和逻辑漏洞，只读",
        tint: "#F2B8C6",
        tools: ["读文件"],
        prompt: "你是一名严谨的审查员。逐段检查事实、数字、错别字和逻辑漏洞；只列出问题和修改建议，不直接改原文。",
      },
    },
    {
      k: "翻译",
      d: "中英互译，保留格式和术语",
      a: {
        name: "翻译",
        role: "中英互译，保留原文格式和术语",
        tint: "#A9BEDB",
        tools: ["读文件", "写文件"],
        prompt: "你是一名专业译者。忠实原意，保留原文的格式、数字和专有名词；拿不准的术语在括号里附原文。",
      },
    },
    {
      k: "空白",
      d: "从零写名字、职责和指令",
      a: { name: "", role: "", tint: "#E6D49A", tools: ["读文件"], prompt: "" },
    },
  ];
  CONCEPTS = [
    [/搜索|阅读|资料|调研|研究|查找/, /调研|研究|搜索|搜集|查一?下|资料|评测|竞品|了解/],
    [/文件|表格|整理|归类|归档/, /整理|表格|归类|归档|文件夹|清单|参数/],
    [/起草|润色|写作|撰写|文案|写/, /写|起草|文案|润色|周报|稿/],
    [/审查|检查|校对|事实|错别字|逻辑/, /检查|审查|校对|错别字|核对|事实|挑错|漏洞/],
    [/翻译|英文|外语|中英/, /翻译|英文|英语|日语|译成|translate/i],
    [/拆解|分派|统筹/, /拆|计划|安排|分工/],
  ];
  reqWord = (text) => {
    for (const [, qr] of CONCEPTS) {
      const q = text.match(qr);
      if (q) return q[0];
    }
    return "";
  };
  TRY_RE = {
    搜索网页: /搜索|搜集|调研|查一?下|资料|评测|网上|竞品/,
    读文件: /读|文件|PDF|报告|文档|检查|审查|校对|总结|这份|这篇/i,
    写文件: /写|起草|生成|保存|整理成|改成|译成|翻译/,
    删除: /删除|清理|删掉/,
    发邮件: /邮件|发给|发送/,
    浏览器: /打开网站|浏览器|登录|下单|网页上/,
  };
  agents = () => (mutable.cfg && mutable.cfg.agents) || [];
  modelOf = (id) => models().find((m) => m.id === id);
  mName = (m) => m.name || m.model;
  esc = (t) => String(t).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  avHTML = (a, big) =>
    `<span class="x-av${big ? " x-avl" : ""}${a.emoji ? " x-ave" : ""}" style="--t:${esc(a.tint)}" aria-hidden="true">${a.emoji ? esc(a.emoji) : ""}</span>`;
}
