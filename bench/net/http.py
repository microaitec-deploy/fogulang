# HTTP: GET /hello → a small JSON body (standard library, a thread per connection)
import json, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def do_GET(self):
        body = json.dumps({"message": "hello", "path": self.path}).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *args):
        pass

port = int(sys.argv[1]) if len(sys.argv) > 1 else 8002
server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
print("ready", flush=True)
server.serve_forever()
