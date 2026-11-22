import { agEdit } from "./agent-editor.js";
import { AG_TPL, mk } from "./agent-form.js";
import { $, invoke, mutable, toast } from "./state.js";

export let hoScanned, hoPrefsPending, hoSaveQueue;

export function renderHandoffPrefs() {
  if (!hoPrefsPending) {
    const h = mutable.cfg.handoff || {};
    if (document.activeElement !== $("hoDefCmd")) $("hoDefCmd").value = h.custom || "";
    $("hoDefHint").setAttribute("aria-checked", h.hint !== false);
  }
  if (!hoScanned) scanHandoffTools();
}

export async function scanHandoffTools() {
  const b = $("hoReScan"),
    status = $("hoScanR"),
    list = $("hoTools");
  if (b.disabled) return;
  hoScanned = true;
  b.disabled = true;
  status.textContent = "正在扫描";
  list.textContent = "";
  try {
    const tools = (await invoke("handoff_scan")).tools;
    tools.forEach((t) => {
      const row = mk("li", "x-hotool"),
        name = mk("b", ""),
        bin = mk("code", ""),
        direct = mk("small", "");
      name.textContent = t.name;
      bin.textContent = t.bin;
      direct.textContent = t.direct ? "可直接带入说明" : "打开后粘贴说明";
      row.append(name, direct, bin);
      list.appendChild(row);
    });
    status.textContent = tools.length ? `找到 ${tools.length} 个工具` : "未找到工具，可用自定义命令";
  } catch (e) {
    status.textContent = "扫描失败：" + e;
  } finally {
    b.disabled = false;
  }
}

export function saveHandoffPrefs() {
  const prefs = { custom: $("hoDefCmd").value, hint: $("hoDefHint").getAttribute("aria-checked") === "true" };
  hoPrefsPending++;
  hoSaveQueue = hoSaveQueue
    .catch(() => {})
    .then(async () => {
      try {
        await invoke("save_handoff_prefs", prefs);
        mutable.cfg.handoff = { ...(mutable.cfg.handoff || {}), ...prefs };
        if (hoPrefsPending === 1) toast("已保存");
      } catch (e) {
        toast(String(e));
      } finally {
        hoPrefsPending--;
        if (!hoPrefsPending) renderHandoffPrefs();
      }
    });
  return hoSaveQueue;
}

export function initHandoff() {
  // 交给终端：扫描只读；编辑偏好走专门的保存命令，不复用 handoff_go。
  hoScanned = false;
  hoPrefsPending = 0;
  hoSaveQueue = Promise.resolve();
  $("hoReScan").onclick = scanHandoffTools;
  $("hoDefCmd").onchange = saveHandoffPrefs;
  $("hoDefHint").onclick = () => {
    const b = $("hoDefHint");
    b.setAttribute("aria-checked", b.getAttribute("aria-checked") !== "true");
    saveHandoffPrefs();
  };
  $("agAdd").addEventListener("click", () => {
    const box = $("agTpl"),
      on = box.hidden;
    box.hidden = !on;
    $("agAdd").setAttribute("aria-expanded", on);
    if (!on) return;
    box.innerHTML = "<p>从模板开始</p>";
    AG_TPL.forEach((t) => {
      const b = mk("button", "x-agtb");
      b.type = "button";
      b.innerHTML = "<b></b><small></small>";
      b.firstChild.textContent = t.k;
      b.lastChild.textContent = t.d;
      b.addEventListener("click", () => agEdit(null, t.a));
      box.appendChild(b);
    });
    box.querySelector("button").focus({ preventScroll: true });
  });
}
