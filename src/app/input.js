import { stop } from "./agents.js";
import { openSettings } from "./appearance.js";
import { askHistory } from "./conversation.js";
import { mood } from "./orb.js";
import { send } from "./send.js";
import { $, ask, mutable, orb, S } from "./state.js";

export function autosize() {
  ask.style.height = "auto";
  ask.style.height = Math.min(40, ask.scrollHeight) + "px";
}

export function initInput() {
  $("#stopBtn").onclick = stop;
  $("#capsule").addEventListener("submit", (e) => {
    e.preventDefault();
    send();
  });
  ask.addEventListener("keydown", (e) => {
    if (e.isComposing) return;
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
      return;
    }
    if (ask.selectionStart !== ask.selectionEnd || !askHistory.length) return;
    const caret = ask.selectionStart;
    if (e.key === "ArrowUp" && !ask.value.slice(0, caret).includes("\n")) {
      if (mutable.historyIndex < 0) mutable.historyDraft = ask.value;
      mutable.historyIndex = Math.min(mutable.historyIndex + 1, askHistory.length - 1);
    } else if (e.key === "ArrowDown" && mutable.historyIndex >= 0 && !ask.value.slice(caret).includes("\n")) {
      mutable.historyIndex--;
    } else return;
    e.preventDefault();
    ask.value = mutable.historyIndex < 0 ? mutable.historyDraft : askHistory[mutable.historyIndex];
    ask.setSelectionRange(e.key === "ArrowUp" ? 0 : ask.value.length, e.key === "ArrowUp" ? 0 : ask.value.length);
    autosize();
  });
  ask.addEventListener("input", () => {
    mutable.historyIndex = -1;
    autosize();
    S.lastActive = Date.now();
  });
  ask.addEventListener("focus", () => {
    if (!S.busy) mood("listen");
  });
  ask.addEventListener("blur", () => {
    if (!S.busy && orb.className.includes("s-listen")) mood("idle");
  });
  $("#setupBtn").onclick = openSettings;
  document.querySelectorAll("#empty .chips button").forEach(
    (b) =>
      (b.onclick = () => {
        ask.value = b.dataset.q;
        autosize();
        ask.focus();
      }),
  );
}
