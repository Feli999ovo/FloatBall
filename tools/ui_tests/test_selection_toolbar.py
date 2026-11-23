import os

from ui_support import check, OUT, ROOT


async def test_selection_toolbar(b, base_url):
    # ───────── 划词工具栏窗口（原型 selBar） ─────────
    ctx3 = await b.new_context(viewport={"width": 420, "height": 200}, device_scale_factor=2)
    bp = await ctx3.new_page(); berrs = []
    bp.on("pageerror", lambda e: berrs.append(str(e)))
    await bp.add_init_script(path=os.path.join(ROOT, "tools", "mock-tauri.js"))
    await bp.goto(base_url + "/selbar.html"); await bp.wait_for_timeout(300)
    bm = lambda: bp.evaluate("__mock")
    await bp.evaluate("__emit('sel:show', {text: '下季度重点：海外支付', app: 'Microsoft Edge', x: 500, y: 300})"); await bp.wait_for_timeout(300)
    labels = await bp.eval_on_selector_all("#selBar button", "x => x.map(b => b.textContent || b.getAttribute('aria-label'))")
    check(labels == ["悬浮球 · 问问它", "问问它", "解释", "翻译", "总结", "改写", "复制", "⋯"], f"工具栏：球 + 前 6 个 + ⋯（同原型）{labels}")
    pl = (await bm()).get("selPlace") or {}
    check(pl.get("x") == 500 and pl.get("y") == 300 and 300 < pl.get("w", 0) < 420 and 36 < pl.get("h", 0) < 50, f"量好大小请程序放到选区旁 {pl}")
    await bp.click(".x-selmore"); await bp.wait_for_timeout(150)
    dd = await bp.eval_on_selector_all("#selDd button", "x => x.map(b => b.textContent)")
    rs = (await bm()).get("selResize") or {}
    check(dd == ["朗读", "搜索", "扩写", "缩写"] and rs.get("h", 0) > pl["h"] + 100 and await bp.get_attribute(".x-selmore", "aria-expanded") == "true", f"⋯ 展开其余 4 个，窗口跟着变高 {dd} {rs}")
    await bp.screenshot(path=f"{OUT}/26_selbar_more.png", omit_background=True)
    await bp.click(".x-selmore"); await bp.wait_for_timeout(100)
    check(await bp.is_hidden("#selDd"), "再点 ⋯ 收起")
    await bp.click("#selBar [data-a=copy]"); await bp.wait_for_timeout(100)
    check((await bm()).get("copied") == "下季度重点：海外支付" and await bp.inner_text("#selBar [data-a=copy]") == "已复制", "复制：写进剪贴板，按钮原地显示已复制")
    await bp.wait_for_timeout(1100)
    check(not (await bm()).get("selShown"), "复制 1 秒后收起")
    await bp.evaluate("__emit('sel:show', {text: '海外  支付', app: 'Word', x: 10, y: 10})"); await bp.wait_for_timeout(200)
    check(await bp.inner_text("#selBar [data-a=copy]") == "复制", "再次出现时复制按钮复原")
    await bp.screenshot(path=f"{OUT}/26_selbar.png", omit_background=True)
    await bp.click("#selBar [data-a=explain]"); await bp.wait_for_timeout(100)
    check((await bm()).get("selActs", [{}])[-1] == {"act": "explain", "text": "海外  支付"}, "点「解释」交给小球")
    await bp.evaluate("__emit('sel:show', {text: '海外  支付', app: 'Word', x: 10, y: 10})"); await bp.wait_for_timeout(200)
    await bp.click(".x-selmore"); await bp.click("#selDd [data-a=search]"); await bp.wait_for_timeout(100)
    check((await bm()).get("opened") == "https://www.bing.com/search?q=%E6%B5%B7%E5%A4%96%20%E6%94%AF%E4%BB%98", f"搜索：用浏览器打开 {(await bm()).get('opened')}")
    await bp.evaluate("__mock.cfg.sel.order.forEach(o => o.show = ['tr', 'copy'].includes(o.id))")
    await bp.evaluate("__emit('sel:show', {text: 'x', app: 'Word', x: 10, y: 10})"); await bp.wait_for_timeout(200)
    labels = await bp.eval_on_selector_all("#selBar button", "x => x.map(b => b.textContent || b.getAttribute('aria-label'))")
    check(labels == ["悬浮球 · 问问它", "翻译", "复制", "⋯"], f"按设置里的勾选和顺序排按钮 {labels}")
    # 毛玻璃：和对话卡片同一种（程序截下工具栏后面的桌面传回来）
    mkbd = """(wh) => { const c = document.createElement('canvas'); c.width = 40; c.height = 30; const g = c.getContext('2d');
        const gr = g.createLinearGradient(0, 0, 40, 30); gr.addColorStop(0, '#f3a26b'); gr.addColorStop(1, '#7c3a6e'); g.fillStyle = gr; g.fillRect(0, 0, 40, 30);
        __mock.selBackdrop = { url: c.toDataURL('image/jpeg'), w: wh[0], h: wh[1] }; }"""
    check(not await bp.evaluate("document.querySelector('#selBar').classList.contains('glassy')"), "拿不到底图（macOS/Linux）：用卡片同款不透明底")
    bgc = await bp.evaluate("getComputedStyle(document.querySelector('#selBar')).backgroundImage")
    check("rgba(34, 30, 58, 0.94)" in bgc, f"不透明底和对话卡片同色 {bgc[:60]}")
    await bp.evaluate(mkbd, [420, 300]); await bp.evaluate("__mock.cfg.glass = 40")
    await bp.evaluate("__emit('sel:show', {text: '海外支付', app: 'Word', x: 300, y: 300})"); await bp.wait_for_timeout(400)
    gs = await bp.evaluate("""(() => { const e = document.querySelector('#selBar'), r = e.getBoundingClientRect();
        return [e.classList.contains('glassy'), e.style.getPropertyValue('--bd-pos'), -Math.round(r.left) + 'px ' + -Math.round(r.top) + 'px', getComputedStyle(document.documentElement).getPropertyValue('--tint').trim()]; })()""")
    check(gs[0] and gs[1] == gs[2] and gs[3] == "0.348", f"拿到底图：工具栏垫上模糊的桌面，对齐自己的位置，透明度跟设置·外观 {gs}")
    await bp.click(".x-selmore"); await bp.wait_for_timeout(250)
    check(await bp.evaluate("document.querySelector('#selDd').classList.contains('glassy')"), "⋯ 菜单也是同一种玻璃")
    await bp.screenshot(path=f"{OUT}/26_selbar_glass.png", omit_background=True)
    await bp.evaluate(mkbd, [420, 50])
    await bp.evaluate("__emit('sel:show', {text: '海外支付', app: 'Word', x: 300, y: 300})"); await bp.wait_for_timeout(250)
    await bp.click(".x-selmore"); await bp.wait_for_timeout(250)
    check(await bp.evaluate("document.querySelector('#selBar').classList.contains('glassy') && !document.querySelector('#selDd').classList.contains('glassy')"), "底图不够高（贴着屏幕底）：菜单用不透明底，不露出半截")
    await bp.evaluate("delete __mock.selBackdrop; __mock.cfg.glass = 25")
    await bp.evaluate("__emit('sel:hint', {text: '', app: '', x: 10, y: 10})"); await bp.wait_for_timeout(200)
    check(await bp.is_visible("#selHint") and await bp.is_hidden("#selBar"), "macOS 没开辅助功能：只显示权限提示")
    await bp.click("#selHintGo"); await bp.wait_for_timeout(100)
    check((await bm()).get("privacy") == 1 and not (await bm()).get("selShown"), "点「去设置」打开系统的辅助功能设置")
    await bp.evaluate("__emit('sel:hint', {text: '', app: '', x: 10, y: 10})"); await bp.wait_for_timeout(100)
    check(not (await bm()).get("selShown"), "权限提示每次运行只出现一次")
    await bp.evaluate("__emit('sel:show', {text: 'y', app: 'Word', x: 10, y: 10})"); await bp.wait_for_timeout(200)
    check(await bp.is_visible("#selBar") and await bp.is_hidden("#selHint"), "提示过后正常显示工具栏")
    check(not berrs, "工具栏页面无 JS 错误 " + "; ".join(berrs))
    await ctx3.close()
