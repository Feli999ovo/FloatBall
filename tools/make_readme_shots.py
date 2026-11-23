"""用 Chromium 渲染当前界面，输出 README 横幅和三联图。"""
import argparse
import asyncio
import base64
from io import BytesIO
from pathlib import Path
import subprocess

from PIL import Image, ImageDraw, ImageFont, ImageOps
from playwright.async_api import async_playwright

from local_http import serve

ROOT = Path(__file__).resolve().parents[1]
BANNER_SOURCE = "44a95fe:docs/banner.png"
WALL = ROOT / "design-reference/wallpaper.jpg"

SCENE = """async ({wallpaper, extra, banner}) => {
  const im = new Image();
  im.src = wallpaper;
  await im.decode();
  __mock.cfg.models = [{id: 1, name: '', model: 'gpt-4o-mini', vision: true, has_key: true}];
  __mock.cfg.default_model = 1;
  __mock.pos.y = 310;
  __mock.cfg.follow_system = true;
  __mock.backdrop = ({x, y, w, h}) => {
    if (banner) return null;
    h += extra * 2;
    const c = document.createElement('canvas');
    c.width = Math.max(1, w >> 3);
    c.height = Math.max(1, h >> 3);
    c.getContext('2d').drawImage(im, x, y, w, h, 0, 0, c.width, c.height);
    return c.toDataURL('image/jpeg');
  };
  __emit('config');
}"""

SELECTION_REPLY = """() => {
  const invoke = __TAURI__.core.invoke;
  __TAURI__.core.invoke = async (name, args) => {
    if (name !== 'chat' || !__mock.selectionDemo) return invoke(name, args);
    setTimeout(() => {
      const out = event => args.onEvent.onmessage(event);
      out({type: 'delta', text: '这句话是说：发送请求后，界面可以一边接收回复，一边把收到的文字显示出来。'});
      out({type: 'usage', prompt: 86, completion: 39});
      out({type: 'done'});
    }, 30);
  };
}"""


async def ask(page, prompt):
    await page.locator('#ask').fill(prompt)
    await page.locator('#ask').press('Enter')


async def finished(page):
    await page.locator('.m.ai .meta:not([hidden])').last.wait_for()
    await page.wait_for_function('!S.busy')


async def capture(page):
    await page.mouse.move(0, 0)
    await page.wait_for_timeout(450)  # 等窗口变形、卡片淡入和滚动结束
    image = Image.open(BytesIO(await page.screenshot(omit_background=True))).convert('RGBA')
    box = image.getbbox()
    assert box is not None, '窗口截图没有可见内容'
    x, y, _, _ = box
    pos = await page.evaluate('__mock.pos')
    return image.crop(box), (round(pos['x'] * 2) + x, round(pos['y'] * 2) + y)


async def take_shots(browser, url, wallpaper):
    photos = []
    mock = (ROOT / 'tools/mock-tauri.js').read_text(encoding='utf-8')
    for scene in ('banner', 'ask', 'agent', 'tasks'):
        async with await browser.new_context(viewport={'width': 96, 'height': 96}, device_scale_factor=2,
                                             color_scheme='dark') as context:
            page = await context.new_page()
            # 展示图里的卡片略加高，让长回复和任务三组都能完整呈现。
            extra = {'banner': 175, 'ask': 0, 'agent': 250, 'tasks': 190}[scene]
            await page.expose_function('__resize', lambda w, h: asyncio.ensure_future(
                page.set_viewport_size({'width': int(w), 'height': int(h) + (extra if h > 300 else 0)})))
            await page.add_init_script(mock + '\n;(' + SELECTION_REPLY + ')();')
            await page.goto(url + '/index.html')
            await page.wait_for_function("typeof newChat === 'function' && !!S.cfg")
            errors = []
            page.on('pageerror', lambda error: errors.append(str(error)))
            await page.evaluate(SCENE, {'wallpaper': wallpaper, 'extra': extra, 'banner': scene == 'banner'})
            await page.locator('#orb').click()
            await page.locator('#card').wait_for()
            if extra and scene != 'banner':
                await page.locator('#card').evaluate('''(card, extra) => {
                  const [width, height] = card.style.getPropertyValue('--bd-size').split(' ');
                  card.style.setProperty('--bd-size', `${width} ${parseFloat(height) + extra}px`, 'important');
                }''', extra)
            if scene == 'banner':
                await ask(page, '怎么开始开发悬浮球？')
                await finished(page)
                assert '184 tokens' in await page.locator('.m.ai .meta').inner_text()
            elif scene == 'ask':
                await page.evaluate("""() => {
                  __mock.selectionDemo = true;
                  __emit('sel:feed', {Ok: '流式输出'});
                }""")
                await page.wait_for_function("document.querySelector('#ask').value.startsWith('流式输出')")
                selected = await page.locator('#ask').input_value()
                await ask(page, selected + '什么意思？')
                await finished(page)
                assert '一边接收回复' in await page.locator('.m.ai .txt').inner_text()
            elif scene == 'agent':
                await page.evaluate("__mock.receiptProbe = 'done'")
                await ask(page, '核对两份周报并写成汇总')
                await page.get_by_role('button', name='允许一次', exact=True).click()
                await finished(page)
                assert '3 步 · 读 2 · 写 1 · 确认 1' in await page.locator('.receipt-summary').inner_text()
                await page.locator('#msgs').evaluate('(el) => { el.scrollTop = 0; }')
                await page.locator('.receipt-summary').scroll_into_view_if_needed()
            else:
                await ask(page, '整理今天的待办')
                await finished(page)
                await page.evaluate('newChat(); __mock.chatProbe = []')
                await ask(page, '比较两份需求文档')
                await page.evaluate("""() => {
                  __mock.cfg.routines = [{id:'weekly', name:'汇总本周工作', prompt:'汇总本周工作',
                    every:'week', weekday:5, time:'17:30', on:true, history:[]}];
                  const next = new Date();
                  next.setDate(next.getDate() + (5 - next.getDay() + 7) % 7);
                  next.setHours(17, 30, 0, 0);
                  if (next <= new Date()) next.setDate(next.getDate() + 7);
                  __mock.rtNextAt = Math.floor(next.getTime() / 1000);
                  __emit('config');
                }""")
                await page.locator('#tabTask').click()
                assert await page.locator('#tasks .hg').all_text_contents() == ['进行中', '定时任务', '已完成']
            photos.append(await capture(page))
            assert not errors, errors
    return photos


