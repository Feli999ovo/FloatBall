/* 设置窗口。系统能力通过 window.__TAURI__ 调到 Rust（src-tauri/src/lib.rs）。 */
"use strict";

const { invoke } = window.__TAURI__.core;
const thisWin = window.__TAURI__.window.getCurrentWindow();
const $ = (s) => document.querySelector(s);
let cfg = null;      // get_config 的结果
let editing = 0;     // 正在编辑的模型编号；0 = 新增

function say(el, text, kind) { el.textContent = text; el.className = "msg" + (kind ? " " + kind : ""); }
async function reload() { cfg = await invoke("get_config"); return cfg; }

/* ───────── 左侧分类 ───────── */
const tabs = [...document.querySelectorAll("#nav [role=tab]")];
function pick(tab) {
  tabs.forEach((t) => {
    const on = t === tab;
    t.setAttribute("aria-selected", on);
    t.tabIndex = on ? 0 : -1;
    $("#" + t.getAttribute("aria-controls")).hidden = !on;
  });
}
tabs.forEach((t) => (t.onclick = () => pick(t)));
$("#nav").addEventListener("keydown", (e) => {
  const d = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
  if (!d) return;
  e.preventDefault();
  const next = tabs[(tabs.indexOf(document.activeElement) + d + tabs.length) % tabs.length];
  next.focus(); pick(next);
});

/* ───────── 模型库 ───────── */
function host(url) { try { return new URL(url).host; } catch (_) { return url; } }
function renderModels() {
  const ul = $("#modelList");
  ul.textContent = "";
  $("#modelEmpty").hidden = cfg.models.length > 0;
  for (const m of cfg.models) {
    const isDef = m.id === cfg.default_model;
    const li = document.createElement("li");
    li.classList.toggle("def", isDef);
    const info = document.createElement("div"); info.className = "info";
    const name = document.createElement("div"); name.className = "name";
    const title = document.createElement("span"); title.textContent = m.name || m.model; name.appendChild(title);
    const tag = (text, cls) => { const s = document.createElement("span"); s.className = "tag" + (cls ? " " + cls : ""); s.textContent = text; name.appendChild(s); };
    if (isDef) tag("默认", "def");
    if (m.vision) tag("支持图片");
    if (!m.no_key && !m.has_key) tag("没有 Key", "warn");
    const sub = document.createElement("div"); sub.className = "sub";
    sub.textContent = (m.name ? m.model + " · " : "") + host(m.base_url);
    info.append(name, sub);
    li.appendChild(info);
    const btn = (text, fn, cls) => { const b = document.createElement("button"); b.type = "button"; b.textContent = text; if (cls) b.className = cls; b.onclick = fn; li.appendChild(b); return b; };
    if (!isDef) btn("设为默认", async () => { await invoke("set_default_model", { id: m.id }); await reload(); renderModels(); });
    btn("编辑", () => openForm(m));
    const del = btn("删除", async () => {
      // 点两次才删，免得误删
      if (del.dataset.sure !== "1") { del.dataset.sure = "1"; del.textContent = "确认删除"; setTimeout(() => { del.dataset.sure = ""; del.textContent = "删除"; }, 3000); return; }
      await invoke("delete_model", { id: m.id }); await reload(); renderModels();
    }, "danger");
    ul.appendChild(li);
  }
}

function openForm(m) {
  editing = m ? m.id : 0;
  $("#formTitle").textContent = m ? "编辑模型" : "添加模型";
  $("#fBase").value = m ? m.base_url : "";
  $("#fModel").value = m ? m.model : "";
  $("#fName").value = m ? m.name : "";
  $("#fKey").value = "";
  $("#fKey").placeholder = m && m.has_key ? "已保存（留空不改，填新的会替换）" : "sk-…";
  $("#fNoKey").checked = m ? m.no_key : false;
  $("#fVision").checked = m ? m.vision : true;
  $("#modelOptions").textContent = "";
  say($("#formMsg"), "");
  $("#libView").hidden = true;
  $("#formView").hidden = false;
  $("#fBase").focus();
}
function closeForm() { $("#formView").hidden = true; $("#libView").hidden = false; $("#addBtn").focus(); }
$("#addBtn").onclick = () => openForm(null);
$("#cancelBtn").onclick = closeForm;
document.querySelectorAll("#presets button").forEach((b) => (b.onclick = () => {
  $("#fBase").value = b.dataset.b; $("#fModel").value = b.dataset.m;
  $("#fNoKey").checked = b.dataset.k === "1";
  $("#fVision").checked = b.dataset.v !== "0";
  say($("#formMsg"), b.dataset.k === "1" ? "已填入" : "已填入，再填上 API Key");
}));

