import { run } from "./chat.js";
import { addUserBubble, Conv, goConv, saveMsg, showConv } from "./conversation.js";
import { showPeek } from "./peek.js";
import { RT_ALLOW, rtBox, rtBtns, rtDl, WK } from "./routines.js";
import { invoke, mutable, S, T } from "./state.js";
import { renderTasks } from "./tasks.js";
import { toast, toastErr } from "./toast.js";

// 编辑 / 新建：指令、名字、每天 / 每周几 / 每小时、时间
export function rtForm(dr) {
  const d = rtBox();
  d.classList.add("ed");
  if (!dr.id) {
    const h = document.createElement("p");
    h.className = "rth";
    h.textContent = "新建定时任务";
    d.appendChild(h);
  }
  const row = rtDl(d);
  const inp = (k, ph, label) => {
    const x = document.createElement("input");
    x.className = "x-in";
    x.id = "rtEd" + k;
    x.value = dr[k];
    x.placeholder = ph;
    x.spellcheck = false;
    x.setAttribute("aria-label", label);
    x.oninput = () => (dr[k] = x.value);
    return x;
  };
  row("指令", inp("prompt", "每次运行时让它做什么", "指令"));
  row("名字", inp("name", "不填就用指令开头", "名字"));
  const wrap = document.createElement("div");
  wrap.className = "rtw";
  const sel = (id, label, opts, v) => {
    const s = document.createElement("select");
    s.className = "x-in";
    s.id = id;
    s.setAttribute("aria-label", label);
    opts.forEach(([val, t]) => s.add(new Option(t, val)));
    s.value = v;
    return s;
  };
  const ev = sel(
    "rtEdevery",
    "多久一次",
    [
      ["day", "每天"],
      ["workday", "工作日"],
      ["week", "每周"],
      ["month", "每月"],
      ["hour", "每小时"],
      ["once", "只一次"],
    ],
    dr.every,
  );
  const wk = sel(
    "rtEdweekday",
    "星期几",
    [1, 2, 3, 4, 5, 6, 0].map((i) => [i, "周" + WK[i]]),
    dr.weekday,
  );
  const tm = document.createElement("input");
  tm.type = "time";
  tm.className = "x-in";
  tm.id = "rtEdtime";
  tm.value = dr.time;
  tm.setAttribute("aria-label", "时间");
  const md = sel(
    "rtEdmonthday",
    "每月几号",
    Array.from({ length: 31 }, (_, i) => [i + 1, `${i + 1} 号`]),
    dr.monthday || 1,
  );
  const date = document.createElement("input");
  date.type = "date";
  date.className = "x-in";
  date.id = "rtEddate";
  date.setAttribute("aria-label", "日期");
  date.value = dr.date || "";
  const now = new Date();
  date.min = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  const sync = () => {
    wk.hidden = dr.every !== "week";
    md.hidden = dr.every !== "month";
    date.hidden = dr.every !== "once";
  };
  ev.onchange = () => {
    dr.every = ev.value;
    sync();
  };
  wk.onchange = () => (dr.weekday = +wk.value);
  md.onchange = () => (dr.monthday = +md.value);
  date.oninput = () => (dr.date = date.value);
  tm.oninput = () => (dr.time = tm.value || "09:00");
  sync();
  wrap.append(ev, wk, md, date, tm);
  row("何时运行", wrap);
  const permissions = document.createElement("details"),
    legend = document.createElement("summary");
  legend.textContent = "不在电脑前时也允许";
  permissions.appendChild(legend);
  const explanation = document.createElement("p");
  explanation.textContent = "只对这个定时任务有效";
  permissions.appendChild(explanation);
  for (const [kind, text] of RT_ALLOW) {
    const label = document.createElement("label"),
      input = document.createElement("input");
    input.type = "checkbox";
    input.checked = (dr.allow || []).includes(kind);
    input.onchange = () => {
      dr.allow = input.checked ? [...(dr.allow || []), kind] : (dr.allow || []).filter((k) => k !== kind);
    };
    label.append(input, text);
    permissions.appendChild(label);
  }
  permissions.className = "rt-allow";
  d.appendChild(permissions);
  const close = () => {
    if (dr === mutable.rtNew) mutable.rtNew = null;
    else {
      const r = ((S.cfg && S.cfg.routines) || []).find((x) => x.id === dr.id);
      if (r) r._edit = null;
    }
  };
  const save = async () => {
    if (!dr.prompt.trim()) {
      toast("要写上每次让它做什么");
      document.getElementById("rtEdprompt").focus();
      return;
    }
    const isNew = !dr.id,
      { _edit, _del, ...r } = dr;
    const label = r.name.trim() || r.prompt.trim().slice(0, 12);
    if (await rtSave(r, isNew ? `已创建：${label}` : `已保存：${label}`)) {
      close();
      renderTasks();
    }
  };
  rtBtns(d, [
    ["保存", save, "x-pri"],
    [
      "取消",
      () => {
        close();
        renderTasks();
      },
    ],
  ]);
  d.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      close();
      renderTasks();
    }
  });
  return d;
}

export async function rtSave(r, msg) {
  try {
    await invoke("save_routine", { routine: r });
    toast(msg);
    return true;
  } catch (e) {
    toastErr(e);
    return false;
  }
}

// 跑一次定时任务：开一个新对话在后台跑（和普通提问一样，审批卡、预算、历史都照常），不打扰当前对话
export function rtRun(r, note) {
  const keep = S.conv,
    c = Conv(null, "定时 · " + r.name);
  showConv(c);
  S.msgs.push({ role: "user", content: r.prompt });
  addUserBubble(r.prompt, []);
  run(undefined, c, false, r.allow || []);
  const task = c.task;
  if (task) {
    task.title = "定时 · " + r.name;
    task.rt = { id: r.id, note };
  }
  saveMsg(c, "user", r.prompt).catch(() => {});
  if (keep !== c) showConv(keep);
  renderTasks();
  if (!S.open) showPeek(`定时任务开始：${r.name}`, false, () => goConv(c));
  else toast(`已开始：${r.name}`);
}

export function initRoutineForm() {
  T.event.listen("routine", (e) => {
    if (e.payload && e.payload.id) rtRun(e.payload, "按时运行");
  });
}
