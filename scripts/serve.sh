#!/bin/bash
# Local dev server with cache-busting headers
cd "$(dirname "$0")/../docs" || exit 1
echo "Serving on http://localhost:3080"
python3 -c "
from http.server import HTTPServer, SimpleHTTPRequestHandler
class NoCacheHandler(SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cache-Control', 'no-cache, no-store, must-revalidate')
        self.send_header('Pragma', 'no-cache')
        self.send_header('Expires', '0')
        super().end_headers()
HTTPServer(('', 3080), NoCacheHandler).serve_forever()
"
