# WebSocket echo server (websockets, asyncio)
import asyncio, sys
import websockets

async def echo(ws):
    async for msg in ws:
        await ws.send(msg)

async def main(port):
    async with websockets.serve(echo, "127.0.0.1", port, compression=None):
        print("ready", flush=True)
        await asyncio.Future()

asyncio.run(main(int(sys.argv[1]) if len(sys.argv) > 1 else 9002))
