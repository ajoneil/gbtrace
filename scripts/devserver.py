#!/usr/bin/env python3
"""Dev server that serves local files and proxies missing ones to Pages.

Usage: devserver.py [pages-url]

Serves the current directory on port 3080. If a requested file doesn't
exist locally, proxies the request to the deployed Pages site. This lets
you develop the viewer locally while using traces from the last deploy.
"""
import sys
import urllib.request
from http.server import HTTPServer, SimpleHTTPRequestHandler

PAGES_URL = sys.argv[1] if len(sys.argv) > 1 else None
PORT = 3080


class DevHandler(SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cache-Control', 'no-cache, no-store, must-revalidate')
        self.send_header('Pragma', 'no-cache')
        self.send_header('Expires', '0')
        self.send_header('Access-Control-Allow-Origin', '*')
        super().end_headers()

    def do_GET(self):
        import os
        path = self.translate_path(self.path)
        is_manifest = self.path.endswith('manifest.json')

        # Always proxy manifests from Pages so all emulators are visible,
        # even when only some traces exist locally.
        if PAGES_URL and is_manifest:
            if self._proxy(self.path):
                return

        # Try serving locally first (non-manifest files)
        if os.path.exists(path) and not os.path.isdir(path):
            return super().do_GET()

        # Proxy missing files from Pages
        if PAGES_URL and not os.path.exists(path):
            if self._proxy(self.path):
                return

        # Normal handling (directory listings or 404)
        return super().do_GET()

    def _proxy(self, url_path):
        """Proxy a request to the Pages site. Returns True on success."""
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
        # Tag proxied requests
        msg = format % args
        if hasattr(self, '_headers_buffer'):
            for line in self._headers_buffer:
                if b'X-Proxied-From' in line:
                    msg += ' (proxied)'
                    break
        print(msg)


if __name__ == '__main__':
    print(f'Serving on http://localhost:{PORT}')
    if PAGES_URL:
        print(f'  Proxying missing files to {PAGES_URL}')
    else:
        print('  No Pages URL — serving local files only')
    HTTPServer(('', PORT), DevHandler).serve_forever()
