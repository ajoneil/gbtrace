#!/usr/bin/env python3
"""Dev server that serves files directly from source directories.

Usage: devserver.py <project-dir> [pages-url]

Serves on port 3080 with live source files — no copying needed.
Routes:
  /src/*          → web/src/*
  /pkg/*          → web/pkg/*
  /index.html, /  → web/index.html
  /tests/*        → build/traces/* (with directory remapping)
  everything else → proxy to Pages if available
"""
import os
import sys
import mimetypes
from http.server import HTTPServer, SimpleHTTPRequestHandler

PROJECT_DIR = sys.argv[1] if len(sys.argv) > 1 else os.getcwd()
PAGES_URL = sys.argv[2] if len(sys.argv) > 2 else None
PORT = 3080

# Map URL prefixes to local filesystem paths
ROUTE_MAP = [
    ('/src/',  os.path.join(PROJECT_DIR, 'web/src/')),
    ('/pkg/',  os.path.join(PROJECT_DIR, 'web/pkg/')),
]

# Test suite directories map /tests/<suite>/ to build/traces/<suite>/
TRACE_DIR = os.path.join(PROJECT_DIR, 'build/traces')
PROFILE_DIRS = os.path.join(PROJECT_DIR, 'test-suites')


class DevHandler(SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cache-Control', 'no-cache, no-store, must-revalidate')
        self.send_header('Pragma', 'no-cache')
        self.send_header('Expires', '0')
        self.send_header('Access-Control-Allow-Origin', '*')
        super().end_headers()

    def do_GET(self):
        url_path = self.path.split('?')[0]

        # Root / index.html
        if url_path in ('/', '/index.html'):
            return self._serve_file(os.path.join(PROJECT_DIR, 'web/index.html'))

        # Source and pkg routes
        for prefix, local_dir in ROUTE_MAP:
            if url_path.startswith(prefix):
                rel = url_path[len(prefix):]
                local_path = os.path.join(local_dir, rel)
                if os.path.isfile(local_path):
                    return self._serve_file(local_path)
                break

        # Test traces: /tests/<suite>/profile.toml → test-suites/<suite>/profile.toml
        if url_path.startswith('/tests/') and url_path.endswith('/profile.toml'):
            parts = url_path.split('/')
            if len(parts) >= 4:
                suite = parts[2]
                profile_path = os.path.join(PROFILE_DIRS, suite, 'profile.toml')
                if os.path.isfile(profile_path):
                    return self._serve_file(profile_path)

        # Test traces: /tests/<suite>/* → build/traces/<suite>/*
        if url_path.startswith('/tests/'):
            rel = url_path[len('/tests/'):]
            local_path = os.path.join(TRACE_DIR, rel)
            if os.path.isfile(local_path):
                return self._serve_file(local_path)

        # Proxy missing files from Pages
        if PAGES_URL:
            if self._proxy(url_path):
                return

        self.send_error(404, 'File not found')

    def _serve_file(self, path):
        try:
            with open(path, 'rb') as f:
                data = f.read()
            self.send_response(200)
            ct, _ = mimetypes.guess_type(path)
            self.send_header('Content-Type', ct or 'application/octet-stream')
            self.send_header('Content-Length', len(data))
            self.end_headers()
            self.wfile.write(data)
        except IOError:
            self.send_error(404, 'File not found')

    def _proxy(self, url_path):
        import urllib.request
        remote_url = PAGES_URL.rstrip('/') + url_path
        try:
            req = urllib.request.Request(remote_url)
            with urllib.request.urlopen(req, timeout=30) as resp:
                data = resp.read()
                self.send_response(200)
                ct = resp.headers.get('Content-Type', 'application/octet-stream')
                self.send_header('Content-Type', ct)
                self.send_header('Content-Length', len(data))
                self.send_header('X-Proxied-From', remote_url)
                self.end_headers()
                self.wfile.write(data)
                return True
        except Exception:
            return False

    def log_message(self, format, *args):
        msg = format % args
        if hasattr(self, '_headers_buffer'):
            for line in self._headers_buffer:
                if b'X-Proxied-From' in line:
                    msg += ' (proxied)'
                    break
        print(msg)


if __name__ == '__main__':
    print(f'Serving on http://localhost:{PORT}')
    print(f'  Source:  {PROJECT_DIR}/web/')
    print(f'  Traces:  {TRACE_DIR}/')
    if PAGES_URL:
        print(f'  Proxy:   {PAGES_URL}')
    HTTPServer(('', PORT), DevHandler).serve_forever()
