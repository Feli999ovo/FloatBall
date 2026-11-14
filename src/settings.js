/* 设置窗口。界面和交互照搬原型的设置窗口（模型库 → 添加 / 编辑模型），
   原型里的演示数据换成真的：模型库存在配置里（Rust：save_model / delete_model / set_default_model），
   Key 存系统钥匙串，「获取列表」请求接口的 /models，能力标签查 models.dev 的资料（model_caps）。 */
"use strict";

const T = window.__TAURI__;
const { invoke } = T.core;
const win = T.window.getCurrentWindow();
const $ = (id) => document.getElementById(id);
const jslog = (m) => invoke("js_log", { msg: "[设置] " + String(m) }).catch(() => {});
window.addEventListener("error", (e) => jslog(e.message + " @" + e.lineno));

let cfg = null;
let toastT = 0;
function toast(t) {
  const el = $("toast"); el.textContent = t; el.classList.add("on");
  clearTimeout(toastT); toastT = setTimeout(() => el.classList.remove("on"), 1800);
}

/* ───────── 服务商（原型的 PROV；模型名只用作输入框的示例） ───────── */
const PROV = {
  "OpenAI": ["https://api.openai.com/v1", ["gpt-4o", "gpt-4o-mini", "o3-mini"]],
  "DeepSeek": ["https://api.deepseek.com/v1", ["deepseek-chat", "deepseek-reasoner", "deepseek-coder"]],
  "通义千问": ["https://dashscope.aliyuncs.com/compatible-mode/v1", ["qwen-max", "qwen-plus", "qwen-turbo"]],
  "Kimi": ["https://api.moonshot.cn/v1", ["moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"]],
  "OpenRouter": ["https://openrouter.ai/api/v1", ["openai/gpt-4o-mini", "deepseek/deepseek-chat", "anthropic/claude-3.5-sonnet"]],
  "Ollama（本地）": ["http://localhost:11434/v1", ["llama3.1:8b", "qwen2.5:7b", "deepseek-r1:7b"]],
  "自定义": ["", ["gpt-4o-mini", "qwen-plus", "deepseek-chat"]],
};
const fProv = $("fProv"), fBase = $("fBase"), fKey = $("fKey"), fModel = $("fModel"), fList = $("fList"), fRes = $("fRes"), fHint = $("fHint");
const fEye = $("fEye"), slash = $("fEyeSlash"), fFetch = $("fFetch"), fTest = $("fTest"), fSave = $("fSave");
slash.style.display = "none";
let editing = null, detected = null, fromTip = false;

const models = () => (cfg && cfg.models) || [];
const isDef = (m) => cfg && m.id === cfg.default_model;

