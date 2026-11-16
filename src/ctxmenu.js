// 右键菜单窗口（原型 #menu）。Rust 放好位置、截好底图后发 ctx:show；点一项、点别处、Esc 都通过 ctx_act 关掉。
const T = window.__TAURI__;
const invoke = T.core.invoke;
const menu = document.getElementById("menu");
let open = false;
function close(id) {
  if (!open) return;
  open = false;
  menu.classList.remove("on");
  invoke("ctx_act", { id: id || "" }).catch(() => {});
}
T.event.listen("ctx:show", (e) => {
  const { items, bd } = e.payload;
  menu.innerHTML = "";
  for (const it of items) {
    if (it.sep) menu.appendChild(document.createElement("hr"));
    const b = document.createElement("button");
    b.type = "button";
    b.dataset.id = it.id;
    b.setAttribute("role", it.check == null ? "menuitem" : "menuitemcheckbox");
    b.textContent = it.label;
    if (it.check != null) {
      b.setAttribute("aria-checked", String(!!it.check));
      const ck = document.createElement("i"); ck.className = "x-ck"; ck.setAttribute("aria-hidden", "true"); ck.textContent = "✓";
      b.appendChild(ck);
    }
    menu.appendChild(b);
  }
  if (bd) menu.style.setProperty("--bd", 'url("' + bd + '")');
  menu.classList.toggle("glassy", !!bd);
  open = true;
  requestAnimationFrame(() => menu.classList.add("on"));
  menu.focus({ preventScroll: true }); // 焦点放在菜单上（不高亮任何一项，和原型一样）；方向键从第一项开始
});
menu.addEventListener("click", (e) => { const b = e.target.closest("button"); if (b) close(b.dataset.id); });
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") return close("");
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const bs = [...menu.querySelectorAll("button")]; if (!bs.length) return;
    const i = bs.indexOf(document.activeElement);
    const n = i < 0 ? (e.key === "ArrowDown" ? 0 : bs.length - 1) : (i + (e.key === "ArrowDown" ? 1 : bs.length - 1)) % bs.length;
    bs[n].focus();
  }
});
// 点到菜单外（窗口失去焦点）就关掉
window.addEventListener("blur", () => setTimeout(() => { if (!document.hasFocus()) close(""); }, 60));
document.addEventListener("contextmenu", (e) => e.preventDefault());
