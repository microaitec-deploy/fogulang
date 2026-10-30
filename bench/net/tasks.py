# memory per task: N tasks blocked on an event, then all released
import asyncio, sys

async def main(n):
    gate = asyncio.Event()
    async def task():
        await gate.wait()
    tasks = [asyncio.create_task(task()) for _ in range(n)]
    await asyncio.sleep(0.05)
    gate.set()
    await asyncio.gather(*tasks)
    print(n)

asyncio.run(main(int(sys.argv[1]) if len(sys.argv) > 1 else 100000))
