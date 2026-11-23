import base64
import os

from ui_support import check, OUT, ROOT


async def test_context_menu(b, base_url):
    # ───────── 右键菜单窗口（原型 #menu） ─────────
    # 底图：壁纸（铺满 1440×900）里菜单将要盖住的那块（小球旁 1180,420），和 capture.rs 一样缩小 8 倍、模糊、提饱和度
    from PIL import Image as _I, ImageFilter, ImageEnhance
    import io
    _w = _I.open(os.path.join(ROOT, "design-reference", "wallpaper.jpg")).convert("RGB"); _k = max(1440 / _w.width, 900 / _w.height)
    _w = _w.resize((round(_w.width * _k), round(_w.height * _k))); _ox, _oy = (_w.width - 1440) // 2, (_w.height - 900) // 2
    _c = ImageEnhance.Color(_w.crop((_ox + 1180, _oy + 420, _ox + 1396, _oy + 953)).resize((54, 133)).filter(ImageFilter.GaussianBlur(2.5))).enhance(1.65)
    _bb = io.BytesIO(); _c.save(_bb, "JPEG", quality=85)
    sbd_url_small = "data:image/jpeg;base64," + base64.b64encode(_bb.getvalue()).decode()
    ctxm = await b.new_context(viewport={"width": 216, "height": 533}, device_scale_factor=2)
    mp = await ctxm.new_page(); merrs = []
    mp.on("pageerror", lambda e: merrs.append(str(e)))
    await mp.add_init_script(path=os.path.join(ROOT, "tools", "mock-tauri.js"))
    await mp.goto(base_url + "/ctxmenu.html"); await mp.wait_for_timeout(200)
    items = [{"id": "new_chat", "label": "新对话", "check": None, "sep": False}, {"id": "new_tmp", "label": "新临时对话", "check": None, "sep": False},
             {"id": "history", "label": "历史记录", "check": None, "sep": False}, {"id": "capture", "label": "截屏提问", "check": None, "sep": False},
             {"id": "capture_window", "label": "发送当前窗口", "check": None, "sep": False}, {"id": "share", "label": "共享屏幕", "check": None, "sep": False},
             {"id": "handoff", "label": "交给终端", "check": None, "sep": False}, {"id": "routines", "label": "定时任务", "check": None, "sep": False},
             {"id": "settings", "label": "设置", "check": None, "sep": False}, {"id": "sleep", "label": "睡眠", "check": None, "sep": False},
             {"id": "toggle_ball", "label": "隐藏小球", "check": None, "sep": False}, {"id": "autostart", "label": "开机启动", "check": True, "sep": False},
             {"id": "quit", "label": "退出", "check": None, "sep": True}]
    await mp.evaluate("([it, bd]) => __emit('ctx:show', { items: it, bd })", [items, sbd_url_small])
    await mp.wait_for_timeout(300)
    g = await mp.evaluate("""(() => { const m = document.querySelector('#menu'), r = m.getBoundingClientRect();
        return { labels: [...m.querySelectorAll('button')].map(b => b.firstChild.textContent), hr: m.querySelectorAll('hr').length,
                 ck: m.querySelector('[data-id=autostart]').getAttribute('aria-checked'), glassy: m.classList.contains('glassy'),
                 x: r.x, y: r.y, w: r.width, h: Math.round(r.height), bottom: innerHeight - r.bottom, focus: document.activeElement.id }; })()""")
    check(g["labels"] == ["新对话", "新临时对话", "历史记录", "截屏提问", "发送当前窗口", "共享屏幕", "交给终端", "定时任务", "设置", "睡眠", "隐藏小球", "开机启动", "退出"] and g["hr"] == 1, f"右键菜单：13 项和一条分隔线照原型顺序 {g['labels']}")
    check(g["ck"] == "true", "开机启动打勾")
    check(g["glassy"] and g["x"] == 8 and g["y"] == 8 and g["w"] == 200 and abs(g["bottom"] - 8) <= 1, f"菜单 200 宽、四周留 8 给阴影、窗口大小正好（和 Rust ctx_size 一致）{g}")
    check(g["focus"] == "menu", "打开后焦点在菜单上，不预先高亮任何一项（同原型）")
    await mp.screenshot(path=f"{OUT}/14_ctx_menu.png", omit_background=True)
    await mp.keyboard.press("ArrowDown"); await mp.keyboard.press("ArrowDown"); await mp.keyboard.press("ArrowDown"); await mp.keyboard.press("ArrowDown")
    check(await mp.evaluate("document.activeElement.dataset.id") == "capture", "方向键在菜单里移动")
    await mp.click("[data-id=settings]"); await mp.wait_for_timeout(100)
    calls = (await mp.evaluate("__mock")).get("ctxActs") or []
    check(calls == ["settings"], f"点一项：关掉菜单并执行 {calls}")
    for mid in ["capture_window", "share", "new_tmp", "handoff", "routines", "sleep"]:
        await mp.evaluate("it => __emit('ctx:show', { items: it, bd: null })", items); await mp.wait_for_timeout(150)
        await mp.click(f"[data-id={mid}]")
    check((await mp.evaluate("__mock")).get("ctxActs") == ["settings", "capture_window", "share", "new_tmp", "handoff", "routines", "sleep"], "点击后传回正确的菜单 id")
    await mp.evaluate("([it, bd]) => __emit('ctx:show', { items: it, bd: null })", [items, None]); await mp.wait_for_timeout(200)
    check(not await mp.evaluate("document.querySelector('#menu').classList.contains('glassy')"), "拿不到底图：用卡片同款不透明底")
    await mp.keyboard.press("Escape"); await mp.wait_for_timeout(100)
    check((await mp.evaluate("__mock")).get("ctxActs") == ["settings", "capture_window", "share", "new_tmp", "handoff", "routines", "sleep", ""], "Esc 关掉菜单，什么也不做")
    check(not merrs, "菜单窗口无 JS 错误 " + "; ".join(merrs))
    await ctxm.close()
