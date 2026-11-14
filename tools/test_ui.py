"""界面自动测试：在 Chromium 里加载 src/index.html + mock-tauri.js，走一遍主要流程并截图。
用法：python3 tools/test_ui.py 输出目录"""
import sys, os, json, asyncio, base64
from PIL import Image
from playwright.async_api import async_playwright

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = sys.argv[1] if len(sys.argv) > 1 else "/tmp/fbui"
WALL = sys.argv[2] if len(sys.argv) > 2 else None
os.makedirs(OUT, exist_ok=True)
fails = []

def check(cond, msg):
    print(("PASS " if cond else "FAIL ") + msg)
    if not cond: fails.append(msg)

async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch()
        ctx = await b.new_context(viewport={"width": 96, "height": 96}, device_scale_factor=2)
        await ctx.grant_permissions(["clipboard-read", "clipboard-write"])
        pg = await ctx.new_page()
        errs = []
        pg.on("pageerror", lambda e: errs.append(str(e)))
        pg.on("console", lambda m: m.type == "error" and errs.append(m.text))
        await pg.expose_function("__resize", lambda w, h: asyncio.ensure_future(pg.set_viewport_size({"width": int(w), "height": int(h)})))
        await pg.add_init_script(path=os.path.join(ROOT, "tools", "mock-tauri.js"))
        await pg.goto("file://" + os.path.join(ROOT, "src", "index.html"))
        await pg.wait_for_timeout(400)

        async def shot(name):
            await pg.wait_for_timeout(450)
            path = f"{OUT}/{name}.png"
            await pg.screenshot(path=path, omit_background=True)
            m = await pg.evaluate("__mock")
            if WALL:
                desk = Image.open(WALL).convert("RGBA").resize((1440 * 2, 900 * 2))
                w = Image.open(path).convert("RGBA")
                desk.alpha_composite(w, (int(m["pos"]["x"] * 2), int(m["pos"]["y"] * 2)))
                desk.convert("RGB").save(f"{OUT}/{name}_desk.jpg", quality=85)
            return m

        # 1 首次启动：没 Key 也只显示小球，不自动弹出设置挡住屏幕
        await pg.wait_for_timeout(900)
        m = await shot("01_first_run_orb")
        check(m["size"] == {"w": 96, "h": 96}, f"首次启动只显示小球，不自动展开 {m['size']}")
        await pg.click("#orb"); await pg.wait_for_timeout(500)
        check(await pg.is_visible("#setupBtn") and not await pg.is_visible("#empty .chips"), "没设置模型时点开小球，提示先去设置")
        await shot("01b_first_run_tip")
        await pg.click("#setupBtn"); await pg.wait_for_timeout(200)
        check((await pg.evaluate("__mock")).get("settingsOpened") == 1, "点「去设置」打开设置窗口")
        check(await pg.evaluate("getComputedStyle(document.querySelector('#ask'), '::-webkit-scrollbar').display") == "none", "输入框不显示滚动条箭头")
        m = await pg.evaluate("__mock")
        bodycls = await pg.get_attribute("body", "class")
        check("flip-x" in bodycls, f"小球在右侧时卡片向左展开 ({bodycls})")
        check(m["pos"]["x"] + 420 <= 1440, f"展开后不超出屏幕右边 x={m['pos']['x']}")

        # 2 设置窗口里加好了模型：小球窗口收到通知，提示换成正常的引导
        await pg.evaluate("""() => {
            __mock.cfg.models = [{ id: 1, name: "", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini", vision: true, no_key: false, has_key: true }];
            __mock.cfg.default_model = 1;
            __emit("config");
        }""")
        await pg.wait_for_timeout(200)
        check(not await pg.is_visible("#setupBtn") and await pg.is_visible("#empty .chips"), "加好模型后不再提示去设置")

        # 3 空状态
        check(await pg.is_visible("#empty"), "空对话显示引导")
        await shot("03_empty_chat")

        # 4 发消息，流式输出
        await pg.evaluate("__mock.slow = true")
        await pg.fill("#ask", "怎么开始开发悬浮球？")
        await pg.press("#ask", "Enter")
        await pg.wait_for_timeout(300)
        cls = await pg.get_attribute("#orb", "class")
        check("s-think" in cls, f"等待时小球是思考表情 ({cls})")
        await pg.wait_for_timeout(1300)
        cls = await pg.get_attribute("#orb", "class")
        check("s-work" in cls, f"输出时小球是工作表情 ({cls})")
        check(await pg.is_visible("#stopRow"), "生成中显示停止按钮")
        check(await pg.is_hidden("#empty"), "有消息时隐藏空状态引导")
        await shot("04_streaming")
        await pg.evaluate("__mock.slow = false")
        await pg.wait_for_function("!__mock.calls.includes('x') && document.querySelector('.m.ai .meta') && !document.querySelector('.m.ai .meta').hidden", timeout=15000)
        html = await pg.inner_html(".m.ai .txt")
        check("<ol>" in html and "<pre><code>" in html and "<b>" in html, "Markdown：列表、代码块、粗体")
        check("tokens" in await pg.inner_text(".m.ai .meta"), "完成后显示用量")
        cls = await pg.get_attribute("#orb", "class")
        check("s-done" in cls, f"完成后小球眯眼笑 ({cls})")
        await shot("05_done")
        await pg.click(".m.ai .meta button:has-text('复制')")
        clip = await pg.evaluate("navigator.clipboard.readText()")
        check("三个要点" in clip, "复制回复")

        # 5 停止
        await pg.evaluate("__mock.slow = true")
        await pg.fill("#ask", "再说一遍"); await pg.press("#ask", "Enter")
        await pg.wait_for_timeout(1100)
        await pg.click("#stopBtn"); await pg.wait_for_timeout(200)
        last = pg.locator(".m.ai").last
        check("已停止" in await last.inner_text(), "停止生成")
        check(await pg.is_hidden("#stopRow"), "停止后按钮消失")
        await pg.evaluate("__mock.slow = false")

        # 6 出错 + 重试按钮
        await pg.fill("#ask", "制造一个错误"); await pg.press("#ask", "Enter")
        await pg.wait_for_selector(".m.ai.err", timeout=5000)
        check("401" in await pg.locator(".m.ai.err").inner_text(), "错误信息显示")
        check("s-error" in await pg.get_attribute("#orb", "class"), "出错时小球表情")
        check(await pg.locator(".m.ai.err .meta button:has-text('重试')").count() == 1, "错误有重试按钮")
        await shot("06_error")

        # 6b Agent 用工具：药丸显示在做什么；删除先停下来等确认（原型的 toolChip / askAppr）
        await pg.fill("#ask", "删除下载文件夹里重复的报告"); await pg.press("#ask", "Enter")
        await pg.wait_for_selector(".x-ap", timeout=5000)
        last = pg.locator(".m.ai").last
        pills = await last.locator(".x-tw").evaluate_all("ws => ws.map(w => [w.className, w.querySelector('.x-tl').textContent])")
        check(pills == [["x-tw done", "查看文件夹 · /Users/j/Downloads"], ["x-tw wait", "删除文件 · 报告 (1).pdf"]], f"工具药丸：完成的 ✓、等确认的暂停 {pills}")
        check("我先看看下载文件夹。" in await last.locator(".txt").first.inner_text(), "药丸前的文字")
        ap = await last.locator(".x-ap").inner_text()
        check("高风险" in ap and "删除 /Users/j/Downloads/报告 (1).pdf" in ap and "允许一次" in ap and "总是允许" in ap, f"确认卡片 {ap!r}")
        check("s-curious" in await pg.get_attribute("#orb", "class"), "等确认时小球好奇地看着你")
        await shot("06b_ask")
        await last.locator(".x-tw.done .x-tool").click()
        check("报告 (1).pdf（20480 字节）" in await last.locator(".x-tw.done .x-tres").inner_text(), "点完成的药丸看结果")
        await last.locator(".x-ap button:has-text('允许一次')").click()
        await pg.wait_for_function("document.querySelector('.m.ai:last-of-type .meta') && !document.querySelector('.m.ai:last-of-type .meta').hidden", timeout=5000)
        m = await pg.evaluate("__mock.answers[__mock.answers.length - 1]")
        check(m == {"ask": 7, "answer": {"allow": True, "always": False, "reason": "", "edit": ""}}, f"允许一次传回 Rust {m}")
        txts = await last.locator(".txt").all_inner_texts()
        check(txts == ["我先看看下载文件夹。", "删好了，重复的那份在悬浮球回收站里。"], f"药丸后面的字另起一段 {txts}")
        nd, meta = await last.locator(".x-ap button:disabled").count(), await last.locator(".meta").inner_text()
        check(nd >= 4 and "1.8K tokens" in meta, f"确认后按钮变灰，结束显示用量 {nd} {meta!r}")
        hist = await pg.evaluate("__mock.convs[0].msgs.slice(-1)[0].content")
        check(hist == "我先看看下载文件夹。\n\n删好了，重复的那份在悬浮球回收站里。", f"历史里存整条回复 {hist!r}")
        await shot("06c_allowed")
        # 拒绝并说明原因
        await pg.fill("#ask", "删除那个旧的"); await pg.press("#ask", "Enter")
        await pg.wait_for_selector(".m.ai:last-of-type .x-ap", timeout=5000)
        last = pg.locator(".m.ai").last
        await last.locator(".x-ap .x-apx").click(); await last.locator(".x-ap button:has-text('拒绝并说明')").click()
        await last.locator(".x-apr input").fill("还要用"); await last.locator(".x-apr button").click()
        await pg.wait_for_timeout(300)
        m = await pg.evaluate("__mock.answers[__mock.answers.length - 1]")
        check(m["answer"]["allow"] is False and m["answer"]["reason"] == "还要用", f"拒绝原因传回 {m}")
        check("已拒绝" in await last.locator(".x-tw.no").inner_text() and "好的，没删。" in await last.inner_text(), "拒绝后药丸标已拒绝")

        # 7 截屏提问：附件 + 发送带图片
        await pg.click("#shotBtn"); await pg.wait_for_timeout(300)
        check(await pg.locator("#attach .chip").count() == 1, "截图出现在输入框")
        await shot("07_screenshot_attached")
        await pg.fill("#ask", "这张图里是什么")
        await pg.press("#ask", "Enter")
        await pg.wait_for_timeout(200)
        lm = await pg.evaluate("__mock.lastMessages")
        c = lm[-1]["content"]
        check(isinstance(c, list) and c[1]["type"] == "image_url", "截图以 image_url 发给模型")
        check(await pg.locator(".m.user img").count() == 1, "对话里显示截图")
        await pg.wait_for_timeout(1500)

        # 8 拖文件
        await pg.evaluate("__mock.drop({payload:{type:'enter'}})")
        check("s-feed" in await pg.get_attribute("#orb", "class"), "拖文件进来小球张嘴")
        await pg.evaluate("__mock.drop({payload:{type:'drop', paths:['/Users/j/notes.md']}})")
        await pg.wait_for_timeout(200)
        check("notes.md" in await pg.inner_text("#attach"), "文件出现在附件")
        await pg.click("#attach .chip button")
        check(await pg.locator("#attach .chip").count() == 0, "移除附件")
        check(await pg.get_attribute("#ask", "placeholder") == "有什么可以帮你？", "移除文件后恢复默认提示")

        # 9 历史
        await pg.click("#histBtn"); await pg.wait_for_timeout(300)
        n = await pg.locator("#histList li").count()
        check(n == 1, f"历史里有 1 个对话（{n}）")
        await shot("08_history")
        await pg.click("#newBtn"); await pg.wait_for_timeout(100)
        check(await pg.locator(".m").count() == 0, "新对话清空")
        await pg.click("#histBtn"); await pg.wait_for_timeout(200)
        await pg.click("#histList li"); await pg.wait_for_timeout(200)
        check(await pg.locator(".m.user").count() >= 3, "打开历史对话恢复消息")

        # 9b 展开后程序挪窗口触发的 moved 事件，不能改掉收起位置
        await pg.evaluate("__mock.moved.forEach(f=>f())"); await pg.wait_for_timeout(600)

        # 10 Esc 收起，回到原位
        before = (await pg.evaluate("__mock"))["pos"]
        await pg.keyboard.press("Escape"); await pg.wait_for_timeout(400)
        m = await pg.evaluate("__mock")
        check(m["size"] == {"w": 96, "h": 96}, f"Esc 收起成小球 {m['size']}")
        check(m["pos"] == {"x": 1300, "y": 380}, f"收起后回到原来的位置 {m['pos']}")
        await shot("09_collapsed")

        # 11 左上角小球：向右下展开；左下角：向上展开
        for name, pos, fx, fy in [("10_topleft", {"x": 20, "y": 80}, False, False), ("11_bottomleft", {"x": 20, "y": 760}, False, True)]:
            await pg.evaluate(f"__mock.pos = {json.dumps(pos)}")
            await pg.click("#orb"); await pg.wait_for_timeout(500)
            cls = await pg.get_attribute("body", "class") or ""
            m = await pg.evaluate("__mock")
            check(("flip-x" in cls) == fx and ("flip-y" in cls) == fy, f"{name} 展开方向 ({cls})")
            check(m["pos"]["y"] >= 50 and m["pos"]["y"] + m["size"]["h"] <= 875, f"{name} 不超出工作区 y={m['pos']['y']}")
            await shot(name)
            await pg.click("#closeBtn"); await pg.wait_for_timeout(400)

        # 11b 拿不到显示器信息时仍能展开
        await pg.evaluate("__mock.monitorFails = true; __mock.pos = {x: 300, y: 200}")
        await pg.click("#orb"); await pg.wait_for_timeout(500)
        mm = await pg.evaluate("[__mock.size, __mock.pos, document.body.className, screen.availWidth, screen.availHeight, innerWidth, innerHeight]")
        print("   monitorFails:", mm)
        check(mm[0]["w"] == 420, "拿不到显示器信息也能展开")
        await pg.click("#closeBtn"); await pg.wait_for_timeout(400)
        await pg.evaluate("__mock.monitorFails = false")
        errs[:] = [e for e in errs if "currentMonitor" not in e]

        # 11a 任务页签：刚才那次提问在列表里，状态是完成
        await pg.click("#orb"); await pg.wait_for_timeout(900)
        await pg.click("#tabTask"); await pg.wait_for_timeout(150)
        tk = await pg.evaluate("[...document.querySelectorAll('#tasks .tk')].map(d => [d.dataset.st, d.innerText.replace(/\\s+/g, ' ')])")
        check(len(tk) >= 1 and tk[0][0] == "done" and "完成" in tk[0][1], f"任务页列出这次打开以来的提问 {tk[:2]}")
        check(await pg.is_hidden("#vChat") and await pg.get_attribute("#tabTask", "aria-selected") == "true", "切到任务页")
        await shot("11a_tasks")
        await pg.click("#tasks .tk"); await pg.wait_for_timeout(150)
        check(await pg.is_visible("#vChat"), "点完成的任务回到对话")
        await pg.click("#closeBtn"); await pg.wait_for_timeout(800)

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
        check((await pg.evaluate("__mock"))["size"]["w"] == 420 and await pg.is_visible("#vChat"), "点气泡打开对话")
        await pg.click("#closeBtn"); await pg.wait_for_timeout(800)
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
            return { barW: Math.round(bar.width), barTop: bar.top, cardBottom: card.getBoundingClientRect().bottom,
                     cls: document.body.className, glassy: card.classList.contains('glassy'), args: __mock.backdropArgs, pos: __mock.pos };
        }""")
        check(g["barW"] == 396, f"展开后小球拉长成和窗口一样宽的胶囊 {g['barW']}")
        check("flip-y" in g["cls"] and g["cardBottom"] <= g["barTop"], "上方放得下时卡片在胶囊上方")
        check(g["pos"] == {"x": 300, "y": 196}, f"卡片在上方时小球留在原地（窗口往上长） {g['pos']}")
        check(g["glassy"] and g["args"]["w"] == 840 and g["args"]["h"] == 1200 and g["args"]["y"] == 392, f"展开前截下窗口后面的桌面当毛玻璃底 {g['args']}")
        await shot("12_capsule_glass")
        await pg.evaluate("__mock.backdrop = null")
        await pg.click("#closeBtn"); await pg.wait_for_timeout(800)
        m = await pg.evaluate("__mock")
        check(m["size"] == {"w": 96, "h": 96} and m["pos"] == {"x": 300, "y": 700}, "收起后小球回到原位")
        await pg.click("#orb"); await pg.wait_for_timeout(900)
        check(not await pg.evaluate("document.querySelector('#card').classList.contains('glassy')"), "拿不到底图时卡片用不透明的底")
        await pg.click("#closeBtn"); await pg.wait_for_timeout(800)

        # 12 菜单事件与快捷键事件
        await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(900)
        check((await pg.evaluate("__mock"))["size"]["w"] == 420, "快捷键呼出")
        await pg.click("#setBtn"); await pg.wait_for_timeout(200)
        check((await pg.evaluate("__mock")).get("settingsOpened") == 2, "卡片上的设置按钮打开设置窗口")
        await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(800)
        check((await pg.evaluate("__mock"))["size"]["w"] == 96, "快捷键收起")
        await pg.click("#orb", button="right")
        check((await pg.evaluate("__mock")).get("menuShown"), "右键弹出系统菜单")

        # 13 拖动：贴边吸附
        await pg.evaluate("__mock.pos = {x: 1355, y: 400}; __mock.moved.forEach(f=>f())")
        await pg.wait_for_timeout(600)
        m = await pg.evaluate("__mock")
        check(m["pos"]["x"] == 1440 - 96 + 12 - 2, f"靠近右边自动贴边 x={m['pos']['x']}")
        check(m["cfg"]["position"] is not None, "记住位置")
        # 鼠标按下移动 → 开始拖动而不是点击
        await pg.mouse.move(48, 48); await pg.mouse.down(); await pg.mouse.move(60, 60); await pg.mouse.up()
        await pg.wait_for_timeout(300)
        m = await pg.evaluate("__mock")
        check(m.get("dragStarted") and m["size"]["w"] == 96, "拖动不会误触展开")

        # 14 外观设置：小球大小、眼睛颜色、卡片透明度
        await pg.evaluate("""() => { Object.assign(__mock.cfg, { orb_size: 84, eye: "#CFE3FF", glass: 50 }); __emit("config"); }""")
        await pg.wait_for_timeout(400)
        look = await pg.evaluate("""() => { const s = getComputedStyle(document.documentElement); return {
            size: __mock.size, orb: document.querySelector('#orb').offsetWidth, eye: s.getPropertyValue('--eye').trim(), tint: s.getPropertyValue('--tint').trim(),
            // 眼睛那一层和它外面那一层一样大才对（两者一起做呼吸动画，所以比的是相对大小）
            face: Math.abs(document.querySelector('.face').getBoundingClientRect().width - document.querySelector('.face').parentElement.getBoundingClientRect().width) }; }""")
        check(look["size"] == {"w": 108, "h": 108} and look["orb"] == 84 and look["face"] < 1, f"小球调大后窗口和眼睛跟着变大 {look}")
        check(look["eye"] == "#CFE3FF" and look["tint"] == "0.5", "眼睛颜色和卡片透明度生效")
        await shot("13_big_orb")

        check(not errs, "页面无 JS 错误 " + "; ".join(errs))

        # ───────── 设置窗口（单独的窗口，照原型：七个分类、模型库 / 添加模型；能力标签查 models.dev） ─────────
        wall = WALL or os.path.join(ROOT, "design-reference", "wallpaper.jpg")
        wu = "data:image/jpeg;base64," + base64.b64encode(open(wall, "rb").read()).decode()
        # 模拟系统亚克力：窗口身后的桌面（原型里设置窗口在 1440×900 的 480,150）模糊后垫在下面，再盖原型舞台的那层暗色
        wall_css = "html{background:#111} body::before{content:'';position:fixed;inset:-40px;background:url(" + wu + ") -440px -110px/1440px 900px no-repeat;filter:blur(24px) saturate(165%);z-index:-1} body::after{content:'';position:fixed;inset:0;background:rgba(6,4,24,.4);z-index:-1}"
        # 服务商 logo 用 CSS mask 加载，file:// 下会被跨域拦掉；真实程序是同源的，这里起个本地 http 服务
        import http.server, threading, functools
        srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(http.server.SimpleHTTPRequestHandler, directory=os.path.join(ROOT, "src")))
        srv.RequestHandlerClass.log_message = lambda *a: None
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        base_url = f"http://127.0.0.1:{srv.server_address[1]}"
        for plat in ["macos", "windows"]:
            ctx2 = await b.new_context(viewport={"width": 480, "height": 600}, device_scale_factor=2)
            sp = await ctx2.new_page()
            serrs = []
            sp.on("pageerror", lambda e: serrs.append(str(e)))
            sp.on("console", lambda m: m.type == "error" and serrs.append(m.text))
            await sp.add_init_script(path=os.path.join(ROOT, "tools", "mock-tauri.js"))
            await sp.add_init_script(f"window.__mock.cfg.platform = '{plat}'")
            await sp.goto(base_url + "/settings.html")
            await sp.add_style_tag(content=wall_css)
            await sp.wait_for_timeout(400)
            async def sp_shot(n):
                await sp.wait_for_timeout(250)
                await sp.screenshot(path=f"{OUT}/{n}.png")
            vis = lambda sel: sp.is_visible(sel)
            txt = lambda sel: sp.inner_text(sel)
            mock = lambda: sp.evaluate("__mock")
            P = f"[{plat}] "
            bc = await sp.get_attribute("body", "class") or ""
            check("fx" in bc and (("win" in bc) == (plat == "windows")), P + f"平台样式 ({bc})")
            check(await sp.get_attribute(".x-wh", "data-tauri-drag-region") is not None, P + "去掉系统标题栏后自己画了一条可拖动的标题栏")
            check((await mock()).get("catalogRefresh") == 1, P + "打开设置时检查模型资料更新")
            # S1 模型库是空的：直接进「添加模型」（原型：首次引导 → 去设置）
            check(await vis("#paneModel") and not await vis("#paneLib") and await txt("#fTitle") == "添加模型", P + "空模型库直接打开添加模型")
            check(await sp.input_value("#fBase") == "https://api.openai.com/v1", P + "OpenAI 默认接口地址")
            check(not await vis("#fCaps"), P + "没填模型名时不显示能力")
            tabs = await sp.eval_on_selector_all(".x-tabs button", "bs => bs.map(b => b.textContent)")
            check(tabs == ["模型", "Agents", "权限", "外观", "快捷键", "划词", "语音"], P + f"七个分类与原型一致 {tabs}")
            await sp_shot(f"20_settings_add_{plat}")
            # S2 输入模型名 → 查 models.dev
            await sp.fill("#fModel", "gpt-4o-mini"); await sp.wait_for_timeout(450)
            caps = await txt("#fCapList")
            check(await vis("#fCaps") and all(t in caps for t in ["工具调用 ✓", "视觉 ✓", "推理 ✗", "上下文 128K"]), P + f"能力标签（含推理）{caps!r}")
            check("models.dev" in await txt("#fCapSrc"), P + "标明资料来源")
            # S3 获取列表 → 选一个资料里没有的
            await sp.click("#fFetch"); await sp.wait_for_timeout(200)
            check(await vis("#fList") and (await sp.eval_on_selector_all("#fList button", "bs => bs.length")) == 3, P + "获取列表显示接口返回的模型")
            await sp_shot(f"21_settings_list_{plat}")
            await sp.evaluate("__mock.listFails = true"); await sp.click("#fFetch"); await sp.wait_for_timeout(200)
            check("连不上" in await txt("#fRes") and not await vis("#fList"), P + "获取列表失败时说明原因")
            await sp.evaluate("__mock.listFails = false"); await sp.click("#fFetch"); await sp.wait_for_timeout(200)
            await sp.click("#fList button:has-text('deepseek-reasoner')"); await sp.wait_for_timeout(300)
            check(await sp.input_value("#fModel") == "deepseek-reasoner" and not await vis("#fList"), P + "点列表填入模型名")
            check("推理 ✓" in await txt("#fCapList"), P + "推理模型标出推理")
            await sp.fill("#fModel", "my-model"); await sp.wait_for_timeout(400)
            caps = await txt("#fCapList")
            check("工具调用 未知" in caps and "上下文 未知" in caps and "没有这个模型" in await txt("#fCapSrc"), P + f"资料里没有的模型显示未知 {caps!r}")
            await sp.click("#fCapList [data-k=tools]"); await sp.click("#fCapList [data-k=ctx]")
            caps = await txt("#fCapList")
            check("工具调用 ✓" in caps and "上下文 8K" in caps and "手动" in await txt("#fCapSrc"), P + f"点标签手动改 {caps!r}")
            # S4 测试连接
            await sp.fill("#fModel", "gpt-4o-mini"); await sp.wait_for_timeout(400)
            await sp.fill("#fKey", "sk-test")
            await sp.click("#fEye"); check(await sp.get_attribute("#fKey", "type") == "text", P + "眼睛按钮显示 Key"); await sp.click("#fEye")
            await sp.click("#fTest"); await sp.wait_for_timeout(200)
            m = await mock()
            check("连接成功 · 412 毫秒" in await txt("#fRes") and await vis("#fRes .x-ok") and m["testArgs"]["apiKey"] == "sk-test", P + "测试连接成功，用的是表单里填的 Key")
            check(["main", "ball-state", "done"] in (m.get("emitted") or []), P + "测试成功时小球开心一下")
            await sp_shot(f"22_settings_test_ok_{plat}")
            await sp.fill("#fBase", "https://bad.example/v1"); await sp.click("#fTest"); await sp.wait_for_timeout(200)
            rh = await sp.evaluate("document.getElementById('fRes').getBoundingClientRect().height")
            check("401" in await txt("#fRes") and "bad" in (await sp.get_attribute("#fRes", "class")) and rh <= 40, P + f"测试失败显示原因（最多两行 {rh}）")
            await sp.fill("#fBase", "ftp://x"); await sp.click("#fSave"); await sp.wait_for_timeout(150)
            check("http" in await txt("#fRes") and not (await mock())["cfg"]["models"], P + "接口地址校验")
            await sp.select_option("#fProv", "OpenAI"); await sp.wait_for_timeout(50)
            await sp.fill("#fModel", ""); await sp.click("#fSave"); await sp.wait_for_timeout(100)
            check("模型名" in await txt("#fRes"), P + "模型名不能为空")
            await sp.fill("#fModel", "gpt-4o-mini"); await sp.fill("#fKey", "sk-test"); await sp.wait_for_timeout(350)
            # S5 保存：首次引导打开的，保存后关掉设置（原型同款）
            await sp.click("#fSave"); await sp.wait_for_timeout(200)
            m = await mock(); ms = m["cfg"]["models"]
            check(len(ms) == 1 and ms[0]["model"] == "gpt-4o-mini" and ms[0]["tools"] is True and ms[0]["reasoning"] is False and ms[0]["ctx"] == 128000
                  and ms[0]["vision"] is True and ms[0]["vision_known"] is True and ms[0]["logo"] == "openai" and ms[0]["provider"] == "OpenAI" and ms[0]["no_key"] is False, P + f"保存进模型库 {ms}")
            check(m["cfg"]["default_model"] == ms[0]["id"] and ms[0]["has_key"] and m["lastKey"] == "sk-test", P + "第一个模型成为默认，Key 已存")
            check(m.get("closed") == 1, P + "首次设置保存后关闭窗口")
            # S6 模型库
            await sp.click(".x-tabs button:has-text('模型')"); await sp.wait_for_timeout(100)
            check(await vis("#paneLib") and await sp.eval_on_selector_all(".x-mc", "x => x.length") == 1, P + "模型库显示一条")
            check("★ 默认" in await txt(".x-mc.def") and "推理 ✗" in await txt(".x-mc"), P + "默认标记和能力标签")
            lg = await sp.evaluate("getComputedStyle(document.querySelector('.x-logo')).getPropertyValue('--logo')")
            check("openai.svg" in lg, P + f"服务商 logo {lg}")
            check("2026-11-14" in await txt("#libSrc"), P + "模型库标明资料日期")
            # 再加两个：DeepSeek（不填 Key）、Ollama 本地
            await sp.click("#libAdd"); await sp.select_option("#fProv", "DeepSeek")
            check(await sp.input_value("#fBase") == "https://api.deepseek.com/v1" and await sp.get_attribute("#fModel", "placeholder") == "如 deepseek-chat", P + "换服务商填入接口地址")
            await sp.fill("#fModel", "deepseek-chat"); await sp.wait_for_timeout(350); await sp.click("#fSave"); await sp.wait_for_timeout(200)
            m = await mock()
            check(await vis("#paneLib") and m.get("closed") == 1, P + "之后保存回到模型库，不关窗口")
            check(m["cfg"]["models"][1]["vision"] is False and m["cfg"]["models"][1]["vision_known"], P + "不支持图片的模型存成不支持（截屏提问不发图）")
            await sp.click("#libAdd"); await sp.select_option("#fProv", "Ollama（本地）")
            check(await vis("#fHint"), P + "本地模型提示可不填 Key")
            await sp.fill("#fModel", "qwen2.5:7b"); await sp.wait_for_timeout(350)
            check("本地 · 免费" in await txt("#fCapList") and "推断" in await txt("#fCapSrc"), P + "本地模型：本地·免费、按相近模型推断")
            await sp.click("#fSave"); await sp.wait_for_timeout(200)
            m = await mock()
            check(m["cfg"]["models"][2]["no_key"] is True, P + "本地模型存成不需要 Key")
            cards = await sp.eval_on_selector_all(".x-mcp", "x => x.map(e => e.textContent)")
            check(len(cards) == 3 and "还没填 Key" in cards[1] and "还没填 Key" not in cards[2] and "还没填 Key" not in cards[0], P + f"模型库三条，没 Key 的标出来 {cards}")
            ov = await sp.evaluate("[document.documentElement.scrollWidth, innerWidth, document.getElementById('paneLib').scrollWidth, document.getElementById('paneLib').clientWidth]")
            check(ov[0] <= ov[1] and ov[2] <= ov[3], P + f"不横向溢出 {ov}")
            await sp_shot(f"23_settings_lib_{plat}")
            # 设为默认 / 编辑 / 删除
            await sp.click(".x-mc:nth-child(2) [data-a=def]"); await sp.wait_for_timeout(150)
            m = await mock()
            check(m["cfg"]["default_model"] == m["cfg"]["models"][1]["id"] and "★ 默认" in await txt(".x-mc:nth-child(2)") and "默认模型" in await txt("#toast"), P + "设为默认")
            await sp.click(".x-mc:nth-child(1) [data-a=edit]")
            check(await txt("#fTitle") == "编辑模型" and await sp.input_value("#fModel") == "gpt-4o-mini" and "已保存" in (await sp.get_attribute("#fKey", "placeholder")), P + "编辑：带出原来的值，Key 留空不改")
            await sp.click("#fFetch"); await sp.wait_for_timeout(150)
            m = await mock()
            check(m["listArgs"]["id"] == m["cfg"]["models"][0]["id"] and not m["listArgs"]["apiKey"], P + "编辑时获取列表用已存的 Key")
            await sp.keyboard.press("Escape"); check(not await vis("#fList") and (await mock()).get("closed") == 1, P + "Esc 先收起列表")
            await sp.fill("#fModel", "gpt-4o"); await sp.wait_for_timeout(350); await sp.click("#fSave"); await sp.wait_for_timeout(200)
            m = await mock()
            check(m["cfg"]["models"][0]["model"] == "gpt-4o" and m["cfg"]["models"][0]["has_key"] and m["lastKey"] is None and len(m["cfg"]["models"]) == 3, P + "编辑后 Key 没动，也没多出一个")
            await sp.click(".x-mc:nth-child(2) [data-a=del]"); await sp.wait_for_timeout(100)
            check(len((await mock())["cfg"]["models"]) == 3 and await txt(".x-mc:nth-child(2) [data-a=del]") == "确认删除", P + "删除要再点一次确认")
            await sp.click(".x-mc:nth-child(2) [data-a=del]"); await sp.wait_for_timeout(150)
            m = await mock()
            check(await sp.eval_on_selector_all(".x-mc", "x => x.length") == 2 and len(m["cfg"]["models"]) == 2 and m["cfg"]["default_model"] == m["cfg"]["models"][0]["id"], P + "删掉默认模型后默认换成剩下的")
            # S7 其余分类
            for name, pane in [("Agents", "#paneAgents"), ("权限", "#panePerm"), ("外观", "#paneLook"), ("快捷键", "#paneKeys"), ("划词", "#paneSel"), ("语音", "#paneVoice")]:
                await sp.click(f".x-tabs button:has-text('{name}')")
                check(await vis(pane) and not await vis("#paneLib") and "on" in (await sp.get_attribute(f".x-tabs button:has-text('{name}')", "class")), P + f"切到{name}")
            # S7a Agents（原型：卡片列表 → 模板 → 编辑器；主控就是提问时用的）
            await sp.click(".x-tabs button:has-text('Agents')"); await sp.wait_for_timeout(100)
            cards = await sp.eval_on_selector_all("#agList .x-agc", "x => x.map(e => e.textContent)")
            check(len(cards) == 1 and "主控" in cards[0] and "拆解与分派" in cards[0] and "（默认）" in cards[0] and "工具 3" in cards[0], P + f"Agents 列表有主控 {cards}")
            check(await sp.input_value("#bTok") == "50000" and await sp.input_value("#bStep") == "50" and await sp.input_value("#bPar") == "3", P + "预算显示当前值")
            await sp_shot(f"30_agents_{plat}")
            await sp.select_option("#bTok", "100000"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["limits"] == {"tokens": 100000, "steps": 50, "parallel": 3}, P + "改预算就保存")
            # 编辑主控：改指令、选模型；读文件锁住
            await sp.click("#agList [data-ag=lead]"); await sp.wait_for_timeout(100)
            check(await vis("#paneAgEd") and not await vis("#paneAgents") and await sp.input_value("#aeName") == "主控", P + "打开主控编辑器")
            check(not await sp.is_visible(".x-aef .x-lbq:has-text('删除 Agent')"), P + "主控不能删")
            await sp.dispatch_event("[data-tool='读文件']", "click"); await sp.wait_for_timeout(100)
            check(await sp.get_attribute("[data-tool='读文件']", "aria-pressed") == "true" and "读文件" in await txt("#toast"), P + "主控的读文件去不掉")
            opts = await sp.eval_on_selector_all("#aeModel option", "o => o.map(x => x.textContent)")
            check(opts[0] == "跟随默认" and len(opts) == 3, P + f"模型下拉是模型库 {opts}")
            await sp.fill("#aePrompt", "只用中文回答"); await sp.select_option("#aeModel", index=2); await sp.select_option("#aeFallback", index=1)
            await sp_shot(f"31_agent_edit_{plat}")
            await sp.click(".x-aef .x-pri"); await sp.wait_for_timeout(150)
            m = await mock(); lead = m["cfg"]["agents"][0]
            check(lead["prompt"] == "只用中文回答" and lead["model"] == m["cfg"]["models"][1]["id"] and lead["fallback"] == m["cfg"]["models"][0]["id"] and len(m["cfg"]["models"]) == 2, P + f"主控的指令、模型、备用模型存上 {lead}")
            check(await vis("#paneAgents") and m["cfg"]["models"][1]["model"] in await txt("#agList [data-ag=lead] >> xpath=.."), P + "保存后回到列表，卡片显示选的模型")
            # 从模板新建：审查员
            await sp.click("#agAdd"); check(await vis("#agTpl") and await sp.eval_on_selector_all("#agTpl .x-agtb", "x => x.length") == 3, P + "新建 Agent 显示三个模板")
            await sp.click("#agTpl .x-agtb:has-text('审查员')"); await sp.wait_for_timeout(100)
            check(await sp.input_value("#aeName") == "审查员" and await sp.get_attribute("[data-tool='读文件']", "aria-pressed") == "true" and await sp.get_attribute("[data-tool='写文件']", "aria-pressed") == "false", P + "模板带出名字和工具")
            await sp.fill("#aeTry", "帮我检查这份报告的错别字"); await sp.wait_for_timeout(100)
            t = await txt("#aeTryR")
            check("主控会交给：审查员" in t and "会用 读文件" in t, P + f"试一试 {t!r}")
            await sp.fill("#aeTry", "帮我删除旧文件"); await sp.wait_for_timeout(100)
            t = await txt("#aeTryR")
            check("不会交给它" in t, P + f"试一试：不相关的不交给它 {t!r}")
            await sp.fill(".x-emo", "审"); await sp.wait_for_timeout(50)
            await sp.click(".x-aeav .x-dot:nth-child(2)")
            await sp_shot(f"32_agent_new_{plat}")
            await sp.click(".x-aef .x-pri"); await sp.wait_for_timeout(150)
            m = await mock(); ag = m["cfg"]["agents"]
            check(len(ag) == 2 and ag[1]["name"] == "审查员" and ag[1]["emoji"] == "审" and ag[1]["tint"] == "#A9BEDB" and ag[1]["tools"] == ["读文件"] and ag[1]["model"] == 0, P + f"新建的 Agent 存上 {ag[1] if len(ag) > 1 else ag}")
            check("已新建 Agent：审查员" in await txt("#toast") and await sp.eval_on_selector_all("#agList .x-agc", "x => x.length") == 2, P + "列表里多了一个")
            # 改名、Esc 返回、删除要确认
            await sp.click(f"#agList [data-ag={ag[1]['id']}]"); await sp.fill("#aeName", "小审"); await sp.click(".x-aef .x-pri"); await sp.wait_for_timeout(150)
            check("已改名：审查员 → 小审" in await txt("#toast") and (await mock())["cfg"]["agents"][1]["name"] == "小审", P + "改名")
            await sp.click(f"#agList [data-ag={ag[1]['id']}]"); await sp.keyboard.press("Escape"); await sp.wait_for_timeout(50)
            check(await vis("#paneAgents") and (await mock()).get("closed") == 1, P + "编辑器里 Esc 返回列表，不关窗口")
            await sp.click(f"#agList [data-ag={ag[1]['id']}]"); await sp.click(".x-aef .x-lbq:has-text('删除 Agent')")
            check("删除「小审」" in await txt(".x-aef") and len((await mock())["cfg"]["agents"]) == 2, P + "删除先确认")
            await sp.click(".x-aef .x-lbx"); await sp.wait_for_timeout(150)
            check(len((await mock())["cfg"]["agents"]) == 1 and await vis("#paneAgents"), P + "删除 Agent")
            hz = await sp.evaluate("[document.getElementById('paneAgEd').scrollWidth, document.getElementById('paneAgEd').clientWidth]")
            check(hz[0] <= hz[1], P + f"编辑器不横向溢出 {hz}")
            # S7b 权限：已总是允许（对话里点「总是允许」会加进来）、撤销、离开时暂停
            await sp.evaluate("__mock.cfg.always_allow = [{kind: '删除', scope: 'Downloads'}, {kind: '写入', scope: '周报'}]; __emit('config')"); await sp.wait_for_timeout(150)
            await sp.click(".x-tabs button:has-text('权限')"); await sp.wait_for_timeout(100)
            items = await sp.eval_on_selector_all("#alList li", "x => x.map(e => e.querySelector('b').textContent + '|' + e.querySelector('small').textContent)")
            check(items == ["删除|范围：Downloads", "写入|范围：周报"] and not await vis("#alEmpty"), P + f"权限页列出总是允许 {items}")
            check(await sp.get_attribute("#awaySw", "aria-checked") == "true", P + "离开时暂停默认开")
            await sp_shot(f"33_perm_{plat}")
            await sp.click("#alList li:first-child button"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["always_allow"] == [{"kind": "写入", "scope": "周报"}] and "已撤销：删除 · Downloads" in await txt("#toast"), P + "撤销一条")
            await sp.click("#alList li button"); await sp.wait_for_timeout(150)
            check(await vis("#alEmpty") and (await mock())["cfg"]["always_allow"] == [], P + "全撤销后显示空状态")
            await sp.click("#awaySw"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["pause_away"] is False and await sp.get_attribute("#awaySw", "aria-checked") == "false", P + "关掉离开时暂停")
            # S8 外观
            await sp.click(".x-tabs button:has-text('外观')")
            check(await vis("#lkGlassF") == (plat == "windows"), P + "卡片透明度只在 Windows 显示")
            await sp.click("#lkSize button:has-text('大')"); await sp.wait_for_timeout(150)
            await sp.click("#lkEye [aria-label='蜜桃']"); await sp.wait_for_timeout(150)
            if plat == "windows":
                await sp.evaluate("""() => { const r = document.querySelector('#lkGlass'); r.value = 40; r.dispatchEvent(new Event('input')); r.dispatchEvent(new Event('change')); }""")
                await sp.wait_for_timeout(150)
            c = (await mock())["cfg"]
            check(c["orb_size"] == 84 and c["eye"] == "#FFC9A6" and c["glass"] == (40 if plat == "windows" else 25), P + f"外观改了就保存 {c['orb_size']} {c['eye']} {c['glass']}")
            check(await sp.get_attribute("#lkSize button:has-text('大')", "aria-checked") == "true" and await sp.get_attribute("#lkEye [aria-label='蜜桃']", "aria-checked") == "true", P + "外观显示当前选中")
            await sp_shot(f"24_settings_look_{plat}")
            # S9 快捷键：点「录制」再按组合键
            await sp.click(".x-tabs button:has-text('快捷键')")
            kc = lambda k: txt(f"#keyList [data-k={k}] .x-kc")
            check(await kc("shortcut") == ("⌥ Space" if False else "Alt+Space"), P + f"显示当前快捷键 {await kc('shortcut')}")
            await sp.click("#keyList [data-k=shortcut] .x-sb")
            check("按下新的组合键" in await kc("shortcut"), P + "点录制进入录入")
            await sp.keyboard.press("KeyK")
            check("至少带一个" in await txt("#keyList [data-k=shortcut] .x-kw") and (await mock())["cfg"]["shortcut"] == "Alt+Space", P + "不带修饰键的不接受")
            await sp.keyboard.press("Control+Shift+KeyK"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["shortcut"] == "Control+Shift+K" and await kc("shortcut") == "Ctrl+Shift+K", P + "录入新的快捷键")
            await sp.click("#keyList [data-k=capture_shortcut] .x-sb"); await sp.keyboard.press("Control+Shift+KeyK"); await sp.wait_for_timeout(150)
            check("相同" in await txt("#keyList [data-k=capture_shortcut] .x-kw") and (await mock())["cfg"]["capture_shortcut"] == "Alt+Shift+S", P + "两个快捷键不能一样")
            await sp.keyboard.press("Escape")
            check(await kc("capture_shortcut") == "Alt+Shift+S" and (await mock()).get("closed") == 1, P + "录快捷键时 Esc 只取消录入，不关窗口")
            await sp.evaluate("__mock.shortcutBusy = true")
            await sp.click("#keyList [data-k=shortcut] .x-sb"); await sp.keyboard.press("Alt+KeyJ"); await sp.wait_for_timeout(150)
            w = await txt("#keyList [data-k=shortcut] .x-kw")
            check((await mock())["cfg"]["shortcut"] == "Alt+J" and "占用" in w and "占用" in await txt("#toast"), P + f"快捷键被占用时提示换一个（{w}）")
            await sp_shot(f"25_settings_keys_{plat}")
            await sp.evaluate("__mock.shortcutBusy = false")
            await sp.click("#keyList [data-k=shortcut] .x-sb"); await sp.keyboard.press("Alt+Space"); await sp.wait_for_timeout(150)
            check(await txt("#keyList [data-k=shortcut] .x-kw") == "" and (await mock())["cfg"]["shortcut"] == "Alt+Space", P + "快捷键没被占用时不再提示")
            await sp.keyboard.press("Escape")
            check((await mock()).get("closed") == 2, P + "Esc 关闭设置窗口")
            await sp.click("#setClose")
            check((await mock()).get("closed") == 3, P + "标题栏的 × 关闭设置窗口")
            check(not serrs, P + "设置窗口无 JS 错误 " + "; ".join(serrs))
            await ctx2.close()
        srv.shutdown()
        await b.close()
    print("\n%d 项失败" % len(fails) if fails else "\n全部通过")
    sys.exit(1 if fails else 0)

asyncio.run(main())