/* ───────── 能力标签 ───────── */
function ctxLabel(n) {
  if (!n) return "未知";
  const pick = (a, b) => (Math.abs(a - Math.round(a)) < Math.abs(b - Math.round(b)) ? a : b);
  const fmt = (v) => (Math.abs(v - Math.round(v)) < 0.05 ? String(Math.round(v)) : v.toFixed(1));
  if (n >= 1e6) return fmt(pick(n / 1048576, n / 1e6)) + "M";
  if (n >= 1000) return fmt(pick(n / 1024, n / 1000)) + "K";
  return String(n);
}
const mark = (v) => (v === true ? "✓" : v === false ? "✗" : "未知");
const cls = (v) => "x-cap" + (v === true ? "" : v === false ? " no" : " unk");
// 原型：工具调用 / 视觉 / 上下文 / 本地 · 免费；按 J 的要求加了「推理」
function capsHTML(c, tag = "span") {
  const t = (k, label, v) => `<${tag} ${tag === "button" ? 'type="button" ' : ""}class="${cls(v)}" data-k="${k}">${label} ${mark(v)}</${tag}>`;
  return t("tools", "工具调用", c.tools) + t("vision", "视觉", c.vision) + t("reasoning", "推理", c.reasoning) +
    `<${tag} ${tag === "button" ? 'type="button" ' : ""}class="x-cap${c.ctx ? "" : " unk"}" data-k="ctx">上下文 ${ctxLabel(c.ctx)}</${tag}>` +
    (c.local ? '<span class="x-cap free">本地 · 免费</span>' : "");
}
const SRC = {
  exact: () => "能力资料来自 models.dev",
  lab: () => "能力资料来自 models.dev",
  guess: (c) => `资料里没有这个名字，按相近的「${c.name}」推断，不对可以点标签改`,
  none: () => "资料里没有这个模型，点标签手动标记",
  manual: () => "已手动修改",
};
function showCaps() {
  $("fCaps").hidden = !detected;
  if (!detected) return;
  $("fCapList").innerHTML = capsHTML(detected, "button");
  $("fCapSrc").textContent = (SRC[detected.src] || SRC.none)(detected);
  $("fCapList").querySelectorAll("button").forEach((b) => b.setAttribute("aria-label", "点一下修改：" + b.textContent));
}
// 点标签手动改：是 → 否 → 是…（未知先变成是）；上下文在常见长度里轮换
const CTX_STEPS = [8192, 32768, 65536, 131072, 200000, 1048576, 0];
$("fCapList").addEventListener("click", (e) => {
  const b = e.target.closest("button[data-k]"); if (!b || !detected) return;
  const k = b.dataset.k;
  if (k === "ctx") { const i = CTX_STEPS.findIndex((v) => v === (detected.ctx || 0)); detected.ctx = CTX_STEPS[(i + 1) % CTX_STEPS.length] || null; }
  else detected[k] = detected[k] === true ? false : true;
  detected.src = "manual"; showCaps();
  const nb = $("fCapList").querySelector(`[data-k="${k}"]`); if (nb) nb.focus({ preventScroll: true });
});
// 输入模型名 / 换服务商 / 改地址后查资料（不联网）
let capT = 0, capSeq = 0;
function lookupCaps(delay = 250) {
  clearTimeout(capT);
  const model = fModel.value.trim();
  if (!model) { detected = null; showCaps(); return; }
  const seq = ++capSeq;
  capT = setTimeout(async () => {
    try {
      const c = await invoke("model_caps", { provider: fProv.value, baseUrl: fBase.value.trim(), model });
      if (seq !== capSeq) return;
      detected = c; showCaps();
    } catch (e) { jslog(e); }
  }, delay);
}
function logoOf(id) { return id && window.FB_LOGOS && window.FB_LOGOS.has(id) ? id : "_default"; }

