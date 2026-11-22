import { mk } from "./agent-form.js";
import { reload } from "./startup.js";
import { $, invoke, mutable, toast } from "./state.js";

/* ───────── 权限（原型的 renderPerm）：撤销总是允许、离开电脑时暂停 ───────── */
export async function savePerm(list, away) {
  try {
    await invoke("save_perm", { alwaysAllow: list, pauseAway: away });
    await reload();
    return true;
  } catch (e) {
    toast(String(e));
    return false;
  }
}

export function renderPerm() {
  const ul = $("alList"),
    list = mutable.cfg.always_allow || [];
  ul.innerHTML = "";
  $("alEmpty").hidden = list.length > 0;
  list.forEach((v, i) => {
    const li = mk("li");
    li.innerHTML = '<span><b></b><small></small></span><button type="button" class="x-sb">撤销</button>';
    li.querySelector("b").textContent = v.kind;
    li.querySelector("small").textContent = "范围：" + (v.scope || "全部");
    const b = li.querySelector("button");
    b.setAttribute("aria-label", `撤销 ${v.kind} · ${v.scope}`);
    b.addEventListener("click", async () => {
      if (
        await savePerm(
          list.filter((_, j) => j !== i),
          mutable.cfg.pause_away,
        )
      ) {
        toast(`已撤销：${v.kind} · ${v.scope}`);
        $("awaySw").focus({ preventScroll: true });
      }
    });
    ul.appendChild(li);
  });
  $("awaySw").setAttribute("aria-checked", !!mutable.cfg.pause_away);
}

export function initPermissions() {
  $("awaySw").addEventListener("click", async () => {
    if (await savePerm(mutable.cfg.always_allow || [], !mutable.cfg.pause_away)) toast("已保存");
  });
}
