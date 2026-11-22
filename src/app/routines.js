import { rtForm, rtRun, rtSave } from "./routine-form.js";
import { invoke, jslog, mutable } from "./state.js";
import { renderTasks } from "./tasks.js";
import { toast, toastErr } from "./toast.js";

export let WK, rtNext, rtOpen, hm2, RT_ALLOW, rtComplete;

export function rtSched(r) {
  switch (r.every) {
    case "hour":
      return `每小时 :${r.time.slice(3)}`;
    case "week":
      return `每周${WK[r.weekday]} ${r.time}`;
    case "workday":
      return `工作日 ${r.time}`;
    case "month":
      return `每月 ${r.monthday} 号 ${r.time}`;
    case "once": {
      const [, month, day] = r.date.split("-");
      return `${+month}月${+day}日 ${r.time} 一次`;
    }
    default:
      return `每天 ${r.time}`;
  }
}

export function rtWhen(ts) {
  if (!ts) return "";
  const d = new Date(ts * 1000),
    day = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const k = Math.round((day(d) - day(new Date())) / 864e5);
  return (
    (["今天", "明天", "后天"][k] || (k < 0 ? `${d.getMonth() + 1}月${d.getDate()}日` : "周" + WK[d.getDay()])) +
    " " +
    hm2(d)
  );
}

export async function rtRefresh() {
  try {
    rtNext = await invoke("routine_next");
  } catch (e) {
    rtNext = {};
    jslog("routine_next 失败：" + e);
  }
}

export function rtAddBtn() {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "rtadd";
  b.textContent = "+ 新建定时任务";
  b.onclick = () => {
    mutable.rtNew = {
      id: "",
      name: "",
      prompt: "",
      every: "day",
      weekday: 1,
      monthday: 1,
      date: "",
      allow: [],
      time: "09:00",
      on: true,
    };
    mutable.rtFocus = "rtEdprompt";
    renderTasks();
  };
  return b;
}

export function rtRow(r) {
  const d = document.createElement("div");
  d.className = "tk rt" + (r.on ? "" : " off");
  d.dataset.rid = r.id;
  const row = document.createElement("div");
  row.className = "row";
  const name = document.createElement("button");
  name.type = "button";
  name.className = "name";
  name.textContent = r.name;
  name.id = "rtN" + r.id;
  name.setAttribute("aria-expanded", rtOpen === r.id);
  const sw = document.createElement("button");
  sw.type = "button";
  sw.className = "x-sw";
  sw.setAttribute("role", "switch");
  sw.setAttribute("aria-checked", r.on);
  sw.setAttribute("aria-label", `定时任务「${r.name}」开关`);
  sw.disabled = rtComplete(r);
  sw.onclick = (e) => {
    e.stopPropagation();
    rtSave({ ...r, on: !r.on }, `${r.on ? "已关闭" : "已开启"}：${r.name}`);
  };
  row.append(name, sw);
  const sub = document.createElement("p");
  sub.className = "rts";
  const status = rtComplete(r) ? "已完成" : !r.on ? "已关闭" : rtNext[r.id] ? "下次 " + rtWhen(rtNext[r.id]) : "";
  sub.textContent = rtSched(r) + (status ? " · " + status : "");
  d.append(row, sub);
  if (rtOpen === r.id) d.appendChild(r._edit ? rtForm(r._edit) : rtDetail(r));
  name.onclick = () => {
    rtOpen = rtOpen === r.id ? "" : r.id;
    mutable.rtFocus = "rtN" + r.id;
    renderTasks();
  };
  return d;
}

export function rtBox() {
  const d = document.createElement("div");
  d.className = "rd";
  d.onclick = (e) => e.stopPropagation();
  return d;
}

export function rtBtns(d, list) {
  const bar = document.createElement("div");
  bar.className = "rda";
  for (const [t, f, cls] of list) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "x-sb" + (cls ? " " + cls : "");
    b.textContent = t;
    b.onclick = f;
    bar.appendChild(b);
  }
  d.appendChild(bar);
  return bar;
}

export function rtDl(d) {
  const dl = document.createElement("dl");
  d.appendChild(dl);
  return (k, v) => {
    const dt = document.createElement("dt"),
      dd = document.createElement("dd");
    dt.textContent = k;
    if (typeof v === "string") dd.textContent = v;
    else dd.appendChild(v);
    dl.append(dt, dd);
    return v;
  };
}

export function rtDetail(r) {
  const d = rtBox(),
    row = rtDl(d);
  row("指令", r.prompt);
  row("何时运行", rtSched(r));
  row("下次运行", rtComplete(r) ? "已完成" : r.on ? rtWhen(rtNext[r.id]) || "—" : "已暂停");
  const ul = document.createElement("ul");
  const runs = r.runs || [];
  if (!runs.length) {
    const li = document.createElement("li");
    li.textContent = "还没有运行过";
    ul.appendChild(li);
  }
  runs.slice(0, 3).forEach((x) => {
    const li = document.createElement("li"),
      st = document.createElement("span"),
      sm = document.createElement("small");
    st.className = x.ok ? "rok" : "rno";
    st.textContent = x.ok ? "✓ 成功" : "✕ 失败";
    sm.textContent = ` ${rtWhen(x.at)} · ${x.note}`;
    li.append(st, sm);
    ul.appendChild(li);
  });
  row("运行记录", ul);
  if (r._del) {
    const bar = rtBtns(d, [
      [
        "确认删除",
        () => {
          rtOpen = "";
          invoke("delete_routine", { id: r.id })
            .then(() => toast(`已删除：${r.name}`))
            .catch(toastErr);
        },
        "danger",
      ],
      [
        "取消",
        () => {
          r._del = false;
          renderTasks();
        },
      ],
    ]);
    const p = document.createElement("p");
    p.textContent = "删除后不再运行";
    bar.prepend(p);
  } else {
    const buttons = [
      ["立即测试", () => rtRun(r, "手动测试")],
      [
        "编辑",
        () => {
          r._edit = { ...r };
          mutable.rtFocus = "rtEdprompt";
          renderTasks();
        },
      ],
    ];
    if (!rtComplete(r))
      buttons.push([
        r.on ? "暂停" : "恢复",
        () => rtSave({ ...r, on: !r.on }, `${r.on ? "已暂停" : "已恢复"}：${r.name}`),
      ]);
    buttons.push([
      "删除",
      () => {
        r._del = true;
        renderTasks();
      },
      "danger",
    ]);
    rtBtns(d, buttons);
  }
  return d;
}

export function initRoutines() {
  /* ───────── 定时任务（B5，照原型 rtRow / rtDetail）：开关、每天 / 每周 / 每小时、下次时间；到点在后台跑 ───────── */
  WK = "日一二三四五六";
  rtNext = {};
  rtOpen = "";
  mutable.rtNew = null;
  mutable.rtFocus = "";
  hm2 = (d) => String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0");
  RT_ALLOW = [
    ["写入", "写文件"],
    ["打开", "打开网页"],
    ["删除", "删除文件（移到回收站）"],
    ["发送", "写邮件草稿"],
  ];
  rtComplete = (r) => r.every === "once" && !!r.last && r.last >= new Date(`${r.date}T${r.time}`).getTime() / 1000;
}
