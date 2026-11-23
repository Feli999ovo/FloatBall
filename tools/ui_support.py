import os
from pathlib import Path

ROOT = str(Path(__file__).resolve().parents[1])
OUT = os.environ.get('FLOATBALL_UI_OUTPUT', '/tmp/fbui')
WALL = os.environ.get('FLOATBALL_UI_WALLPAPER')


_fresh_line = [True]


def check(cond, msg):
    # pytest -q 会在用例开头打一个进度点且不换行；每个用例第一条前先换行，让每条「PASS / FAIL」都在行首（CI 按行首统计检查数）
    if _fresh_line[0]:
        print()
        _fresh_line[0] = False
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
