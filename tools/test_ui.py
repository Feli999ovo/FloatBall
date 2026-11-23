"""Run the browser regression suite, keeping the existing output arguments."""
import os
from pathlib import Path
import subprocess
import sys


def main():
    if len(sys.argv) > 1:
        os.environ['FLOATBALL_UI_OUTPUT'] = sys.argv[1]
    if len(sys.argv) > 2:
        os.environ['FLOATBALL_UI_WALLPAPER'] = sys.argv[2]
    options = sys.argv[3:]
    suite = Path(__file__).with_name('ui_tests')
    return subprocess.call([sys.executable, '-X', 'utf8', '-m', 'pytest', '-q', '-s', str(suite), *options])


if __name__ == '__main__':
    raise SystemExit(main())
