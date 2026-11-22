import { KEY_DEF, keyVal, savePrefs } from "./appearance.js";
import { $, toast } from "./state.js";

export let keyRec, isMac, SYM, KEYN, SHOWN, warn, SEL_ACT, SEL_MAX;

export function keyName(e) {
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3);
  if (/^Digit\d$/.test(e.code)) return e.code.slice(5);
  if (/^F\d{1,2}$/.test(e.code)) return e.code;
  return KEYN[e.code] || null;
}

export function showCombo(v) {
  if (!v) return "未设置";
  if (v === "RightAlt") return isMac ? "右 ⌥（按住）" : "右 Alt（按住）";
  const parts = v.split("+");
  const key = parts.pop();
  const mods = parts.map((p) =>
    isMac ? SYM[p] || p : p === "Super" ? "Win" : p === "CommandOrControl" ? "Ctrl" : p === "Control" ? "Ctrl" : p,
  );
  return mods.concat(SHOWN[key] || key).join(isMac ? " " : "+");
}

export function renderKeys() {
  const ul = $("keyList");
  if (!ul.children.length)
    KEY_DEF.forEach(([k, label]) => {
      const li = document.createElement("li");
      li.dataset.k = k;
      li.innerHTML = `<b></b><kbd class="x-kc" aria-live="polite"></kbd><button type="button" class="x-sb"></button><p class="x-kw"></p>`;
      li.children[0].textContent = label;
      ul.appendChild(li);
    });
  ul.querySelectorAll("li").forEach((li) => {
    const k = li.dataset.k,
      on = keyRec === k;
    li.classList.toggle("rec", on);
    li.querySelector(".x-kc").textContent = on ? "按下新的组合键…" : showCombo(keyVal[k]);
    const b = li.querySelector(".x-sb");
    b.textContent = on ? "取消" : "录制";
    b.setAttribute("aria-label", (on ? "取消录制：" : "录制：") + li.querySelector("b").textContent);
  });
}

export function stopRec() {
  keyRec = null;
  renderKeys();
}

export function initShortcuts() {
  keyRec = null;
  isMac = /Mac/.test(navigator.platform);
  SYM = { Control: "⌃", Alt: "⌥", Shift: "⇧", Super: "⌘", CommandOrControl: isMac ? "⌘" : "⌃" };
  KEYN = {
    Space: "Space",
    Enter: "Enter",
    Tab: "Tab",
    Backquote: "`",
    Minus: "-",
    Equal: "=",
    Comma: ",",
    Period: ".",
    Slash: "/",
    Semicolon: ";",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
  };
  SHOWN = { Up: "↑", Down: "↓", Left: "←", Right: "→", Enter: "↩", Tab: "⇥" };
  $("keyList").addEventListener("click", (e) => {
    const b = e.target.closest(".x-sb");
    if (!b) return;
    const k = b.closest("li").dataset.k;
    keyRec = keyRec === k ? null : k;
    renderKeys();
    b.focus({ preventScroll: true });
  });
  warn = (k, t) => {
    const p = $("keyList").querySelector(`[data-k="${k}"] .x-kw`);
    p.textContent = t || "";
  };
  document.addEventListener(
    "keydown",
    async (e) => {
      if (!keyRec) return;
      e.preventDefault();
      e.stopPropagation();
      const k = keyRec;
      if (e.key === "Escape") {
        warn(k);
        return stopRec();
      }
      if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return;
      const key = keyName(e),
        mods = [e.ctrlKey && "Control", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
      if (!key) return warn(k, "这个键不能用作快捷键");
      if (!mods.length) return warn(k, "至少带一个 Ctrl、Alt 或 Shift");
      const combo = mods.concat(key).join("+"),
        same = KEY_DEF.find(([o]) => o !== k && keyVal[o] === combo);
      if (same) return warn(k, `与「${same[1]}」相同，换一个`);
      keyVal[k] = combo;
      keyRec = null;
      warn(k);
      renderKeys();
      if (!(await savePrefs())) {
        warn(k, "被别的程序占用了，换一个");
      }
      const b = $("keyList").querySelector(`[data-k="${k}"] .x-sb`);
      if (b) b.focus({ preventScroll: true });
    },
    true,
  );
  $("keyReset").addEventListener("click", async () => {
    keyRec = null;
    KEY_DEF.forEach(([k, , d]) => {
      keyVal[k] = d;
      warn(k);
    });
    renderKeys();
    if (await savePrefs()) toast("已恢复默认快捷键");
  });
  // 划词（原型的 selCfg）：开关、出现方式、工具栏按钮（勾选/拖动/上下移，前 6 个在工具栏上）、翻译成、排除的应用
  SEL_ACT = {
    ask: "问问它",
    explain: "解释",
    tr: "翻译",
    sum: "总结",
    rew: "改写",
    copy: "复制",
    read: "朗读",
    search: "搜索",
    expand: "扩写",
    shrink: "缩写",
  };
  SEL_MAX = 6;
}
