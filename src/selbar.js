// 划词工具栏窗口（原型的 selBar / selDd / selHint）。
// Rust 认出在别的应用里选中了文字后发 sel:show；这里按设置排好按钮、量好大小，请 Rust 放到选区旁边。
const T = window.__TAURI__;
const invoke = T.core.invoke, listen = T.event.listen;
const $ = (id) => document.getElementById(id);
const SEL_ACT = { ask: "问问它", explain: "解释", tr: "翻译", sum: "总结", rew: "改写", copy: "复制", read: "朗读", search: "搜索", expand: "扩写", shrink: "缩写" };
const SEL_MAX = 6;
const bar = $("selBar"), dd = $("selDd"), hint = $("selHint");
let sel = null, text = "", hintT = 0, hinted = false;
const mk = (tag, cls, txt) => { const e = document.createElement(tag); if (cls) e.className = cls; if (txt != null) e.textContent = txt; return e; };
const barActs = () => sel.order.filter((o) => o.show).slice(0, SEL_MAX).map((o) => o.id);
const ddActs = () => { const b = barActs(); return sel.order.map((o) => o.id).filter((id) => !b.includes(id)); };
function btn(id) { const b = mk("button", "", SEL_ACT[id]); b.type = "button"; b.dataset.a = id; return b; }
function render() {
  bar.innerHTML = "";
  const o = mk("button", "x-selorb"); o.type = "button"; o.dataset.a = "ask"; o.title = "问问它"; o.setAttribute("aria-label", "悬浮球 · 问问它");
  o.innerHTML = '<span aria-hidden="true"><i></i><i></i></span>'; bar.appendChild(o);
  barActs().forEach((id) => bar.appendChild(btn(id)));
  if (ddActs().length) {
    bar.appendChild(mk("span", "x-selsep"));
    const m = mk("button", "x-selmore", "⋯"); m.type = "button"; m.setAttribute("aria-label", "更多"); m.setAttribute("aria-haspopup", "menu"); m.setAttribute("aria-expanded", "false");
    bar.appendChild(m);
  }
  dd.hidden = true;
}
const size = () => { const r = $("wrap").getBoundingClientRect(); return { w: Math.ceil(r.width), h: Math.ceil(r.height) }; };
function ddToggle(open) {
  const m = bar.querySelector(".x-selmore"); if (!m) return;
  dd.innerHTML = "";
  if (open) ddActs().forEach((id) => { const b = btn(id); b.setAttribute("role", "menuitem"); dd.appendChild(b); });
  dd.hidden = !open; m.setAttribute("aria-expanded", String(open));
  invoke("sel_resize", size());
}
listen("sel:show", async ({ payload }) => {
  try { sel = (await invoke("get_config")).sel; } catch (_) { return; }
  text = payload.text; document.body.classList.remove("hint"); hint.hidden = true; clearTimeout(hintT);
  render();
  bar.style.animation = "none"; void bar.offsetWidth; bar.style.animation = "";
  invoke("sel_place", { ...size(), x: payload.x, y: payload.y });
});
// macOS 还没给「辅助功能」权限：本次运行只提示一次，4 秒后自动收起
listen("sel:hint", ({ payload }) => {
  if (hinted) return; hinted = true;
  document.body.classList.add("hint"); hint.hidden = false; dd.hidden = true;
  invoke("sel_place", { ...size(), x: payload.x, y: payload.y });
  hintT = setTimeout(() => invoke("sel_hide"), 4000);
});
$("selHintGo").addEventListener("click", () => { invoke("sel_open_privacy"); invoke("sel_hide"); });
async function go(a, b) {
  if (a === "copy") {
    try { await invoke("sel_copy", { text }); } catch (_) { b.textContent = "复制失败"; setTimeout(() => invoke("sel_hide"), 1000); return; }
    // 按钮原地显示「已复制」1 秒
    const w = b.offsetWidth; b.textContent = "已复制"; b.classList.add("x-selok"); b.style.minWidth = w + "px";
    setTimeout(() => invoke("sel_hide"), 1000);
    return;
  }
  if (a === "search") { invoke("open_url", { url: "https://www.bing.com/search?q=" + encodeURIComponent(text.replace(/\s+/g, " ").slice(0, 300)) }); invoke("sel_hide"); return; }
  invoke("sel_act", { act: a, text });
}
bar.addEventListener("click", (e) => {
  const b = e.target.closest("button"); if (!b) return;
  if (b.classList.contains("x-selmore")) return ddToggle(dd.hidden);
  go(b.dataset.a, b);
});
dd.addEventListener("click", (e) => { const b = e.target.closest("button"); if (b) go(b.dataset.a, b); });
