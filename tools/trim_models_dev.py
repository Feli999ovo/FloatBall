import json, sys
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
        ms = {}
        for mid, m in sorted(p.get("models", {}).items()):
            c = caps(m)
            if c: ms[mid] = c
        out["providers"][pid] = {"n": p.get("name", pid), "h": host or "", "m": ms}
    for mid, m in sorted(labs.items()):
        c = caps(m)
        if c: out["labs"][mid] = c
    return out
if __name__ == "__main__":
    o = trim(json.load(open("api.json")), json.load(open("models.json")), sys.argv[1])
    json.dump(o, open(sys.argv[2], "w"), ensure_ascii=False, separators=(",", ":"))
