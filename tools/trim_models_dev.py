import json, math, sys
from pathlib import Path
from urllib.parse import urlparse
KNOWN_HOST = {"openai": "api.openai.com", "anthropic": "api.anthropic.com", "google": "generativelanguage.googleapis.com", "xai": "api.x.ai", "mistral": "api.mistral.ai", "groq": "api.groq.com"}
def caps(m):
    mo = m.get("modalities") or {}
    if "text" not in (mo.get("output") or ["text"]): return None
    return [bool(m.get("tool_call")), "image" in (mo.get("input") or []), bool(m.get("reasoning")), int((m.get("limit") or {}).get("context") or 0), m.get("name") or ""]
def trim(api, labs, fetched):
    out = {"v": 1, "fetched": fetched, "providers": {}, "labs": {}}
    for pid, p in sorted(api.items()):
        host = urlparse(p["api"]).hostname if p.get("api") else KNOWN_HOST.get(pid, "")
        ms, prices = {}, {}
        for mid, m in sorted(p.get("models", {}).items()):
            c = caps(m)
            if c is None:
                continue
            ms[mid] = c
            cost = m.get("cost") or {}
            price = [cost.get("input"), cost.get("output")]
            if all(isinstance(x, (int, float)) and not isinstance(x, bool) and math.isfinite(x) and x >= 0 for x in price):
                prices[mid] = price
        out["providers"][pid] = {"n": p.get("name", pid), "h": host or "", "m": ms}
        if prices:
            out["providers"][pid]["c"] = prices
    for mid, m in sorted(labs.items()):
        c = caps(m)
        if c: out["labs"][mid] = c
    return out
if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("用法：python trim_models_dev.py YYYY-MM-DD 输出路径")
    try:
        api = json.loads(Path("api.json").read_text(encoding="utf-8"))
        labs = json.loads(Path("models.json").read_text(encoding="utf-8"))
        o = trim(api, labs, sys.argv[1])
        Path(sys.argv[2]).write_text(json.dumps(o, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    except (OSError, ValueError) as err:
        sys.exit(f"无法生成模型资料：{err}")