/* ───────── 模型库 ───────── */
function renderLib() {
  const ul = $("libList"); ul.innerHTML = ""; $("libEmpty").hidden = !!models().length;
  models().forEach((m) => {
    const def = isDef(m);
    const li = document.createElement("li"); li.className = "x-mc" + (def ? " def" : "");
    li.innerHTML = `<p class="x-mcn"><i class="x-logo" aria-hidden="true"></i><b></b>${def ? '<span class="x-star">★ 默认</span>' : ""}</p><p class="x-mcp"></p><div class="x-caps">${capsHTML(capsOf(m))}</div>` +
      `<div class="x-mca">${def ? "" : '<button type="button" data-a="def">设为默认</button>'}<button type="button" data-a="edit">编辑</button><button type="button" class="x-del" data-a="del">删除</button></div>`;
    li.querySelector(".x-logo").style.setProperty("--logo", `url("assets/logos/${logoOf(m.logo)}.svg")`);
    li.querySelector("b").textContent = m.name || m.model;
    const noKey = !m.no_key && !m.has_key;
    li.querySelector(".x-mcp").textContent = provLabel(m).replace("（本地）", "") + " · " + m.model + (noKey ? " · 还没填 Key" : "");
    li.querySelectorAll(".x-mca button").forEach((b) => b.setAttribute("aria-label", b.textContent + " " + m.name));
    li.querySelector(".x-mca").addEventListener("click", async (e) => {
      const b = e.target.closest("button"); const a = b && b.dataset.a;
      try {
        if (a === "edit") openForm(m);
        if (a === "def") { await invoke("set_default_model", { id: m.id }); await reload(); toast(`默认模型：${m.name || m.model}`); }
        // 原型点一下就删；这里沿用原来的做法，要再点一次「确认删除」，免得误删
        if (a === "del" && b.dataset.sure !== "1") {
          b.dataset.sure = "1"; b.textContent = "确认删除"; b.setAttribute("aria-label", "确认删除 " + (m.name || m.model));
          setTimeout(() => { if (b.isConnected) { b.dataset.sure = ""; b.textContent = "删除"; b.setAttribute("aria-label", "删除 " + (m.name || m.model)); } }, 3000);
          return;
        }
        if (a === "del") { await invoke("delete_model", { id: m.id }); await reload(); toast(`已删除 ${m.name || m.model}`); $("libAdd").focus({ preventScroll: true }); }
      } catch (err) { toast(String(err)); }
    });
    ul.appendChild(li);
  });
  $("libSrc").textContent = cfg && cfg.catalog_date ? `能力资料来自 models.dev（${cfg.catalog_date}），联网时每天自动更新` : "";
}
// 早期版本存的模型没有服务商和能力字段：按地址认服务商，能力当场查资料（只用来显示，保存时才写进配置）
const HOST_PROV = { "api.openai.com": "OpenAI", "api.deepseek.com": "DeepSeek", "dashscope.aliyuncs.com": "通义千问", "api.moonshot.cn": "Kimi", "openrouter.ai": "OpenRouter" };
function provLabel(m) {
  if (m.provider) return m.provider;
  let h = ""; try { h = new URL(m.base_url).hostname; } catch (_) {}
  if (/^(localhost|127\.|0\.0\.0\.0|\[::1\])/.test(h) || m.no_key) return "Ollama（本地）";
  return HOST_PROV[h] || "自定义";
}
const oldCaps = {};
function capsOf(m) {
  const local = m.no_key || provLabel(m).startsWith("Ollama");
  if (m.caps_src) return { tools: m.tools, vision: m.vision_known ? m.vision : null, reasoning: m.reasoning, ctx: m.ctx, local, src: m.caps_src, name: m.name, provider: m.logo };
  const k = m.id + "|" + m.base_url + "|" + m.model;
  if (oldCaps[k]) return { ...oldCaps[k], vision: m.vision, local };
  invoke("model_caps", { provider: provLabel(m), baseUrl: m.base_url, model: m.model })
    .then((c) => { oldCaps[k] = c; m.logo = c.provider; if (!$("paneLib").hidden) renderLib(); }).catch(jslog);
  return { tools: null, vision: m.vision, reasoning: null, ctx: null, local, src: "none" };
}
function openForm(m) {
  editing = m || null; fList.hidden = true; fRes.textContent = ""; fRes.className = "x-res";
  $("fTitle").textContent = m ? "编辑模型" : "添加模型";
  fKey.type = "password"; slash.style.display = "none"; fEye.setAttribute("aria-pressed", "false"); fEye.setAttribute("aria-label", "显示 Key");
  if (m) {
    fProv.value = PROV[provLabel(m)] ? provLabel(m) : "自定义"; syncProv(); fBase.value = m.base_url; fKey.value = ""; fModel.value = m.model;
    fKey.placeholder = m.has_key ? "已保存（留空不改）" : "sk-…";
    detected = capsOf(m);
  } else { syncProv(); fKey.value = ""; fModel.value = ""; fKey.placeholder = "sk-…"; }
  showCaps(); $("paneLib").hidden = true; $("paneModel").hidden = false; $("paneModel").scrollTop = 0;
  setTimeout(() => fProv.focus({ preventScroll: true }), 60);
}
function syncProv() {
  const p = PROV[fProv.value];
  fBase.value = p[0]; fModel.placeholder = "如 " + p[1][0];
  fHint.hidden = !fProv.value.startsWith("Ollama"); fList.hidden = true; fRes.textContent = ""; fRes.className = "x-res";
  detected = null; showCaps();
}