def banner_with_window(window):
    # 固定底稿来自原横幅，重跑时不会把上一版界面留在渐变里。
    source = subprocess.check_output(['git', 'show', BANNER_SOURCE], cwd=ROOT)
    banner = Image.open(BytesIO(source)).convert('RGB')
    assert banner.size == (2200, 1100)
    left = banner.crop((0, 0, 1150, 1100))
    # 旧窗口和阴影两侧的背景色逐行插值，保留外侧渐变及左半边。
    x0, x1, y0, y1 = 1300, 2030, 90, 1030
    draw = ImageDraw.Draw(banner)
    for y in range(y0, y1):
        a, b = banner.getpixel((x0, y)), banner.getpixel((x1, y))
        assert isinstance(a, tuple) and isinstance(b, tuple)
        for x in range(x0 + 1, x1):
            mix = (x - x0) / (x1 - x0)
            draw.point((x, y), tuple(round(c + (d - c) * mix) for c, d in zip(a, b)))
    window.thumbnail((730, 860), Image.Resampling.LANCZOS)
    banner.paste(window, (1665 - window.width // 2, 550 - window.height // 2), window)
    assert banner.crop((0, 0, 1150, 1100)).tobytes() == left.tobytes()
    return banner


def screen_sheet(photos, desk, font):
    captions = [
        ('随手问', '选中一段文字，直接问它是什么意思。'),
        ('交给 Agent', '确认写入，完成后查看工作收据。'),
        ('自动做', '进行中、定时和完成的任务都在这里。'),
    ]
    sheet = Image.new('RGB', (2480, 1250), '#17142a')
    title, subtitle = ImageFont.truetype(font, 42), ImageFont.truetype(font, 30)
    draw = ImageDraw.Draw(sheet)
    for i, ((window, (px, py)), (name, detail)) in enumerate(zip(photos, captions, strict=True)):
        x = 34 + i * 814
        width, height = 784, 1050
        window.thumbnail((width - 24, height - 44), Image.Resampling.LANCZOS)
        left = min(max(0, px + (window.width - width) // 2), desk.width - width)
        top = min(max(0, py + (window.height - height) // 2), desk.height - height)
        panel = desk.crop((left, top, left + width, top + height))
        panel.paste(window, (px - left, py - top), window)
        sheet.paste(panel, (x, 34))
        draw.text((x + 18, 1109), name, font=title, fill='#fff1dc')
        draw.text((x + 18, 1172), detail, font=subtitle, fill='#c7bfd9')
    return sheet


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=ROOT / 'docs')
    parser.add_argument('--font', default='msyh.ttc' if Path('C:/Windows/Fonts/msyh.ttc').exists() else 'NotoSansCJK-Regular.ttc',
                        help='支持简体中文的字体文件')
    args = parser.parse_args()
    desk = ImageOps.fit(Image.open(WALL).convert('RGB'), (2880, 1800), Image.Resampling.LANCZOS)
    buffer = BytesIO()
    desk.save(buffer, format='JPEG', quality=92)
    wallpaper = 'data:image/jpeg;base64,' + base64.b64encode(buffer.getvalue()).decode()

    async def run(url):
        async with async_playwright() as playwright:
            browser = await playwright.chromium.launch()
            return await take_shots(browser, url, wallpaper)

    with serve(ROOT / 'src') as url:
        banner, *photos = asyncio.run(run(url))
    images = {'banner.png': banner_with_window(banner[0]), 'screens.png': screen_sheet(photos, desk, args.font)}
    args.out.mkdir(parents=True, exist_ok=True)
    for name, image in images.items():
        image.save(args.out / name, optimize=True)
        print(name, image.size)


if __name__ == '__main__':
    main()
