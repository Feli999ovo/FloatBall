import { stop } from "./agents.js";
import { closeShareView } from "./attach.js";
import { closeSkillDraft } from "./handoff.js";
import { closeFeedPermission } from "./selection.js";
import { $, appWin, ask, mutable, S, T } from "./state.js";
import { showView } from "./views.js";
import { collapse, grabBackdrop, monitorArea } from "./window.js";

export let clamp01;

// 自适应玻璃：原型的玻璃把身后的桌面直接透出来（×0.9），放到白色的文档、网页上，奶白的眼睛和占位字就看不见了。
// 像系统的液态玻璃那样按身后的亮度加深：胶囊用展开前截的那张底图量它那一条，小球收起后量窗口四角（四角在球外，是桌面本身）。
// 只有 Windows 有底图；拿不到时用一个中等的深度，保证白底上也看得清。
export function lumOf(url, pick) {
  return new Promise((res) => {
    const im = new Image();
    im.onload = () => {
      try {
        const c = document.createElement("canvas");
        c.width = im.width;
        c.height = im.height;
        const g = c.getContext("2d");
        g.drawImage(im, 0, 0);
        let t = 0,
          n = 0;
        for (const [x, y, w, h] of pick(im.width, im.height)) {
          const d = g.getImageData(
            Math.round(x),
            Math.round(y),
            Math.max(1, Math.round(w)),
            Math.max(1, Math.round(h)),
          ).data;
          for (let i = 0; i < d.length; i += 4) {
            t += (0.2126 * d[i] + 0.7152 * d[i + 1] + 0.0722 * d[i + 2]) / 255;
            n++;
          }
        }
        res(n ? t / n : null);
      } catch (_) {
        res(null);
      }
    };
    im.onerror = () => res(null);
    im.src = url;
  });
}

export async function measureCap(bd, w) {
  const r = $("#capsule").getBoundingClientRect();
  const L = bd
    ? await lumOf(bd, (iw) => {
        const k = iw / w;
        return [[r.left * k, r.top * k, r.width * k, r.height * k]];
      })
    : null;
  document.documentElement.style.setProperty(
    "--capk",
    String(L == null ? 0.3 : +clamp01(0.1 + (L - 0.45) * 1.1, 0.1, 0.55).toFixed(3)),
  );
}

export async function measureOrb(p) {
  if (S.open || mutable.morphing) return;
  const a = await monitorArea();
  const bd = await grabBackdrop({ x: p.x, y: p.y, w: mutable.SMALL, h: mutable.SMALL }, a);
  const L = bd
    ? await lumOf(bd, (iw, ih) => {
        const q = Math.max(1, iw * 0.15);
        return [
          [0, 0, q, q],
          [iw - q, 0, q, q],
          [0, ih - q, q, q],
          [iw - q, ih - q, q, q],
        ];
      })
    : null;
  document.documentElement.style.setProperty(
    "--orbk",
    String(L == null ? 0.12 : +clamp01((L - 0.5) * 0.8, 0, 0.35).toFixed(3)),
  );
}

export function initWindowGlass() {
  clamp01 = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
  // 点别处：没在生成、没固定、输入框是空的 → 收起
  // 右键菜单是另一个小窗口，打开时焦点在它那里：这时别把卡片收起来
  T.event.listen("ctx", (e) => {
    S.ctx = !!e.payload;
  });
  appWin.onFocusChanged(({ payload: focused }) => {
    if (focused || !S.open || S.pinned || S.busy || S.ctx || S.sending) return;
    if (ask.value.trim() || S.attach.length) return;
    setTimeout(() => {
      if (!document.hasFocus() && !S.ctx) collapse();
    }, 150);
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && S.open) {
      if (!$("#feedPermission").hidden) closeFeedPermission();
      else if (!$("#vShare").hidden) closeShareView();
      else if (!$("#vSkill").hidden) closeSkillDraft();
      else if (S.busy) stop();
      else if (!$("#vChat").hidden) collapse();
      else showView("chat");
    }
  });
}
