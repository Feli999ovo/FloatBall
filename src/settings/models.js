import { setLogo } from "./capabilities.js";
import {
  capsHTML,
  fBase,
  fEye,
  fHint,
  fKey,
  fList,
  fModel,
  fProv,
  fRes,
  isDef,
  models,
  PROV,
  showCaps,
  slash,
} from "./providers.js";
import { reload } from "./startup.js";
import { $, invoke, jslog, mutable, toast } from "./state.js";

export let HOST_PROV, oldCaps;

/* ───────── 模型库 ───────── */
export function renderLib() {
  const ul = $("libList");
  ul.innerHTML = "";
  $("libEmpty").hidden = !!models().length;
  models().forEach((m) => {
    const def = isDef(m);
    const li = document.createElement("li");
    li.className = "x-mc" + (def ? " def" : "");
    li.innerHTML =
      `<p class="x-mcn"><i class="x-logo" aria-hidden="true"></i><b></b>${def ? '<span class="x-star">★ 默认</span>' : ""}</p><p class="x-mcp"></p><div class="x-caps">${capsHTML(capsOf(m))}</div>` +
      `<div class="x-mca">${def ? "" : '<button type="button" data-a="def">设为默认</button>'}<button type="button" data-a="edit">编辑</button><button type="button" class="x-del" data-a="del">删除</button></div>`;
    setLogo(li.querySelector(".x-logo"), m);
    li.querySelector("b").textContent = m.name || m.model;
    const noKey = !m.no_key && !m.has_key;
    li.querySelector(".x-mcp").textContent =
      provLabel(m).replace("（本地）", "") + " · " + m.model + (noKey ? " · 还没填 Key" : "");
    li.querySelectorAll(".x-mca button").forEach((b) => b.setAttribute("aria-label", b.textContent + " " + m.name));
    li.querySelector(".x-mca").addEventListener("click", async (e) => {
      const b = e.target.closest("button");
      const a = b && b.dataset.a;
      try {
        if (a === "edit") openForm(m);
        if (a === "def") {
          await invoke("set_default_model", { id: m.id });
          await reload();
          toast(`默认模型：${m.name || m.model}`);
        }
        // 原型点一下就删；这里沿用原来的做法，要再点一次「确认删除」，免得误删
        if (a === "del" && b.dataset.sure !== "1") {
          b.dataset.sure = "1";
          b.textContent = "确认删除";
          b.setAttribute("aria-label", "确认删除 " + (m.name || m.model));
          setTimeout(() => {
            if (b.isConnected) {
              b.dataset.sure = "";
              b.textContent = "删除";
              b.setAttribute("aria-label", "删除 " + (m.name || m.model));
            }
          }, 3000);
          return;
        }
        if (a === "del") {
          await invoke("delete_model", { id: m.id });
          await reload();
          toast(`已删除 ${m.name || m.model}`);
          $("libAdd").focus({ preventScroll: true });
        }
      } catch (err) {
        toast(String(err));
      }
    });
    ul.appendChild(li);
  });
  $("libSrc").textContent =
    mutable.cfg && mutable.cfg.catalog_date
      ? `能力资料来自 models.dev（${mutable.cfg.catalog_date}）` +
        (mutable.cfg.catalog_auto ? "，每天自动更新" : "，用的是安装包自带的那份")
      : "";
  $("libAuto").setAttribute("aria-checked", !!(mutable.cfg && mutable.cfg.catalog_auto));
}

export function provLabel(m) {
  if (m.provider) return m.provider;
  let h = "";
  try {
    h = new URL(m.base_url).hostname;
  } catch (_) {}
  if (/^(localhost|127\.|0\.0\.0\.0|\[::1\])/.test(h) || m.no_key) return "Ollama（本地）";
  return HOST_PROV[h] || "自定义";
}

export function capsOf(m) {
  const local = m.no_key || provLabel(m).startsWith("Ollama");
  if (m.caps_src)
    return {
      tools: m.tools,
      vision: m.vision_known ? m.vision : null,
      reasoning: m.reasoning,
      ctx: m.ctx,
      local,
      src: m.caps_src,
      name: m.name,
      provider: m.logo,
    };
  const k = m.id + "|" + m.base_url + "|" + m.model;
  if (oldCaps[k]) return { ...oldCaps[k], vision: m.vision, local };
  invoke("model_caps", { provider: provLabel(m), baseUrl: m.base_url, model: m.model })
    .then((c) => {
      oldCaps[k] = c;
      m.logo = c.provider;
      if (!$("paneLib").hidden) renderLib();
    })
    .catch(jslog);
  return { tools: null, vision: m.vision, reasoning: null, ctx: null, local, src: "none" };
}

export function openForm(m) {
  mutable.editing = m || null;
  fList.hidden = true;
  fRes.textContent = "";
  fRes.className = "x-res";
  $("fTitle").textContent = m ? "编辑模型" : "添加模型";
  fKey.type = "password";
  slash.style.display = "none";
  fEye.setAttribute("aria-pressed", "false");
  fEye.setAttribute("aria-label", "显示 Key");
  if (m) {
    fProv.value = PROV[provLabel(m)] ? provLabel(m) : "自定义";
    syncProv();
    fBase.value = m.base_url;
    fKey.value = "";
    fModel.value = m.model;
    fKey.placeholder = m.has_key ? "已保存（留空不改）" : "sk-…";
    mutable.detected = capsOf(m);
  } else {
    syncProv();
    fKey.value = "";
    fModel.value = "";
    fKey.placeholder = "sk-…";
  }
  showCaps();
  $("paneLib").hidden = true;
  $("paneModel").hidden = false;
  $("paneModel").scrollTop = 0;
  setTimeout(() => fProv.focus({ preventScroll: true }), 60);
}

export function syncProv() {
  const p = PROV[fProv.value];
  fBase.value = p[0];
  fModel.placeholder = "如 " + p[1][0];
  fHint.hidden = !fProv.value.startsWith("Ollama");
  fList.hidden = true;
  fRes.textContent = "";
  fRes.className = "x-res";
  mutable.detected = null;
  showCaps();
}

export function initModels() {
  // 早期版本存的模型没有服务商和能力字段：按地址认服务商，能力当场查资料（只用来显示，保存时才写进配置）
  HOST_PROV = {
    "api.openai.com": "OpenAI",
    "api.deepseek.com": "DeepSeek",
    "dashscope.aliyuncs.com": "通义千问",
    "api.moonshot.cn": "Kimi",
    "openrouter.ai": "OpenRouter",
  };
  oldCaps = {};
}
