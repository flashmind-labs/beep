#!/usr/bin/env python3
"""Deterministic OpenAI-compatible fixture. NOT a real model. Local testing only."""
import gzip
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        data = json.dumps({"data": [{"id": "neobeep-test", "max_model_len": 32768}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        if self.headers.get("Content-Encoding") == "gzip":
            raw = gzip.decompress(raw)
        request = json.loads(raw)
        messages = request.get("messages", [])
        user = next((m.get("content", "") for m in reversed(messages) if m["role"] == "user"), "")
        last = messages[-1] if messages else {}
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()

        def chunk(delta, finish=None):
            event = {"id": "fixture", "object": "chat.completion.chunk", "model": "neobeep-test",
                     "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
            self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
            self.wfile.flush()

        try:
            if last.get("role") != "tool" and ("tea" in user.lower() or "drink preference" in user.lower()):
                storing = "tea" in user.lower()
                chunk({"tool_calls": [{"index": 0, "id": "fixture-memory", "type": "function",
                    "function": {"name": "memory_store" if storing else "memory_recall",
                        "arguments": json.dumps({"key": "drink", "content": "Prefers tea"} if storing else {"query": "drink"})}}]})
                chunk({}, "tool_calls")
            else:
                if last.get("role") == "tool":
                    text = "Test provider: I saved your preference for tea." if '"stored":true' in last.get("content", "").replace(" ", "") else "Test provider: " + last.get("content", "")
                else:
                    text = "Test provider: Beep is connected over QUIC. Configure a real model endpoint for everyday conversations."
                for word in text.split(" "):
                    chunk({"content": word + " "})
                    time.sleep(0.07)
                chunk({}, "stop")
            self.wfile.write(b"data: [DONE]\n\n")
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=18080)
    args = parser.parse_args()
    print(f"Deterministic TEST provider on 127.0.0.1:{args.port}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()
