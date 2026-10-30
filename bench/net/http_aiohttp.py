# HTTP: GET /hello → a small JSON body (aiohttp, asyncio)
import sys
from aiohttp import web

async def hello(request):
    return web.json_response({"message": "hello", "path": request.path})

port = int(sys.argv[1]) if len(sys.argv) > 1 else 8003
app = web.Application()
app.router.add_get("/hello", hello)
print("ready", flush=True)
web.run_app(app, host="127.0.0.1", port=port, print=None, access_log=None)
