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
        b = await p.chromium.launch(args=["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream", "--autoplay-policy=no-user-gesture-required"])
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
        eyes = await pg.evaluate("(() => { const o = document.querySelector('#orb').getBoundingClientRect(), b = document.querySelector('#bar').getBoundingClientRect(), a = document.querySelector('#ask').getBoundingClientRect(); return [o.left - b.left, a.left > o.right]; })()")
        check(abs(eyes[0]) < 1 and eyes[1], f"往左长时眼睛在胶囊左头、输入框在右边（照原型） {eyes}")
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
        oc = await pg.get_attribute("#orb", "class")
        check("s-wait" in oc and "need" in oc, f"等确认时小球是「等你」表情（同原型 setState('wait')）{oc}")
        await shot("06b_ask")
        check(await pg.evaluate("getComputedStyle(document.querySelector('#orb .need')).opacity") == "0", "卡片开着时不显示「!」")
        await pg.evaluate("collapse()"); await pg.wait_for_timeout(800)
        g = await pg.evaluate("""(() => { const n = document.querySelector('#orb .need'), o = document.querySelector('#orb').getBoundingClientRect(), r = n.getBoundingClientRect(),
            ring = getComputedStyle(document.querySelector('#orb .breath'), '::before');
            return { op: getComputedStyle(n).opacity, txt: n.textContent, topRight: r.right <= o.right + 1 && r.left > o.left + o.width / 2 && r.top < o.top + o.height / 3,
                     ring: ring.content !== 'none' && ring.borderTopStyle === 'solid', eyeTop: getComputedStyle(document.querySelector('#orb .eye')).animationName }; })()""")
        check(g["op"] == "1" and g["txt"] == "!" and g["topRight"], f"收起后球右上角亮「!」{g}")
        check(g["ring"] and g["eyeTop"] == "slowblink", f"收起后「等你」：暖白呼吸光环 + 慢速眨眼 {g}")
        await shot("06b2_wait_orb")
        await pg.evaluate("document.body.classList.add('x'); document.querySelector('#orb').dataset.edge = 'r'"); await pg.wait_for_timeout(300)
        g2 = await pg.evaluate("(() => { const n = document.querySelector('#orb .need').getBoundingClientRect(), o = document.querySelector('#orb').getBoundingClientRect(); return n.left < o.left + o.width / 2; })()")
        check(g2, "贴右边时「!」换到左上角")
        await pg.evaluate("document.querySelector('#orb').dataset.edge = ''")
        await pg.click("#orb"); await pg.wait_for_timeout(900)
        last = pg.locator(".m.ai").last
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
        check("need" not in await pg.get_attribute("#orb", "class"), "确认后去掉「!」")
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
        await pg.evaluate("__emit('menu','capture')"); await pg.wait_for_timeout(300)  # 胶囊里不再有截屏按钮（照原型），走菜单/快捷键
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
            await pg.click("#orb"); await pg.wait_for_timeout(400)  # 卡片上不再有 ×（照原型），点小球收起

        # 11b 拿不到显示器信息时仍能展开
        await pg.evaluate("__mock.monitorFails = true; __mock.pos = {x: 300, y: 200}")
        await pg.click("#orb"); await pg.wait_for_timeout(500)
        mm = await pg.evaluate("[__mock.size, __mock.pos, document.body.className, screen.availWidth, screen.availHeight, innerWidth, innerHeight]")
        print("   monitorFails:", mm)
        check(mm[0]["w"] == 420, "拿不到显示器信息也能展开")
        await pg.click("#orb"); await pg.wait_for_timeout(400)  # 卡片上不再有 ×（照原型），点小球收起
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
        check((await pg.evaluate("__mock"))["size"]["w"] == 420 and await pg.is_visible("#vChat"), "点气泡打开对话")
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
            return { barW: Math.round(bar.width), barTop: bar.top, cardBottom: card.getBoundingClientRect().bottom,
                     cls: document.body.className, glassy: card.classList.contains('glassy'), args: __mock.backdropArgs, pos: __mock.pos };
        }""")
        check(g["barW"] == 396, f"展开后小球拉长成和窗口一样宽的胶囊 {g['barW']}")
        check("flip-y" in g["cls"] and g["cardBottom"] <= g["barTop"], "上方放得下时卡片在胶囊上方")
        check(g["pos"] == {"x": 300, "y": 196}, f"卡片在上方时小球留在原地（窗口往上长） {g['pos']}")
        check(g["glassy"] and g["args"]["w"] == 840 and g["args"]["h"] == 1200 and g["args"]["y"] == 392, f"展开前截下窗口后面的桌面当毛玻璃底 {g['args']}")
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
        check(not await pg.is_hidden("#vTask") and (await pg.evaluate("__mock"))["size"]["w"] == 420, "快捷键「打开任务」")
        await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(1000)
        await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(900)
        check((await pg.evaluate("__mock"))["size"]["w"] == 420, "快捷键呼出")
        # 卡片头照原型：没有设置和 ×，⋯ 里是钉住
        check(await pg.locator("#setBtn, #closeBtn").count() == 0, "卡片头照原型：设置在右键菜单，收起点小球")
        await pg.click("#moreBtn"); await pg.wait_for_timeout(200)
        check(await pg.evaluate("document.querySelector('#moreDd').classList.contains('on')"), "⋯ 打开下拉")
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
        check(not await pg.evaluate("orb.classList.contains('docked')") and (await pg.evaluate("__mock"))["size"]["w"] == 420, "贴边时点开正常展开")
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
        check(not await pg.is_hidden("#vHist") and (await pg.evaluate("__mock"))["size"]["w"] == 420, "点小泡「历史记录」打开历史")
        await pg.evaluate("__emit('menu','toggle')"); await pg.wait_for_timeout(1000)
        # 小泡弹出时直接点小球（Windows 真机截图里的做法）：等小泡收完再展开，卡片不被缩回去的窗口截掉
        await pg.mouse.move(5, 5); await pg.wait_for_timeout(100)
        await pg.hover("#orb"); await pg.wait_for_timeout(900)
        assert await pg.evaluate("hv.shown")
        await pg.click("#orb"); await pg.wait_for_timeout(1500)
        m = await pg.evaluate("__mock")
        check(await pg.evaluate("S.open") and m["size"]["w"] == 420 and not await pg.evaluate("document.body.classList.contains('hovering')"), f"小泡弹出时点小球：照常展开成卡片 {m['size']}")
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
        # ───────── 语音（原型的 vcStart / vcRelease / vcDict） ─────────
        lastu = lambda: pg.evaluate("[...document.querySelectorAll('.m.user')].pop()?.textContent.trim()")
        sttn = lambda: pg.evaluate("(__mock.stt || []).length")
        await pg.wait_for_timeout(1500)
        await pg.evaluate("collapse()"); await pg.wait_for_timeout(700)
        sz0 = (await pg.evaluate("__mock"))["size"]
        o = await pg.evaluate("(() => { const r = document.querySelector('#orb').getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()")
        p0 = dict((await pg.evaluate("__mock"))["pos"])
        await pg.mouse.move(o[0], o[1]); await pg.mouse.down(); await pg.wait_for_timeout(1300)
        m = await pg.evaluate("__mock")
        vis_bub = await pg.evaluate("document.querySelector('#vcBub').classList.contains('on') && document.querySelector('#vcRip').classList.contains('on') && document.body.classList.contains('vc-rec')")
        bub_t = await pg.inner_text("#vcBub .vc-txt")
        check(vis_bub and "在听" in bub_t and m["size"]["w"] > sz0["w"] + 200, f"长按小球 350ms：开始录音，窗口扩开放下涟漪和气泡 {m['size']} {bub_t}")
        ob = await pg.evaluate("(() => { const r = document.querySelector('#orb').getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()")
        check(abs(m["pos"]["x"] + ob[0] - (p0["x"] + o[0])) < 2 and abs(m["pos"]["y"] + ob[1] - (p0["y"] + o[1])) < 2, f"窗口扩开后小球留在屏幕上原来的位置 {p0} {o} → {m['pos']} {ob}")
        lv = await pg.evaluate("getComputedStyle(document.body).getPropertyValue('--vc-l')")
        check(lv.strip() != "", f"眼睛跟着音量变化（--vc-l={lv}）")
        check(await pg.inner_text("#vcBub .vc-time") in ("0:00", "0:01"), "气泡里显示录音时长")
        await shot("16_voice_rec")
        await pg.mouse.up(); await pg.wait_for_timeout(1600)
        m = await pg.evaluate("__mock")
        st = (m.get("stt") or [{}])[-1]
        check(len(m.get("stt") or []) == 1 and st.get("bytes", 0) > 0 and st.get("mime", "").startswith("audio/"), f"松开：录音交给识别接口 {st}")
        check(m.get("sttBegin") == 1 and m["cfg"]["voice"]["engine"] == "system", f"默认用系统自带识别：按下时先让系统开始听 {m.get('sttBegin')}")
        check(await lastu() == "帮我把备忘录整理成周报" and "帮我把备忘录整理成周报" in lu(m.get("lastMessages") or [{"role": "user", "content": ""}]), "识别出的文字直接发送（默认「直接发送」）")
        check(not await pg.evaluate("document.querySelector('#vcBub').classList.contains('on')") and not await pg.evaluate("document.body.classList.contains('voicing')"), "气泡收起")
        await pg.wait_for_timeout(1600)
        # 毛玻璃：录音气泡和对话卡片同一种
        await pg.evaluate("collapse()"); await pg.wait_for_timeout(700)
        await pg.evaluate("""(() => { const c = document.createElement('canvas'); c.width = 60; c.height = 25; const g = c.getContext('2d');
            const gr = g.createLinearGradient(0, 0, 60, 25); gr.addColorStop(0, '#f3a26b'); gr.addColorStop(1, '#7c3a6e'); g.fillStyle = gr; g.fillRect(0, 0, 60, 25);
            __mock.backdrop = c.toDataURL('image/jpeg'); })()""")
        n1 = await sttn()
        await pg.mouse.move(o[0], o[1]); await pg.mouse.down(); await pg.wait_for_timeout(1300)
        gv = await pg.evaluate("""(() => { const e = document.querySelector('#vcBub'), eye = document.querySelector('.eye');
            return { glassy: e.classList.contains('glassy'), pos: e.style.getPropertyValue('--bd-pos'), want: -e.offsetLeft + 'px ' + -e.offsetTop + 'px',
                     args: __mock.backdropArgs, size: __mock.size, eyeH: eye.offsetHeight, mood: document.querySelector('#orb').className }; })()""")
        check(gv["glassy"] and gv["pos"] == gv["want"] and gv["args"]["w"] == gv["size"]["w"] * 2 and gv["args"]["h"] == gv["size"]["h"] * 2, f"录音气泡：扩开前截下桌面，垫成和对话卡片同一种毛玻璃 {gv}")
        check(gv["eyeH"] == 21, f"录音时眼睛保持平时的形状（同原型），只跟音量拉伸 {gv['eyeH']} {gv['mood']}")
        await shot("16_voice_glass")
        await pg.keyboard.press("Escape"); await pg.wait_for_timeout(300); await pg.mouse.up(); await pg.wait_for_timeout(700)
        check(await sttn() == n1 and not await pg.evaluate("document.querySelector('#vcBub').classList.contains('glassy')"), "收起后去掉底图（下次重新截）")
        await pg.evaluate("__mock.backdrop = null")
        # 太短
        await pg.evaluate("collapse()"); await pg.wait_for_timeout(700)
        n0 = await sttn()
        await pg.mouse.move(o[0], o[1]); await pg.mouse.down(); await pg.wait_for_timeout(700); await pg.mouse.up(); await pg.wait_for_timeout(900)
        m = await pg.evaluate("__mock")
        check(await sttn() == n0 and "说话时间太短" in await pg.inner_text("#peek") and m["size"]["w"] < 420, "按得太短：不识别，提示「说话时间太短」")
        await pg.evaluate("hidePeek()"); await pg.wait_for_timeout(500)
        # 拖开取消
        await pg.mouse.move(o[0], o[1]); await pg.mouse.down(); await pg.wait_for_timeout(1000)
        ob = await pg.evaluate("(() => { const r = document.querySelector('#orb').getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()")
        # 无头浏览器里 screenX 不随窗口移动，按下点在这里就是 o
        await pg.mouse.move(o[0] + 70, o[1], steps=5); await pg.wait_for_timeout(200)
        check(await pg.evaluate("document.querySelector('#vcBub').classList.contains('vc-x')") and await pg.is_visible("#vcBub .vc-cx"), "拖开 48 像素：气泡变红「松开取消」")
        await shot("16_voice_cancel")
        await pg.mouse.move(o[0] + 10, o[1], steps=3); await pg.wait_for_timeout(150)
        check(not await pg.evaluate("document.querySelector('#vcBub').classList.contains('vc-x')"), "拖回来恢复")
        await pg.mouse.move(o[0] + 80, o[1], steps=3); await pg.wait_for_timeout(150)
        await pg.mouse.up(); await pg.wait_for_timeout(900)
        m = await pg.evaluate("__mock")
        check(await sttn() == n0 and m["size"]["w"] < 420 and not await pg.evaluate("document.body.classList.contains('voicing')"), "松开取消：不识别，窗口缩回")
        # 快捷键：先放进输入框
        await pg.evaluate("__mock.cfg.voice.after = 'box'; __emit('config')"); await pg.wait_for_timeout(300)
        await pg.evaluate("__emit('voice:key', true)"); await pg.wait_for_timeout(150)
        await pg.evaluate("__emit('voice:key', false)"); await pg.wait_for_timeout(500)
        check(await sttn() == n0 and not await pg.evaluate("document.body.classList.contains('vc-rec')"), "快捷键按一下就松（不到 300ms）：不开始")
        await pg.evaluate("__emit('voice:key', true)"); await pg.wait_for_timeout(1400)
        check(await pg.evaluate("document.body.classList.contains('vc-rec')"), "按住快捷键：开始录音")
        await pg.evaluate("__emit('voice:key', false)"); await pg.wait_for_timeout(1600)
        check(await pg.input_value("#ask") == "帮我把备忘录整理成周报" and await lastu() == "帮我把备忘录整理成周报" and await sttn() == n0 + 1, "「先放进输入框」：展开，文字放进输入框，不发送")
        await pg.fill("#ask", "")
        # Esc 取消
        await pg.evaluate("collapse()"); await pg.wait_for_timeout(700)
        await pg.evaluate("__emit('voice:key', true)"); await pg.wait_for_timeout(1200)
        await pg.keyboard.press("Escape"); await pg.wait_for_timeout(700)
        await pg.evaluate("__emit('voice:key', false)"); await pg.wait_for_timeout(600)
        check(await sttn() == n0 + 1 and not await pg.evaluate("document.body.classList.contains('voicing')"), "录音时按 Esc 取消")
        check((await pg.evaluate("__mock")).get("sttCancel", 0) >= 1, "取消时让系统识别也停下、不要结果")
        # 识别出错
        await pg.evaluate("__mock.sttFail = '这个服务商没有语音识别接口，或识别模型名不对（404）'")
        await pg.evaluate("__emit('voice:key', true)"); await pg.wait_for_timeout(1200)
        await pg.evaluate("__emit('voice:key', false)"); await pg.wait_for_timeout(1300)
        check("没有语音识别接口" in await pg.inner_text("#peek") and not await pg.evaluate("document.body.classList.contains('voicing')"), "识别出错：在小球旁说清楚原因")
        await pg.evaluate("delete __mock.sttFail; hidePeek()"); await pg.wait_for_timeout(500)
        # 系统识别开不了（Windows 没开联机语音识别）：说清楚去哪开
        await pg.evaluate("__mock.sttBeginFail = 'Windows 的「联机语音识别」没打开：设置 → 隐私和安全性 → 语音，打开后再试；或在悬浮球设置 → 语音里改用服务商识别'")
        n2 = await sttn()
        await pg.evaluate("__emit('voice:key', true)"); await pg.wait_for_timeout(1200)
        await pg.evaluate("__emit('voice:key', false)"); await pg.wait_for_timeout(700)
        check("联机语音识别" in await pg.inner_text("#peek") and await sttn() == n2 and not await pg.evaluate("document.body.classList.contains('vc-rec')"), "系统识别开不了：在小球旁说清楚去哪打开，不录音")
        await pg.evaluate("delete __mock.sttBeginFail; hidePeek()"); await pg.wait_for_timeout(500)
        # 选了服务商但没配好：按右 Alt 给出明确提示
        saved = await pg.evaluate("JSON.stringify(__mock.cfg.models)")
        await pg.evaluate("__mock.cfg.voice.engine = 'provider'; __mock.cfg.models = []; __emit('config')"); await pg.wait_for_timeout(300)
        await pg.evaluate("__emit('voice:key', true)"); await pg.wait_for_timeout(1200)
        await pg.evaluate("__emit('voice:key', false)"); await pg.wait_for_timeout(500)
        pk = await pg.inner_text("#peek")
        check(pk.startswith("语音识别选的是服务商") and not await pg.evaluate("document.body.classList.contains('vc-rec')"), f"选了服务商但没有模型：小球旁提示 {pk}")
        await pg.click("#peek"); await pg.wait_for_timeout(1200)
        tt = await pg.inner_text("#toast")
        check("还没有添加模型" in tt and "设置 → 模型" in tt and "改用系统识别" in tt and await pg.evaluate("document.querySelector('#toast').classList.contains('on')"), f"点开提示：完整说明去哪添加或改用系统识别 {tt}")
        await pg.evaluate("collapse()"); await pg.wait_for_timeout(800)
        await pg.evaluate(f"__mock.cfg.models = {saved}.map((m) => ({{ ...m, no_key: false, has_key: false }})); __emit('config')"); await pg.wait_for_timeout(300)
        await pg.evaluate("__emit('voice:key', true)"); await pg.wait_for_timeout(1200)
        await pg.evaluate("__emit('voice:key', false)"); await pg.wait_for_timeout(500)
        pk = await pg.inner_text("#peek")
        check("API Key" in pk and await sttn() == n2, f"选了服务商但没填 Key：明确提示 {pk}")
        await pg.evaluate(f"hidePeek(); __mock.cfg.models = {saved}.map((m) => ({{ ...m, has_key: true }})); __mock.cfg.voice.engine = 'provider'; __emit('config')"); await pg.wait_for_timeout(500)
        b0 = (await pg.evaluate("__mock")).get("sttBegin", 0)
        await pg.evaluate("__emit('voice:key', true)"); await pg.wait_for_timeout(1300)
        await pg.evaluate("__emit('voice:key', false)"); await pg.wait_for_timeout(1600)
        check(await sttn() == n2 + 1 and (await pg.evaluate("__mock")).get("sttBegin", 0) == b0, "服务商识别：录音交给服务商，不启动系统识别")
        await pg.evaluate("__mock.cfg.voice.engine = 'system'; __emit('config')"); await pg.wait_for_timeout(300)
        # 胶囊麦克风：听写
        await pg.evaluate("expand('chat')"); await pg.wait_for_timeout(900)
        await pg.fill("#ask", "请")
        await pg.click("#vcMic"); await pg.wait_for_timeout(1000)
        check(await pg.get_attribute("#vcMic", "aria-pressed") == "true" and "在听" in await pg.get_attribute("#ask", "placeholder") and await pg.evaluate("document.activeElement.id") == "ask", "点麦克风：开始听写，焦点留在输入框")
        await shot("17_voice_dict")
        await pg.click("#vcMic"); await pg.wait_for_timeout(800)
        check(await pg.input_value("#ask") == "请帮我把备忘录整理成周报" and await pg.get_attribute("#vcMic", "aria-pressed") == "false" and await pg.get_attribute("#ask", "placeholder") == "有什么可以帮你？", "再点一下：识别结果接在输入框后面，不发送")
        await pg.fill("#ask", "")
        await pg.evaluate("""(() => { window.__utt = []; const ss = window.speechSynthesis;
          ss.speak = (u) => { __utt.push(u); }; ss.cancel = () => { __utt.forEach((u) => { if (!u._done) { u._done = 1; u.onerror && u.onerror({}); } }); }; })()""")
        await pg.evaluate("__mock.cfg.voice.tts = true; __mock.sttText = '你好'; __emit('config')"); await pg.wait_for_timeout(300)
        await pg.click("#vcMic"); await pg.wait_for_timeout(900)
        await pg.press("#ask", "Enter"); await pg.wait_for_timeout(1800)
        check(await lastu() == "你好" and await pg.input_value("#ask") == "", "听写时按回车：识别完直接发送")
        await pg.wait_for_timeout(2500)
        lbl = lambda: pg.evaluate("[...document.querySelectorAll('.vc-tts')].pop()?.querySelector('span').textContent")
        tt = await lbl(); u = await pg.evaluate("__utt.length && [__utt[0].text, __utt[0].lang]")
        check(tt == "朗读中" and await pg.is_visible(".vc-tts:not(.vc-end) button") and u and u[1] == "zh-CN" and len(u[0]) > 0, f"朗读回复：回复下出现「朗读中」和停止按钮，按中文读 {tt} {u}")
        await shot("17_voice_tts")
        await pg.click(".vc-tts:not(.vc-end) button"); await pg.wait_for_timeout(200)
        check(await lbl() == "已停止" and not await pg.is_visible(".vc-tts button"), "点停止：标签变「已停止」，按钮收起")
        await pg.fill("#ask", "再说一句"); await pg.press("#ask", "Enter"); await pg.wait_for_timeout(2500)
        await pg.evaluate("(() => { const u = __utt.pop(); u._done = 1; u.onend({}); })()"); await pg.wait_for_timeout(150)
        check(await lbl() == "已读完", "读完：标签变「已读完」")
        await pg.evaluate("__mock.cfg.voice = { lang: 'auto', after: 'send', tts: false, model: 0, stt: 'whisper-1', engine: 'system' }; delete __mock.sttText; __emit('config')"); await pg.wait_for_timeout(300)

        # 划词：工具栏上点了动作，小球展开对话并发出去
        await pg.evaluate("__emit('sel:act', {act: 'explain', text: '新用户留存：41% → 47%'})"); await pg.wait_for_timeout(1400)
        lm = (await pg.evaluate("__mock")).get("lastMessages") or []
        q = await pg.evaluate("(() => { const m = [...document.querySelectorAll('.m.user')].pop(); return m && [m.querySelector('.x-sel-q')?.textContent, m.querySelector('.x-sel-qa')?.textContent]; })()")
        check(q == ["新用户留存：41% → 47%", "解释"] and lm and "解释下面这段文字" in lu(lm) and "41% → 47%" in lu(lm), f"划词「解释」：引用块 + 动作名，发给模型的是解释提示 {q}")
        await pg.wait_for_timeout(1500)
        await pg.evaluate("__emit('sel:act', {act: 'tr', text: '旧版小程序是否下线'})"); await pg.wait_for_timeout(1500)
        lm = (await pg.evaluate("__mock")).get("lastMessages") or []
        check("翻译成英文" in lu(lm), "划词「翻译」：自动模式下中文译成英文")
        await pg.wait_for_timeout(1500)
        await pg.evaluate("__emit('sel:act', {act: 'ask', text: '海外支付'})"); await pg.wait_for_timeout(900)
        check(await pg.input_value("#ask") == "「海外支付」 " and await pg.evaluate("document.activeElement.id") == "ask", "划词「问问它」：引用放进输入框，不自动发送")
        await pg.fill("#ask", "")
        await shot("15_sel_reply")

        check(not errs, "页面无 JS 错误 " + "; ".join(errs))

        # ───────── 右键菜单窗口（原型 #menu） ─────────
        # 底图：壁纸（铺满 1440×900）里菜单将要盖住的那块（小球旁 1180,420），和 capture.rs 一样缩小 8 倍、模糊、提饱和度
        from PIL import Image as _I, ImageFilter, ImageEnhance
        import io
        _w = _I.open(os.path.join(ROOT, "design-reference", "wallpaper.jpg")).convert("RGB"); _k = max(1440 / _w.width, 900 / _w.height)
        _w = _w.resize((round(_w.width * _k), round(_w.height * _k))); _ox, _oy = (_w.width - 1440) // 2, (_w.height - 900) // 2
        _c = ImageEnhance.Color(_w.crop((_ox + 1180, _oy + 420, _ox + 1396, _oy + 725)).resize((54, 76)).filter(ImageFilter.GaussianBlur(2.5))).enhance(1.65)
        _bb = io.BytesIO(); _c.save(_bb, "JPEG", quality=85)
        sbd_url_small = "data:image/jpeg;base64," + base64.b64encode(_bb.getvalue()).decode()
        ctxm = await b.new_context(viewport={"width": 216, "height": 305}, device_scale_factor=2)
        mp = await ctxm.new_page(); merrs = []
        mp.on("pageerror", lambda e: merrs.append(str(e)))
        await mp.add_init_script(path=os.path.join(ROOT, "tools", "mock-tauri.js"))
        await mp.goto("file://" + os.path.join(ROOT, "src", "ctxmenu.html")); await mp.wait_for_timeout(200)
        items = [{"id": "new_chat", "label": "新对话", "check": None, "sep": False}, {"id": "history", "label": "历史记录", "check": None, "sep": False},
                 {"id": "capture", "label": "截屏提问", "check": None, "sep": False}, {"id": "settings", "label": "设置", "check": None, "sep": False},
                 {"id": "toggle_ball", "label": "隐藏小球", "check": None, "sep": False}, {"id": "autostart", "label": "开机启动", "check": True, "sep": False},
                 {"id": "quit", "label": "退出", "check": None, "sep": True}]
        await mp.evaluate("([it, bd]) => __emit('ctx:show', { items: it, bd })", [items, sbd_url_small])
        await mp.wait_for_timeout(300)
        g = await mp.evaluate("""(() => { const m = document.querySelector('#menu'), r = m.getBoundingClientRect();
            return { labels: [...m.querySelectorAll('button')].map(b => b.firstChild.textContent), hr: m.querySelectorAll('hr').length,
                     ck: m.querySelector('[data-id=autostart]').getAttribute('aria-checked'), glassy: m.classList.contains('glassy'),
                     x: r.x, y: r.y, w: r.width, h: Math.round(r.height), bottom: innerHeight - r.bottom, focus: document.activeElement.id }; })()""")
        check(g["labels"] == ["新对话", "历史记录", "截屏提问", "设置", "隐藏小球", "开机启动", "退出"] and g["hr"] == 1, f"右键菜单：项目照原型顺序（暂只放已有功能）{g['labels']}")
        check(g["ck"] == "true", "开机启动打勾")
        check(g["glassy"] and g["x"] == 8 and g["y"] == 8 and g["w"] == 200 and abs(g["bottom"] - 8) <= 1, f"菜单 200 宽、四周留 8 给阴影、窗口大小正好（和 Rust ctx_size 一致）{g}")
        check(g["focus"] == "menu", "打开后焦点在菜单上，不预先高亮任何一项（同原型）")
        await mp.screenshot(path=f"{OUT}/14_ctx_menu.png", omit_background=True)
        await mp.keyboard.press("ArrowDown"); await mp.keyboard.press("ArrowDown"); await mp.keyboard.press("ArrowDown")
        check(await mp.evaluate("document.activeElement.dataset.id") == "capture", "方向键在菜单里移动")
        await mp.click("[data-id=settings]"); await mp.wait_for_timeout(100)
        calls = (await mp.evaluate("__mock")).get("ctxActs") or []
        check(calls == ["settings"], f"点一项：关掉菜单并执行 {calls}")
        await mp.evaluate("([it, bd]) => __emit('ctx:show', { items: it, bd: null })", [items, None]); await mp.wait_for_timeout(200)
        check(not await mp.evaluate("document.querySelector('#menu').classList.contains('glassy')"), "拿不到底图：用卡片同款不透明底")
        await mp.keyboard.press("Escape"); await mp.wait_for_timeout(100)
        check((await mp.evaluate("__mock")).get("ctxActs") == ["settings", ""], "Esc 关掉菜单，什么也不做")
        check(not merrs, "菜单窗口无 JS 错误 " + "; ".join(merrs))
        await ctxm.close()

        # ───────── 划词工具栏窗口（原型 selBar） ─────────
        ctx3 = await b.new_context(viewport={"width": 420, "height": 200}, device_scale_factor=2)
        bp = await ctx3.new_page(); berrs = []
        bp.on("pageerror", lambda e: berrs.append(str(e)))
        await bp.add_init_script(path=os.path.join(ROOT, "tools", "mock-tauri.js"))
        await bp.goto("file://" + os.path.join(ROOT, "src", "selbar.html")); await bp.wait_for_timeout(300)
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

        # ───────── 设置窗口（单独的窗口，照原型：七个分类、模型库 / 添加模型；能力标签查 models.dev） ─────────
        wall = WALL or os.path.join(ROOT, "design-reference", "wallpaper.jpg")
        wu = "data:image/jpeg;base64," + base64.b64encode(open(wall, "rb").read()).decode()
        from PIL import Image, ImageFilter, ImageEnhance
        import io
        _im = Image.open(wall).convert("RGB"); _k = max(1440 / _im.width, 900 / _im.height)
        _im = _im.resize((round(_im.width * _k), round(_im.height * _k))); _x, _y = (_im.width - 1440) // 2, (_im.height - 900) // 2
        _im = ImageEnhance.Color(_im.crop((_x, _y, _x + 1440, _y + 900)).resize((360, 225)).filter(ImageFilter.GaussianBlur(2.5))).enhance(1.65)
        _b = io.BytesIO(); _im.save(_b, "JPEG", quality=85)
        sbd_url = "data:image/jpeg;base64," + base64.b64encode(_b.getvalue()).decode()
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
            await sp.add_init_script(f"window.__mock.cfg.platform = '{plat}'; window.__mock.cfg.fx = {'true' if plat == 'macos' else 'false'}")
            if plat == "windows":
                # Windows 不用亚克力：程序在窗口出现前截下整块屏幕（缩小 8 倍、模糊），页面按窗口位置垫在墨层下面。
                # 这里拿同一张壁纸做一张同样的小图，屏幕 1440×900（2 倍像素），窗口在 480,150
                await sp.add_init_script("window.__mock.settingsBd = { url: " + json.dumps(sbd_url) + ", x: 0, y: 0, w: 2880, h: 1800 }; window.__mock.pos = { x: 480, y: 150 };")
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
            check((("fx" in bc.split()) == (plat == "macos")) and (("win" in bc) == (plat == "windows")), P + f"平台样式 ({bc})")
            if plat == "windows":
                g = await sp.evaluate("[document.body.className, getComputedStyle(document.querySelector('.x-win.x-setw')).backgroundImage, getComputedStyle(document.querySelector('.x-win.x-setw')).backgroundPosition]")
                check("bd" in g[0].split() and "url(" in g[1] and "-480px -150px" in g[2], P + f"设置窗口用截图玻璃，按窗口位置对齐 {g[2]}")
                # 拖到别的地方：底图跟着挪；挪出这块屏幕就用不透明的墨色底
                await sp.evaluate("__mock.moved.forEach(f => f({ payload: { x: 400, y: 200 } }))"); await sp.wait_for_timeout(150)
                g = await sp.evaluate("getComputedStyle(document.querySelector('.x-win.x-setw')).backgroundPosition")
                check("-200px -100px" in g, P + f"拖动设置窗口时玻璃底图跟着挪 {g}")
                await sp.evaluate("__mock.moved.forEach(f => f({ payload: { x: 4000, y: 200 } }))"); await sp.wait_for_timeout(150)
                g = await sp.evaluate("[document.body.className, getComputedStyle(document.querySelector('.x-win.x-setw')).backgroundImage]")
                check("bd" not in g[0].split() and "url(" not in g[1] and "linear-gradient" in g[1], P + f"挪到没截过的屏幕上用不透明底 {g[0]}")
                await sp.evaluate("__mock.moved.forEach(f => f({ payload: { x: 960, y: 300 } }))"); await sp.wait_for_timeout(150)
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
            check(await sp.get_attribute("#libAuto", "aria-checked") == "false" and "自带" in await txt("#libSrc") and "IP" in await txt("#libAutoL"), P + "模型资料：默认不联网更新，写明会暴露 IP")
            await sp.click("#libAuto"); await sp.wait_for_timeout(200)
            check((await mock())["cfg"]["catalog_auto"] is True and "每天自动更新" in await txt("#libSrc"), P + "打开自动更新并保存")
            await sp.click("#libAuto"); await sp.wait_for_timeout(200)
            check((await mock())["cfg"]["catalog_auto"] is False, P + "关掉自动更新")
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
            check(len(cards) == 1 and "主控" in cards[0] and "拆解与分派" in cards[0] and "（默认）" in cards[0] and "工具 2" in cards[0], P + f"Agents 列表有主控 {cards}")
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
            await sp.evaluate("document.getElementById('toast').classList.remove('on')"); await sp.wait_for_timeout(300)
            await sp_shot(f"33_perm_{plat}")
            await sp.click("#alList li:first-child button"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["always_allow"] == [{"kind": "写入", "scope": "周报"}] and "已撤销：删除 · Downloads" in await txt("#toast"), P + "撤销一条")
            await sp.click("#alList li button"); await sp.wait_for_timeout(150)
            check(await vis("#alEmpty") and (await mock())["cfg"]["always_allow"] == [], P + "全撤销后显示空状态")
            await sp.click("#awaySw"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["pause_away"] is False and await sp.get_attribute("#awaySw", "aria-checked") == "false", P + "关掉离开时暂停")
            # S8 外观
            await sp.click(".x-tabs button:has-text('外观')")
            check(await vis("#lkGlassF") == (plat == "windows"), P + "玻璃通透度只在 Windows 显示")
            await sp.click("#lkSize button:has-text('大')"); await sp.wait_for_timeout(150)
            await sp.click("#lkEye [aria-label='蜜桃']"); await sp.wait_for_timeout(150)
            if plat == "windows":
                await sp.evaluate("""() => { const r = document.querySelector('#lkGlass'); r.value = 40; r.dispatchEvent(new Event('input')); r.dispatchEvent(new Event('change')); }""")
                await sp.wait_for_timeout(150)
            c = (await mock())["cfg"]
            check(c["orb_size"] == 84 and c["eye"] == "#FFC9A6" and c["glass"] == (40 if plat == "windows" else 25), P + f"外观改了就保存 {c['orb_size']} {c['eye']} {c['glass']}")
            check(await sp.get_attribute("#lkSize button:has-text('大')", "aria-checked") == "true" and await sp.get_attribute("#lkEye [aria-label='蜜桃']", "aria-checked") == "true", P + "外观显示当前选中")
            check(await sp.eval_on_selector("#lkIdle", "e => e.value") == "600000" and await sp.get_attribute("#lkDock", "aria-checked") == "true", P + "睡觉时间、贴边开关显示当前值")
            await sp.select_option("#lkIdle", "0"); await sp.wait_for_timeout(150)
            for i in ("#lkSys", "#lkDock", "#lkCalm"): await sp.click(i); await sp.wait_for_timeout(120)
            c = (await mock())["cfg"]
            check(c["idle_ms"] == 0 and c["follow_system"] is True and c["auto_dock"] is False and c["calm"] is True, P + f"睡觉时间和三个开关改了就保存 {c['idle_ms']} {c['follow_system']} {c['auto_dock']} {c['calm']}")
            for i in ("#lkSys", "#lkDock", "#lkCalm"): await sp.click(i); await sp.wait_for_timeout(120)
            await sp.select_option("#lkIdle", "600000"); await sp.wait_for_timeout(150)
            # 悬停快捷入口：开关、速度、勾选、排序
            await sp.click("#hvSpd button:has-text('快')"); await sp.wait_for_timeout(150)
            await sp.click("#hvList [data-id=tasks] [data-d=up]"); await sp.wait_for_timeout(150)
            check(await sp.eval_on_selector("#hvList [data-id=tasks] [data-d=up]", "e => !!(document.activeElement.closest && document.activeElement.closest('li') && document.activeElement.closest('li').dataset.id === 'tasks')"), P + "排序后焦点跟着这一项")
            await sp.click("#hvList [data-id=shot] input"); await sp.wait_for_timeout(150)
            h = (await mock())["cfg"]["hover"]
            check(h["delay"] == 200 and [o["id"] for o in h["order"]] == ["shot", "new", "tasks", "hist"] and not h["order"][0]["show"], P + f"悬停入口：速度、排序、勾选都保存 {h}")
            await sp.click("#hvOn"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["hover"]["on"] is False and await sp.eval_on_selector("#hvSet", "e => e.classList.contains('x-hvdis')"), P + "关掉悬停入口")
            # 多显示器
            await sp.click("#mdsWhere button:has-text('固定在主屏幕')"); await sp.click("#mdsSlide"); await sp.wait_for_timeout(200)
            c = (await mock())["cfg"]
            check(c["screen"] == "main" and c["screen_slide"] is False and "主屏" in await sp.text_content("#mdsCap"), P + f"多显示器：位置和滑动都保存 {c['screen']} {c['screen_slide']}")
            await sp.click("#mdsWhere button:has-text('鼠标所在的屏幕')"); await sp.click("#mdsSlide"); await sp.wait_for_timeout(200)
            await sp.click("#hvOn"); await sp.click("#hvList [data-id=shot] input"); await sp.click("#hvList [data-id=tasks] [data-d=dn]"); await sp.click("#hvSpd button:has-text('中')"); await sp.wait_for_timeout(200)
            await sp.evaluate("document.getElementById('toast').classList.remove('on')"); await sp.wait_for_timeout(300)
            await sp_shot(f"24_settings_look_{plat}")
            # S10 划词（原型 paneSel）
            await sp.click(".x-tabs button:has-text('划词')"); await sp.wait_for_timeout(200)
            await sp_shot(f"27a_settings_sel_{plat}")
            check(await sp.get_attribute("#selOn", "aria-checked") == "false" and await txt("#selModeAlt") == ("按住 ⌥ 再选中" if plat == "macos" else "按住 Alt 再选中"), P + "划词：默认关闭，Alt 写法随系统")
            check(await vis("#selMacNote") == (plat == "macos") and await vis("#selWinNote") == (plat == "windows") and not await vis("#selNo"), P + "说明随系统")
            clip = await txt("#selClip")
            check(await vis("#selClip") == (plat == "windows") and all(w in clip for w in ("不碰剪贴板", "复制", "文件", "格式")), P + "Windows：开关下写明一般不碰剪贴板、退到复制时哪些放不回")
            if plat == "windows":
                cb = await sp.eval_on_selector("#selOn", "e => e.closest('.x-ar').getBoundingClientRect().bottom"); nt = await sp.eval_on_selector("#selClip", "e => e.getBoundingClientRect().top")
                check(0 <= nt - cb < 20, P + f"剪贴板说明紧挨开关 {cb} {nt}")
            await sp.click("#selOn"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["sel"]["on"] is True and await sp.get_attribute("#selOn", "aria-checked") == "true", P + "手动打开划词并保存")
            ords = await sp.eval_on_selector_all("#selOrd li", "x => x.map(l => l.querySelector('b').firstChild.textContent + (l.querySelector('input').checked ? '✓' : ''))")
            check(ords == ["问问它✓", "解释✓", "翻译✓", "总结✓", "改写✓", "复制✓", "朗读", "搜索", "扩写", "缩写"], P + f"工具栏按钮列表同原型 {ords}")
            await sp.click("#selOrd [data-id=read] input"); await sp.wait_for_timeout(150)
            check("放在「⋯」" in await txt("#selOrd [data-id=read] b") and (await mock())["cfg"]["sel"]["order"][6] == {"id": "read", "show": True}, P + "勾第 7 个：标「放在⋯」并保存")
            await sp.click("#selOrd [data-id=read] [data-d=up]"); await sp.wait_for_timeout(150)
            o = [x["id"] for x in (await mock())["cfg"]["sel"]["order"]]
            check(o[5] == "read" and o[6] == "copy" and "放在「⋯」" in await txt("#selOrd [data-id=copy] b") and await sp.evaluate("document.activeElement.dataset.d") == "up", P + "上移：保存顺序，焦点留在上移按钮")
            await sp.evaluate("document.getElementById('selOrd').scrollIntoView({ block: 'start' })"); await sp.wait_for_timeout(100)
            h = await sp.query_selector("#selOrd [data-id=shrink] .x-selh"); bb = await h.bounding_box()
            t = await (await sp.query_selector("#selOrd [data-id=ask]")).bounding_box()
            await sp.mouse.move(bb["x"] + 10, bb["y"] + 10); await sp.mouse.down(); await sp.mouse.move(bb["x"] + 10, t["y"] + 3, steps=6); await sp.mouse.up(); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["sel"]["order"][0]["id"] == "shrink", P + "拖动把手排序")
            await sp.click("#selMode [data-v=alt]"); await sp.select_option("#selTr", "ja"); await sp.wait_for_timeout(150)
            c = (await mock())["cfg"]["sel"]
            check(c["mode"] == "alt" and c["tr"] == "ja", P + "出现方式、翻译成：保存")
            await sp.click("#selApps [aria-label='移除 KeePass']"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["sel"]["apps"] == ["1Password", "Bitwarden"] and await sp.evaluate("document.activeElement.id") == "selAdd", P + "移除排除的应用")
            await sp.click("#selAdd"); await sp.wait_for_timeout(100)
            check(await sp.evaluate("[...document.querySelectorAll('#selRecent option')].map(o => o.value).join()") == "微信,Microsoft Edge", P + "添加应用：候选是最近划过词的应用")
            await sp.keyboard.type("微信"); await sp.keyboard.press("Enter"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["sel"]["apps"] == ["1Password", "Bitwarden", "微信"], P + "输入应用名回车添加")
            await sp.click("#selAdd"); await sp.keyboard.type("微信"); await sp.keyboard.press("Enter"); await sp.wait_for_timeout(150)
            check(len((await mock())["cfg"]["sel"]["apps"]) == 3 and "已在列表里" in await txt("#toast"), P + "重复的不加")
            await sp.click("#selAdd"); await sp.keyboard.type("abc"); await sp.keyboard.press("Escape"); await sp.wait_for_timeout(150)
            check(len((await mock())["cfg"]["sel"]["apps"]) == 3 and (await mock()).get("closed") == 1 and await vis("#selAdd"), P + "Esc 取消添加，不关窗口")
            await sp_shot(f"27_settings_sel_{plat}")
            await sp.click("#selOn"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["sel"]["on"] is False, P + "关掉划词")

            # S11 语音（原型 paneVoice）
            await sp.click(".x-tabs button:has-text('语音')"); await sp.wait_for_timeout(200)
            await sp_shot(f"28_settings_voice_{plat}")
            check(await sp.input_value("#vcLang") == "auto" and await sp.get_attribute("#vcAfter [data-v=send]", "aria-checked") == "true" and await sp.get_attribute("#vcTts", "aria-checked") == "false", P + "语音：默认值同原型（自动、直接发送、不朗读）")
            note = await txt("#vcSysNote")
            check(await sp.get_attribute("#vcEng [data-v=system]", "aria-checked") == "true" and not await vis(".x-vcsvc") and not await vis("#vcSvcNote")
                  and (("微软云端" in note and "联机语音识别" in note) if plat == "windows" else ("本机" in note and "授权" in note)), P + f"识别方式默认系统自带，如实写明去向 {note}")
            await sp.click("#vcEng [data-v=provider]"); await sp.wait_for_timeout(200)
            check((await mock())["cfg"]["voice"]["engine"] == "provider" and await vis(".x-vcsvc") and await vis("#vcSvcNote"), P + "改用服务商：保存，出现服务商和识别模型")
            check(await sp.is_disabled("#vcWake") and "还没做" in await txt("#vcWakeL"), P + "唤醒词：如实标出还没做，开关不能点")
            opts = await sp.eval_on_selector_all("#vcModel option", "x => x.map(o => o.textContent)")
            check(len(opts) >= 2 and opts[0].startswith("默认模型") and await sp.input_value("#vcStt") == "whisper-1", P + f"识别服务商列出模型库，识别模型默认 whisper-1 {opts}")
            await sp.select_option("#vcLang", "yue"); await sp.click("#vcAfter [data-v=box]"); await sp.click("#vcTts")
            await sp.select_option("#vcModel", index=1)
            await sp.fill("#vcStt", "whisper-large-v3"); await sp.press("#vcStt", "Enter"); await sp.wait_for_timeout(200)
            v = (await mock())["cfg"]["voice"]
            check(v["lang"] == "yue" and v["after"] == "box" and v["tts"] is True and v["model"] > 0 and v["stt"] == "whisper-large-v3", P + f"改了就保存 {v}")
            await sp.fill("#vcStt", ""); await sp.press("#vcStt", "Enter"); await sp.wait_for_timeout(150)
            check(await sp.input_value("#vcStt") == "whisper-large-v3" and (await mock())["cfg"]["voice"]["stt"] == "whisper-large-v3", P + "识别模型清空不保存")
            check(await txt("#vcAltK") == ("⌥" if plat == "macos" else "Alt"), P + "说明里的 Alt 写法随系统")
            await sp.evaluate("__mock.cfg.voice = { lang: 'auto', after: 'send', tts: false, model: 0, stt: 'whisper-1', engine: 'system' }; __emit('config')"); await sp.wait_for_timeout(200)

            # S9 快捷键：点「录制」再按组合键
            await sp.click(".x-tabs button:has-text('快捷键')")
            kc = lambda k: txt(f"#keyList [data-k={k}] .x-kc")
            check(await kc("shortcut") == ("⌥ Space" if False else "Alt+Space"), P + f"显示当前快捷键 {await kc('shortcut')}")
            rows = await sp.eval_on_selector_all("#keyList li", "x => x.map(l => l.dataset.k + ':' + l.querySelector('b').textContent + ':' + l.querySelector('.x-kc').textContent)")
            check(rows == ["shortcut:唤起/收起:Alt+Space", "new_shortcut:新对话:Alt+N", "capture_shortcut:截屏提问:Alt+Shift+S", "tasks_shortcut:打开任务:Alt+T", "talk_shortcut:按住说话:右 Alt（按住）"], P + f"五个快捷键，按住说话默认右 Alt（同原型）{rows}")
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
            await sp.click("#keyList [data-k=tasks_shortcut] .x-sb"); await sp.keyboard.press("Alt+KeyN"); await sp.wait_for_timeout(150)
            check("新对话" in await txt("#keyList [data-k=tasks_shortcut] .x-kw") and (await mock())["cfg"]["tasks_shortcut"] == "Alt+T", P + "和别的快捷键重复：说出是哪个")
            await sp.keyboard.press("Alt+KeyY"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["tasks_shortcut"] == "Alt+Y" and await kc("tasks_shortcut") == "Alt+Y", P + "录入「打开任务」")
            await sp.click("#keyList [data-k=talk_shortcut] .x-sb"); await sp.keyboard.press("Alt+KeyV"); await sp.wait_for_timeout(150)
            check((await mock())["cfg"]["talk_shortcut"] == "Alt+V" and await kc("talk_shortcut") == "Alt+V", P + "「按住说话」也能改成组合键")
            await sp.click("#keyReset"); await sp.wait_for_timeout(200)
            c = (await mock())["cfg"]
            check(c["talk_shortcut"] == "RightAlt", P + "恢复默认：按住说话回到右 Alt")
            check([c["shortcut"], c["new_shortcut"], c["capture_shortcut"], c["tasks_shortcut"]] == ["Alt+Space", "Alt+N", "Alt+S", "Alt+T"] and "恢复默认" in await txt("#toast"), P + "恢复默认（同原型）")
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
