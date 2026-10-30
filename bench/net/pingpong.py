# message passing: 100k round trips between two tasks over queues
import asyncio

async def main():
    ping, pong = asyncio.Queue(1), asyncio.Queue(1)
    async def echo():
        while True:
            n = await ping.get()
            if n is None:
                break
            await pong.put(n + 1)
    t = asyncio.create_task(echo())
    total = 0
    for i in range(100000):
        await ping.put(i)
        total += (await pong.get()) - i
    await ping.put(None)
    await t
    print(total)

asyncio.run(main())
