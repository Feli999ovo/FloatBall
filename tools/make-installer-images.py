"""用现有图标 src-tauri/icons/icon.png 生成 Windows 安装器用的两张图，图标本身不改。
换了图标后重新运行一次：python tools/make-installer-images.py（需要 Pillow）"""
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "windows"
ICON = Image.open(ROOT / "src-tauri" / "icons" / "icon.png").convert("RGBA")
INK, INK_2 = (0x17, 0x14, 0x2A), (0x22, 0x1E, 0x3A)  # 和 src/style.css 的 --ink、--ink-2 一致


def put_icon(canvas, size, cx, cy):
    icon = ICON.resize((size, size), Image.LANCZOS)
    canvas.alpha_composite(icon, (cx - size // 2, cy - size // 2))


# 欢迎页和完成页左侧的竖图，安装器要求 164×314；墨色渐变底，和对话卡片的底色一样从上往下变深
side = Image.new("RGBA", (164, 314))
for y in range(314):
    t = y / 313
    side.paste(tuple(round(a + (b - a) * t) for a, b in zip(INK_2, INK)) + (255,), (0, y, 164, y + 1))
put_icon(side, 104, 82, 128)

# 其余页面页眉里的横图，安装器要求 150×57；页眉是白底，所以图也是白底
head = Image.new("RGBA", (150, 57), (255, 255, 255, 255))
put_icon(head, 45, 75, 28)

# 安装器只认不带透明通道的 24 位 BMP
side.convert("RGB").save(OUT / "installer-sidebar.bmp")
head.convert("RGB").save(OUT / "installer-header.bmp")
print("已生成", OUT / "installer-sidebar.bmp", OUT / "installer-header.bmp")
