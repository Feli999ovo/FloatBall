import { hv, hvHide } from "./hover.js";
import { mood } from "./orb.js";
import { hidePeek, peekBox, showPeek } from "./peek.js";
import { $, ask, body, invoke, mutable, PAD, S } from "./state.js";
import { toast } from "./toast.js";
import { vcDictStop, vcRelease } from "./voice-finish.js";
import { collapse, grabBackdrop, monitorArea, reshape, winPos } from "./window.js";

export let VC_LEAD,
  VC_MIN,
  VC_CANCEL,
  VC_HOLD,
  VC_MAXMS,
  vcBub,
  vcRip,
  vcMic,
  vcTxt,
  vcTime,
  vcRings,
  vcBox,
  vcAsk0,
  vcCfg,
  vcSys,
  vcFmt,
  vcMime;

// 按住说话之前先看识别能不能用：系统识别要 Windows / macOS；服务商识别要有模型、有 Key。不能用就说清楚去哪改
export function vcNotReady() {
  if (vcSys())
    return S.cfg && ["windows", "macos"].includes(S.cfg.platform)
      ? ""
      : "这个系统没有自带语音识别，请在设置 → 语音里改用服务商识别";
  const ms = (S.cfg && S.cfg.models) || [];
  const v = vcCfg(),
    m = ms.find((x) => x.id === v.model) || ms.find((x) => x.id === S.cfg.default_model) || ms[0];
  if (!m) return "语音识别选的是服务商，但还没有添加模型：设置 → 模型里添加，或在设置 → 语音里改用系统识别";
  if (!m.no_key && !m.has_key)
    return "语音识别选的服务商还没填 API Key：设置 → 模型里补上，或在设置 → 语音里改用系统识别";
  return "";
}

export function vcSay(t, dim) {
  vcTxt.textContent = "";
  if (dim) {
    const i = document.createElement("i");
    i.textContent = t;
    vcTxt.appendChild(i);
  } else vcTxt.textContent = t;
}

// 收起时窗口只有小球那么大：录音时扩成「小球 + 涟漪 + 一侧的气泡」，小球留在原地（同悬停小泡、回复气泡的做法）
export async function vcOpenBox() {
  if (vcBox) return;
  const p = await winPos(),
    a = await monitorArea();
  const ORB = mutable.SMALL - PAD * 2,
    E = Math.ceil(ORB / 2 + 62),
    BW = 16 + 260,
    H = Math.max(E * 2, 132);
  const cxs = p.x + mutable.SMALL / 2,
    cys = p.y + mutable.SMALL / 2,
    right = cxs < a.x + a.w / 2;
  const w = E * 2 + BW,
    h = H;
  let x = right ? cxs - E : cxs - E - BW,
    y = cys - h / 2;
  x = Math.max(a.x, Math.min(x, a.x + a.w - w));
  y = Math.max(a.y, Math.min(y, a.y + a.h - h));
  const lx = cxs - x,
    ly = cys - y; // 小球中心在新窗口里的位置
  vcBox = { p, right, lx, ly, R: ORB / 2 };
  mutable.morphing = true;
  try {
    // 气泡和对话卡片用同一种毛玻璃：扩开前截下窗口将要盖住的那块桌面（见 grabBackdrop）
    vcBub.classList.remove("glassy");
    const bd = await grabBackdrop({ x, y, w, h }, a);
    await reshape(w, h, x, y, true, () => {
      body.classList.add("voicing");
      $("#app").style.padding = `${ly - ORB / 2}px 0 0 ${lx - ORB / 2}px`;
      vcBub.style.transformOrigin = right ? "0 50%" : "100% 50%";
      vcBub.style.borderRadius = right ? "4px 16px 16px 16px" : "16px 4px 16px 16px";
      vcBub.style.left = right ? lx + ORB / 2 + 16 + "px" : "auto";
      vcBub.style.right = right ? "auto" : w - lx + ORB / 2 + 16 + "px";
      if (bd) {
        vcBub.style.setProperty("--bd", 'url("' + bd + '")');
        vcBub.style.setProperty("--bd-size", w + "px " + h + "px");
        vcBub.classList.add("glassy");
      }
    });
  } finally {
    mutable.morphing = false;
  }
}

export async function vcCloseBox() {
  const b = vcBox;
  if (!b) return;
  vcBox = null;
  vcBub.classList.remove("glassy");
  if (S.open) {
    body.classList.remove("voicing");
    return;
  } // 已经展开：展开时会自己排版
  mutable.morphing = true;
  try {
    await reshape(mutable.SMALL, mutable.SMALL, b.p.x, b.p.y, true, () => {
      body.classList.remove("voicing");
      $("#app").style.padding = "";
    });
  } finally {
    mutable.morphing = false;
  }
}

