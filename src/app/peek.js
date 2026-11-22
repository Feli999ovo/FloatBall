import { askTmp, newChat } from "./conversation.js";
import { hoOpen } from "./handoff.js";
import { hv } from "./hover.js";
import { $, body, mutable, PAD, S } from "./state.js";
import { toast } from "./toast.js";
import { showView } from "./views.js";
import { expand, focusAsk, monitorArea, reshape, winPos } from "./window.js";

export let PEEK_W, PEEK_H, peekTimer, peekBox, peekFull, peekGo, moreDd, moreBtn;

// 点小泡时要回到的对话（后台任务的提示）
export async function showPeek(text, err, go) {
  const t = String(text).replace(/\s+/g, " ").trim();
  if (!t || S.open || mutable.morphing || peekBox || hv.shown) return;
  peekFull = err && t.length > 20 ? t : "";
  peekGo = go || null;
  mutable.morphing = true;
  try {
    const p = await winPos(),
      a = await monitorArea();
    const toL = p.x + mutable.SMALL / 2 > a.x + a.w / 2,
      below = p.y - PEEK_H < a.y + 8;
    const w = mutable.SMALL + PEEK_W,
      h = mutable.SMALL + PEEK_H;
    const x = toL ? p.x - PEEK_W : p.x,
      y = below ? p.y : p.y - PEEK_H;
    peekBox = { p, toL, below };
    const el = $("#peek");
    el.textContent = t.length <= 20 ? t : t.slice(0, 20) + "…";
    await reshape(w, h, x, y, toL || !below, () => {
      // 小球留在原来的位置：窗口往哪边长，小球就靠哪边
      body.classList.add("peeking");
      $("#app").style.padding =
        `${below ? PAD : PAD + PEEK_H}px ${toL ? PAD : PAD + PEEK_W}px ${below ? PAD + PEEK_H : PAD}px ${toL ? PAD + PEEK_W : PAD}px`;
      el.style.left = toL ? "auto" : mutable.SMALL - 4 + "px";
      el.style.right = toL ? mutable.SMALL - 4 + "px" : "auto";
      el.style.top = below ? mutable.SMALL - 4 + "px" : "auto";
      el.style.bottom = below ? "auto" : mutable.SMALL - 4 + "px";
      el.style.borderRadius = below
        ? toL
          ? "14px 4px 14px 14px"
          : "4px 14px 14px 14px"
        : toL
          ? "14px 14px 4px 14px"
          : "14px 14px 14px 4px";
      el.style.transformOrigin = (toL ? "100% " : "0 ") + (below ? "0" : "100%");
    });
    el.classList.add("on");
  } finally {
    mutable.morphing = false;
  }
  clearTimeout(peekTimer);
  peekTimer = setTimeout(hidePeek, 6000);
}

export async function hidePeek() {
  clearTimeout(peekTimer);
  const box = peekBox;
  if (!box) return;
  $("#peek").classList.remove("on");
  await new Promise((r) => setTimeout(r, 220));
  if (S.open || mutable.morphing) {
    peekBox = null;
    return;
  } // 已经点开了：展开会自己排版
  mutable.morphing = true;
  try {
    await reshape(mutable.SMALL, mutable.SMALL, box.p.x, box.p.y, box.toL || !box.below, () => {
      body.classList.remove("peeking");
      $("#app").style.padding = "";
    });
  } finally {
    mutable.morphing = false;
    peekBox = null;
  }
}

export function setPin(v) {
  S.pinned = v;
  $("#pinMark").hidden = !v;
  moreDd.querySelector("[data-cm=pin]").setAttribute("aria-checked", v);
  toast(v ? "已钉住，点别处不会收起" : "取消钉住");
}

export function moreOpen(v) {
  moreDd.classList.toggle("on", v);
  moreBtn.setAttribute("aria-expanded", v);
  if (v) {
    const c = $("#card").getBoundingClientRect(),
      b = moreBtn.getBoundingClientRect();
    moreDd.style.left = Math.max(6, b.right - c.left - moreDd.offsetWidth) + "px";
    moreDd.style.top = b.bottom - c.top + 4 + "px";
    moreDd.querySelector("button").focus();
  }
}

export function initPeek() {
  /* ───────── 气泡：卡片收起时回复完成，在小球旁冒一句（取自参考版本） ─────────
     收起时窗口只有小球那么大，放不下气泡：冒泡时临时把窗口扩大，小球留在原地，消失后再缩回。 */
  PEEK_W = 260;
  PEEK_H = 60;
  peekTimer = 0;
  peekBox = null;
  // err：出错提示。小泡只放得下 20 个字，点开后把整句（去哪设置）完整显示出来
  peekFull = "";
  peekGo = null;
  $("#peek").onclick = async () => {
    const box = peekBox;
    clearTimeout(peekTimer);
    $("#peek").classList.remove("on");
    if (box) {
      mutable.morphing = true;
      try {
        await reshape(mutable.SMALL, mutable.SMALL, box.p.x, box.p.y, false, () => {
          body.classList.remove("peeking");
          $("#app").style.padding = "";
        });
      } finally {
        mutable.morphing = false;
        peekBox = null;
      }
    }
    const full = peekFull,
      go = peekGo;
    peekFull = "";
    peekGo = null;
    if (go) go();
    else await expand("chat");
    if (full) toast(full);
  };
  $("#newBtn").onclick = () => {
    newChat();
    showView("chat");
    focusAsk();
  };
  $("#histBtn").onclick = () => showView($("#vHist").hidden ? "hist" : "chat");
  // 「⋯」下拉（照原型 chatMoreDd）：钉住。钉住后标题旁出现实心图钉，点它取消
  moreDd = $("#moreDd");
  moreBtn = $("#moreBtn");
  moreBtn.onclick = (e) => {
    e.stopPropagation();
    moreOpen(!moreDd.classList.contains("on"));
  };
  moreDd.addEventListener("click", (e) => {
    const b = e.target.closest("[data-cm]");
    if (!b) return;
    moreOpen(false);
    if (b.dataset.cm === "pin") setPin(!S.pinned);
    if (b.dataset.cm === "tmp") askTmp(!S.conv.tmp);
    if (b.dataset.cm === "ho") hoOpen();
  });
}
