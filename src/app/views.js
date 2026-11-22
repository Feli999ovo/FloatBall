import { syncShare } from "./attach.js";
import { syncTmp, updateEmpty } from "./conversation.js";
import { loadHistory } from "./history.js";
import { rtRefresh } from "./routines.js";
import { $ } from "./state.js";
import { pendingCards, renderTasks } from "./tasks.js";
import { focusAsk } from "./window.js";

/* ───────── 视图 ───────── */
export function showView(v) {
  $("#feedPermission").hidden = true;
  $("#vChat").hidden = v !== "chat";
  const openingTasks = v === "task" && $("#vTask").hidden;
  $("#vTask").hidden = v !== "task";
  $("#tabChat").setAttribute("aria-selected", v !== "task");
  $("#tabTask").setAttribute("aria-selected", v === "task");
  $("#tabChat").tabIndex = v === "task" ? -1 : 0;
  $("#tabTask").tabIndex = v === "task" ? 0 : -1;
  $("#title").hidden = v === "task";
  $("#vHist").hidden = v !== "hist";
  $("#vHo").hidden = v !== "ho";
  $("#vShare").hidden = v !== "share";
  $("#vSkill").hidden = v !== "skill";
  $("#card").classList.toggle("ho", v === "ho" || v === "skill"); // 交给终端打开时把「对话 | 任务」和标题栏让给面板：卡片只有 260 高
  $("#histBtn").setAttribute("aria-pressed", v === "hist");
  if (v === "hist") loadHistory();
  if (v === "task") {
    if (openingTasks && pendingCards.size) $("#tasks").scrollTop = 0;
    renderTasks();
    rtRefresh().then(renderTasks);
  }
  if (v === "chat") updateEmpty();
  syncTmp();
  syncShare();
}

export function initViews() {
  $("#tabChat").onclick = () => {
    showView("chat");
    focusAsk();
  };
  $("#tabTask").onclick = () => showView("task");
  $(".seg").addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const toTask = $("#vTask").hidden;
    showView(toTask ? "task" : "chat");
    $(toTask ? "#tabTask" : "#tabChat").focus();
  });
}