export function vcPlaceBub() {
  if (!vcBox) return;
  const bh = vcBub.offsetHeight;
  vcBub.style.top = Math.max(4, Math.min(innerHeight - bh - 4, vcBox.ly - bh / 2)) + "px";
  vcBub.style.setProperty("--bd-pos", -vcBub.offsetLeft + "px " + -vcBub.offsetTop + "px"); // 底图按窗口截的，挪到气泡自己的位置
}

export function vcTick() {
  if (!mutable.vc || mutable.vc.stopped) return;
  const now = performance.now(),
    el = now - mutable.vc.t0;
  let L = 0;
  if (mutable.vc.an) {
    mutable.vc.an.getByteTimeDomainData(mutable.vc.buf);
    let sum = 0;
    for (const v of mutable.vc.buf) {
      const d = (v - 128) / 128;
      sum += d * d;
    }
    L = Math.min(1, Math.sqrt(sum / mutable.vc.buf.length) * 5);
  }
  mutable.vc.lv += (L - mutable.vc.lv) * 0.35;
  if (mutable.vc.mode === "dict") {
    vcMic.style.setProperty("--vc-l", mutable.vc.lv.toFixed(3));
  } else {
    body.style.setProperty("--vc-l", (0.8 + 0.45 * mutable.vc.lv).toFixed(3));
    vcTime.textContent = vcFmt(el);
    if (vcBox) {
      const { lx, ly, R } = vcBox;
      vcRings.forEach((g, i) => {
        const d = R + 8 + i * 13 + mutable.vc.lv * (5 + i * 4) + Math.sin(now / 260 - i * 0.9) * 1.5;
        g.style.width = g.style.height = d * 2 + "px";
        g.style.transform = `translate(${lx - d}px,${ly - d}px)`;
        g.style.opacity = ((0.7 - i * 0.2) * (0.35 + 0.65 * mutable.vc.lv)).toFixed(3);
      });
      vcPlaceBub();
    }
  }
  if (el > VC_MAXMS) return mutable.vc.mode === "dict" ? vcDictStop() : vcRelease(); // 最长 2 分钟
  mutable.vc.raf = requestAnimationFrame(vcTick);
}

// 开始录音。mode：press 长按小球 / key 快捷键 / dict 胶囊麦克风
export async function vcStart(mode) {
  if (mutable.vc || S.busy || mutable.morphing) return false;
  const nr = vcNotReady();
  if (nr) {
    toast(nr);
    if (!S.open) showPeek(nr, true);
    mood("error", 2400);
    return false;
  }
  const me = { mode, t0: performance.now(), cancel: false, stopped: false, lv: 0, chunks: [], raf: 0 };
  mutable.vc = me;
  if (hv.shown || hv.hiding) await hvHide();
  if (peekBox) await hidePeek();
  if (mode === "key" && S.open) await collapse();
  try {
    me.stream = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: true, noiseSuppression: true },
    });
  } catch (e) {
    mutable.vc = null;
    const msg =
      e && e.name === "NotAllowedError"
        ? "没有麦克风权限。macOS：系统设置 → 隐私与安全性 → 麦克风；Windows：设置 → 隐私 → 麦克风"
        : e && e.name === "NotFoundError"
          ? "没找到麦克风"
          : "打不开麦克风：" + ((e && e.message) || e);
    toast(msg);
    if (!S.open) showPeek(msg, true);
    mood("error", 2400);
    return false;
  }
  if (mutable.vc !== me) {
    me.stream.getTracks().forEach((t) => t.stop());
    return false;
  } // 等权限时已经松手、取消了
  // 系统识别：Windows 从这里开始听（macOS 松开后识别录音文件）
  if (vcSys()) {
    try {
      await invoke("stt_begin");
    } catch (e) {
      me.stream.getTracks().forEach((t) => t.stop());
      if (mutable.vc === me) mutable.vc = null;
      const msg = String(e);
      toast(msg);
      if (!S.open) showPeek(msg, true);
      mood("error", 2400);
      return false;
    }
    if (mutable.vc !== me) {
      me.stream.getTracks().forEach((t) => t.stop());
      invoke("stt_cancel").catch(() => {});
      return false;
    }
  }
  me.mime = vcMime();
  me.rec = new MediaRecorder(me.stream, me.mime ? { mimeType: me.mime } : undefined);
  me.rec.ondataavailable = (e) => {
    if (e.data && e.data.size) me.chunks.push(e.data);
  };
  me.rec.start(250);
  try {
    me.ctx = new AudioContext();
    me.an = me.ctx.createAnalyser();
    me.an.fftSize = 512;
    me.ctx.createMediaStreamSource(me.stream).connect(me.an);
    me.buf = new Uint8Array(me.an.fftSize);
  } catch (_) {
    me.an = null;
  }
  me.t0 = performance.now();
  if (mode === "dict") {
    vcAsk0 = ask.placeholder;
    ask.placeholder = "在听…";
    vcMic.classList.add("vc-on");
    vcMic.setAttribute("aria-pressed", "true");
    vcMic.setAttribute("aria-label", "停止听写");
    ask.focus();
  } else {
    if (!S.open) await vcOpenBox();
    if (mutable.vc !== me) return true;
    mood("listen");
    body.classList.add("vc-rec");
    vcBub.classList.remove("vc-x", "vc-out");
    vcRip.classList.remove("vc-x");
    vcSay(S.cfg.voice && S.cfg.voice.lang === "en" ? "Listening…" : "在听…", true);
    vcTime.textContent = "0:00";
    vcBub.classList.add("on");
    vcRip.classList.add("on");
  }
  vcTick();
  return true;
}

