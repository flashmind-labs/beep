#!/usr/bin/env python3
"""Serve platform mockups with reload support: serve.py --bind ADDRESS."""

import argparse
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import socket
from urllib.parse import urlsplit


ROOT = Path(__file__).resolve().parent
WATCHED = {".html", ".css", ".js", ".json", ".svg", ".png", ".jpg", ".webp", ".woff", ".woff2"}


def revision():
    digest = hashlib.sha256()
    for path in sorted(ROOT.rglob("*")):
        if path.suffix not in WATCHED or any(part.startswith(".") for part in path.relative_to(ROOT).parts):
            continue
        try:
            stat = path.stat()
        except FileNotFoundError:
            continue
        digest.update(f"{path.relative_to(ROOT)}:{stat.st_mtime_ns}:{stat.st_size}\n".encode())
    return digest.hexdigest()


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(ROOT), **kwargs)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def send_bytes(self, body, content_type, head):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if not head:
            self.wfile.write(body)

    def serve(self, head=False):
        route = urlsplit(self.path).path
        if route == "/__mockup/revision":
            self.send_bytes(json.dumps({"revision": revision()}).encode(), "application/json", head)
        elif route == "/__mockup/reload.js":
            self.send_bytes((ROOT / "reload.js").read_bytes(), "text/javascript; charset=utf-8", head)
        else:
            path = Path(self.translate_path(self.path))
            if path.is_dir():
                path = path / "index.html"
            if path.is_file() and path.suffix == ".html":
                script = f'<script src="/__mockup/reload.js" data-revision="{revision()}"></script>'
                html = path.read_text().replace("</head>", script + "</head>", 1)
                self.send_bytes(html.encode(), "text/html; charset=utf-8", head)
            elif head:
                super().do_HEAD()
            else:
                super().do_GET()

    def do_GET(self):
        self.serve()

    def do_HEAD(self):
        self.serve(head=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", required=True, help="Interface address to listen on")
    parser.add_argument("--port", type=int, default=8000)
    args = parser.parse_args()
    family = socket.getaddrinfo(args.bind, args.port, type=socket.SOCK_STREAM)[0][0]

    class Server(ThreadingHTTPServer):
        address_family = family
        daemon_threads = True

    with Server((args.bind, args.port), Handler) as server:
        host = f"[{args.bind}]" if family == socket.AF_INET6 else args.bind
        print(f"Mockups: http://{host}:{args.port}/desktop/index.html", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
