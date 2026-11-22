import { $, mutable } from "./state.js";

export let PROV,
  fProv,
  fBase,
  fKey,
  fModel,
  fList,
  fRes,
  fHint,
  fEye,
  slash,
  fFetch,
  fTest,
  fSave,
  models,
  isDef,
  mark,
  cls,
  SRC,
  CTX_STEPS;

/* ───────── 能力标签 ───────── */
export function ctxLabel(n) {
  if (!n) return "未知";
  const pick = (a, b) => (Math.abs(a - Math.round(a)) < Math.abs(b - Math.round(b)) ? a : b);
  const fmt = (v) => (Math.abs(v - Math.round(v)) < 0.05 ? String(Math.round(v)) : v.toFixed(1));
  if (n >= 1e6) return fmt(pick(n / 1048576, n / 1e6)) + "M";
  if (n >= 1000) return fmt(pick(n / 1024, n / 1000)) + "K";
  return String(n);
}

// 原型：工具调用 / 视觉 / 上下文 / 本地 · 免费；按 J 的要求加了「推理」
export function capsHTML(c, tag = "span") {
  const t = (k, label, v) =>
    `<${tag} ${tag === "button" ? 'type="button" ' : ""}class="${cls(v)}" data-k="${k}">${label} ${mark(v)}</${tag}>`;
  return (
    t("tools", "工具调用", c.tools) +
    t("vision", "视觉", c.vision) +
    t("reasoning", "推理", c.reasoning) +
    `<${tag} ${tag === "button" ? 'type="button" ' : ""}class="x-cap${c.ctx ? "" : " unk"}" data-k="ctx">上下文 ${ctxLabel(c.ctx)}</${tag}>` +
    (c.local ? '<span class="x-cap free">本地 · 免费</span>' : "")
  );
}

export function showCaps() {
  $("fCaps").hidden = !mutable.detected;
  if (!mutable.detected) return;
  $("fCapList").innerHTML = capsHTML(mutable.detected, "button");
  $("fCapSrc").textContent = (SRC[mutable.detected.src] || SRC.none)(mutable.detected);
  $("fCapList")
    .querySelectorAll("button")
    .forEach((b) => b.setAttribute("aria-label", "点一下修改：" + b.textContent));
}

export function initProviders() {
  /* ───────── 服务商（原型的 PROV；模型名只用作输入框的示例） ───────── */
  PROV = {
    OpenAI: ["https://api.openai.com/v1", ["gpt-4o", "gpt-4o-mini", "o3-mini"]],
    DeepSeek: ["https://api.deepseek.com/v1", ["deepseek-chat", "deepseek-reasoner", "deepseek-coder"]],
    通义千问: ["https://dashscope.aliyuncs.com/compatible-mode/v1", ["qwen-max", "qwen-plus", "qwen-turbo"]],
    Kimi: ["https://api.moonshot.cn/v1", ["moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"]],
    OpenRouter: [
      "https://openrouter.ai/api/v1",
      ["openai/gpt-4o-mini", "deepseek/deepseek-chat", "anthropic/claude-3.5-sonnet"],
    ],
    "Ollama（本地）": ["http://localhost:11434/v1", ["llama3.1:8b", "qwen2.5:7b", "deepseek-r1:7b"]],
    自定义: ["", ["gpt-4o-mini", "qwen-plus", "deepseek-chat"]],
  };
  fProv = $("fProv");
  fBase = $("fBase");
  fKey = $("fKey");
  fModel = $("fModel");
  fList = $("fList");
  fRes = $("fRes");
  fHint = $("fHint");
  fEye = $("fEye");
  slash = $("fEyeSlash");
  fFetch = $("fFetch");
  fTest = $("fTest");
  fSave = $("fSave");
  slash.style.display = "none";
  mutable.editing = null;
  mutable.detected = null;
  mutable.fromTip = false;
  models = () => (mutable.cfg && mutable.cfg.models) || [];
  isDef = (m) => mutable.cfg && m.id === mutable.cfg.default_model;
  mark = (v) => (v === true ? "✓" : v === false ? "✗" : "未知");
  cls = (v) => "x-cap" + (v === true ? "" : v === false ? " no" : " unk");
  SRC = {
    exact: () => "能力资料来自 models.dev",
    lab: () => "能力资料来自 models.dev",
    guess: (c) => `资料里没有这个名字，按相近的「${c.name}」推断，不对可以点标签改`,
    none: () => "资料里没有这个模型，点标签手动标记",
    manual: () => "已手动修改",
  };
  // 点标签手动改：是 → 否 → 是…（未知先变成是）；上下文在常见长度里轮换
  CTX_STEPS = [8192, 32768, 65536, 131072, 200000, 1048576, 0];
}