// 停止录音，返回录到的音频（取消时不要）
export function vcStopRec(me) {
  me.stopped = true;
  cancelAnimationFrame(me.raf);
  body.classList.remove("vc-rec");
  body.style.removeProperty("--vc-l");
  vcRip.classList.remove("on");
  const done = new Promise((r) => {
    if (!me.rec || me.rec.state === "inactive") return r();
    me.rec.onstop = () => r();
    me.rec.stop();
  });
  return done.then(() => {
    me.stream && me.stream.getTracks().forEach((t) => t.stop());
    me.ctx && me.ctx.close().catch(() => {});
    return new Blob(me.chunks, { type: (me.mime || "audio/webm").split(";")[0] });
  });
}

export function vcHideBub() {
  vcBub.classList.add("vc-out");
  return new Promise((r) =>
    setTimeout(() => {
      vcBub.classList.remove("on", "vc-out", "vc-x");
      vcRip.classList.remove("vc-x");
      r();
    }, 220),
  );
}

// 取消：气泡缩小淡出，小球「嗯？」眯一下眼
export async function vcCancel(msg) {
  const me = mutable.vc;
  if (!me || me.mode === "dict") return false;
  mutable.vc = null;
  vcStopRec(me);
  if (vcSys()) invoke("stt_cancel").catch(() => {});
  body.classList.add("vc-eh");
  setTimeout(() => body.classList.remove("vc-eh"), 450);
  mood("idle");
  await vcHideBub();
  await vcCloseBox();
  if (msg) {
    toast(msg);
    if (!S.open) showPeek(msg, true);
  }
  return true;
}

export function blobB64(b) {
  return new Promise((res, rej) => {
    const r = new FileReader();
    r.onload = () => res(String(r.result).split(",")[1] || "");
    r.onerror = rej;
    r.readAsDataURL(b);
  });
}

export async function vcRecognize(blob) {
  const audio = await blobB64(blob);
  return invoke("transcribe", { audio, mime: blob.type || "audio/webm" });
}

export function initVoice() {
  /* ───────── 语音（原型的 vcStart / vcRelease / vcDict） ─────────
     三种开始方式：长按小球（按住说话，松开结束）· 「按住说话」快捷键（默认单独按住右 Alt）· 胶囊里的麦克风（听写进输入框）。
     录音用网页的麦克风接口，松开后把整段录音交给语音设置里选的服务商识别（/audio/transcriptions）。
     和原型不同：识别接口不是流式的，说话时气泡里显示「在听…」和时长，松开后显示「识别中…」，没有逐字出现。 */
  VC_LEAD = 300;
  VC_MIN = 600;
  VC_CANCEL = 48;
  VC_HOLD = 350;
  VC_MAXMS = 120000;
  vcBub = $("#vcBub");
  vcRip = $("#vcRip");
  vcMic = $("#vcMic");
  vcTxt = vcBub.querySelector(".vc-txt");
  vcTime = vcBub.querySelector(".vc-time");
  vcRings = [...vcRip.children];
  mutable.vc = null;
  vcBox = null;
  mutable.vcKeyT = 0;
  vcAsk0 = "";
  vcCfg = () => (S.cfg && S.cfg.voice) || { lang: "auto", after: "send", tts: false, engine: "system" };
  vcSys = () => vcCfg().engine !== "provider";
  vcFmt = (ms) => {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  };
  // macOS 系统识别读不了 webm，录成 mp4（m4a）
  vcMime = () =>
    (vcSys() && S.cfg.platform === "macos"
      ? ["audio/mp4", "audio/webm;codecs=opus", "audio/webm"]
      : ["audio/webm;codecs=opus", "audio/webm", "audio/mp4", "audio/ogg;codecs=opus"]
    ).find((t) => window.MediaRecorder && MediaRecorder.isTypeSupported && MediaRecorder.isTypeSupported(t)) || "";
}
