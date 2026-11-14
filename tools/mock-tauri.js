// 只用于在普通浏览器里测试界面：模拟 window.__TAURI__ 的一小部分。真实程序不会加载它。
(() => {
  const st = { pos: { x: 1300, y: 380 }, size: { w: 96, h: 96 }, mon: { w: 1440, h: 900 },
    cfg: { models: [], default_model: 0, agents: [{ id: "lead", name: "主控", role: "拆解与分派", prompt: "你是住在用户桌面上的悬浮球助手。", tint: "#C9B8F2", emoji: "", tools: ["搜索网页", "读文件", "写文件"], model: 0, fallback: 0, auto: false }], limits: { tokens: 50000, steps: 50, parallel: 3 }, always_allow: [], pause_away: true, shortcut: "Alt+Space", capture_shortcut: "Alt+Shift+S", new_shortcut: "Alt+N", tasks_shortcut: "Alt+T", position: null, orb_size: 72, glass: 25, eye: "#FFF1DC", idle_ms: 600000, follow_system: false, auto_dock: true, calm: false, screen: "mouse", screen_slide: true, hover: { on: true, delay: 350, order: [{ id: "shot", show: true }, { id: "new", show: true }, { id: "hist", show: true }, { id: "tasks", show: true }] }, autostart: false, platform: "windows", catalog_date: "2026-11-14" },
    convs: [], nextId: 1, listeners: {}, moved: [], focus: [], calls: [] };
  window.__mock = st;
  class Channel { constructor() { this.onmessage = () => {}; } }
  const reply = (q) => q.includes("错误") ? null :
    "好的，这是**三个要点**：\n\n1. 先把窗口做成透明置顶\n2. 再接上 `chat/completions` 流式接口\n3. 最后加托盘和快捷键\n\n```rust\nfn main() {\n    floatball_lib::run()\n}\n```\n\n需要的话我可以继续展开。";
  const cur = () => st.cfg.models.find((m) => m.id === st.cfg.default_model);
  const view = () => { const m = cur(); return { ...st.cfg, models: st.cfg.models.map((x) => ({ ...x })), ready: !!m && (m.no_key || m.has_key) }; };
  // 真实程序里设置变了会给小球窗口发 config 事件
  const changed = () => setTimeout(() => window.__emit("config"), 0);
  async function invoke(cmd, a = {}) {
    st.calls.push(cmd);
    switch (cmd) {
      case "get_config": return view();
      case "screens": return { cursor: st.cursor || null, list: [{ x: 0, y: 0, w: st.mon.w * 2, h: st.mon.h * 2, scale: 2, primary: true }, ...(st.screens || [])] };
      case "save_model": {
        const m = { ...a.model };
        let old = st.cfg.models.find((x) => m.id && x.id === m.id);
        if (old) { const k = old.has_key; Object.assign(old, m); old.has_key = k; }
        else { m.id = 1; while (st.cfg.models.some((x) => x.id === m.id)) m.id++; m.has_key = false; st.cfg.models.push(m); old = m; if (!cur()) st.cfg.default_model = m.id; }
        if (a.apiKey !== null && a.apiKey !== undefined) old.has_key = a.apiKey !== "";
        st.lastKey = a.apiKey; changed(); return old.id;
      }
      case "delete_model": st.cfg.agents.forEach((g) => { if (g.model === a.id) g.model = 0; if (g.fallback === a.id) g.fallback = 0; }); st.cfg.models = st.cfg.models.filter((x) => x.id !== a.id); if (st.cfg.default_model === a.id) st.cfg.default_model = st.cfg.models.length ? st.cfg.models[0].id : 0; changed(); return null;
      case "set_default_model": st.cfg.default_model = a.id; changed(); return null;
      case "save_agent": {
        const g = { ...a.agent, tools: [...new Set(a.agent.tools)] };
        if (!st.cfg.models.some((m) => m.id === g.model)) g.model = 0;
        if (!st.cfg.models.some((m) => m.id === g.fallback) || g.fallback === g.model) g.fallback = 0;
        if (g.id === "lead" && !g.tools.includes("读文件")) g.tools.push("读文件");
        const i = st.cfg.agents.findIndex((x) => g.id && x.id === g.id);
        if (i >= 0) st.cfg.agents[i] = g; else { let n = 1; while (st.cfg.agents.some((x) => x.id === "ag" + n)) n++; g.id = "ag" + n; st.cfg.agents.push(g); }
        (st.savedAgents ||= []).push(g); changed(); return g.id;
      }
      case "delete_agent": if (a.id === "lead") throw "主控不能删"; st.cfg.agents = st.cfg.agents.filter((x) => x.id !== a.id); changed(); return null;
      case "save_limits": st.cfg.limits = { ...a.limits }; changed(); return null;
      case "answer_ask": (st.answers ||= []).push(a); if (st.onAnswer) { const f = st.onAnswer; st.onAnswer = null; setTimeout(() => f(a.answer), 30); return true; } return false;
      case "save_perm": st.cfg.always_allow = a.alwaysAllow; st.cfg.pause_away = a.pauseAway; changed(); return null;
      case "save_prefs": Object.assign(st.cfg, a.prefs); changed(); return st.shortcutBusy ? a.prefs.shortcut + " 被别的程序占用了" : null;
      // 能力资料：真实程序查内置的 models.dev 快照，这里只放测试用到的几个
      case "model_caps": {
        const T = { "gpt-4o-mini": [true, true, false, 128000, "GPT-4o mini", "exact"], "gpt-4o": [true, true, false, 128000, "GPT-4o", "exact"], "deepseek-chat": [true, false, false, 128000, "DeepSeek Chat", "exact"], "deepseek-reasoner": [true, false, true, 128000, "DeepSeek Reasoner", "exact"], "qwen2.5:7b": [true, false, false, 131072, "Qwen2.5 7B Instruct", "guess"] };
        const LOGO = { "OpenAI": "openai", "DeepSeek": "deepseek", "通义千问": "alibaba-cn", "Kimi": "moonshotai-cn", "OpenRouter": "openrouter", "Ollama（本地）": "ollama-cloud" };
        const r = T[a.model.trim().toLowerCase()], local = a.provider.startsWith("Ollama") || /localhost|127\.0\.0\.1/.test(a.baseUrl);
        st.capsCalls = (st.capsCalls || 0) + 1;
        return r ? { tools: r[0], vision: r[1], reasoning: r[2], ctx: r[3], local, src: r[5], name: r[4], provider: LOGO[a.provider] || "" }
                 : { tools: null, vision: null, reasoning: null, ctx: null, local, src: "none", name: "", provider: LOGO[a.provider] || "" };
      }
      case "catalog_refresh": st.catalogRefresh = (st.catalogRefresh || 0) + 1; return null;
      case "js_log": (st.logs ||= []).push(a.msg); return null;
      case "list_models": st.listArgs = a; if (st.listFails) throw "连不上：connection refused"; return ["deepseek-chat", "deepseek-reasoner", "gpt-4o-mini"];
      case "open_settings": st.settingsOpened = (st.settingsOpened || 0) + 1; return null;
      case "set_autostart": st.cfg.autostart = a.on; return a.on;
      case "save_position": st.cfg.position = [a.x, a.y]; return null;
      case "test_connection": st.testArgs = a; if (/bad/.test(a.baseUrl)) throw "API Key 不对或没有权限（401）：Incorrect API key provided: sk-xx****. You can find your API key at https://platform.openai.com/account/api-keys."; return "连接成功，用时 412 毫秒";
      case "backdrop": st.backdropArgs = a; return st.backdrop || null;
      case "show_context_menu": st.menuShown = true; return null;
      case "capture_screen": return "data:image/svg+xml;base64," + btoa('<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100"><rect width="160" height="100" fill="#4b6cb7"/><rect x="20" y="20" width="80" height="12" fill="#fff"/></svg>');
      case "read_text_file": return { name: a.path.split("/").pop(), text: "第一行\n第二行" };
      case "history_append": {
        let c = st.convs.find((c) => c.id === a.convId);
        if (!c) { c = { id: st.nextId++, title: a.content.slice(0, 24), updated: Date.now(), msgs: [] }; st.convs.unshift(c); }
        c.msgs.push({ role: a.role, content: a.content }); c.updated = Date.now(); return c.id;
      }
      case "history_list": return st.convs.map((c) => ({ id: c.id, title: c.title, updated: c.updated, count: c.msgs.length }));
      case "history_load": return st.convs.find((c) => c.id === a.convId).msgs;
      case "history_delete": st.convs = st.convs.filter((c) => c.id !== a.convId); return null;
      case "history_clear": st.convs = []; return null;
      case "cancel_chat": st.cancelled = a.id; return null;
      case "chat": {
        const last = a.messages[a.messages.length - 1];
        const q = typeof last.content === "string" ? last.content : last.content[0].text;
        st.lastMessages = a.messages;
        const ch = a.onEvent;
        if (!view().ready) { setTimeout(() => ch.onmessage({ type: "error", message: "还没有填 API Key。右键小球 → 设置 → 模型。" }), 50); return null; }
        // 用工具、需要确认（删除）：看文件夹 → 等确认 → 删或不删
        if (/删除/.test(q)) {
          const send = (e) => { if (st.cancelled !== a.id) ch.onmessage(e); };
          const T = (call, name, arg, status, result = "") => send({ type: "tool", call, agent: "主控", name, arg, status, result });
          setTimeout(() => {
            send({ type: "delta", text: "我先看看下载文件夹。" });
            T("t0", "查看文件夹", "/Users/j/Downloads", "run");
            setTimeout(() => {
              T("t0", "查看文件夹", "/Users/j/Downloads", "done", "报告.pdf（20480 字节）\n报告 (1).pdf（20480 字节）");
              T("t1", "删除文件", "报告 (1).pdf", "wait");
              send({ type: "ask", ask: 7, call: "t1", kind: "删除", desc: "删除 /Users/j/Downloads/报告 (1).pdf", scope: "Downloads", hi: true });
              st.onAnswer = (ans) => {
                if (ans.allow) {
                  T("t1", "删除文件", "报告 (1).pdf", "run");
                  setTimeout(() => { T("t1", "删除文件", "报告 (1).pdf", "done", "已删除，移到了悬浮球回收站"); send({ type: "delta", text: "删好了，重复的那份在悬浮球回收站里。" }); send({ type: "usage", prompt: 1840, completion: 0 }); send({ type: "done" }); }, 100);
                } else {
                  T("t1", "删除文件", "报告 (1).pdf", "no", "用户拒绝了这个操作" + (ans.reason ? "，原因：" + ans.reason : ""));
                  setTimeout(() => { send({ type: "delta", text: "好的，没删。" }); send({ type: "done" }); }, 50);
                }
              };
            }, 150);
          }, 100);
          return null;
        }
        const r = reply(q);
        if (!r) { setTimeout(() => ch.onmessage({ type: "error", message: "API Key 不对或没有权限（401）：Incorrect API key" }), 300); return null; }
        const parts = r.match(/[\s\S]{1,6}/g); let i = 0;
        setTimeout(function tick() {
          if (st.cancelled === a.id) return;
          if (i < parts.length) { ch.onmessage({ type: "delta", text: parts[i++] }); setTimeout(tick, st.slow ? 200 : 15); }
          else { ch.onmessage({ type: "usage", prompt: 120, completion: 64 }); ch.onmessage({ type: "done" }); }
        }, st.slow ? 600 : 250);
        return null;
      }
    }
    throw new Error("mock: unknown command " + cmd);
  }
  const win = {
    close: async () => { st.closed = (st.closed || 0) + 1; },
    // 多显示器：st.screens 里第二块屏（物理像素，缩放都按 2）。没有就只有主屏
    _mon: async () => {
      const cx = (st.pos.x + st.size.w / 2) * 2, cy = (st.pos.y + st.size.h / 2) * 2;
      const o = (st.screens || []).find((m) => !m.primary && cx >= m.x && cx < m.x + m.w && cy >= m.y && cy < m.y + m.h);
      if (o) return { scaleFactor: 2, position: { x: o.x, y: o.y }, size: { width: o.w, height: o.h }, workArea: { position: { x: o.x, y: o.y }, size: { width: o.w, height: o.h } } };
      return { scaleFactor: 2, position: { x: 0, y: 0 }, size: { width: st.mon.w * 2, height: st.mon.h * 2 }, workArea: { position: { x: 0, y: 50 }, size: { width: st.mon.w * 2, height: (st.mon.h - 25) * 2 } } };
    },
    outerPosition: async () => ({ x: st.pos.x * 2, y: st.pos.y * 2 }),
    outerSize: async () => ({ width: st.size.w * 2, height: st.size.h * 2 }),
    scaleFactor: async () => 2,
    setSize: async (s) => { st.size = { w: s.width, h: s.height }; if (window.__resize) await window.__resize(s.width, s.height); },
    setPosition: async (p) => { st.pos = p.phys ? { x: p.x / 2, y: p.y / 2 } : { x: p.x, y: p.y }; (st.posLog ||= []).push(st.pos); },
    setFocus: async () => { st.focused = true; },
    startDragging: async () => { st.dragStarted = true; },
    onMoved: (f) => st.moved.push(f),
    onFocusChanged: (f) => st.focus.push(f),
  };
  window.__TAURI__ = {
    core: { invoke, Channel },
    event: { listen: async (n, f) => { (st.listeners[n] ||= []).push(f); }, emitTo: async (to, n, p) => { (st.emitted ||= []).push([to, n, p]); } },
    // 与真实 @tauri-apps/api 一致：currentMonitor 是 window 模块的函数，不是窗口对象的方法
    window: { getCurrentWindow: () => win, currentMonitor: () => (st.monitorFails ? Promise.reject(new Error("no monitor")) : win._mon()) },
    webview: { getCurrentWebview: () => ({ onDragDropEvent: (f) => (st.drop = f) }) },
    dpi: { LogicalSize: class { constructor(w, h) { this.width = w; this.height = h; } }, LogicalPosition: class { constructor(x, y) { this.x = x; this.y = y; } }, PhysicalPosition: class { constructor(x, y) { this.x = x; this.y = y; this.phys = true; } } },
  };
  window.__emit = (n, p) => (st.listeners[n] || []).forEach((f) => f({ payload: p }));
})();
