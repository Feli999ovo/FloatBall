import { initAgents } from "./app/agents.js";
import { initAppearance } from "./app/appearance.js";
import { initApproval } from "./app/approval.js";
import { initAttach } from "./app/attach.js";
import { initChat } from "./app/chat.js";
import { askTmp, Conv, goConv, initConversation, newChat, newTmpChat, saveMsg, setTmp } from "./app/conversation.js";
import { initHandoff } from "./app/handoff.js";
import { initHistory } from "./app/history.js";
import { hv, initHover } from "./app/hover.js";
import { initInput } from "./app/input.js";
import { initMonitors } from "./app/monitors.js";
import { initOrb, mood } from "./app/orb.js";
import { hidePeek, initPeek, showPeek } from "./app/peek.js";
import { historyText, initReceipt } from "./app/receipt.js";
import { initRoutineForm } from "./app/routine-form.js";
import { initRoutines } from "./app/routines.js";
import { initSelection } from "./app/selection.js";
import { initSend } from "./app/send.js";
import { initStartup } from "./app/startup.js";
import { ask, initState, mutable, orb, S } from "./app/state.js";
import { initTasks, pendingCards } from "./app/tasks.js";
import { initTeam } from "./app/team.js";
import { initToast } from "./app/toast.js";
import { initViews } from "./app/views.js";
import { initVoice } from "./app/voice.js";
import { initVoiceFinish } from "./app/voice-finish.js";
import { collapse, expand, initWindow } from "./app/window.js";
import { initWindowGlass } from "./app/window-glass.js";

initState();
initOrb();
initWindow();
initWindowGlass();
initViews();
initTasks();
initRoutines();
initRoutineForm();
initHover();
initPeek();
initHandoff();
initToast();
initAppearance();
initConversation();
initReceipt();
initSend();
initChat();
initTeam();
initAgents();
initApproval();
initInput();
initAttach();
initHistory();
initMonitors();
initSelection();
initVoice();
initVoiceFinish();
initStartup();
if (window.__mock) {
  Object.defineProperty(window, "morphing", { get: () => mutable.morphing });
  Object.assign(window, {
    Conv,
    S,
    ask,
    askTmp,
    collapse,
    expand,
    goConv,
    hidePeek,
    historyText,
    hv,
    mood,
    newChat,
    newTmpChat,
    orb,
    pendingCards,
    saveMsg,
    setTmp,
    showPeek,
  });
}
