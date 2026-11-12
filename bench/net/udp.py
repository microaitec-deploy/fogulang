# UDP echo server (asyncio datagram endpoint)
import asyncio, sys

class Echo(asyncio.DatagramProtocol):
    def connection_made(self, transport):
        self.transport = transport
    def datagram_received(self, data, addr):
        self.transport.sendto(data, addr)

async def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9201
    loop = asyncio.get_running_loop()
    await loop.create_datagram_endpoint(Echo, local_addr=("127.0.0.1", port))
    print("ready", flush=True)
    await asyncio.Event().wait()

asyncio.run(main())
