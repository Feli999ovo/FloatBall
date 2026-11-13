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

        # ───────── 设置窗口（单独的窗口，单独的页面） ─────────
        ctx2 = await b.new_context(viewport={"width": 760, "height": 560}, device_scale_factor=2)
        s = await ctx2.new_page()
        errs2 = []
        s.on("pageerror", lambda e: errs2.append(str(e)))
        s.on("console", lambda m: m.type == "error" and errs2.append(m.text))
        await s.add_init_script(path=os.path.join(ROOT, "tools", "mock-tauri.js"))
        await s.goto("file://" + os.path.join(ROOT, "src", "settings.html"))
        await s.wait_for_timeout(400)
        mock = lambda: s.evaluate("__mock")

        check(await s.is_visible("#top") and await s.get_attribute("#top", "data-tauri-drag-region") is not None, "去掉系统标题栏后自己画了一条可拖动的标题栏")
        check((await s.evaluate("getComputedStyle(document.body).backgroundImage")).startswith("linear-gradient"), "底色是半透明的，让系统毛玻璃透出来")

        # S1 模型库是空的：直接打开「添加模型」
        check(await s.is_visible("#formView") and await s.inner_text("#formTitle") == "添加模型", "模型库是空的时直接打开添加模型")
        await s.click("#presets button:has-text('DeepSeek')")
        check(await s.input_value("#fModel") == "deepseek-chat" and not await s.is_checked("#fVision"), "预设填入模型名，DeepSeek 关闭图片")
        await s.fill("#fBase", "ftp://x"); await s.click("#saveModelBtn")
        check("http" in await s.inner_text("#formMsg") and len((await mock())["cfg"]["models"]) == 0, "接口地址校验")
        await s.click("#presets button:has-text('DeepSeek')")
        await s.fill("#fKey", "sk-test")
        await s.click("#testBtn"); await s.wait_for_timeout(200)
        check("连接成功" in await s.inner_text("#formMsg") and (await mock())["testArgs"]["apiKey"] == "sk-test", "测试连接用的是表单里填的 Key")
        await s.click("#fetchBtn"); await s.wait_for_timeout(200)
        check(await s.locator("#modelOptions option").count() == 3 and "3 个" in await s.inner_text("#formMsg"), "获取模型列表")
        await s.screenshot(path=f"{OUT}/20_settings_add_model.png")
        await s.evaluate("__mock.listFails = true")
        await s.click("#fetchBtn"); await s.wait_for_timeout(200)
        check("连不上" in await s.inner_text("#formMsg"), "获取模型列表失败时说明原因")
        await s.evaluate("__mock.listFails = false")
        await s.fill("#fModel", ""); await s.click("#saveModelBtn")
        check("模型名" in await s.inner_text("#formMsg"), "模型名不能为空")
        await s.fill("#fModel", "deepseek-chat")
        await s.click("#saveModelBtn"); await s.wait_for_timeout(200)
        m = await mock()
        check(len(m["cfg"]["models"]) == 1 and m["cfg"]["models"][0]["has_key"] and m["cfg"]["default_model"] == 1, "保存模型，第一个自动成为默认")
        check(await s.is_visible("#libView") and "默认" in await s.inner_text("#modelList li"), "回到模型库，标出默认模型")

        # S2 第二个模型（本地，不需要 Key），设为默认
        await s.click("#addBtn")
        await s.click("#presets button:has-text('Ollama')")
        check(await s.is_checked("#fNoKey"), "本地模型预设勾上不需要 Key")
        await s.fill("#fName", "本地千问")
        await s.click("#saveModelBtn"); await s.wait_for_timeout(200)
        check(await s.locator("#modelList li").count() == 2 and "本地千问" in await s.inner_text("#modelList"), "模型库里有两个模型，显示自己起的名字")
        check("没有 Key" not in await s.inner_text("#modelList"), "不需要 Key 的模型不提示没有 Key")
        await s.click("#modelList li:nth-child(2) button:has-text('设为默认')"); await s.wait_for_timeout(200)
        check((await mock())["cfg"]["default_model"] == 2 and "默认" in await s.inner_text("#modelList li:nth-child(2)"), "设为默认")
        await s.screenshot(path=f"{OUT}/21_settings_models.png")

        # S3 编辑：不重填 Key 就不改 Key
        await s.click("#modelList li:nth-child(1) button:has-text('编辑')")
        check("已保存" in await s.get_attribute("#fKey", "placeholder") and await s.input_value("#fKey") == "", "编辑时不显示已存的 Key")
        await s.fill("#fModel", "deepseek-reasoner")
        await s.click("#saveModelBtn"); await s.wait_for_timeout(200)
        m = await mock()
        check(m["cfg"]["models"][0]["model"] == "deepseek-reasoner" and m["cfg"]["models"][0]["has_key"] and m["lastKey"] is None, "编辑后 Key 没动")
        check(len(m["cfg"]["models"]) == 2, "编辑不会多出一个模型")

        # S4 删除要点两次；删掉默认的，默认换成剩下的
        await s.click("#modelList li:nth-child(2) button:has-text('删除')")
        check(len((await mock())["cfg"]["models"]) == 2 and await s.locator("#modelList button:has-text('确认删除')").count() == 1, "删除要再点一次确认")
        await s.click("#modelList button:has-text('确认删除')"); await s.wait_for_timeout(200)
        m = await mock()
        check(len(m["cfg"]["models"]) == 1 and m["cfg"]["default_model"] == 1, "删掉默认模型后默认换成剩下的")

        # S5 左侧分类：键盘上下切换
        await s.focus("#tabModels"); await s.keyboard.press("ArrowDown")
        check(await s.is_visible("#pLook") and not await s.is_visible("#pModels") and await s.get_attribute("#tabLook", "aria-selected") == "true", "方向键切换分类")

        # S6 外观
        await s.click("#fSize button:has-text('大')"); await s.wait_for_timeout(150)
        await s.click("#fEye button[aria-label='冷白']"); await s.wait_for_timeout(150)
        await s.evaluate("""() => { const r = document.querySelector('#fGlass'); r.value = 40; r.dispatchEvent(new Event('input')); r.dispatchEvent(new Event('change')); }""")
        await s.wait_for_timeout(150)
        c = (await mock())["cfg"]
        check(c["orb_size"] == 84 and c["eye"] == "#CFE3FF" and c["glass"] == 40, f"外观改了就保存 {c['orb_size']} {c['eye']} {c['glass']}")
        check(await s.inner_text("#glassOut") == "40%" and await s.inner_text("#lookMsg") == "已保存", "外观页显示当前值和已保存")
        await s.screenshot(path=f"{OUT}/22_settings_look.png")

        # S7 快捷键：点一下再按组合键
        await s.click("#tabKeys")
        await s.click("#fKeyMain")
        check("请按" in await s.inner_text("#fKeyMain"), "点一下进入录入")
        await s.keyboard.press("KeyK")
        check("至少带一个" in await s.inner_text("#keysMsg") and (await mock())["cfg"]["shortcut"] == "Alt+Space", "不带修饰键的不接受")
        await s.keyboard.press("Control+Shift+KeyK"); await s.wait_for_timeout(150)
        check((await mock())["cfg"]["shortcut"] == "Control+Shift+K" and await s.inner_text("#fKeyMain") == "Control+Shift+K", "录入新的快捷键")
        await s.click("#fKeyCap"); await s.keyboard.press("Control+Shift+KeyK"); await s.wait_for_timeout(150)
        check("不能一样" in await s.inner_text("#keysMsg") and (await mock())["cfg"]["capture_shortcut"] == "Alt+Shift+S", "两个快捷键不能一样")
        await s.keyboard.press("Escape")
        check(await s.inner_text("#fKeyCap") == "Alt+Shift+S" and not (await mock()).get("closed"), "录快捷键时 Esc 只取消录入，不关窗口")
        await s.evaluate("__mock.shortcutBusy = true")
        await s.click("#fKeyMain"); await s.keyboard.press("Alt+KeyJ"); await s.wait_for_timeout(150)
        msg = await s.inner_text("#keysMsg")
        check((await mock())["cfg"]["shortcut"] == "Alt+J" and "已保存" in msg and "占用" in msg and "换一个" in msg, f"快捷键被占用时其余设置照常保存，提示换一个（{msg}）")
        await s.screenshot(path=f"{OUT}/23_settings_shortcut_busy.png")
        await s.evaluate("__mock.shortcutBusy = false")
        await s.click("#fKeyMain"); await s.keyboard.press("Alt+Space"); await s.wait_for_timeout(150)
        check(await s.inner_text("#keysMsg") == "已保存" and (await mock())["cfg"]["shortcut"] == "Alt+Space", "快捷键没被占用时提示已保存")

        # S8 通用
        await s.click("#tabGeneral")
        await s.check("#fAuto"); await s.wait_for_timeout(150)
        await s.fill("#fSys", "只用中文回答"); await s.press("#fSys", "Tab"); await s.wait_for_timeout(150)
        c = (await mock())["cfg"]
        check(c["autostart"] and c["system_prompt"] == "只用中文回答", "开机启动和系统提示词")
        await s.screenshot(path=f"{OUT}/24_settings_general.png")
        await s.click("#fSys"); await s.keyboard.press("Escape"); await s.wait_for_timeout(100)
        check((await mock()).get("closed") == 1, "Esc 关闭设置窗口")
        await s.click("#closeBtn")
        check((await mock()).get("closed") == 2, "标题栏的 × 关闭设置窗口")
        check(not errs2, "设置窗口无 JS 错误 " + "; ".join(errs2))
        await b.close()
    print("\n%d 项失败" % len(fails) if fails else "\n全部通过")
    sys.exit(1 if fails else 0)

asyncio.run(main())
