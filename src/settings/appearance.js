import { mk } from "./agent-form.js";
import { reload } from "./startup.js";
import { $, invoke, mutable, toast } from "./state.js";

export let radio, showGlass, sw, MDS_POS, MDS_CAP, HV_NAME, KEY_DEF, keyVal;

/* ───────── 外观、快捷键：改了就存（save_prefs） ───────── */
export function prefs() {
  const on = (g) => {
    const b = $(g).querySelector('[aria-checked="true"]');
    return b && b.dataset.v;
  };
  return {
    ...keyVal,
    orb_size: +(on("lkSize") || mutable.cfg.orb_size),
    glass: +$("lkGlass").value,
    eye: on("lkEye") || mutable.cfg.eye,
    idle_ms: +$("lkIdle").value,
    follow_system: sw("lkSys"),
    auto_dock: sw("lkDock"),
    calm: sw("lkCalm"),
    hover: { on: sw("hvOn"), delay: +(on("hvSpd") || 350), order: mutable.hvOrder },
    ...(mutable.selCfg ? { sel: mutable.selCfg } : {}),
    ...(mutable.vcCfg ? { voice: mutable.vcCfg } : {}),
    usd_cny: Number($("usdCny").value),
    screen: on("mdsWhere") || "mouse",
    screen_slide: sw("mdsSlide"),
    catalog_auto: sw("libAuto"),
  };
}

export async function savePrefs() {
  const rate = Number($("usdCny").value);
  if (!Number.isFinite(rate) || rate <= 0 || rate > 1000) {
    toast("汇率需大于 0 且不超过 1000");
    return false;
  }
  try {
    const busy = await invoke("save_prefs", { prefs: prefs() });
    await reload();
    toast(busy ? busy + "，换一个快捷键才能用" : "已保存");
    return !busy;
  } catch (e) {
    toast(String(e));
    return false;
  }
}

export function mdsFig(anim) {
  const b = $("mdsWhere").querySelector('[aria-checked="true"]'),
    v = (b && b.dataset.v) || "mouse",
    [px, py] = MDS_POS[v],
    fig = $("mdsFig");
  if (anim && !sw("mdsSlide")) {
    fig.classList.remove("x-mdsjump");
    void fig.offsetWidth;
    fig.classList.add("x-mdsjump");
    clearTimeout(mdsFig.t);
    mdsFig.t = setTimeout(() => fig.classList.remove("x-mdsjump"), 340);
  }
  $("mdsOrb").style.transform = `translate(${px}px,${py}px)`;
  $("mdsCur").style.opacity = v === "mouse" ? 1 : 0.45;
  $("mdsCap").textContent = MDS_CAP[v];
}

export function hvRender(focusId, focusSel) {
  const ul = $("hvList");
  ul.innerHTML = "";
  const full = mutable.hvOrder.filter((o) => o.show).length >= 4;
  mutable.hvOrder.forEach((o, i) => {
    const name = HV_NAME[o.id] || o.id,
      li = mk("li");
    li.dataset.id = o.id;
    const lb = mk("label"),
      cb = mk("input");
    cb.type = "checkbox";
    cb.checked = o.show;
    cb.disabled = !o.show && full;
    lb.append(cb, document.createTextNode(name));
    if (cb.disabled) lb.className = "x-hvoff";
    cb.addEventListener("change", () => {
      o.show = cb.checked;
      hvRender(o.id, "input");
      savePrefs();
    });
    const up = mk("button", "x-hvmv", "↑"),
      dn = mk("button", "x-hvmv", "↓");
    up.type = dn.type = "button";
    up.dataset.d = "up";
    dn.dataset.d = "dn";
    up.setAttribute("aria-label", `上移 ${name}`);
    dn.setAttribute("aria-label", `下移 ${name}`);
    up.disabled = i === 0;
    dn.disabled = i === mutable.hvOrder.length - 1;
    const mv = (j, sel) => {
      const [it] = mutable.hvOrder.splice(i, 1);
      mutable.hvOrder.splice(j, 0, it);
      hvRender(it.id, sel);
      savePrefs();
    };
    up.addEventListener("click", () => mv(i - 1, "[data-d=up]"));
    dn.addEventListener("click", () => mv(i + 1, "[data-d=dn]"));
    li.append(lb, up, dn);
    ul.appendChild(li);
  });
  $("hvSet").classList.toggle("x-hvdis", !sw("hvOn"));
  if (focusId) {
    const li = ul.querySelector(`[data-id="${focusId}"]`),
      f = li && (li.querySelector(focusSel + ":not(:disabled)") || li.querySelector("input"));
    if (f) f.focus({ preventScroll: true });
  }
}

