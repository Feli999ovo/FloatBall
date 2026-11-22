import { mk } from "./agent-form.js";
import { radio, savePrefs, sw } from "./appearance.js";
import { SEL_ACT, SEL_MAX } from "./shortcuts.js";
import { $, invoke, mutable, toast } from "./state.js";

export let selRecent, selBarActs;

export function renderOrd(focusId, focusSel) {
  const ul = $("selOrd");
  ul.innerHTML = "";
  const bar = selBarActs();
  mutable.selCfg.order.forEach((o, i) => {
    const li = document.createElement("li");
    li.dataset.id = o.id;
    const h = mk("button", "x-selh", "⋮⋮");
    h.type = "button";
    h.setAttribute("aria-label", `拖动排序：${SEL_ACT[o.id]}`);
    h.title = "拖动排序";
    const nm = mk("b", "", SEL_ACT[o.id]);
    if (o.show && !bar.includes(o.id)) nm.appendChild(mk("small", "", "放在「⋯」"));
    const lb = document.createElement("label"),
      cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = o.show;
    cb.setAttribute("aria-label", `在工具栏显示「${SEL_ACT[o.id]}」`);
    lb.append(cb);
    cb.addEventListener("change", () => {
      o.show = cb.checked;
      renderOrd(o.id, "input");
      savePrefs();
    });
    const up = mk("button", "x-selmv", "↑"),
      dn = mk("button", "x-selmv", "↓");
    up.type = dn.type = "button";
    up.setAttribute("aria-label", `上移 ${SEL_ACT[o.id]}`);
    dn.setAttribute("aria-label", `下移 ${SEL_ACT[o.id]}`);
    up.disabled = i === 0;
    dn.disabled = i === mutable.selCfg.order.length - 1;
    up.dataset.d = "up";
    dn.dataset.d = "dn";
    up.addEventListener("click", () => moveOrd(i, i - 1, "[data-d=up]"));
    dn.addEventListener("click", () => moveOrd(i, i + 1, "[data-d=dn]"));
    h.addEventListener("keydown", (e) => {
      if (e.key === "ArrowUp" && i > 0) {
        e.preventDefault();
        moveOrd(i, i - 1, ".x-selh");
      }
      if (e.key === "ArrowDown" && i < mutable.selCfg.order.length - 1) {
        e.preventDefault();
        moveOrd(i, i + 1, ".x-selh");
      }
    });
    h.addEventListener("pointerdown", (e) => dragOrd(e, li));
    li.append(h, nm, lb, up, dn);
    ul.appendChild(li);
  });
  if (focusId) {
    const li = ul.querySelector(`[data-id="${focusId}"]`),
      f = li && (li.querySelector(focusSel + ":not(:disabled)") || li.querySelector(".x-selh"));
    if (f) f.focus({ preventScroll: true });
  }
}

export function moveOrd(i, j, focusSel) {
  const [o] = mutable.selCfg.order.splice(i, 1);
  mutable.selCfg.order.splice(j, 0, o);
  renderOrd(o.id, focusSel);
  savePrefs();
}

// 拖动把手排序：移动 li 本身，松手后按 DOM 顺序写回
export function dragOrd(e, li) {
  if (e.button !== 0) return;
  const ul = $("selOrd");
  e.preventDefault();
  li.classList.add("drag");
  const mv = (ev) => {
    const sib = [...ul.children].filter((n) => n !== li);
    const nx = sib.find((n) => {
      const r = n.getBoundingClientRect();
      return ev.clientY < r.top + r.height / 2;
    });
    if (nx) {
      if (li.nextSibling !== nx) ul.insertBefore(li, nx);
    } else if (ul.lastChild !== li) ul.appendChild(li);
  };
  const up = () => {
    removeEventListener("pointermove", mv);
    removeEventListener("pointerup", up);
    removeEventListener("pointercancel", up);
    li.classList.remove("drag");
    const ids = [...ul.children].map((n) => n.dataset.id),
      before = mutable.selCfg.order.map((o) => o.id).join();
    mutable.selCfg.order.sort((a, b) => ids.indexOf(a.id) - ids.indexOf(b.id));
    renderOrd(li.dataset.id, ".x-selh");
    if (mutable.selCfg.order.map((o) => o.id).join() !== before) savePrefs();
  };
  addEventListener("pointermove", mv);
  addEventListener("pointerup", up);
  addEventListener("pointercancel", up);
}