/* ───────── 分类（原型的 showTab） ───────── */
const PANES = { model: "paneLib", agents: "paneAgents", perm: "panePerm", look: "paneLook", keys: "paneKeys", sel: "paneSel", voice: "paneVoice" };
function showTab(t) {
  const k = t === "add" ? "model" : t;
  document.querySelectorAll(".x-tabs button").forEach((o) => o.classList.toggle("on", o.dataset.tab === k));
  for (const [tab, id] of Object.entries(PANES)) $(id).hidden = tab !== k;
  $("paneModel").hidden = true; fList.hidden = true;
  if (k === "model") renderLib();
  if (t === "add") openForm();
  if (k === "keys") stopRec();
  document.querySelectorAll(".x-setw .x-pane").forEach((p) => { p.scrollTop = 0; });
}
document.querySelectorAll(".x-tabs button").forEach((b) => b.addEventListener("click", () => { fromTip = false; showTab(b.dataset.tab); }));
$("libAdd").addEventListener("click", () => openForm());
$("fBack").addEventListener("click", () => { fromTip = false; showTab("model"); $("libAdd").focus({ preventScroll: true }); });
fModel.addEventListener("input", () => lookupCaps());
fBase.addEventListener("input", () => lookupCaps(400));
fProv.addEventListener("change", () => { syncProv(); lookupCaps(0); });

/* ───────── 表单按钮 ───────── */
fEye.addEventListener("click", () => {
  const show = fKey.type === "password";
  fKey.type = show ? "text" : "password"; slash.style.display = show ? "" : "none";
  fEye.setAttribute("aria-pressed", show); fEye.setAttribute("aria-label", show ? "隐藏 Key" : "显示 Key");
});
function res(kind, text) {
  fRes.className = "x-res" + (kind === "bad" ? " bad" : "");
  fRes.innerHTML = kind === "ok" ? '<span class="x-ok"></span>' : kind === "spin" ? '<span class="x-spin"></span>' : "";
  if (text) { const s = document.createElement("span"); s.className = "t"; s.textContent = text; s.title = text; fRes.appendChild(s); }
}
fFetch.addEventListener("click", async () => {
  if (!fBase.value.trim()) return res("bad", "请填写接口地址");
  fFetch.disabled = true; fFetch.innerHTML = '<span class="x-spin" aria-label="加载中"></span>';
  try {
    const ids = await invoke("list_models", { baseUrl: fBase.value.trim(), apiKey: fKey.value.trim() || null, id: editing ? editing.id : null });
    fList.innerHTML = "";
    ids.forEach((m) => {
      const o = document.createElement("button"); o.type = "button"; o.textContent = m;
      o.addEventListener("click", () => { fModel.value = m; fList.hidden = true; lookupCaps(0); });
      fList.appendChild(o);
    });
    fList.hidden = false; fList.scrollTop = 0; res("", "");
  } catch (e) { fList.hidden = true; res("bad", String(e)); }
  fFetch.disabled = false; fFetch.textContent = "获取列表";
});
fTest.addEventListener("click", async () => {
  if (!fBase.value.trim()) return res("bad", "请填写接口地址");
  const model = fModel.value.trim() || PROV[fProv.value][1][0];
  fTest.disabled = true; res("spin", "测试中…");
  try {
    const r = await invoke("test_connection", { baseUrl: fBase.value.trim(), model, apiKey: fKey.value.trim() || null, id: editing ? editing.id : null });
    const ms = String(r).match(/(\d+)\s*毫秒/);
    res("ok", "连接成功" + (ms ? ` · ${ms[1]} 毫秒` : ""));
    if (!fModel.value.trim()) fModel.value = model;
    if (!detected || detected.src !== "manual") lookupCaps(0);
    // 原型：悬浮球用「完成」状态开心一下
    T.event.emitTo("main", "ball-state", "done").catch(() => {});
  } catch (e) { res("bad", String(e)); }
  fTest.disabled = false;
});
fSave.addEventListener("click", async () => {
  const model = fModel.value.trim();
  if (!model) { res("bad", "请填写模型名"); fModel.focus(); return; }
  if (!/^https?:\/\//.test(fBase.value.trim())) { res("bad", "接口地址要以 http:// 或 https:// 开头"); fBase.focus(); return; }
  let c = detected;
  if (!c) { try { c = await invoke("model_caps", { provider: fProv.value, baseUrl: fBase.value.trim(), model }); } catch (_) { c = {}; } }
  // vision 不知道时按支持处理（截图照发，模型不收会报错），和以前一样
  const local = !!c.local || fProv.value.startsWith("Ollama");
  const entry = {
    id: editing ? editing.id : 0,
    name: editing && editing.model === model ? editing.name : "",
    provider: fProv.value, base_url: fBase.value.trim(), model,
    vision: c.vision !== false, vision_known: c.vision === true || c.vision === false,
    tools: c.tools ?? null, reasoning: c.reasoning ?? null, ctx: c.ctx || null,
    no_key: local && !fKey.value.trim() && !(editing && editing.has_key),
    caps_src: c.src || "none", logo: c.provider || "",
  };
  fSave.disabled = true;
  try {
    await invoke("save_model", { model: entry, apiKey: fKey.value.trim() || null });
    editing = null; fKey.value = ""; await reload(); toast("已保存");
    if (fromTip) { fromTip = false; win.close(); } else { showTab("model"); $("libAdd").focus({ preventScroll: true }); }
  } catch (e) { res("bad", String(e)); }
  fSave.disabled = false;
});
document.addEventListener("click", (e) => { if (!e.target.closest("#fList,#fFetch")) fList.hidden = true; });

/* ───────── 外观、Agents、快捷键：改了就存（save_prefs） ───────── */
function prefs() {
  const on = (g) => { const b = $(g).querySelector('[aria-checked="true"]'); return b && b.dataset.v; };
  return {
    system_prompt: $("fSys").value,
    shortcut: keyVal.shortcut, capture_shortcut: keyVal.capture_shortcut,
    orb_size: +(on("lkSize") || cfg.orb_size), glass: +$("lkGlass").value, eye: on("lkEye") || cfg.eye,
  };
}
async function savePrefs() {
  try {
    const busy = await invoke("save_prefs", { prefs: prefs() });
    await reload();
    toast(busy ? busy + "，换一个快捷键才能用" : "已保存");
    return !busy;
  } catch (e) { toast(String(e)); return false; }
}
$("fSys").addEventListener("change", savePrefs);

// 外观（原型的 lkSize / lkGlass / lkEye）
const radio = (g, v) => $(g).querySelectorAll("[role=radio]").forEach((b) => {
  const on = b.dataset.v.toLowerCase() === String(v).toLowerCase(); b.setAttribute("aria-checked", on); b.tabIndex = on ? 0 : -1;
});
["lkSize", "lkEye"].forEach((g) => {
  $(g).addEventListener("click", (e) => { const b = e.target.closest("[role=radio]"); if (!b) return; radio(g, b.dataset.v); savePrefs(); });
  $(g).addEventListener("keydown", (e) => {
    const bs = [...$(g).querySelectorAll("[role=radio]")], i = bs.indexOf(document.activeElement);
    const d = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!d || i < 0) return; e.preventDefault(); const n = bs[(i + d + bs.length) % bs.length]; n.focus(); n.click();
  });
});
// 原型的眼睛颜色（奶白、暖黄、蜜桃、珊瑚）
[["#FFF1DC", "奶白（默认）"], ["#FFE2A8", "暖黄"], ["#FFC9A6", "蜜桃"], ["#FFB7A0", "珊瑚"]].forEach(([c, name]) => {
  const b = document.createElement("button"); b.type = "button"; b.className = "x-dot"; b.setAttribute("role", "radio");
  b.dataset.v = c; b.setAttribute("aria-label", name); b.title = name; b.style.setProperty("--t", c); $("lkEye").appendChild(b);
});
const showGlass = () => { const v = +$("lkGlass").value; $("lkGlassV").textContent = v ? v + "%" : "不透明"; };
$("lkGlass").addEventListener("input", showGlass);
$("lkGlass").addEventListener("change", savePrefs);

