import { captureAsk } from "./attach.js";
import { newChat } from "./conversation.js";
import { peekBox } from "./peek.js";
import { $, body, mutable, orb, PAD, S } from "./state.js";
import { vcBox } from "./voice.js";
import { docked, expand, monitorArea, peel, press, reshape, winPos } from "./window.js";

export let HV_ACT, hv, hvQ, hvTip, hvIds, hvOk;

export async function hvShow() {
  const ids = hvIds();
  mutable.morphing = true;
  try {
    const p = await winPos(),
      a = await monitorArea();
    const ORB = mutable.SMALL - PAD * 2,
      BS = 34,
      ring = ORB / 2 + 12 + BS / 2,
      E = Math.ceil(ring + BS / 2 + 6);
    const side = E * 2,
      cxs = p.x + mutable.SMALL / 2,
      cys = p.y + mutable.SMALL / 2; // 小球中心（屏幕坐标）
    hv.box = { p };
    // 弧线朝屏幕中间；上下放不下就转一点
    const right = cxs < a.x + a.w / 2,
      base = right ? 0 : Math.PI,
      sg = right ? 1 : -1,
      n = ids.length;
    const step = 2 * Math.asin(Math.min(1, (BS + 8) / 2 / ring));
    const tilts = [0];
    for (let k = 1; k <= 7; k++) {
      const t = k * 0.18;
      tilts.push(cys < a.y + a.h / 2 ? t : -t, cys < a.y + a.h / 2 ? -t : t);
    }
    let pts;
    for (const t of tilts) {
      pts = [];
      let fit = true;
      for (let i = 0; i < n; i++) {
        const ang = base + sg * (t + (i - (n - 1) / 2) * step),
          bx = Math.cos(ang) * ring,
          by = Math.sin(ang) * ring;
        if (cys + by < a.y + 20 || cys + by > a.y + a.h - 20 || cxs + bx < a.x + 20 || cxs + bx > a.x + a.w - 20)
          fit = false;
        pts.push({ x: bx, y: by });
      }
      if (fit) break;
    }
    await reshape(side, side, cxs - E, cys - E, true, () => {
      body.classList.add("hovering");
      $("#app").style.padding = `${E - ORB / 2}px 0 0 ${E - ORB / 2}px`;
      hvQ.innerHTML = "";
      ids.forEach((id, i) => {
        const [label, path, run] = HV_ACT[id],
          b = document.createElement("button");
        b.type = "button";
        b.className = "x-hvb";
        b.dataset.id = id;
        b.setAttribute("aria-label", label);
        b.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true">${path}</svg>`;
        b.style.left = E + "px";
        b.style.top = E + "px";
        b.style.setProperty("--dx", pts[i].x.toFixed(1) + "px");
        b.style.setProperty("--dy", pts[i].y.toFixed(1) + "px");
        b.style.setProperty("--dl", i * 40 + "ms");
        b.addEventListener("click", async () => {
          await hvHide();
          run();
        });
        const tip = () => {
          hvTip.textContent = label;
          const left = pts[i].x < 0,
            tw = hvTip.offsetWidth,
            th = hvTip.offsetHeight;
          hvTip.style.left =
            Math.max(2, Math.min(side - tw - 2, left ? E + pts[i].x - 23 - tw : E + pts[i].x + 23)) + "px";
          hvTip.style.top = Math.max(2, Math.min(side - th - 2, E + pts[i].y - th / 2)) + "px";
          hvTip.classList.add("on");
        };
        b.addEventListener("pointerenter", tip);
        b.addEventListener("focus", tip);
        b.addEventListener("pointerleave", () => hvTip.classList.remove("on"));
        b.addEventListener("blur", () => hvTip.classList.remove("on"));
        hvQ.appendChild(b);
      });
    });
    void hvQ.offsetWidth;
    hvQ.classList.add("on");
    hvQ.setAttribute("aria-hidden", "false");
    hv.shown = true;
  } finally {
    mutable.morphing = false;
  }
}

