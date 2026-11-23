import os
from pathlib import Path

ROOT = str(Path(__file__).resolve().parents[1])
OUT = os.environ.get('FLOATBALL_UI_OUTPUT', '/tmp/fbui')
WALL = os.environ.get('FLOATBALL_UI_WALLPAPER')


def check(cond, msg):
    print(('PASS ' if cond else 'FAIL ') + msg)
    if not cond:
        raise AssertionError(msg)


def last_user_content(messages):
    return [m for m in messages if m['role'] == 'user'][-1]['content']


async def enable_model(page):
    await page.evaluate('''() => {
        __mock.cfg.models = [{ id: 1, name: "", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini", vision: true, no_key: false, has_key: true }];
        __mock.cfg.default_model = 1;
        __emit("config");
    }''')
    await page.wait_for_timeout(200)
