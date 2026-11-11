"""本地假模型服务（OpenAI 兼容，流式）。用于端到端测试：python3 tools/mock_openai.py 8765"""
import json, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
LOG = []
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        n = int(self.headers.get("content-length", 0)); body = json.loads(self.rfile.read(n))
        with open("/tmp/mock_openai_requests.jsonl", "a") as f:
            f.write(json.dumps({"auth": self.headers.get("authorization"), "body": body}, ensure_ascii=False) + "\n")
        if not body.get("stream"):
            out = json.dumps({"choices": [{"message": {"content": "pong"}}]}).encode()
            self.send_response(200); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(out))); self.end_headers(); self.wfile.write(out); return
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.end_headers()
        text = "你好！我是**本地假模型**，流式输出正常。\n\n1. 第一项\n2. 第二项\n\n```js\nconsole.log('ok')\n```"
        for i in range(0, len(text), 4):
            self.wfile.write(("data: " + json.dumps({"choices": [{"delta": {"content": text[i:i+4]}}]}, ensure_ascii=False) + "\n\n").encode()); self.wfile.flush(); time.sleep(0.04)
        self.wfile.write(b'data: {"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":30}}\n\ndata: [DONE]\n\n')
ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1]) if len(sys.argv) > 1 else 8765), H).serve_forever()
