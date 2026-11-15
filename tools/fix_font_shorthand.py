"""Chrome 的 cssText 遇到带 var() 的 font 简写会写成一串空的 font-*: ; 长属性，等于丢掉字号。
这里按选择器回到原型 CSS 找原来的 font: 值，换回去。用法：python3 tools/fix_font_shorthand.py src/settings.css"""
import re, sys
PROTO = "/workspace/work/fb/proto/proto.css"
BROKEN = re.compile(r"(?:font(?:-[a-z-]+)?: ;\s*)+")
def main(path):
    proto = open(PROTO).read()
    css = open(path).read()
    out, n, miss = [], 0, []
    for line in css.split("\n"):
        if "font-size: ;" in line:
            sel = line.split("{", 1)[0].strip()
            m = re.search(r"(?:^|[}\s])" + re.escape(sel).replace(r"\ ", r"\s*") + r"\s*\{[^}]*?\bfont:([^;}]+)", proto, re.S)
            if m:
                line = BROKEN.sub("font:" + m.group(1).strip() + "; ", line, count=1); n += 1
            else:
                miss.append(sel)
        out.append(line)
    # 剩下的空声明（line-height: ; 之类）本来就无效，删掉
    open(path, "w").write(re.sub(r"(?<![\w-])[a-z-]+: ;\s*", "", "\n".join(out)))
    print(f"fixed {n}", "missing:", miss)
if __name__ == "__main__":
    main(sys.argv[1])
