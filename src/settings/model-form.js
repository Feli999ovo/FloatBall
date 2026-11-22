import { lookupCaps } from "./capabilities.js";
import { fBase, fEye, fFetch, fKey, fList, fModel, fProv, fRes, fSave, fTest, PROV, slash } from "./providers.js";
import { reload } from "./startup.js";
import { $, invoke, mutable, T, toast, win } from "./state.js";
import { showTab } from "./tabs.js";

export function res(kind, text) {
  fRes.className = "x-res" + (kind === "bad" ? " bad" : "");
  fRes.innerHTML = kind === "ok" ? '<span class="x-ok"></span>' : kind === "spin" ? '<span class="x-spin"></span>' : "";
  if (text) {
    const s = document.createElement("span");
    s.className = "t";
    s.textContent = text;
    s.title = text;
    fRes.appendChild(s);
  }
}

export function initModelForm() {
  /* ───────── 表单按钮 ───────── */
  fEye.addEventListener("click", () => {
    const show = fKey.type === "password";
    fKey.type = show ? "text" : "password";
    slash.style.display = show ? "" : "none";
    fEye.setAttribute("aria-pressed", show);
    fEye.setAttribute("aria-label", show ? "隐藏 Key" : "显示 Key");
  });
  fFetch.addEventListener("click", async () => {
    if (!fBase.value.trim()) return res("bad", "请填写接口地址");
    fFetch.disabled = true;
    fFetch.innerHTML = '<span class="x-spin" aria-label="加载中"></span>';
    try {
      const ids = await invoke("list_models", {
        baseUrl: fBase.value.trim(),
        apiKey: fKey.value.trim() || null,
        id: mutable.editing ? mutable.editing.id : null,
      });
      fList.innerHTML = "";
      ids.forEach((m) => {
        const o = document.createElement("button");
        o.type = "button";
        o.textContent = m;
        o.addEventListener("click", () => {
          fModel.value = m;
          fList.hidden = true;
          lookupCaps(0);
        });
        fList.appendChild(o);
      });
      fList.hidden = false;
      fList.scrollTop = 0;
      res("", "");
    } catch (e) {
      fList.hidden = true;
      res("bad", String(e));
    }
    fFetch.disabled = false;
    fFetch.textContent = "获取列表";
  });
  fTest.addEventListener("click", async () => {
    if (!fBase.value.trim()) return res("bad", "请填写接口地址");
    const model = fModel.value.trim() || PROV[fProv.value][1][0];
    fTest.disabled = true;
    res("spin", "测试中…");
    try {
      const r = await invoke("test_connection", {
        baseUrl: fBase.value.trim(),
        model,
        apiKey: fKey.value.trim() || null,
        id: mutable.editing ? mutable.editing.id : null,
      });
      const ms = String(r).match(/(\d+)\s*毫秒/);
      res("ok", "连接成功" + (ms ? ` · ${ms[1]} 毫秒` : ""));
      if (!fModel.value.trim()) fModel.value = model;
      if (!mutable.detected || mutable.detected.src !== "manual") lookupCaps(0);
      // 原型：悬浮球用「完成」状态开心一下
      T.event.emitTo("main", "ball-state", "done").catch(() => {});
    } catch (e) {
      res("bad", String(e));
    }
    fTest.disabled = false;
  });
  fSave.addEventListener("click", async () => {
    const model = fModel.value.trim();
    if (!model) {
      res("bad", "请填写模型名");
      fModel.focus();
      return;
    }
    if (!/^https?:\/\//.test(fBase.value.trim())) {
      res("bad", "接口地址要以 http:// 或 https:// 开头");
      fBase.focus();
      return;
    }
    let c = mutable.detected;
    if (!c) {
      try {
        c = await invoke("model_caps", { provider: fProv.value, baseUrl: fBase.value.trim(), model });
      } catch (_) {
        c = {};
      }
    }
    // vision 不知道时按支持处理（截图照发，模型不收会报错），和以前一样
    const local = !!c.local || fProv.value.startsWith("Ollama");
    const entry = {
      id: mutable.editing ? mutable.editing.id : 0,
      name: mutable.editing && mutable.editing.model === model ? mutable.editing.name : "",
      provider: fProv.value,
      base_url: fBase.value.trim(),
      model,
      vision: c.vision !== false,
      vision_known: c.vision === true || c.vision === false,
      tools: c.tools ?? null,
      reasoning: c.reasoning ?? null,
      ctx: c.ctx || null,
      no_key: local && !fKey.value.trim() && !(mutable.editing && mutable.editing.has_key),
      caps_src: c.src || "none",
      logo: c.provider || "",
    };
    fSave.disabled = true;
    try {
      await invoke("save_model", { model: entry, apiKey: fKey.value.trim() || null });
      mutable.editing = null;
      fKey.value = "";
      await reload();
      toast("已保存");
      if (mutable.fromTip) {
        mutable.fromTip = false;
        win.close();
      } else {
        showTab("model");
        $("libAdd").focus({ preventScroll: true });
      }
    } catch (e) {
      res("bad", String(e));
    }
    fSave.disabled = false;
  });
  document.addEventListener("click", (e) => {
    if (!e.target.closest("#fList,#fFetch")) fList.hidden = true;
  });
}
