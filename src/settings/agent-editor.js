import { AG_TINTS, agents, matchRole, mk, mName, modelOf, reqWord, TOOLS, TRY_RE } from "./agent-form.js";
import { renderAgents } from "./agents.js";
import { agDelete, agRedraw, agSave, memoryControls } from "./memory.js";
import { models } from "./providers.js";
import { renderAgentSkills } from "./skills.js";
import { $, mutable, toast } from "./state.js";
import { showTab } from "./tabs.js";

export function gotoAgent(id) {
  const a = agents().find((x) => x.id === id);
  showTab("agents");
  if (a) agEdit(a);
}

export function agEdit(a, tpl) {
  const src = a || { id: "", emoji: "", model: 0, fallback: 0, auto: false, ...tpl };
  const d = { ...src, tools: src.tools.slice(), skills: (src.skills || []).slice() };
  mutable.agEd = { a, d, del: false, tryText: "" };
  $("paneAgents").hidden = true;
  $("paneAgEd").hidden = false;
  agDraw();
  $("paneAgEd").scrollTop = 0;
  setTimeout(() => {
    const el = mutable.agEd && $("aeName");
    if (el) el.focus({ preventScroll: true });
  }, 60);
}

export function agBack(id) {
  mutable.agEd = null;
  renderAgents();
  const b = id && $("agList").querySelector(`[data-ag="${id}"]`);
  (b || $("agAdd")).focus({ preventScroll: true });
}

export function fillSel(sel, first, val) {
  sel.innerHTML = "";
  sel.add(new Option(first, "0"));
  models().forEach((m) => {
    const o = new Option(mName(m) + (m.tools === false ? "（不支持工具调用）" : ""), m.id);
    o.disabled = m.tools === false;
    sel.add(o);
  });
  const m = modelOf(val);
  sel.value = m && m.tools !== false ? String(val) : "0";
}

