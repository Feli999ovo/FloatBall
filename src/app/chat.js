import { agSync, needOn } from "./agents.js";
import { msgsEl } from "./appearance.js";
import { askCard, budgetCard, fmtTok, lastText, leadModelName, toolPill } from "./approval.js";
import { addAiBubble, convs, doneMark, goConv, saveMsg, syncTmp } from "./conversation.js";
import { draftSkill, hoOpen } from "./handoff.js";
import { autosize } from "./input.js";
import { md } from "./markdown.js";
import { mood, poke } from "./orb.js";
import { showPeek } from "./peek.js";
import { agentModel, renderChanges, renderReceipt, setMeta } from "./receipt.js";
import { HO_RE, renderQueue, submitPrompt } from "./send.js";
import { $, ask, Channel, invoke, mutable, S } from "./state.js";
import { pendingCards, renderTasks, syncPending } from "./tasks.js";
import { fmtCny, handoffRow, teamRow } from "./team.js";
import { vcSpeak } from "./voice-finish.js";

export let chatSeq;

export function run(currentContent, conv = S.conv, plan = conv.plan, preAllow = [], reviewAgent = null) {
  const id = "c" + Date.now() + "-" + ++chatSeq;
  const userMessage = conv.msgs.at(-1);
  const prompt = currentContent ?? userMessage.content;
  const codeTask = HO_RE.test(typeof prompt === "string" ? prompt : prompt[0].text);
  const here = () => conv === S.conv; // 这个对话是不是正显示在卡片里
  conv.busy = id;
  convs.add(conv);
  if (here()) {
    S.busy = id;
    syncTmp();
    $("#stopRow").hidden = false;
    mood("think");
  }
  const task = { title: reviewAgent ? `审查 · ${reviewAgent.name}` : lastText(conv), st: "run", t0: Date.now(), conv };
  conv.task = task;
  S.tasks.unshift(task);
  renderTasks();
  const el = addAiBubble(conv);
  if (reviewAgent) {
    const title = document.createElement("strong");
    title.className = "review-title";
    title.textContent = task.title;
    el.prepend(title);
  }
  for (const item of conv.queue) el.parentNode.appendChild(item.el);
  renderQueue(conv);
  const scrollEnd = () => {
    if (here()) msgsEl.scrollTop = msgsEl.scrollHeight;
  };
  scrollEnd();
  let txt = el.querySelector(".txt");
  // out：整条回复（存历史、复制）；seg：当前这段文字。工具药丸把回复分成几段，药丸后面的字另起一段
  let out = "",
    seg = "",
    usage = null,
    raf = 0;
  const tools = new Map(),
    confirmations = new Map(),
    changes = [];
  const reviewModel = reviewAgent && agentModel(reviewAgent);
  const model = reviewAgent ? reviewModel?.name || reviewModel?.model || "" : leadModelName();
  const receipt = () => {
    renderReceipt(el, tools, confirmations, task, usage, model, changes);
    renderChanges(el, changes, conv);
  };
  const clearPending = (ask) => {
    if (pendingCards.delete(ask)) syncPending();
  };
  const addPending = (ev, card, desc) => {
    pendingCards.set(ev.ask, { conv, card, desc, call: ev.call });
    syncPending();
    mood("wait");
  };
  const paint = () => {
    raf = 0;
    const live = conv.busy === id;
    txt.innerHTML =
      live && !seg ? "" : live ? md(seg + "\uE000").replace("\uE000", '<span class="cur"></span>') : md(seg);
    scrollEnd();
  };
  const ch = new Channel();
  ch.onmessage = (ev) => {
    if (conv.busy !== id) return;
    if (ev.type === "delta") {
      out += ev.text;
      seg += ev.text; // 说话时保持思考的光点（照原型：流式输出时是 think）
      if (!raf) raf = requestAnimationFrame(paint);
    } else if (ev.type === "change") {
      changes.push(ev.change);
    } else if (ev.type === "tool") {
      tools.set(ev.call, ev);
      if (ev.status !== "wait") {
        for (const [ask, pending] of pendingCards) {
          if (pending.conv !== conv || pending.call !== ev.call) continue;
          pending.card.querySelectorAll("button,input").forEach((control) => (control.disabled = true));
          clearPending(ask);
        }
      }
      toolPill(el, ev);
      if (seg) {
        txt.classList.remove("cursor");
        txt.innerHTML = md(seg);
        seg = "";
        out += "\n\n";
        const t2 = document.createElement("div");
        t2.className = "txt cursor";
        txt = t2;
      }
      el.insertBefore(txt, el.querySelector(".meta"));
      if (ev.status === "run" && here()) mood("work");
      scrollEnd();
    } else if (ev.type === "team") {
      tools.set(ev.call, { name: `交给 ${ev.agent}`, status: ev.status });
      teamRow(el, ev);
      if (seg) {
        txt.classList.remove("cursor");
        txt.innerHTML = md(seg);
        seg = "";
        out += "\n\n";
        const t2 = document.createElement("div");
        t2.className = "txt cursor";
        txt = t2;
      }
      el.insertBefore(txt, el.querySelector(".meta"));
      if (here()) mood("work");
      scrollEnd();
    } else if (ev.type === "handoff") {
      handoffRow(el, ev);
      scrollEnd();
    } else if (ev.type === "budget") {
      const card = budgetCard(el, ev, task, () => clearPending(ev.ask));
      addPending(ev, card, "预算用完，已暂停");
      if (!S.open || !here()) showPeek(`预算用完，已暂停：${task.title}`, false, () => goConv(conv));
      scrollEnd();
    } else if (ev.type === "ask") {
      confirmations.set(ev.ask, null);
      const card = askCard(el, ev, (allow) => {
        confirmations.set(ev.ask, allow);
        clearPending(ev.ask);
      });
      addPending(ev, card, ev.desc);
      if (!S.open || !here()) showPeek("需要你确认：" + ev.desc, false, () => goConv(conv));
      scrollEnd();
    } else if (ev.type === "usage") {
      usage = ev;
    } else if (ev.type === "cost") {
      usage = { ...(usage || {}), cost: ev.amount };
    } else if (ev.type === "done") {
      finish();
      txt.classList.remove("cursor");
      paint();
      out = out.replace(/\n\n$/, "");
      if (!out) {
        txt.textContent = "（模型没有返回内容）";
      }
      conv.msgs.push({ role: "assistant", content: out });
      saveMsg(conv, "assistant", out).catch(() => {});
      const used = usage && usage.prompt + usage.completion;
      const cost = Number.isFinite(usage?.cost)
        ? ` · 约 ${fmtCny(usage.cost)}${Number(S.cfg?.usd_cny) === 7.2 ? "（默认汇率）" : ""}`
        : "";
      const usageLine = used ? `${model} · ${fmtTok(used)} tokens${cost}` : `${model}${cost}`;
      setMeta(el, tools.size ? "" : usageLine, { copy: out });
      receipt();
      const meta = el.querySelector(".meta");
      if (plan && !reviewAgent) {
        const execute = document.createElement("button");
        execute.textContent = "改成执行";
        execute.onclick = () => {
          conv.plan = false;
          goConv(conv);
          syncTmp();
          ask.value = "按上面的方案执行";
          autosize();
          ask.focus();
          mutable.historyIndex = -1;
        };
        meta.appendChild(execute);
      }
      if (!reviewAgent && codeTask && (S.cfg.handoff || {}).hint !== false) {
        const b = document.createElement("button");
        b.type = "button";
        b.className = "ho-hint";
        b.textContent = "交给终端";
        b.onclick = () => {
          goConv(conv);
          hoOpen();
        };
        meta.appendChild(b);
      }
      const skill = document.createElement("button");
      skill.type = "button";
      skill.className = "skill-add";
      skill.textContent = "存为技能";
      skill.onclick = () => draftSkill(conv, skill, prompt, out);
      meta.appendChild(skill);
      mood("done", 1500);
      if (here()) vcSpeak(el, out);
      // 在后台做完的：球角上记一个数，点开回到那条对话（照原型：完成时小球开心 + 角标）
      if (!S.open || !here()) {
        if (!here()) doneMark(conv);
        poke();
        showPeek(out || "回复好了", false, here() ? null : () => goConv(conv));
      }
      const next = conv.queue.findIndex((item) => !item.paused);
      if (next >= 0) {
        const [item] = conv.queue.splice(next, 1);
        item.el.remove();
        submitPrompt(conv, item);
      }
    } else if (ev.type === "error") {
      finish("err");
      txt.classList.remove("cursor");
      el.classList.add("err");
      txt.textContent = ev.message;
      const noKey = /API Key|设置/.test(ev.message);
      setMeta(el, "", {
        retry: () => {
          if (conv.busy || conv.msgs.at(-1) !== userMessage) return;
          el.remove();
          goConv(conv);
          run(currentContent, conv, plan, preAllow, reviewAgent);
        },
        settings: noKey,
      });
      receipt();
      mood("error", 4000);
      if (!here()) doneMark(conv);
    }
  };
  function finish(st = "done") {
    if (conv.busy === id) {
      conv.busy = conv.stop = null;
      convs.delete(conv);
    }
    for (const [ask, pending] of pendingCards) if (pending.conv === conv) pendingCards.delete(ask);
    if (st !== "done") for (const item of conv.queue) item.paused = true;
    renderQueue(conv);
    if (here()) {
      S.busy = S.stopFn = null;
      $("#stopRow").hidden = true;
      syncTmp();
    }
    if (Object.keys(S.agBusy).length && !convs.size) {
      S.agBusy = {};
      agSync();
    }
    needOn(pendingCards.size > 0);
    if (task.st === "run") {
      task.st = st;
      task.t1 = Date.now();
      renderTasks();
      if (task.rt)
        invoke("routine_done", {
          id: task.rt.id,
          ok: st === "done",
          note: { done: task.rt.note, err: "运行失败，下次按时重试", stop: "已停止" }[st] || "",
        }).catch(() => {});
    }
  }
  el._stop = () => {
    invoke("cancel_chat", { id }).catch(() => {});
    finish("stop");
    txt.classList.remove("cursor");
    el.querySelectorAll(".x-ap button, .x-ap input").forEach((b) => (b.disabled = true));
    if (out) {
      txt.innerHTML = md(seg);
      conv.msgs.push({ role: "assistant", content: out });
      saveMsg(conv, "assistant", out + "\n\n（已停止）").catch(() => {});
    } else txt.textContent = "（已停止）";
    setMeta(el, tools.size ? "" : "已停止", out ? { copy: out } : {});
    receipt();
    if (here() || !convs.size) mood("idle");
  };
  conv.stop = el._stop;
  if (here()) S.stopFn = el._stop;
  const messages =
    currentContent === undefined ? conv.msgs : [...conv.msgs.slice(0, -1), { role: "user", content: currentContent }];
  invoke("chat", {
    id,
    messages,
    temporary: conv.tmp || conv.tmpChanging,
    plan,
    preAllow,
    reviewAgent: reviewAgent?.id ?? null,
    onEvent: ch,
  }).catch((e) => ch.onmessage({ type: "error", message: String(e) }));
}

export function initChat() {
  chatSeq = 0;
}