// 正在收起时再调用，返回同一个收起过程：点小球展开时要等小泡收完，
// 否则收起晚 200ms 才把窗口缩回小球大小，会把刚展开的卡片截掉（Windows 真机截图里见过）
export function hvHide() {
  if (!hv.shown) return hv.hiding || Promise.resolve();
  hv.shown = false;
  clearTimeout(hv.hideT);
  hvQ.classList.remove("on");
  hvQ.setAttribute("aria-hidden", "true");
  hvTip.classList.remove("on");
  hv.hiding = (async () => {
    await new Promise((r) => setTimeout(r, 200));
    mutable.morphing = true;
    try {
      await reshape(mutable.SMALL, mutable.SMALL, hv.box.p.x, hv.box.p.y, true, () => {
        body.classList.remove("hovering");
        $("#app").style.padding = "";
        hvQ.innerHTML = "";
      });
    } finally {
      mutable.morphing = false;
    }
    // 贴边时露出来的球：指针已经不在球上就贴回去
    if (docked && orb.classList.contains("peel") && !orb.matches(":hover")) peel(false);
  })().finally(() => {
    hv.hiding = null;
  });
  return hv.hiding;
}

export function initHover() {
  /* ───────── 悬停快捷入口（原型的 hoverEntries）：指针停在球上，旁边沿弧线弹出几个小泡 ─────────
     收起时窗口只有小球那么大，放不下小泡：弹出时把窗口扩成以小球为中心的方块，小球留在原地；收起时缩回。 */
  HV_ACT = {
    shot: [
      "截屏提问",
      '<path d="M4 8V5.5A1.5 1.5 0 0 1 5.5 4H8M16 4h2.5A1.5 1.5 0 0 1 20 5.5V8M20 16v2.5a1.5 1.5 0 0 1-1.5 1.5H16M8 20H5.5A1.5 1.5 0 0 1 4 18.5V16"/><circle cx="12" cy="12" r="2.6"/>',
      () => captureAsk(),
    ],
    new: [
      "新对话",
      '<path d="M5 4.5h14a2 2 0 0 1 2 2v8.5a2 2 0 0 1-2 2h-7l-4.5 3.5V17H5a2 2 0 0 1-2-2V6.5a2 2 0 0 1 2-2z"/><path d="M12 8v5.5M9.25 10.75h5.5"/>',
      () => {
        newChat();
        expand("chat");
      },
    ],
    hist: ["历史记录", '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>', () => expand("hist")],
    tasks: [
      "任务列表",
      '<path d="M10 6.5h10M10 12h10M10 17.5h10M3.8 6.5l1.2 1.2 2.2-2.4M3.8 12l1.2 1.2 2.2-2.4M3.8 17.5l1.2 1.2 2.2-2.4"/>',
      () => expand("task"),
    ],
  };
  hv = { shown: false, t: 0, hideT: 0, box: null };
  hvQ = $("#hvq");
  hvTip = $("#hvTip");
  hvIds = () => {
    const h = S.cfg && S.cfg.hover;
    return h && h.on
      ? h.order
          .filter((o) => o.show && HV_ACT[o.id])
          .slice(0, 4)
          .map((o) => o.id)
      : [];
  };
  hvOk = () =>
    hvIds().length &&
    !mutable.vc &&
    !vcBox &&
    !S.open &&
    !mutable.morphing &&
    !peekBox &&
    !press &&
    !orb.className.includes("s-sleep");
  orb.addEventListener("pointerenter", () => {
    clearTimeout(hv.hideT);
    if (hv.shown || hv.t || !hvOk()) return;
    hv.t = setTimeout(
      () => {
        hv.t = 0;
        if (hvOk() && orb.matches(":hover")) hvShow();
      },
      (S.cfg.hover.delay || 350) + (docked ? 120 : 0),
    );
  });
  orb.addEventListener("pointerleave", () => {
    clearTimeout(hv.t);
    hv.t = 0;
  });
  orb.addEventListener(
    "pointerdown",
    () => {
      clearTimeout(hv.t);
      hv.t = 0;
      if (hv.shown) hvHide();
    },
    true,
  );
  // 指针离开窗口（小球和小泡以外的地方都在窗口外）就收起
  document.documentElement.addEventListener("pointerleave", () => {
    if (hv.shown) {
      clearTimeout(hv.hideT);
      hv.hideT = setTimeout(hvHide, 400);
    }
  });
  document.documentElement.addEventListener("pointerenter", () => clearTimeout(hv.hideT));
  addEventListener("keydown", (e) => {
    if (e.key === "Escape" && hv.shown) hvHide();
  });
}
