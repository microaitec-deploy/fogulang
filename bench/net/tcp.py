# TCP echo server (asyncio streams)
import asyncio, sys

async def handle(reader, writer):
    try:
        while True:
            data = await reader.read(4096)
            if not data:
                break
            writer.write(data)
            await writer.drain()
    except ConnectionError:
        pass
    writer.close()

async def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9101
    server = await asyncio.start_server(handle, "127.0.0.1", port)
    print("ready", flush=True)
    async with server:
        await server.serve_forever()

asyncio.run(main())
