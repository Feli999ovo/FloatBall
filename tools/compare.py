"""原型 ↔ 程序 并排对比图。

用法：python3 tools/compare.py 原型index.html 输出目录 [字体]

- 原型：用 Playwright 打开，在 1440×900 下点「状态预览」里的按钮切到对应状态，截出小球/胶囊/卡片那一块。
- 程序：test_ui.py 的截图是透明底（omit_background），按 2 倍像素截的；这里缩回 1 倍，
  贴到同一张壁纸（design-reference/wallpaper.jpg，和原型同一张，按 cover 铺满 1440×900）的同一位置上，
  对齐方式：程序截图右下角的胶囊/小球中心对准原型里小球的中心。
- 左原型、右程序，上面标题，存成 cmp_<名字>.png。
"""
import asyncio, base64, os, sys
from PIL import Image, ImageDraw, ImageFont
from playwright.async_api import async_playwright

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VW, VH = 1440, 900

# 名字, 原型里要点的按钮（依次）, 程序这边的动作
PAIRS = [
    ("小球待机", ["待机"], []),
    ("对话卡片", ["对话卡片"], ["#orb", "ask"]),
    ("更多菜单", ["对话卡片", "#chatMore"], ["#orb", "ask", "#moreBtn"]),
    # 小球表情：原型点「状态预览」里的按钮；程序直接切 #orb 的状态类（真实流程见 test_ui：确认卡 → 等你 + 「!」）
    ("小球等你", ["[data-s=wait]"], ["js:document.querySelector('#orb').classList.remove('s-idle');document.querySelector('#orb').classList.add('s-wait','need')"]),
    ("小球需要确认", ["[data-s=confirm]"], ["js:document.querySelector('#orb').classList.remove('s-idle');document.querySelector('#orb').classList.add('s-confirm')"]),
    # 多 Agent（B1）：原型点「多 Agent」演示，等它跑到第 2 步；程序用 mock 的 team 事件，停在第 2 步进行中
    ("多Agent计划卡", ["#teamBtn", "wait:3600"], ["#orb", "js:" + "__TEAM__", "team"]),
    ("Agents弹卡", ["#teamBtn", "wait:3600", "#tabTask", ".x-ag:nth-child(2)"], ["#orb", "js:" + "__TEAM__", "team", "#tabTask", "#tasks .x-ag[data-ag=organize]"]),
]
TEAM = """const A = (id, name, role, tint, tools) => ({ id, name, role, tint, emoji: "", tools, prompt: "", model: 0, fallback: 0, auto: false });
  __mock.cfg.agents = [__mock.cfg.agents[0], A("research", "研究员", "搜索与阅读", "#A9BEDB", ["读文件"]), A("organize", "整理员", "文件与表格", "#AECDB6", ["读文件", "写文件"]), A("writer", "写手", "起草与润色", "#DCC0A8", ["读文件", "写文件"])];
  __mock.holdTeam = true; __emit("config");"""
PAIRS = [(n, c, [a.replace("__TEAM__", TEAM) for a in acts]) for n, c, acts in PAIRS]
Q = "帮我总结一下今天的待办"


def cover(im):
    k = max(VW / im.width, VH / im.height)
    im = im.resize((round(im.width * k), round(im.height * k)), Image.LANCZOS)
    x, y = (im.width - VW) // 2, (im.height - VH) // 2
    return im.crop((x, y, x + VW, y + VH))


async def proto_shots(b, url, out):
    res = {}
    for name, clicks, _ in PAIRS:
        pg = await b.new_page(viewport={"width": VW, "height": VH})
        await pg.goto(url); await pg.wait_for_timeout(1500)
        for c in clicks:
            if c.startswith("wait:"): await pg.wait_for_timeout(int(c[5:])); continue
            if c[0] in "#[.": await pg.click(c)
            else: await pg.get_by_text(c, exact=True).click()
            await pg.wait_for_timeout(1200)
        path = os.path.join(out, f"proto_{name}.png")
        await pg.screenshot(path=path)
        res[name] = path
        await pg.close()
    return res


