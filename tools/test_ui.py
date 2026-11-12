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
        m = await shot("01c_first_run_settings")
        check(await pg.is_visible("#vSet"), "点「去设置」打开设置")

        # 1b 设置填到一半点了别处：收起成小球，再点开还在设置页，填的内容没丢
        await pg.fill("#fModel", "half-typed")
        await pg.evaluate("document.hasFocus = () => false; __mock.focus[0]({ payload: false })")
        await pg.wait_for_timeout(600)
        check((await pg.evaluate("__mock"))["size"] == {"w": 96, "h": 96}, "设置页点别处会收起，不再一直挡着")
        await pg.evaluate("delete document.hasFocus")
        await pg.click("#orb"); await pg.wait_for_timeout(500)
        check(await pg.is_visible("#vSet") and await pg.input_value("#fModel") == "half-typed", "再点开接着填，内容没丢")
        check(await pg.evaluate("getComputedStyle(document.querySelector('#ask'), '::-webkit-scrollbar').display") == "none", "输入框不显示滚动条箭头")
        m = await pg.evaluate("__mock")
        bodycls = await pg.get_attribute("body", "class")
        check("flip-x" in bodycls, f"小球在右侧时卡片向左展开 ({bodycls})")
        check(m["pos"]["x"] + 420 <= 1440, f"展开后不超出屏幕右边 x={m['pos']['x']}")

        # 2 预设 + 填 Key + 测试 + 保存
        await pg.click("#presets button:has-text('DeepSeek')")
        check(await pg.input_value("#fModel") == "deepseek-chat", "预设填入模型名")
        check(not await pg.is_checked("#fVision"), "DeepSeek 预设关闭图片")
        await pg.fill("#fKey", "sk-test")
        await pg.click("#testBtn"); await pg.wait_for_timeout(200)
        check("连接成功" in await pg.inner_text("#setMsg"), "测试连接显示结果")
        await pg.fill("#fBase", "ftp://x"); await pg.click("#saveBtn")
        check("http" in await pg.inner_text("#setMsg"), "接口地址校验")
        await pg.click("#presets button:has-text('OpenAI')")
        await pg.fill("#fKey", "sk-test")
        await pg.check("#fAuto")
        await pg.click("#saveBtn"); await pg.wait_for_timeout(200)
        m = await pg.evaluate("__mock")
        check(m["cfg"]["has_key"] and m["cfg"]["model"] == "gpt-4o-mini" and m["cfg"]["autostart"], "保存设置（Key、模型、开机启动）")
        await shot("02_settings_saved")

        # 2b 快捷键被别的程序占用：其余设置照常保存，只提示换快捷键
        await pg.evaluate("__mock.shortcutBusy = true")
        await pg.fill("#fModel", "gpt-4o")
        await pg.click("#saveBtn"); await pg.wait_for_timeout(200)
        m = await pg.evaluate("__mock")
        msg = await pg.inner_text("#setMsg")
        check(m["cfg"]["model"] == "gpt-4o", "快捷键被占用时其余设置照常保存")
        check("已保存" in msg and "占用" in msg and "换一个" in msg, f"快捷键被占用时提示换一个（{msg}）")
        await shot("02b_shortcut_busy")
        await pg.evaluate("__mock.shortcutBusy = false")
        await pg.fill("#fModel", "gpt-4o-mini")
        await pg.click("#saveBtn"); await pg.wait_for_timeout(200)
        check(await pg.inner_text("#setMsg") == "已保存", "快捷键没被占用时照常提示已保存")

        # 3 回到对话，空状态
        await pg.click("#setBtn"); await pg.wait_for_timeout(200)
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
        await pg.evaluate("__emit('menu','settings')"); await pg.wait_for_timeout(500)
        check(await pg.is_visible("#vSet"), "菜单「设置」打开设置")
        await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(400)
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

        check(not errs, "页面无 JS 错误 " + "; ".join(errs))
        await b.close()
    print("\n%d 项失败" % len(fails) if fails else "\n全部通过")
    sys.exit(1 if fails else 0)

asyncio.run(main())
