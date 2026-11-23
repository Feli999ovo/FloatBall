import asyncio
import os
from pathlib import Path
import sys

from PIL import Image
from playwright.async_api import async_playwright
import pytest
import pytest_asyncio

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from local_http import serve
import ui_support
from ui_support import OUT, ROOT, WALL


@pytest.fixture(autouse=True)
def _check_starts_on_fresh_line():
    # 每个用例的第一条 PASS/FAIL 前换行，避开 pytest 的进度点（见 ui_support.check）
    ui_support._fresh_line[0] = True


def pytest_configure(config):
    Path(OUT).mkdir(parents=True, exist_ok=True)


@pytest.fixture(scope='session')
def base_url():
    with serve(os.path.join(ROOT, 'src')) as url:
        yield url


@pytest_asyncio.fixture
async def b():
    async with async_playwright() as browser_tools:
        browser = await browser_tools.chromium.launch(args=[
            '--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream',
            '--autoplay-policy=no-user-gesture-required',
        ])
        try:
            yield browser
        finally:
            await browser.close()


@pytest_asyncio.fixture
async def orb_page(b, base_url):
    ctx = await b.new_context(viewport={'width': 96, 'height': 96}, device_scale_factor=2)
    try:
        await ctx.grant_permissions(['clipboard-read', 'clipboard-write'])
        pg = await ctx.new_page()
        errs = []
        pg.on('pageerror', lambda e: errs.append(str(e)))
        pg.on('console', lambda m: m.type == 'error' and errs.append(m.text))
        await pg.expose_function('__resize', lambda w, h: asyncio.ensure_future(
            pg.set_viewport_size({'width': int(w), 'height': int(h)})))
        await pg.add_init_script(path=os.path.join(ROOT, 'tools', 'mock-tauri.js'))
        await pg.goto(base_url + '/index.html')
        await pg.wait_for_timeout(400)

        async def shot(name):
            await pg.wait_for_timeout(450)
            path = f'{OUT}/{name}.png'
            await pg.screenshot(path=path, omit_background=True)
            m = await pg.evaluate('__mock')
            if WALL:
                desk = Image.open(WALL).convert('RGBA').resize((1440 * 2, 900 * 2))
                w = Image.open(path).convert('RGBA')
                desk.alpha_composite(w, (int(m['pos']['x'] * 2), int(m['pos']['y'] * 2)))
                desk.convert('RGB').save(f'{OUT}/{name}_desk.jpg', quality=85)
            return m

        yield pg, shot, errs
    finally:
        await ctx.close()