export function initAppearance() {
  // 外观（原型的 lkSize / lkGlass / lkEye）
  radio = (g, v) =>
    $(g)
      .querySelectorAll("[role=radio]")
      .forEach((b) => {
        const on = b.dataset.v.toLowerCase() === String(v).toLowerCase();
        b.setAttribute("aria-checked", on);
        b.tabIndex = on ? 0 : -1;
      });
  ["lkSize", "lkEye", "hvSpd", "mdsWhere"].forEach((g) => {
    $(g).addEventListener("click", (e) => {
      const b = e.target.closest("[role=radio]");
      if (!b) return;
      radio(g, b.dataset.v);
      if (g === "mdsWhere") mdsFig(true);
      savePrefs();
    });
    $(g).addEventListener("keydown", (e) => {
      const bs = [...$(g).querySelectorAll("[role=radio]")],
        i = bs.indexOf(document.activeElement);
      const d =
        e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
      if (!d || i < 0) return;
      e.preventDefault();
      const n = bs[(i + d + bs.length) % bs.length];
      n.focus();
      n.click();
    });
  });
  // 原型的眼睛颜色（奶白、暖黄、蜜桃、珊瑚）
  [
    ["#FFF1DC", "奶白（默认）"],
    ["#FFE2A8", "暖黄"],
    ["#FFC9A6", "蜜桃"],
    ["#FFB7A0", "珊瑚"],
  ].forEach(([c, name]) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "x-dot";
    b.setAttribute("role", "radio");
    b.dataset.v = c;
    b.setAttribute("aria-label", name);
    b.title = name;
    b.style.setProperty("--t", c);
    $("lkEye").appendChild(b);
  });
  showGlass = () => {
    const v = +$("lkGlass").value;
    $("lkGlassV").textContent = v ? "更通透 +" + v + "%" : "标准";
  };
  sw = (id) => $(id).getAttribute("aria-checked") === "true";
  $("lkIdle").addEventListener("change", savePrefs);
  // 多显示器的示意图（原型原样）：小球点跟着所选的方式挪；没开「滑过去」时闪一下表示跳过去
  MDS_POS = { mouse: [270, 60], main: [150, 46], last: [40, 78] };
  MDS_CAP = {
    mouse: "鼠标在副屏，小球跟到副屏",
    main: "不管鼠标在哪，小球都留在主屏",
    last: "回到上次待过的屏幕（这里是主屏左下角）",
  };
  ["lkSys", "lkDock", "lkCalm", "hvOn", "mdsSlide"].forEach((id) =>
    $(id).addEventListener("click", () => {
      $(id).setAttribute("aria-checked", !sw(id));
      savePrefs();
    }),
  );
  $("lkGlass").addEventListener("input", showGlass);
  $("lkGlass").addEventListener("change", savePrefs);
  // 悬停快捷入口的列表（原型的 hvRenderList）：勾选、上移、下移，最多 4 个
  HV_NAME = { shot: "截屏提问", new: "新对话", hist: "历史记录", tasks: "任务列表" };
  mutable.hvOrder = [];
  // 快捷键：原型的「录制」。写进配置用 Tauri 的写法（Alt+Space），显示用原型的写法（⌥ Space）
  KEY_DEF = [
    ["shortcut", "唤起/收起", "Alt+Space"],
    ["new_shortcut", "新对话", "Alt+N"],
    ["capture_shortcut", "截屏提问", "Alt+S"],
    ["win_shortcut", "发送当前窗口", "Alt+W"],
    ["share_shortcut", "开始/停止共享", "Alt+Shift+S"],
    ["feed_shortcut", "喂当前选中的文字", "Alt+C"],
    ["tasks_shortcut", "打开任务", "Alt+T"],
    ["talk_shortcut", "按住说话", "RightAlt"],
  ];
  keyVal = Object.fromEntries(KEY_DEF.map(([k]) => [k, ""]));
}