export function agDraw() {
  const { a, d } = mutable.agEd,
    p = $("paneAgEd"),
    lead = d.id === "lead",
    fresh = !a;
  p.innerHTML = "";
  const fld = (label, id, ...kids) => {
    const f = mk("div", "x-field"),
      l = mk(id ? "label" : "span", "", label);
    if (id) l.htmlFor = id;
    f.appendChild(l);
    kids.forEach((k) => k && f.appendChild(k));
    return f;
  };
  const inp = (id, key, ph, max) => {
    const i = mk("input", "x-in");
    i.id = id;
    i.value = d[key];
    i.placeholder = ph;
    i.spellcheck = false;
    if (max) i.maxLength = max;
    i.addEventListener("input", () => {
      d[key] = i.value;
      tryRun();
    });
    return i;
  };
  const h = mk("div", "x-fh"),
    back = mk("button", "x-sb", "‹ Agents");
  back.type = "button";
  back.addEventListener("click", () => agBack(a && a.id));
  h.appendChild(back);
  h.appendChild(mk("p", "", fresh ? "新建 Agent" : "编辑 Agent"));
  p.appendChild(h);
  // 头像：6 个柔和色点 + 可选 1 个 emoji 或字
  const row = mk("div", "x-aeav"),
    pvw = mk("span", "x-aepvw"),
    pv = mk("span", "x-av x-aepv");
  pv.setAttribute("aria-hidden", "true");
  const fbCh = () => Array.from((d.name || "").trim())[0] || "";
  let emo = null;
  const drawPv = () => {
    const ch = d.emoji || fbCh();
    pv.style.setProperty("--t", d.tint);
    pv.textContent = ch;
    pv.classList.toggle("has-ch", !!ch);
    if (emo) emo.placeholder = fbCh() || "字";
  };
  pvw.appendChild(pv);
  row.appendChild(pvw);
  const dots = mk("div", "x-dots");
  dots.setAttribute("role", "radiogroup");
  dots.setAttribute("aria-label", "头像颜色");
  AG_TINTS.forEach((c, i) => {
    const b = mk("button", "x-dot");
    b.type = "button";
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", d.tint === c);
    b.setAttribute("aria-label", `颜色 ${i + 1}`);
    b.style.setProperty("--t", c);
    b.addEventListener("click", () => {
      d.tint = c;
      dots.querySelectorAll(".x-dot").forEach((x) => x.setAttribute("aria-checked", x === b));
      drawPv();
    });
    dots.appendChild(b);
  });
  row.appendChild(dots);
  emo = mk("input", "x-in x-emo");
  emo.value = d.emoji;
  emo.spellcheck = false;
  emo.setAttribute("aria-label", "头像上的 emoji 或一个字，可不填");
  const takeEmo = () => {
    const ch = Array.from(emo.value.trim()).pop() || "";
    d.emoji = ch;
    emo.value = ch;
    drawPv();
  };
  emo.addEventListener("input", (e) => {
    if (!e.isComposing) takeEmo();
  });
  emo.addEventListener("compositionend", takeEmo);
  row.appendChild(emo);
  drawPv();
  p.appendChild(fld("头像", null, row));
  const nameIn = inp("aeName", "name", "比如：小白", 12);
  nameIn.addEventListener("input", drawPv);
  p.appendChild(fld("名字", "aeName", nameIn));
  p.appendChild(
    fld(
      "职责",
      "aeRole",
      inp("aeRole", "role", "一句话，比如：检查事实、错别字和逻辑漏洞", 40),
      mk("small", "x-lbhint", "主控根据这句话决定把什么交给它"),
    ),
  );
  // 指令（可折叠）
  const det = mk("details", "x-aed");
  det.open = !!d.prompt;
  const sum = mk("summary", "", "指令 ");
  sum.appendChild(mk("small", "", d.prompt ? "已写" : "选填 · 它的系统提示"));
  det.appendChild(sum);
  const ta = mk("textarea", "x-in");
  ta.id = "aePrompt";
  ta.rows = 4;
  ta.value = d.prompt;
  ta.spellcheck = false;
  ta.setAttribute("aria-label", "指令");
  ta.placeholder = "比如：你是一名严谨的审查员。逐段检查事实、数字和错别字；只列出问题和修改建议，不直接改原文。";
  ta.addEventListener("input", () => (d.prompt = ta.value));
  det.appendChild(ta);
  p.appendChild(det);
  // 能用的工具
  const tl = mk("div", "x-lbcs");
  tl.setAttribute("role", "group");
  tl.setAttribute("aria-label", "能用的工具");
  TOOLS.forEach(([n, ap]) => {
    const b = mk("button", "x-lbtl");
    b.type = "button";
    b.dataset.tool = n;
    const lock = lead && n === "读文件";
    b.setAttribute("aria-pressed", d.tools.includes(n));
    if (lock) {
      b.setAttribute("aria-disabled", "true");
      b.title = "主控至少要能读文件";
    }
    b.innerHTML = `<span>${n}</span>` + (ap ? '<span class="x-lbcf">需确认</span>' : "");
    b.addEventListener("click", () => {
      if (lock) return toast("主控至少要能读文件");
      const i = d.tools.indexOf(n);
      if (i < 0) d.tools.push(n);
      else d.tools.splice(i, 1);
      b.setAttribute("aria-pressed", i < 0);
      tryRun();
    });
    tl.appendChild(b);
  });
  p.appendChild(
    fld(
      "能用的工具",
      null,
      tl,
      mk(
        "small",
        "x-lbhint",
        lead ? "需确认的工具每次用前先问你 · 主控至少保留「读文件」" : "需确认的工具每次用前先问你",
      ),
    ),
  );
  const sk = mk("div", "x-lbcs");
  sk.id = "aeSkills";
  sk.setAttribute("role", "group");
  sk.setAttribute("aria-label", "会的技能");
  renderAgentSkills(sk, d);
  p.appendChild(fld("会的技能", null, sk));
  // 模型 · 备用模型 · 自动省钱
  const g2 = mk("div", "x-g2");
  g2.innerHTML = `<label class="x-field"><span>模型</span><select class="x-in" id="aeModel"></select></label><label class="x-field"><span>备用模型</span><select class="x-in" id="aeFallback"></select></label>`;
  const [sm, sf] = g2.querySelectorAll("select");
  fillSel(sm, "跟随默认", d.model);
  fillSel(sf, "无", d.fallback);
  sm.addEventListener("change", () => (d.model = +sm.value));
  sf.addEventListener("change", () => (d.fallback = +sf.value));
  p.appendChild(g2);
  const ar = mk("div", "x-ar");
  ar.style.margin = "6px 0 12px";
  ar.innerHTML =
    '<span id="aeAutoL">自动省钱<small>简单步骤自动换便宜/本地模型</small></span><button type="button" class="x-sw" id="aeAuto" role="switch" aria-labelledby="aeAutoL"></button>';
  const sw = ar.lastChild;
  sw.setAttribute("aria-checked", d.auto);
  sw.addEventListener("click", () => {
    d.auto = !d.auto;
    sw.setAttribute("aria-checked", d.auto);
  });
  p.appendChild(ar);
  p.appendChild(fld("记忆", null, memoryControls(a?.id)));
  // 试一试
  const ti = mk("input", "x-in");
  ti.id = "aeTry";
  ti.spellcheck = false;
  ti.placeholder = "输入一句请求，比如：帮我检查这份报告的错别字";
  ti.value = mutable.agEd.tryText || "";
  const tr = mk("div", "x-tryr");
  tr.id = "aeTryR";
  tr.setAttribute("aria-live", "polite");
  tr.hidden = true;
  ti.addEventListener("input", (e) => {
    if (e.isComposing) return;
    mutable.agEd.tryText = ti.value;
    tryRun();
  });
  ti.addEventListener("compositionend", () => {
    mutable.agEd.tryText = ti.value;
    tryRun();
  });
  p.appendChild(fld("试一试", "aeTry", ti, tr));
  function tryRun() {
    const text = (mutable.agEd.tryText || "").trim();
    tr.hidden = !text;
    if (!text) return;
    const name = d.name.trim() || "它",
      m = matchRole(d.role, text);
    const need = TOOLS.map(([t]) => t).filter((t) => TRY_RE[t].test(text)),
      use = need.filter((t) => d.tools.includes(t)),
      miss = need.filter((t) => !d.tools.includes(t));
    tr.className = "x-tryr" + (m ? "" : " no");
    tr.innerHTML = "<span></span><small></small>";
    if (m) {
      const snip = d.role.slice(m.at, m.at + 8) + (d.role.length > m.at + 8 ? "…" : "");
      tr.firstChild.textContent = `主控会交给：${name}（匹配职责：${snip}）`;
      tr.lastChild.textContent =
        (use.length ? "会用 " + use.join("、") : "不需要工具") +
        (miss.length ? ` · 缺少 ${miss.join("、")}，这部分它做不了` : "");
    } else {
      const kw = reqWord(text) || text.slice(0, 8);
      tr.firstChild.textContent = d.role.trim() ? `不会交给它：职责里没有「${kw}」相关的事` : "不会交给它：还没写职责";
      const o = agents().find((x) => x.id !== d.id && matchRole(x.role, text));
      tr.lastChild.textContent =
        (o ? `主控会交给 ${o.name}` : "主控会自己处理") + (need.length ? ` · 要用 ${need.join("、")}` : "");
    }
  }
  tryRun();
  // 底部：保存 / 删除 Agent（主控不能删）
  const f = mk("div", "x-aef"),
    btn = (t, cls, fn) => {
      const b = mk("button", "x-sb" + (cls ? " " + cls : ""), t);
      b.type = "button";
      b.addEventListener("click", fn);
      f.appendChild(b);
      return b;
    };
  if (mutable.agEd.del) {
    f.appendChild(mk("p", "", `删除「${a.name}」？`));
    btn("取消", "x-lbq", () => {
      mutable.agEd.del = false;
      agRedraw(".x-aef .x-lbq");
    });
    btn("删除", "x-lbx", agDelete).focus({ preventScroll: true });
  } else {
    if (a && !lead)
      btn("删除 Agent", "x-lbq", () => {
        mutable.agEd.del = true;
        agRedraw(".x-aef .x-lbx");
      });
    btn("取消", "x-lbq x-sp", () => agBack(a && a.id));
    btn(fresh ? "创建" : "保存", "x-pri", agSave);
  }
  p.appendChild(f);
}