// 排除的应用：原型是标签 + 「+ 添加应用」。程序里点添加后输入应用名，候选是最近出现过划词的应用
export function renderApps(focus) {
  const box = $("selApps");
  box.innerHTML = "";
  mutable.selCfg.apps.forEach((a, i) => {
    const c = mk("span", "x-selchip", a),
      x = mk("button", "", "×");
    c.setAttribute("role", "listitem");
    x.type = "button";
    x.setAttribute("aria-label", `移除 ${a}`);
    x.addEventListener("click", () => {
      mutable.selCfg.apps.splice(i, 1);
      renderApps("add");
      savePrefs();
    });
    c.appendChild(x);
    box.appendChild(c);
  });
  const add = mk("button", "x-sb", "+ 添加应用");
  add.type = "button";
  add.id = "selAdd";
  add.addEventListener("click", () => {
    const inp = mk("input", "x-in x-selnew");
    inp.id = "selNew";
    inp.placeholder = "应用名，如 微信";
    inp.setAttribute("aria-label", "要排除的应用名");
    inp.setAttribute("list", "selRecent");
    const dl = mk("datalist");
    dl.id = "selRecent";
    selRecent
      .filter((r) => !mutable.selCfg.apps.some((a) => a.toLowerCase() === r.toLowerCase()))
      .forEach((r) => dl.appendChild(Object.assign(document.createElement("option"), { value: r })));
    add.replaceWith(inp, dl);
    inp.focus();
    let done = false;
    const commit = (keep) => {
      if (done) return;
      done = true;
      const v = inp.value.trim();
      if (keep && v) {
        if (mutable.selCfg.apps.some((a) => a.toLowerCase() === v.toLowerCase())) toast(`「${v}」已在列表里`);
        else {
          mutable.selCfg.apps.push(v);
          savePrefs();
        }
      }
      renderApps("add");
    };
    inp.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        commit(true);
      }
      if (e.key === "Escape") {
        e.preventDefault();
        commit(false);
      }
    });
    inp.addEventListener("blur", () => commit(true));
  });
  box.appendChild(add);
  if (focus === "add") add.focus({ preventScroll: true });
}

export function renderSel() {
  if (!mutable.selCfg) return;
  $("selOn").setAttribute("aria-checked", !!mutable.selCfg.on);
  radio("selMode", mutable.selCfg.mode);
  $("selTr").value = mutable.selCfg.tr;
  const a = document.activeElement,
    li = a && a.closest && a.closest("#selOrd li");
  renderOrd(
    li && li.dataset.id,
    a && a.dataset && a.dataset.d
      ? `[data-d=${a.dataset.d}]`
      : a && a.classList && a.classList.contains("x-selh")
        ? ".x-selh"
        : "input",
  );
  if (!$("selNew")) renderApps(a && a.id === "selAdd" ? "add" : null);
}

export async function selStatus() {
  let st = { supported: true, trusted: true, recent: [] };
  try {
    st = await invoke("sel_status");
  } catch (_) {}
  selRecent = st.recent || [];
  const mac = mutable.cfg.platform === "macos",
    winp = mutable.cfg.platform === "windows";
  $("selNo").hidden = st.supported;
  $("selMacNote").hidden = !mac;
  $("selWinNote").hidden = !winp;
  $("selClip").hidden = !winp;
  $("selPriv").hidden = !mac || st.trusted;
  $("selModeAlt").textContent = `按住 ${mac ? "⌥" : "Alt"} 再选中`;
}

export function initSelection() {
  mutable.selCfg = null;
  selRecent = [];
  selBarActs = () =>
    mutable.selCfg.order
      .filter((o) => o.show)
      .slice(0, SEL_MAX)
      .map((o) => o.id);
  $("libAuto").addEventListener("click", () => {
    $("libAuto").setAttribute("aria-checked", !sw("libAuto"));
    savePrefs();
  });
  $("selOn").addEventListener("click", () => {
    mutable.selCfg.on = !sw("selOn");
    $("selOn").setAttribute("aria-checked", mutable.selCfg.on);
    savePrefs();
  });
  $("selMode").addEventListener("click", (e) => {
    const b = e.target.closest("[role=radio]");
    if (!b) return;
    radio("selMode", b.dataset.v);
    mutable.selCfg.mode = b.dataset.v;
    savePrefs();
  });
  $("selMode").addEventListener("keydown", (e) => {
    const bs = [...$("selMode").querySelectorAll("[role=radio]")],
      i = bs.indexOf(document.activeElement);
    const d =
      e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!d || i < 0) return;
    e.preventDefault();
    const n = bs[(i + d + bs.length) % bs.length];
    n.focus();
    n.click();
  });
  $("selTr").addEventListener("change", (e) => {
    mutable.selCfg.tr = e.target.value;
    savePrefs();
  });
  $("selPriv").addEventListener("click", () => invoke("sel_open_privacy").catch(() => {}));
}