// 表单里填了 Key 就用填的；没填就让程序用这个模型已经存的
function formKey() { return { apiKey: $("#fKey").value.trim() || null, id: editing || null }; }
function baseOk() {
  if (/^https?:\/\//.test($("#fBase").value.trim())) return true;
  say($("#formMsg"), "接口地址要以 http:// 或 https:// 开头", "bad"); $("#fBase").focus();
  return false;
}
$("#fetchBtn").onclick = async () => {
  if (!baseOk()) return;
  const msg = $("#formMsg"), b = $("#fetchBtn");
  say(msg, "正在获取"); b.disabled = true;
  try {
    const ids = await invoke("list_models", { baseUrl: $("#fBase").value.trim(), ...formKey() });
    const dl = $("#modelOptions"); dl.textContent = "";
    ids.forEach((id) => { const o = document.createElement("option"); o.value = id; dl.appendChild(o); });
    say(msg, ids.length ? `找到 ${ids.length} 个模型，在「模型名」里选一个` : "接口没有返回模型，手动填模型名", ids.length ? "ok" : "");
    if (ids.length) $("#fModel").focus();
  } catch (e) { say(msg, String(e), "bad"); }
  b.disabled = false;
};
$("#testBtn").onclick = async () => {
  if (!baseOk()) return;
  const msg = $("#formMsg");
  if (!$("#fModel").value.trim()) { say(msg, "先填模型名", "bad"); $("#fModel").focus(); return; }
  say(msg, "连接中");
  try { say(msg, await invoke("test_connection", { baseUrl: $("#fBase").value.trim(), model: $("#fModel").value.trim(), ...formKey() }), "ok"); }
  catch (e) { say(msg, String(e), "bad"); }
};
$("#formView").onsubmit = async (e) => {
  e.preventDefault();
  if (!baseOk()) return;
  const msg = $("#formMsg");
  if (!$("#fModel").value.trim()) { say(msg, "模型名不能为空", "bad"); $("#fModel").focus(); return; }
  const key = $("#fKey").value.trim();
  try {
    await invoke("save_model", {
      model: { id: editing, name: $("#fName").value.trim(), base_url: $("#fBase").value.trim(), model: $("#fModel").value.trim(), vision: $("#fVision").checked, no_key: $("#fNoKey").checked },
      apiKey: key || null,
    });
    await reload(); renderModels(); closeForm();
  } catch (err) { say(msg, String(err), "bad"); }
};

/* ───────── 外观、快捷键、通用：改了就存 ───────── */
function radio(group, value) {
  [...group.querySelectorAll("[role=radio]")].forEach((b) => {
    const on = b.dataset.v.toLowerCase() === String(value).toLowerCase();
    b.setAttribute("aria-checked", on);
    b.tabIndex = on ? 0 : -1;
  });
  if (!group.querySelector('[aria-checked="true"]')) group.querySelector("[role=radio]").tabIndex = 0;
}
function radioValue(group, fallback) { const b = group.querySelector('[aria-checked="true"]'); return b ? b.dataset.v : fallback; }
function wireRadios(group, onChange) {
  const items = [...group.querySelectorAll("[role=radio]")];
  items.forEach((b) => (b.onclick = () => { radio(group, b.dataset.v); onChange(); }));
  group.addEventListener("keydown", (e) => {
    const d = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    const next = items[(items.indexOf(document.activeElement) + d + items.length) % items.length];
    next.focus(); next.click();
  });
}
function prefs() {
  return {
    system_prompt: $("#fSys").value,
    shortcut: $("#fKeyMain").dataset.v,
    capture_shortcut: $("#fKeyCap").dataset.v,
    orb_size: +radioValue($("#fSize"), cfg.orb_size),
    glass: +$("#fGlass").value,
    eye: radioValue($("#fEye"), cfg.eye),
  };
}
async function savePrefs(msg) {
  try {
    const busy = await invoke("save_prefs", { prefs: prefs() });
    await reload();
    if (busy) say(msg, "其余设置已保存。" + busy + "，换一个快捷键才能用", "bad");
    else say(msg, "已保存", "ok");
  } catch (e) { say(msg, String(e), "bad"); }
}
function showGlass() { $("#glassOut").textContent = $("#fGlass").value + "%"; }
wireRadios($("#fSize"), () => savePrefs($("#lookMsg")));
wireRadios($("#fEye"), () => savePrefs($("#lookMsg")));
document.querySelectorAll("#fEye button").forEach((b) => b.style.setProperty("--c", b.dataset.v));
$("#fGlass").oninput = showGlass;
$("#fGlass").onchange = () => savePrefs($("#lookMsg"));
$("#fSys").onchange = () => savePrefs($("#generalMsg"));
$("#fAuto").onchange = async () => {
  const msg = $("#generalMsg");
  try { $("#fAuto").checked = await invoke("set_autostart", { on: $("#fAuto").checked }); say(msg, "已保存", "ok"); }
  catch (e) { say(msg, String(e), "bad"); $("#fAuto").checked = !$("#fAuto").checked; }
};

// 快捷键：点一下，再按新的组合键
const MODS = { Control: 1, Alt: 1, Shift: 1, Meta: 1 };
function keyName(e) {
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3);
  if (/^Digit\d$/.test(e.code)) return e.code.slice(5);
  if (/^F\d{1,2}$/.test(e.code)) return e.code;
  return { Space: "Space", Enter: "Enter", Tab: "Tab", Backquote: "`", Minus: "-", Equal: "=", Comma: ",", Period: ".", Slash: "/", Semicolon: ";", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right" }[e.code] || null;
}
function showKey(b) { b.textContent = b.dataset.v; b.classList.remove("rec"); }
document.querySelectorAll(".keycap").forEach((b) => {
  b.onclick = () => { b.classList.add("rec"); b.textContent = "请按新的组合键"; };
  b.onblur = () => showKey(b);
  b.onkeydown = (e) => {
    if (!b.classList.contains("rec")) return;
    e.preventDefault(); e.stopPropagation();
    if (e.key === "Escape") return showKey(b);
    if (MODS[e.key]) return;
    const key = keyName(e), mods = [e.ctrlKey && "Control", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
    const msg = $("#keysMsg");
    if (!key) return say(msg, "这个键不能用作快捷键", "bad");
    if (!mods.length) return say(msg, "至少带一个 Ctrl、Alt 或 Shift", "bad");
    const combo = mods.concat(key).join("+"), other = b.id === "fKeyMain" ? $("#fKeyCap") : $("#fKeyMain");
    if (combo === other.dataset.v) return say(msg, "两个快捷键不能一样", "bad");
    b.dataset.v = combo; showKey(b);
    savePrefs(msg);
  };
});

/* ───────── 关闭 ───────── */
$("#closeBtn").onclick = () => thisWin.close();
// Esc 关窗口；录快捷键时 Esc 是取消录入（那边先拦下了）
document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !e.defaultPrevented) thisWin.close(); });

/* ───────── 启动 ───────── */
(async () => {
  await reload();
  // 有系统毛玻璃的平台：半透明底 + 自己画的标题栏（Rust 那边按同样的平台去掉了系统标题栏）
  document.body.classList.toggle("fx", cfg.platform === "windows" || cfg.platform === "macos");
  renderModels();
  radio($("#fSize"), cfg.orb_size);
  radio($("#fEye"), cfg.eye);
  $("#fGlass").value = cfg.glass; showGlass();
  $("#fKeyMain").dataset.v = cfg.shortcut; showKey($("#fKeyMain"));
  $("#fKeyCap").dataset.v = cfg.capture_shortcut; showKey($("#fKeyCap"));
  $("#fSys").value = cfg.system_prompt;
  $("#fAuto").checked = cfg.autostart;
  // 卡片的毛玻璃只有 Windows 有，别的系统上这一项不起作用，就不显示
  if (cfg.platform !== "windows") { $("#fGlass").closest(".field").hidden = true; $("#glassHint").hidden = true; }
  if (!cfg.models.length) openForm(null);
})();
