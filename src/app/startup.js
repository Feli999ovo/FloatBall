import { applyLook, needsSetup } from "./appearance.js";
import { updateEmpty } from "./conversation.js";
import { screenCheck } from "./monitors.js";
import { mood } from "./orb.js";
import { invoke, S } from "./state.js";
import { settle } from "./window.js";

export function initStartup() {
  /* ───────── 启动 ───────── */
  (async () => {
    try {
      S.cfg = await invoke("get_config");
    } catch (_) {}
    applyLook();
    updateEmpty();
    mood("idle");
    settle(); // 上次贴着边：重新压平
    if (S.cfg && S.cfg.screen === "main") setTimeout(screenCheck, 300); // 固定在主屏：上次在副屏也挪回来
    // 还没设置模型：小球露出好奇的表情提醒一下，不自动展开，免得挡住屏幕
    if (needsSetup()) mood("curious", 4000);
  })();
}
