import { agBack, gotoAgent } from "./agent-editor.js";
import { renderAgents } from "./agents.js";
import { hvRender, KEY_DEF, keyVal, mdsFig, radio, showGlass } from "./appearance.js";
import { lookupCaps } from "./capabilities.js";
import { renderLib } from "./models.js";
import { renderPerm } from "./permissions.js";
import { fList, models } from "./providers.js";
import { renderSel } from "./selection.js";
import { keyRec, renderKeys } from "./shortcuts.js";
import { renderAgentSkills, skills } from "./skills.js";
import { $, invoke, jslog, mutable, T, toast, win } from "./state.js";
import { showTab } from "./tabs.js";
import { renderVoice } from "./voice.js";

export let sbd;

/* ───────── 读配置 ───────── */
export async function reload() {
  mutable.cfg = await invoke("get_config");
  $("usdCny").value = String(mutable.cfg.usd_cny || 7.2);
  ["bTok", "bStep", "bPar"].forEach((id, i) => {
    $(id).value = String(mutable.cfg.limits[["tokens", "steps", "parallel"][i]]);
  });
  KEY_DEF.forEach(([k]) => (keyVal[k] = mutable.cfg[k] || ""));
  if (!keyRec) renderKeys();
  radio("lkSize", mutable.cfg.orb_size);
  radio("lkEye", mutable.cfg.eye);
  $("lkGlass").value = mutable.cfg.glass;
  showGlass();
  $("lkIdle").value = String(mutable.cfg.idle_ms ?? 600000);
  if ($("lkIdle").selectedIndex < 0) $("lkIdle").value = "600000";
  const hv = mutable.cfg.hover || { on: true, delay: 350, order: [] };
  mutable.hvOrder = hv.order.map((o) => ({ ...o }));
  $("hvOn").setAttribute("aria-checked", hv.on !== false);
  radio("hvSpd", hv.delay || 350);
  {
    const a = document.activeElement,
      li = a && a.closest && a.closest("#hvList li"); // 重新渲染时焦点留在原来那一项
    hvRender(li && li.dataset.id, a && a.dataset && a.dataset.d ? `[data-d=${a.dataset.d}]` : "input");
  }
  if (mutable.cfg.sel) {
    mutable.selCfg = JSON.parse(JSON.stringify(mutable.cfg.sel));
    renderSel();
  }
  if (mutable.cfg.voice) {
    mutable.vcCfg = { ...mutable.cfg.voice };
    renderVoice();
  }
  radio("mdsWhere", mutable.cfg.screen || "mouse");
  $("mdsSlide").setAttribute("aria-checked", mutable.cfg.screen_slide !== false);
  mdsFig(false);
  $("lkSys").setAttribute("aria-checked", !!mutable.cfg.follow_system);
  $("lkDock").setAttribute("aria-checked", mutable.cfg.auto_dock !== false);
  $("lkCalm").setAttribute("aria-checked", !!mutable.cfg.calm);
  if (!$("paneLib").hidden) renderLib();
  if (!$("paneAgents").hidden) renderAgents();
  if (!$("panePerm").hidden) renderPerm();
}

export async function placeBackdrop(pos) {
  if (!sbd) return;
  const s = await win.scaleFactor();
  const p = pos || (await win.outerPosition());
  const inside = p.x >= sbd.x - 4 && p.y >= sbd.y - 4 && p.x < sbd.x + sbd.w && p.y < sbd.y + sbd.h;
  document.body.classList.toggle("bd", inside);
  const st = document.documentElement.style;
  st.setProperty("--sbx", (sbd.x - p.x) / s + "px");
  st.setProperty("--sby", (sbd.y - p.y) / s + "px");
  st.setProperty("--sbw", sbd.w / s + "px");
  st.setProperty("--sbh", sbd.h / s + "px");
}

export async function initBackdrop() {
  try {
    sbd = await invoke("settings_backdrop");
  } catch (_) {
    sbd = null;
  }
  if (!sbd) return;
  document.documentElement.style.setProperty("--sbd", 'url("' + sbd.url + '")');
  await placeBackdrop();
  Promise.resolve(win.onMoved((e) => placeBackdrop(e.payload))).catch(() => {});
}

export function initStartup() {
  // 对话里点了「总是允许」，设置窗口开着时跟着刷新
  T.event.listen("config", () => reload().then(() => document.body.classList.toggle("fx", mutable.cfg.fx === true)));
  T.event.listen("skills:changed", async () => {
    await reload();
    if (mutable.agEd && !$("paneAgEd").hidden) {
      mutable.agEd.d.skills = mutable.agEd.d.skills.filter((id) => skills().some((s) => s.id === id));
      renderAgentSkills($("aeSkills"), mutable.agEd.d);
    }
  });
  T.event.listen("catalog-updated", async () => {
    await reload();
    if (!$("paneModel").hidden && (!mutable.detected || mutable.detected.src !== "manual")) lookupCaps(0);
  });
  $("setClose").addEventListener("click", () => win.close());
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || e.defaultPrevented) return;
    if (!fList.hidden) {
      fList.hidden = true;
      return;
    }
    if (mutable.agEd) {
      agBack(mutable.agEd.a && mutable.agEd.a.id);
      return;
    }
    win.close();
  });
  // Windows 的玻璃：和对话卡片同一种。窗口出现前程序截下整块屏幕、缩小模糊（settings_backdrop），
  // 这里把它按窗口在屏幕上的位置垫在墨层下面，拖动时跟着挪；挪到别的屏幕上没有图，就用不透明的墨色底。
  sbd = null;
  (async () => {
    try {
      await reload();
      document.body.classList.toggle("fx", mutable.cfg.fx === true); // 系统毛玻璃可用（Windows 透明效果没关、或 macOS）
      document.body.classList.toggle("win", mutable.cfg.platform === "windows");
      if (!mutable.cfg.fx) initBackdrop();
      // 卡片的透明度只在 Windows 上起作用（别的系统没有卡片毛玻璃），别处不显示这一项
      $("lkGlassF").hidden = mutable.cfg.platform !== "windows";
      // 原型：首次引导「先设置一个模型吧 → 去设置」直接打开添加模型，保存后关掉设置
      if (!models().length) {
        mutable.fromTip = true;
        showTab("add");
      } else showTab("model");
      // 对话里 Agent 弹卡点「更换」：直接打开那个 Agent 的编辑页
      const goto = await invoke("settings_goto").catch(() => null);
      if (goto && models().length) gotoAgent(goto);
      Promise.resolve(
        T.event.listen("settings:goto", async (e) => {
          await reload();
          invoke("settings_goto").catch(() => {});
          gotoAgent(e.payload);
        }),
      ).catch(() => {});
    } catch (e) {
      toast(String(e));
      jslog(e);
    }
    invoke("catalog_refresh").catch(() => {});
  })();
}