// 快捷键：原型的「录制」。写进配置用 Tauri 的写法（Alt+Space），显示用原型的写法（⌥ Space）
const keyVal = { shortcut: "", capture_shortcut: "" };
let keyRec = null;
const isMac = /Mac/.test(navigator.platform);
const SYM = { Control: "⌃", Alt: "⌥", Shift: "⇧", Super: "⌘", CommandOrControl: isMac ? "⌘" : "⌃" };
const KEYN = { Space: "Space", Enter: "Enter", Tab: "Tab", Backquote: "`", Minus: "-", Equal: "=", Comma: ",", Period: ".", Slash: "/", Semicolon: ";", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right" };
const SHOWN = { Up: "↑", Down: "↓", Left: "←", Right: "→", Enter: "↩", Tab: "⇥" };
function keyName(e) {
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3);
  if (/^Digit\d$/.test(e.code)) return e.code.slice(5);
  if (/^F\d{1,2}$/.test(e.code)) return e.code;
  return KEYN[e.code] || null;
}
function showCombo(v) {
  if (!v) return "未设置";
  const parts = v.split("+"); const key = parts.pop();
  const mods = parts.map((p) => (isMac ? SYM[p] || p : p === "Super" ? "Win" : p === "CommandOrControl" ? "Ctrl" : p === "Control" ? "Ctrl" : p));
  return mods.concat(SHOWN[key] || key).join(isMac ? " " : "+");
}
function renderKeys() {
  $("keyList").querySelectorAll("li").forEach((li) => {
    const k = li.dataset.k, on = keyRec === k, other = k === "shortcut" ? "capture_shortcut" : "shortcut";
    li.classList.toggle("rec", on);
    li.querySelector(".x-kc").textContent = on ? "按下新的组合键…" : showCombo(keyVal[k]);
    const b = li.querySelector(".x-sb"); b.textContent = on ? "取消" : "录制";
    b.setAttribute("aria-label", (on ? "取消录制：" : "录制：") + li.querySelector("b").textContent);
    const dup = keyVal[k] && keyVal[k] === keyVal[other];
    li.classList.toggle("bad", !!dup);
  });
}
function stopRec() { keyRec = null; renderKeys(); }
$("keyList").addEventListener("click", (e) => {
  const b = e.target.closest(".x-sb"); if (!b) return;
  const k = b.closest("li").dataset.k; keyRec = keyRec === k ? null : k; renderKeys(); b.focus({ preventScroll: true });
});
const warn = (k, t) => { const p = $("keyList").querySelector(`[data-k="${k}"] .x-kw`); p.textContent = t || ""; };
document.addEventListener("keydown", async (e) => {
  if (!keyRec) return;
  e.preventDefault(); e.stopPropagation();
  const k = keyRec;
  if (e.key === "Escape") { warn(k); return stopRec(); }
  if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return;
  const key = keyName(e), mods = [e.ctrlKey && "Control", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
  if (!key) return warn(k, "这个键不能用作快捷键");
  if (!mods.length) return warn(k, "至少带一个 Ctrl、Alt 或 Shift");
  const combo = mods.concat(key).join("+"), other = k === "shortcut" ? "capture_shortcut" : "shortcut";
  if (combo === keyVal[other]) return warn(k, "与另一个快捷键相同，换一个");
  keyVal[k] = combo; keyRec = null; warn(k); renderKeys();
  if (!(await savePrefs())) { warn(k, "被别的程序占用了，换一个"); }
}, true);

/* ───────── 读配置 ───────── */
async function reload() {
  cfg = await invoke("get_config");
  $("fSys").value = cfg.system_prompt;
  keyVal.shortcut = cfg.shortcut; keyVal.capture_shortcut = cfg.capture_shortcut; if (!keyRec) renderKeys();
  radio("lkSize", cfg.orb_size); radio("lkEye", cfg.eye); $("lkGlass").value = cfg.glass; showGlass();
  if (!$("paneLib").hidden) renderLib();
}
T.event.listen("catalog-updated", async () => { await reload(); if (!$("paneModel").hidden && (!detected || detected.src !== "manual")) lookupCaps(0); });

$("setClose").addEventListener("click", () => win.close());
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || e.defaultPrevented) return;
  if (!fList.hidden) { fList.hidden = true; return; }
  win.close();
});

(async () => {
  try {
    await reload();
    document.body.classList.toggle("fx", cfg.platform === "windows" || cfg.platform === "macos");
    document.body.classList.toggle("win", cfg.platform === "windows");
    // 卡片的透明度只在 Windows 上起作用（别的系统没有卡片毛玻璃），别处不显示这一项
    $("lkGlassF").hidden = cfg.platform !== "windows";
    // 原型：首次引导「先设置一个模型吧 → 去设置」直接打开添加模型，保存后关掉设置
    if (!models().length) { fromTip = true; showTab("add"); } else showTab("model");
  } catch (e) { toast(String(e)); jslog(e); }
  invoke("catalog_refresh").catch(() => {});
})();