# 程序：真的跑 src/index.html（mock-tauri 假装是 Tauri），屏幕 1440×900、2 倍像素，小球放在和原型同一个位置；
# 展开时要的「身后桌面」底图，从同一张壁纸的对应位置裁出来、缩小 8 倍（和 capture.rs 一样）
MOCK = """async (b64) => {
  const im = new Image(); im.src = "data:image/jpeg;base64," + b64; await im.decode();
  const W = 1440, H = 900, k = Math.max(W / im.width, H / im.height);
  const full = document.createElement("canvas"); full.width = W * 2; full.height = H * 2;
  full.getContext("2d").drawImage(im, (W - im.width * k), (H - im.height * k), im.width * k * 2, im.height * k * 2);
  __mock.backdrop = (a) => { const c = document.createElement("canvas"); c.width = Math.max(1, a.w >> 3); c.height = Math.max(1, a.h >> 3);
    c.getContext("2d").drawImage(full, a.x, a.y, a.w, a.h, 0, 0, c.width, c.height); return c.toDataURL("image/jpeg"); };
  __mock.cfg.models = [{ id: 1, name: "", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini", vision: true, no_key: false, has_key: true }];
  __mock.cfg.default_model = 1; __emit("config");
}"""


async def prog_shots(b, wall_b64, out):
    res = {}
    for name, _, acts in PAIRS:
        ctx = await b.new_context(viewport={"width": 96, "height": 96}, device_scale_factor=2)
        pg = await ctx.new_page()
        await pg.expose_function("__resize", lambda w, h, pg=pg: asyncio.ensure_future(pg.set_viewport_size({"width": int(w), "height": int(h)})))
        await pg.add_init_script(path=os.path.join(ROOT, "tools", "mock-tauri.js"))
        await pg.goto("file://" + os.path.join(ROOT, "src", "index.html")); await pg.wait_for_timeout(500)
        await pg.evaluate(MOCK, wall_b64); await pg.wait_for_timeout(300)
        for a in acts:
            if a.startswith("js:"):
                await pg.evaluate(a[3:]); await pg.wait_for_timeout(1200)
            elif a == "team":
                await pg.fill("#ask", "帮我调研三款降噪耳机并写个对比报告"); await pg.press("#ask", "Enter"); await pg.wait_for_timeout(2500)
            elif a == "ask":
                await pg.fill("#ask", Q); await pg.press("#ask", "Enter"); await pg.wait_for_timeout(3500)
            else:
                await pg.click(a); await pg.wait_for_timeout(1000)
        path = os.path.join(out, f"prog_{name}.png")
        await pg.screenshot(path=path, omit_background=True)
        m = await pg.evaluate("__mock")
        res[name] = (path, m["pos"])
        await ctx.close()
    return res


def main():
    proto, out = sys.argv[1], sys.argv[2]
    font = sys.argv[3] if len(sys.argv) > 3 else os.path.expanduser("~/.fonts/NotoSansSC.otf")
    os.makedirs(out, exist_ok=True)
    wp = os.path.join(ROOT, "design-reference", "wallpaper.jpg")
    wall = cover(Image.open(wp).convert("RGB"))

    async def run():
        async with async_playwright() as p:
            b = await p.chromium.launch(args=["--allow-file-access-from-files"])
            r1 = await proto_shots(b, "file://" + os.path.abspath(proto), out)
            r2 = await prog_shots(b, base64.b64encode(open(wp, "rb").read()).decode(), out)
            await b.close()
            return r1, r2
    r1, r2 = asyncio.run(run())
    f = ImageFont.truetype(font, 22)
    for name, _, _ in PAIRS:
        pim = Image.open(r1[name]).convert("RGB")
        path, pos = r2[name]
        g = Image.open(path).convert("RGBA"); g = g.resize((g.width // 2, g.height // 2), Image.LANCZOS)
        x0, y0 = round(pos["x"]), round(pos["y"])
        comp = wall.copy().convert("RGBA"); comp.alpha_composite(g, (x0, y0))
        # 两边裁同一块：程序窗口的范围 ∪ 原型右侧小球/卡片那一片，留边
        L = max(0, min(x0, 1060) - 24); T = max(0, min(y0, 90) - 24)
        R = min(VW, max(x0 + g.width, 1420) + 12); B = min(VH, max(y0 + g.height, 480) + 24)
        a, c = pim.crop((L, T, R, B)), comp.convert("RGB").crop((L, T, R, B))
        W, H = a.width, a.height
        sheet = Image.new("RGB", (W * 2 + 30, H + 54), (24, 22, 34))
        sheet.paste(a, (10, 44)); sheet.paste(c, (W + 20, 44))
        d = ImageDraw.Draw(sheet)
        d.text((14, 10), f"原型 · {name}", font=f, fill=(255, 241, 220))
        d.text((W + 24, 10), f"程序 · {name}", font=f, fill=(255, 241, 220))
        sheet.save(os.path.join(out, f"cmp_{name}.png"))
        print("ok", name, sheet.size, pos)


if __name__ == "__main__":
    main()
