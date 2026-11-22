import { renderAgents } from "./agents.js";
import { lookupCaps } from "./capabilities.js";
import { openForm, renderLib, syncProv } from "./models.js";
import { renderPerm } from "./permissions.js";
import { fBase, fList, fModel, fProv } from "./providers.js";
import { selStatus } from "./selection.js";
import { stopRec } from "./shortcuts.js";
import { $, mutable } from "./state.js";

export let PANES;

export function showTab(t) {
  const k = t === "add" ? "model" : t;
  document.querySelectorAll(".x-tabs button").forEach((o) => o.classList.toggle("on", o.dataset.tab === k));
  for (const [tab, id] of Object.entries(PANES)) $(id).hidden = tab !== k;
  $("paneModel").hidden = true;
  fList.hidden = true;
  if (k === "model") renderLib();
  if (k === "agents") renderAgents();
  if (k === "perm") renderPerm();
  if (t === "add") openForm();
  if (k === "keys") stopRec();
  if (k === "sel") selStatus();
  document.querySelectorAll(".x-setw .x-pane").forEach((p) => {
    p.scrollTop = 0;
  });
}

export function initTabs() {
  /* ───────── 分类（原型的 showTab） ───────── */
  PANES = {
    model: "paneLib",
    agents: "paneAgents",
    agEd: "paneAgEd",
    perm: "panePerm",
    look: "paneLook",
    keys: "paneKeys",
    sel: "paneSel",
    voice: "paneVoice",
  };
  document.querySelectorAll(".x-tabs button").forEach((b) =>
    b.addEventListener("click", () => {
      mutable.fromTip = false;
      showTab(b.dataset.tab);
    }),
  );
  $("libAdd").addEventListener("click", () => openForm());
  $("fBack").addEventListener("click", () => {
    mutable.fromTip = false;
    showTab("model");
    $("libAdd").focus({ preventScroll: true });
  });
  fModel.addEventListener("input", () => lookupCaps());
  fBase.addEventListener("input", () => lookupCaps(400));
  fProv.addEventListener("change", () => {
    syncProv();
    lookupCaps(0);
  });
}
