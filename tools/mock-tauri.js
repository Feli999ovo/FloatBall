// 只用于在普通浏览器里测试界面：模拟 window.__TAURI__ 的一小部分。真实程序不会加载它。
(() => {
  const st = { pos: { x: 1300, y: 380 }, size: { w: 96, h: 96 }, mon: { w: 1440, h: 900 },
    cfg: { base_url: "https://api.openai.com/v1", model: "gpt-4o-mini", vision: true, system_prompt: "你是住在用户桌面上的悬浮球助手。", shortcut: "Alt+Space", capture_shortcut: "Alt+Shift+S", position: null, allow_no_key: false, has_key: false, autostart: false, platform: "macos" },
    convs: [], nextId: 1, listeners: {}, moved: [], focus: [], calls: [] };
  window.__mock = st;
  class Channel { constructor() { this.onmessage = () => {}; } }
  const reply = (q) => q.includes("错误") ? null :
    "好的，这是**三个要点**：\n\n1. 先把窗口做成透明置顶\n2. 再接上 `chat/completions` 流式接口\n3. 最后加托盘和快捷键\n\n```rust\nfn main() {\n    floatball_lib::run()\n}\n```\n\n需要的话我可以继续展开。";
  async function invoke(cmd, a = {}) {
    st.calls.push(cmd);
    switch (cmd) {
      case "get_config": return { ...st.cfg };
      case "save_config": Object.assign(st.cfg, a.cfg); if (a.apiKey) st.cfg.has_key = true; return st.shortcutBusy ? a.cfg.shortcut + " 被别的程序占用了" : null;
      case "set_autostart": st.cfg.autostart = a.on; return a.on;
      case "save_position": st.cfg.position = [a.x, a.y]; return null;
      case "test_connection": return "连接成功，用时 412 毫秒";
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
        if (!st.cfg.has_key && !st.cfg.allow_no_key) { setTimeout(() => ch.onmessage({ type: "error", message: "还没有填 API Key。右键小球 → 设置 → 模型。" }), 50); return null; }
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
    _mon: async () => ({ scaleFactor: 2, position: { x: 0, y: 0 }, size: { width: st.mon.w * 2, height: st.mon.h * 2 }, workArea: { position: { x: 0, y: 50 }, size: { width: st.mon.w * 2, height: (st.mon.h - 25) * 2 } } }),
    outerPosition: async () => ({ x: st.pos.x * 2, y: st.pos.y * 2 }),
    outerSize: async () => ({ width: st.size.w * 2, height: st.size.h * 2 }),
    scaleFactor: async () => 2,
    setSize: async (s) => { st.size = { w: s.width, h: s.height }; if (window.__resize) await window.__resize(s.width, s.height); },
    setPosition: async (p) => { st.pos = { x: p.x, y: p.y }; },
    setFocus: async () => { st.focused = true; },
    startDragging: async () => { st.dragStarted = true; },
    onMoved: (f) => st.moved.push(f),
    onFocusChanged: (f) => st.focus.push(f),
  };
  window.__TAURI__ = {
    core: { invoke, Channel },
    event: { listen: async (n, f) => { (st.listeners[n] ||= []).push(f); } },
    // 与真实 @tauri-apps/api 一致：currentMonitor 是 window 模块的函数，不是窗口对象的方法
    window: { getCurrentWindow: () => win, currentMonitor: () => (st.monitorFails ? Promise.reject(new Error("no monitor")) : win._mon()) },
    webview: { getCurrentWebview: () => ({ onDragDropEvent: (f) => (st.drop = f) }) },
    dpi: { LogicalSize: class { constructor(w, h) { this.width = w; this.height = h; } }, LogicalPosition: class { constructor(x, y) { this.x = x; this.y = y; } } },
  };
  window.__emit = (n, p) => (st.listeners[n] || []).forEach((f) => f({ payload: p }));
})();
