import { mood } from "./orb.js";
import { peekBox, showPeek } from "./peek.js";
import { $, appWin, ask, invoke, orb, S, T } from "./state.js";
import { toast, toastErr } from "./toast.js";
import { showView } from "./views.js";
import { expand, focusAsk } from "./window.js";

export let SHARE_MS, sharingConvs, shareList, shareStatus, shareSettings, sharePicker;

/* ───────── 附件：截图、拖进来的文件 ───────── */
export function renderAttach() {
  const box = $("#attach");
  box.innerHTML = "";
  S.attach.forEach((a, i) => {
    const c = document.createElement("div");
    c.className = "chip";
    if (a.kind === "image") {
      const im = document.createElement("img");
      im.src = a.url;
      im.alt = "截图";
      c.appendChild(im);
    }
    const sp = document.createElement("span");
    sp.textContent = a.kind === "image" ? "截图" : a.name;
    c.appendChild(sp);
    const x = document.createElement("button");
    x.type = "button";
    x.textContent = "×";
    x.setAttribute("aria-label", "移除");
    x.onclick = () => {
      S.attach.splice(i, 1);
      renderAttach();
    };
    c.appendChild(x);
    box.appendChild(c);
  });
  box.hidden = S.attach.length === 0;
  if (!S.attach.some((a) => a.kind === "file")) ask.placeholder = "有什么可以帮你？";
}

export async function captureAsk(command = "capture_screen") {
  const conv = S.conv;
  if (S.sending) return;
  try {
    mood("think");
    const url = await invoke(command);
    if (conv !== S.conv) return;
    S.attach = S.attach.filter((a) => a.kind !== "image");
    S.attach.push({ kind: "image", url });
    renderAttach();
    await expand("chat");
    if (S.cfg?.lead_vision === false) toast("当前模型设置为不支持图片，截图不会发给它（设置里可改）");
    mood("listen");
    ask.focus();
  } catch (e) {
    if (conv !== S.conv) return;
    if (e === "screen_permission") await sharePermission();
    else {
      await expand("chat");
      toastErr(e);
    }
  }
}

export function activeShare(conv) {
  const share = sharingConvs.get(conv);
  return share && share.until > Date.now() ? share : null;
}

export function syncShare() {
  const share = activeShare(S.conv);
  orb.classList.toggle("sharing", [...sharingConvs.keys()].some(activeShare));
  $("#shareBanner").hidden = !share || $("#vChat").hidden;
  $("#shareName").textContent = share ? "正在共享：" + share.title : "";
}

export function stopSharing(conv, expired = false) {
  clearTimeout(sharingConvs.get(conv)?.timer);
  sharingConvs.delete(conv);
  syncShare();
  if (!expired) toast("已停止共享");
  else if (S.open) toast("共享已满 10 分钟，已自动结束");
  else if (!peekBox) showPeek("共享已满 10 分钟，已自动结束", false);
}

export function closeShareView() {
  sharePicker++;
  shareList.replaceChildren();
  showView("chat");
  focusAsk();
}

export function showSharePermission() {
  shareList.replaceChildren();
  shareStatus.textContent = "请在系统设置 → 隐私与安全性 → 屏幕录制中允许悬浮球，然后重启应用。";
  shareSettings.hidden = false;
  shareSettings.focus();
}

export async function sharePermission() {
  await expand("share");
  showSharePermission();
}

export async function toggleShare() {
  const conv = S.conv;
  if (activeShare(conv)) {
    stopSharing(conv);
    return;
  }
  if (!$("#vShare").hidden) {
    closeShareView();
    return;
  }
  const request = ++sharePicker;
  await expand("share");
  await appWin.setFocus();
  const visible = () => sharePicker === request && S.open && conv === S.conv && !$("#vShare").hidden;
  if (!visible()) return;
  shareSettings.hidden = true;
  shareList.replaceChildren();
  shareStatus.textContent = "正在查找可共享的窗口和屏幕";
  $("#shareCancel").focus();
  try {
    const sources = await invoke("share_sources");
    if (!visible()) return;
    shareStatus.textContent = sources.length ? "每次提问时截取一张，10 分钟后自动停止。" : "没有找到可共享的窗口或屏幕";
    for (const source of sources) {
      const b = document.createElement("button");
      b.type = "button";
      b.title = source.title;
      if (source.thumbnail) {
        const img = document.createElement("img");
        img.alt = "";
        img.src = source.thumbnail;
        b.appendChild(img);
      }
      const title = document.createElement("span");
      title.textContent = source.title;
      b.appendChild(title);
      b.onclick = () => {
        if (!visible()) return;
        const timer = setTimeout(() => stopSharing(conv, true), SHARE_MS);
        sharingConvs.set(conv, { target: source.target, title: source.title, until: Date.now() + SHARE_MS, timer });
        shareList.replaceChildren();
        closeShareView();
      };
      shareList.appendChild(b);
    }
    shareList.querySelector("button")?.focus();
  } catch (e) {
    if (!visible()) return;
    if (e === "screen_permission") showSharePermission();
    else shareStatus.textContent = String(e);
  }
}

export function initAttach() {
  SHARE_MS = 10 * 60 * 1000;
  sharingConvs = new Map();
  shareList = $("#shareList");
  shareStatus = $("#shareStatus");
  shareSettings = $("#shareSettings");
  sharePicker = 0;
  $("#shareCancel").onclick = closeShareView;
  $("#shareStop").onclick = () => stopSharing(S.conv);
  shareSettings.onclick = () => invoke("open_screen_privacy").catch(toastErr);
  T.webview.getCurrentWebview().onDragDropEvent(async (e) => {
    const p = e.payload;
    if (p.type === "enter" || p.type === "over") {
      if (!orb.className.includes("s-feed")) mood("feed");
      return;
    }
    if (p.type === "leave") {
      mood("idle");
      return;
    }
    if (p.type === "drop") {
      let n = 0;
      for (const path of p.paths || []) {
        try {
          const f = await invoke("read_text_file", { path });
          S.attach.push({ kind: "file", name: f.name, text: f.text });
          n++;
        } catch (err) {
          toast(String(err));
        }
      }
      renderAttach();
      if (n) {
        mood("done", 1200);
        await expand("chat");
        ask.placeholder = "想让我对这个文件做什么？";
      } else mood("idle");
    }
  });
}
