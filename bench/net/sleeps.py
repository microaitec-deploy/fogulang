# concurrency: 10k tasks, each waits 100 ms then adds to a counter
import asyncio

async def main():
    done = 0
    async def task():
        nonlocal done
        await asyncio.sleep(0.1)
        done += 1
    await asyncio.gather(*(task() for _ in range(10000)))
    print(done)

asyncio.run(main())
