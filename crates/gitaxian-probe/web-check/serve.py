"""Serve a directory the way the web host needs a page served.

Cross-origin isolation (COOP + COEP) is what gives the page SharedArrayBuffer,
which the engine's thread pool cannot start without. A production host sets
the same two headers.
"""

import functools
import http.server
import socketserver
import sys


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".wasm": "application/wasm",
        ".js": "text/javascript",
    }

    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        super().end_headers()

    def log_message(self, *args):
        pass


def main():
    root, port = sys.argv[1], int(sys.argv[2])
    handler = functools.partial(Handler, directory=root)
    socketserver.ThreadingTCPServer.allow_reuse_address = True
    with socketserver.ThreadingTCPServer(("127.0.0.1", port), handler) as server:
        server.serve_forever()


if __name__ == "__main__":
    main()
