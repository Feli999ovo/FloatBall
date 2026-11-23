import base64
import json
import os

from ui_support import check, enable_model, ROOT


async def test_window(orb_page):
    pg, shot, errs = orb_page
    await enable_model(pg)
    await pg.evaluate('expand()')
    # 9b 展开后程序挪窗口触发的 moved 事件，不能改掉收起位置
    await pg.evaluate("__mock.moved.forEach(f=>f())"); await pg.wait_for_timeout(600)

    # 10 Esc 收起，回到原位
    before = (await pg.evaluate("__mock"))["pos"]
    await pg.keyboard.press("Escape")
    await pg.wait_for_function("document.body.classList.contains('collapsed') && !morphing")
    m = await pg.evaluate("__mock")
    check(m["size"] == {"w": 96, "h": 96}, f"Esc 收起成小球 {m['size']}")
    check(m["pos"] == {"x": 1300, "y": 380}, f"收起后回到原来的位置 {m['pos']}")
    await shot("09_collapsed")

    # 11 左上角小球：向右下展开；左下角：向上展开
    for name, pos, fx, fy in [("10_topleft", {"x": 20, "y": 80}, False, False), ("11_bottomleft", {"x": 20, "y": 760}, False, True)]:
        await pg.mouse.move(0, 0)
        await pg.evaluate("document.documentElement.dispatchEvent(new PointerEvent('pointerleave'))")
        await pg.wait_for_function("!morphing && !hv.shown")
        await pg.evaluate(f"__mock.pos = {json.dumps(pos)}")
        await pg.evaluate("expand()")
        cls = await pg.get_attribute("body", "class") or ""
        m = await pg.evaluate("__mock")
        check(("flip-x" in cls) == fx and ("flip-y" in cls) == fy, f"{name} 展开方向 ({cls})")
        check(m["pos"]["y"] >= 50 and m["pos"]["y"] + m["size"]["h"] <= 875, f"{name} 不超出工作区 y={m['pos']['y']}")
        await shot(name)
        await pg.evaluate("collapse()")
    await pg.evaluate("__mock.pos = {x: 600, y: 420}")
    await pg.click("#orb"); await pg.wait_for_timeout(900)
    mid = await pg.evaluate("(() => { const b = document.querySelector('#bar').getBoundingClientRect(), c = document.querySelector('#card').getBoundingClientRect(); return [document.body.classList.contains('flip-y'), c.bottom <= b.top]; })()")
    check(mid == [True, True], f"小球在屏幕中部时卡片在胶囊上方（照原型） {mid}")
    await pg.click("#orb"); await pg.wait_for_timeout(500)

    # 11b 拿不到显示器信息时仍能展开
    await pg.evaluate("__mock.monitorFails = true; __mock.pos = {x: 300, y: 200}")
    await pg.click("#orb"); await pg.wait_for_timeout(500)
    mm = await pg.evaluate("[__mock.size, __mock.pos, document.body.className, screen.availWidth, screen.availHeight, innerWidth, innerHeight]")
    print("   monitorFails:", mm)
    check(mm[0]["w"] == 364, "拿不到显示器信息也能展开")
    await pg.click("#orb"); await pg.wait_for_timeout(400)  # 卡片上不再有 ×（照原型），点小球收起
    await pg.evaluate("__mock.monitorFails = false")
    errs[:] = [e for e in errs if "currentMonitor" not in e]

    # 11a 任务页签：刚才那次提问在列表里，状态是完成
    await pg.evaluate("expand()")
    await pg.fill("#ask", "窗口测试中的一条对话")
    await pg.press("#ask", "Enter")
    await pg.wait_for_function("!S.busy && document.querySelector('.m.ai')")
    await pg.click("#tabTask"); await pg.wait_for_timeout(150)
    tk = await pg.evaluate("[...document.querySelectorAll('#tasks .tk')].map(d => [d.dataset.st, d.innerText.replace(/\\s+/g, ' ')])")
    check(len(tk) >= 1 and tk[0][0] == "done" and "完成" in tk[0][1], f"任务页列出这次打开以来的提问 {tk[:2]}")
    check(await pg.is_hidden("#vChat") and await pg.get_attribute("#tabTask", "aria-selected") == "true", "切到任务页")
    await shot("11a_tasks")
    await pg.click("#tasks .tk"); await pg.wait_for_timeout(150)
    check(await pg.is_visible("#vChat"), "点完成的任务回到对话")
    await pg.click("#orb"); await pg.wait_for_timeout(800)  # 卡片上不再有 ×（照原型），点小球收起

    # 11a2 卡片收起时回复完成：小球旁冒气泡，小球留在原地；点气泡打开对话
    before = await pg.evaluate("__mock.pos")
    await pg.evaluate("showPeek('这是一段比较长的回复，用来看气泡会不会截断')"); await pg.wait_for_timeout(700)
    pk = await pg.evaluate("""() => { const o = document.querySelector('#orb').getBoundingClientRect(); return {
        on: document.querySelector('#peek').classList.contains('on'), text: document.querySelector('#peek').textContent,
        size: __mock.size, pos: __mock.pos, orb: [o.left, o.top] }; }""")
    check(pk["on"] and pk["text"].endswith("…") and len(pk["text"]) == 21, f"回复完成时冒气泡，只显示开头 {pk['text']}")
    ox, oy = pk["pos"]["x"] + pk["orb"][0] - 12, pk["pos"]["y"] + pk["orb"][1] - 12
    check(pk["size"]["w"] > 96 and abs(ox - before["x"]) < 1 and abs(oy - before["y"]) < 1, f"冒泡时窗口临时变大，小球留在原地 {pk} {before}")
    await shot("11a2_peek")
    await pg.click("#peek"); await pg.wait_for_timeout(1000)
    check((await pg.evaluate("__mock"))["size"]["w"] == 364 and await pg.is_visible("#vChat"), "点气泡打开对话")
    await pg.click("#orb"); await pg.wait_for_timeout(800)  # 卡片上不再有 ×（照原型），点小球收起
    await pg.evaluate("showPeek('好了')"); await pg.wait_for_timeout(600)
    await pg.evaluate("hidePeek()"); await pg.wait_for_timeout(700)
    m = await pg.evaluate("__mock")
    check(m["size"] == {"w": 96, "h": 96} and m["pos"] == before, f"气泡消失后窗口缩回原样 {m['size']} {m['pos']}")

    # 11c 小球拉长成胶囊；上方放得下时卡片在胶囊上方；有底图时卡片是毛玻璃
    check(await pg.evaluate("document.querySelector('#bar').offsetWidth") == 72, "收起时胶囊缩成小球大小")
    check(await pg.evaluate("getComputedStyle(document.querySelector('.cap')).overflow") == "hidden", "胶囊的玻璃边不会露到小球外面")
    wall = base64.b64encode(open(os.path.join(ROOT, "design-reference", "wallpaper.jpg"), "rb").read()).decode()
    # 和程序里一样：把壁纸缩得很小当底图，放大后就是糊的
    await pg.evaluate("""async (b64) => {
        const im = new Image(); im.src = "data:image/jpeg;base64," + b64; await im.decode();
        const c = document.createElement("canvas"); c.width = 52; c.height = 75;
        c.getContext("2d").drawImage(im, 0, 0, 52, 75);
        __mock.backdrop = c.toDataURL("image/jpeg");
        __mock.pos = { x: 300, y: 700 };
    }""", wall)
    await pg.click("#orb"); await pg.wait_for_timeout(900)
    g = await pg.evaluate("""() => {
        const bar = document.querySelector('#bar').getBoundingClientRect(), card = document.querySelector('#card');
        const cr = card.getBoundingClientRect();
        return { barW: Math.round(bar.width), cardW: Math.round(cr.width), cardH: Math.round(cr.height), barTop: bar.top, cardBottom: card.getBoundingClientRect().bottom,
                 cls: document.body.className, glassy: card.classList.contains('glassy'), args: __mock.backdropArgs, pos: __mock.pos };
    }""")
    check(g["barW"] == 288, f"展开后胶囊照原型 288 长，不跟窗口宽 {g['barW']}")
    check(g["cardW"] == 340 and g["cardH"] == 260, f"卡片照原型 340×260 {g['cardW']}×{g['cardH']}")
    check("flip-y" in g["cls"] and g["cardBottom"] <= g["barTop"], "上方放得下时卡片在胶囊上方")
    check(g["pos"] == {"x": 274, "y": 432}, f"卡片在上方时小球留在原地（窗口往上长） {g['pos']}")
    check(g["glassy"] and g["args"]["w"] == 728 and g["args"]["h"] == 728 and g["args"]["y"] == 864, f"展开前截下窗口后面的桌面当毛玻璃底 {g['args']}")
    await shot("12_capsule_glass")
    capk = await pg.evaluate("getComputedStyle(document.documentElement).getPropertyValue('--capk').trim()")
    check(capk not in ("", "0.3"), f"胶囊按身后底图的亮度自适应加深 --capk={capk}")
    await pg.evaluate("__mock.backdrop = null")
    await pg.click("#orb"); await pg.wait_for_timeout(800)  # 卡片上不再有 ×（照原型），点小球收起
    m = await pg.evaluate("__mock")
    check(m["size"] == {"w": 96, "h": 96} and m["pos"] == {"x": 300, "y": 700}, "收起后小球回到原位")
    await pg.click("#orb"); await pg.wait_for_timeout(900)
    check(not await pg.evaluate("document.querySelector('#card').classList.contains('glassy')"), "拿不到底图时卡片用不透明的底")
    await pg.click("#orb"); await pg.wait_for_timeout(800)  # 卡片上不再有 ×（照原型），点小球收起

    # 12 菜单事件与快捷键事件
    await pg.evaluate("__emit('menu','tasks')"); await pg.wait_for_timeout(900)
    check(not await pg.is_hidden("#vTask") and (await pg.evaluate("__mock"))["size"]["w"] == 364, "快捷键「打开任务」")
    await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(1000)
    await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(900)
    check((await pg.evaluate("__mock"))["size"]["w"] == 364, "快捷键呼出")
    # 卡片头照原型：没有设置和 ×，⋯ 里是钉住
    check(await pg.locator("#setBtn, #closeBtn").count() == 0, "卡片头照原型：设置在右键菜单，收起点小球")
    await pg.click("#moreBtn"); await pg.wait_for_timeout(200)
    check(await pg.evaluate("document.querySelector('#moreDd').classList.contains('on')"), "⋯ 打开下拉")
    cm = await pg.evaluate("[...document.querySelectorAll('#moreDd [data-cm]')].map(b => [b.dataset.cm, b.getAttribute('role')])")
    check(cm == [["pin", "menuitemcheckbox"], ["tmp", "menuitemcheckbox"], ["ho", "menuitem"]], f"更多菜单含临时模式开关 {cm}")
    bg = await pg.evaluate("getComputedStyle(document.querySelector('#moreDd')).backgroundImage")
    check("rgba" not in bg and "rgb(" in bg, f"⋯ 下拉用不透明的底，后面的气泡不会透出来 {bg}")
    await shot("12b_more_menu")
    await pg.click("#moreDd [data-cm=pin]"); await pg.wait_for_timeout(200)
    check(await pg.is_visible("#pinMark") and await pg.get_attribute("#moreDd [data-cm=pin]", "aria-checked") == "true", "钉住后标题旁出现实心图钉")
    await pg.click("#pinMark"); await pg.wait_for_timeout(200)
    check(await pg.is_hidden("#pinMark"), "点图钉取消钉住")
    await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(800)
    check((await pg.evaluate("__mock"))["size"]["w"] == 96, "快捷键收起")
    await pg.click("#orb", button="right")
    check((await pg.evaluate("__mock")).get("menuShown"), "右键弹出菜单（自绘玻璃窗口，由程序放到鼠标旁）")

    # 13 拖动：贴边吸附
    await pg.evaluate("__mock.pos = {x: 1355, y: 400}; __mock.moved.forEach(f=>f())")
    await pg.wait_for_timeout(600)
    m = await pg.evaluate("__mock")
    check(abs(m["pos"]["x"] - (1440 - 12 - 72 * 0.9)) < 0.01 and await pg.evaluate("orb.classList.contains('docked') && orb.dataset.edge === 'r'"), f"靠近右边贴边压平 x={m['pos']['x']}")
    check(m["cfg"]["position"] is not None, "记住位置")
    await shot("13b_docked")
    await pg.mouse.move(5, 5)  # 真指针不在球上（下面用事件模拟指针进出）
    await pg.dispatch_event("#orb", "pointerenter"); await pg.wait_for_timeout(200)
    m = await pg.evaluate("__mock")
    check(abs(m["pos"]["x"] - (1440 - 12 - 72 * 0.9 - 9.2)) < 0.01 and await pg.evaluate("orb.classList.contains('peel')"), f"指针移上去滑出来 x={m['pos']['x']}")
    await pg.wait_for_timeout(500)
    check(await pg.evaluate("orb.classList.contains('peel')"), "滑出来后不会被吸回去")
    await pg.dispatch_event("#orb", "pointerleave"); await pg.wait_for_timeout(600)
    m = await pg.evaluate("__mock")
    check(abs(m["pos"]["x"] - (1440 - 12 - 72 * 0.9)) < 0.01 and not await pg.evaluate("orb.classList.contains('peel')"), f"移开后贴回去 x={m['pos']['x']}")
    # 关掉「贴边时自动收起」：只吸到边上，不压平
    await pg.evaluate("__mock.cfg.auto_dock = false; __emit('config')"); await pg.wait_for_timeout(500)
    m = await pg.evaluate("__mock")
    check(m["pos"]["x"] == 1440 - 96 + 12 - 2 and not await pg.evaluate("orb.classList.contains('docked')"), f"关掉贴边：只吸附 x={m['pos']['x']}")
    await pg.evaluate("__mock.cfg.auto_dock = true; __emit('config')"); await pg.wait_for_timeout(500)
    # 贴着边点开：卡片从完整露出的位置展开，收起后贴回去
    await pg.click("#orb"); await pg.wait_for_timeout(900)
    check(not await pg.evaluate("orb.classList.contains('docked')") and (await pg.evaluate("__mock"))["size"]["w"] == 364, "贴边时点开正常展开")
    await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(1200)
    m = await pg.evaluate("__mock")
    check(abs(m["pos"]["x"] - (1440 - 12 - 72 * 0.9)) < 0.01 and await pg.evaluate("orb.classList.contains('docked')"), f"收起后贴回边上 x={m['pos']['x']}")
    # 悬停快捷入口：停在球上弹出小泡（窗口扩成以球为中心的方块），点一个就执行
    await pg.evaluate("__mock.cfg.auto_dock = false; __mock.pos = {x: 600, y: 400}; __emit('config')"); await pg.wait_for_timeout(500)
    await pg.mouse.move(5, 5); await pg.wait_for_timeout(100)
    await pg.hover("#orb"); await pg.wait_for_timeout(900)
    m = await pg.evaluate("__mock")
    ids = await pg.eval_on_selector_all("#hvq .x-hvb", "x => x.map(b => b.getAttribute('aria-label'))")
    cx = m["pos"]["x"] + m["size"]["w"] / 2
    check(ids == ["截屏提问", "新对话", "历史记录", "任务列表"] and m["size"]["w"] > 96 and abs(cx - (600 + 48)) < 1, f"悬停弹出小泡，球留在原地 {ids} {m['size']} cx={cx}")
    await shot("13c_hover")
    await pg.evaluate("document.documentElement.dispatchEvent(new PointerEvent('pointerleave'))"); await pg.wait_for_timeout(1000)
    m = await pg.evaluate("__mock")
    check(m["size"]["w"] == 96 and m["pos"]["x"] == 600 and not await pg.evaluate("document.querySelectorAll('#hvq .x-hvb').length"), f"移开后收起，窗口复原 {m['size']} {m['pos']}")
    await pg.mouse.move(5, 5); await pg.wait_for_timeout(100)
    await pg.hover("#orb"); await pg.wait_for_timeout(900)
    await pg.click("#hvq [aria-label='历史记录']"); await pg.wait_for_timeout(1200)
    check(not await pg.is_hidden("#vHist") and (await pg.evaluate("__mock"))["size"]["w"] == 364, "点小泡「历史记录」打开历史")
    await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(1000)
    # 小泡弹出时直接点小球（Windows 真机截图里的做法）：等小泡收完再展开，卡片不被缩回去的窗口截掉
    await pg.mouse.move(5, 5); await pg.wait_for_timeout(100)
    await pg.hover("#orb"); await pg.wait_for_timeout(900)
    assert await pg.evaluate("hv.shown")
    await pg.click("#orb"); await pg.wait_for_timeout(1500)
    m = await pg.evaluate("__mock")
    check(await pg.evaluate("S.open") and m["size"]["w"] == 364 and not await pg.evaluate("document.body.classList.contains('hovering')"), f"小泡弹出时点小球：照常展开成卡片 {m['size']}")
    await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(1000)
    await pg.evaluate("__mock.cfg.hover.on = false; __emit('config')"); await pg.wait_for_timeout(300)
    await pg.mouse.move(5, 5); await pg.hover("#orb"); await pg.wait_for_timeout(900)
    check((await pg.evaluate("__mock"))["size"]["w"] == 96, "关掉悬停入口就不弹")
    await pg.mouse.move(5, 5)
    await pg.evaluate("__mock.cfg.hover.on = true; __mock.cfg.auto_dock = true; __emit('config')"); await pg.wait_for_timeout(300)
    # 多显示器：鼠标到副屏，小球按相对位置滑过去；固定主屏就回来；上次所在就不动
    await pg.evaluate("__mock.pos = {x: 600, y: 400}; __mock.screens = [{x: 2880, y: 0, w: 2560, h: 1440, scale: 2, primary: false}]; __mock.cursor = [3500, 700]; __mock.posLog = []")
    await pg.wait_for_timeout(1600)
    m = await pg.evaluate("__mock")
    check(abs(m["pos"]["x"] - 1968) < 1 and abs(m["pos"]["y"] - 310.5) < 1 and len(m["posLog"]) >= 8, f"鼠标到副屏：小球滑到副屏同样的相对位置 {m['pos']} 步数 {len(m['posLog'])}")
    await pg.evaluate("__mock.cfg.screen = 'main'; __emit('config')"); await pg.wait_for_timeout(1200)
    m = await pg.evaluate("__mock")
    check(m["pos"]["x"] < 1440 - 96, f"固定在主屏：回到主屏 {m['pos']}")
    await pg.evaluate("__mock.cfg.screen = 'last'; __emit('config'); __mock.posLog = []"); await pg.wait_for_timeout(1200)
    check(len((await pg.evaluate("__mock"))["posLog"]) == 0, "上次所在的屏幕：鼠标去副屏也不动")
    await pg.evaluate("__mock.cfg.screen = 'mouse'; __mock.cfg.screen_slide = false; __emit('config'); __mock.posLog = []"); await pg.wait_for_timeout(1200)
    m = await pg.evaluate("__mock")
    check(m["pos"]["x"] > 1440 and len(m["posLog"]) <= 2, f"关掉滑过去：直接跳 {m['pos']} 步数 {len(m['posLog'])}")
    await pg.evaluate("__mock.screens = null; __mock.cursor = null; __mock.cfg.screen_slide = true; __mock.pos = {x: 600, y: 400}; __emit('config')"); await pg.wait_for_timeout(500)
    # 减少动态效果、跟随系统深浅色
    await pg.evaluate("__mock.cfg.calm = true; __mock.cfg.follow_system = true; __emit('config')"); await pg.wait_for_timeout(300)
    anim = await pg.evaluate("getComputedStyle(document.querySelector('#orb .breath')).animationName")
    check(await pg.evaluate("document.documentElement.classList.contains('calm')") and anim == "none", f"减少动态：关掉呼吸 {anim}")
    await pg.emulate_media(color_scheme="dark"); await pg.wait_for_timeout(200)
    check(await pg.evaluate("document.documentElement.classList.contains('dark')"), "跟随系统：系统深色时卡片变深")
    await pg.emulate_media(color_scheme="light"); await pg.wait_for_timeout(200)
    check(not await pg.evaluate("document.documentElement.classList.contains('dark')"), "跟随系统：系统浅色时恢复")
    await pg.evaluate("__mock.cfg.calm = false; __mock.cfg.follow_system = false; __emit('config')"); await pg.wait_for_timeout(300)
    # 空闲多久后睡觉
    await pg.evaluate("__mock.cfg.idle_ms = 1000; __emit('config')"); await pg.wait_for_timeout(300)
    await pg.evaluate("S.lastActive = Date.now() - 2000"); await pg.wait_for_timeout(5500)
    check(await pg.evaluate("orb.className.includes('s-sleep')"), "空闲到时间就睡觉")
    await pg.evaluate("__mock.cfg.idle_ms = 0; S.lastActive = 0; __emit('config'); mood('idle')"); await pg.wait_for_timeout(5500)
    check(not await pg.evaluate("orb.className.includes('s-sleep')"), "设成从不就不睡")
    await pg.evaluate("__mock.cfg.idle_ms = 600000; S.lastActive = Date.now(); __emit('config')")
    # 鼠标按下移动 → 开始拖动而不是点击
    await pg.mouse.move(48, 48); await pg.mouse.down(); await pg.mouse.move(60, 60); await pg.mouse.up()
    await pg.wait_for_timeout(300)
    m = await pg.evaluate("__mock")
    check(m.get("dragStarted") and m["size"]["w"] == 96, "拖动不会误触展开")

    # 14 外观设置：小球大小、眼睛颜色、卡片透明度
    await pg.evaluate("""() => { Object.assign(__mock.cfg, { orb_size: 84, eye: "#CFE3FF", glass: 20 }); __emit("config"); }""")
    await pg.wait_for_timeout(400)
    look = await pg.evaluate("""() => { const s = getComputedStyle(document.documentElement); return {
        size: __mock.size, orb: document.querySelector('#orb').offsetWidth, eye: s.getPropertyValue('--eye').trim(), tint: s.getPropertyValue('--tint').trim(),
        // 眼睛那一层和它外面那一层一样大才对（两者一起做呼吸动画，所以比的是相对大小）
        face: Math.abs(document.querySelector('.face').getBoundingClientRect().width - document.querySelector('.face').parentElement.getBoundingClientRect().width) }; }""")
    check(look["size"] == {"w": 108, "h": 108} and look["orb"] == 84 and look["face"] < 1, f"小球调大后窗口和眼睛跟着变大 {look}")
    check(look["eye"] == "#CFE3FF" and look["tint"] == "0.464", "眼睛颜色和卡片透明度生效（原型墨层 0.58，更通透 +20%）")
    await shot("13_big_orb")

    lu = lambda lm: [m for m in lm if m["role"] == "user"][-1]["content"]
