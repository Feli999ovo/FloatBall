export const mutable = {};

export let T,
  invoke,
  Channel,
  listen,
  appWin,
  LogicalSize,
  LogicalPosition,
  PhysicalPosition,
  jslog,
  $,
  body,
  orb,
  ask,
  PAD,
  CARD_W,
  CARD_H,
  CAP_W,
  OVER,
  BIG_W,
  bigH,
  S;

export function initState() {
  T = window.__TAURI__;
  ({ invoke, Channel } = T.core);
  ({ listen } = T.event);
  appWin = T.window.getCurrentWindow();
  ({ LogicalSize, LogicalPosition, PhysicalPosition } = T.dpi);
  // 前端出错时打到终端
  jslog = (m) => invoke("js_log", { msg: String(m) }).catch(() => {});
  window.addEventListener("error", (e) => jslog(e.message + " @" + e.lineno));
  window.addEventListener("unhandledrejection", (e) =>
    jslog(
      "promise: " +
        (e.reason && e.reason.message ? e.reason.message + " " : "") +
        ((e.reason && e.reason.stack) || e.reason),
    ),
  );
  $ = (s) => document.querySelector(s);
  body = document.body;
  orb = $("#orb");
  ask = $("#ask");
  // 尺寸（逻辑像素）
  PAD = 12;
  mutable.SMALL = 72 + PAD * 2;
  // 收起时的窗口边长 = 小球直径 + 两边留白；小球大小在设置里能改（见 applyLook）
  // 展开后照原型：卡片 340×260，胶囊 288 长；卡片在胶囊上居中，所以小球那一侧多出 OVER
  CARD_W = 340;
  CARD_H = 260;
  CAP_W = 288;
  OVER = (CARD_W - CAP_W) / 2;
  BIG_W = CARD_W + PAD * 2;
  bigH = () => PAD * 2 + (mutable.SMALL - PAD * 2) + 8 + CARD_H;
  // 留白 + 胶囊（小球高）+ 间距 + 卡片
  S = {
    open: false,
    pinned: false,
    busy: null, // 当前对话里正在生成的请求 id（别的对话在后台跑的不算）
    convId: null, // 历史里的对话 id
    msgs: [], // 发给模型的消息 {role, content}
    conv: null, // 当前对话（见 Conv，启动时建一个空的）。切走时它的气泡收进 conv.dom，在后台接着生成
    attach: [], // 待发送的附件 {kind:'image'|'file', url|name,text}
    anchor: null, // 收起时小球窗口左上角（逻辑坐标）
    cfg: null,
    lastActive: Date.now(),
    tasks: [], // 这次打开以来发出的请求 {title, st: run|done|err|stop, t0, t1}
  };
}
