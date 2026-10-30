# Benchmarks: Fogu vs Bun vs Go vs Python

Generated from `bench/results.json` by `docs/tools/gen_bench.py` — do not edit by hand.

Machine: AMD Ryzen 7 5700U with Radeon Graphics (16 logical CPUs), linux x86_64. Date: 2026-10-30.
Versions: bun 1.4.2, fogu 0.7.2, go version go1.26.0 linux/amd64, Python 3.10.12. Machine load (1-minute average): 1.1 at the start, 1.3 at the end.
Each number is the **median of 5 runs**, in milliseconds, of the whole process
(start-up included). Lower is better. The languages are interleaved (each repetition runs
every language once), so changes in the machine's load affect all of them alike. Every
program prints the same result in every language; the runner checks it. Bun is the main rival, Go the ceiling.

| benchmark | what it measures | Fogu | Fogu (no JIT) | Bun | Go | Python |
|---|---|---:|---:|---:|---:|---:|
| startup | Start the runtime and print one line. | 11 | 11 | 8 | 4 | 30 |
| fib | Recursive calls: fib(32). | 75 | 4,890 | 50 | 30 | 1,280 |
| loop | An integer loop: 20 million iterations. | 88 | 8,355 | 90 | 51 | 4,053 |
| mandel | Floating point: a 300×300 Mandelbrot grid, 50 iterations. | 41 | 2,861 | 27 | 13 | 1,434 |
| strings | Build 200k strings, join, split and measure them. | 135 | 287 | 111 | 89 | 232 |
| maps | Insert 300k keys in a map, then read them back. | 309 | 745 | 561 | 324 | 578 |
| records | JSON: stringify and parse 50k records. | 318 | 239 | 110 | 332 | 420 |
| sort | Generate and sort 300k pseudo-random integers. | 102 | 247 | 45 | 86 | 467 |

## Before and after the optimizations

Fogu (with the JIT), median in ms: the run kept in `bench/baseline.json` (before the string, loop,
task and server changes below) against this one — two separate runs on the same machine, so
differences of a few percent are noise.

| benchmark | before | now | change |
|---|---:|---:|---:|
| startup | 8 | 11 | +39% |
| fib | 37 | 75 | +102% |
| loop | 38 | 88 | +135% |
| mandel | 52 | 41 | -22% |
| strings | 265 | 135 | -49% |
| maps | 654 | 309 | -53% |
| records | 233 | 318 | +37% |
| sort | 222 | 102 | -54% |

## Optimizations made

- **Native modules built on first import.** The runtime used to build all 70 standard-library modules at start-up; now each one is built the first time a program imports it. A script starts in about 8 ms instead of about 50 ms.
- **list.sort(): merge sort and fast paths.** sort() was a binary insertion sort: few comparisons but O(n²) element moves (300k numbers took minutes). It is now a stable merge sort, and lists of only integers, only strings or only numbers use the standard library's sorts directly. Results and comparator rules are unchanged.
- **JSON: one buffer and a byte parser.** json.stringify wrote a string per value and joined them; it now writes into one buffer, escapes strings as JSON requires (control characters became invalid Rust-style escapes before; NaN and infinity become null). json.parse copied the whole text into a list of characters; it now reads the bytes in place and copies strings without escapes in one step.
- **Strings: shared, copy-on-write.** A string value owned its text, so reading a variable, passing an argument or iterating a list copied it. Strings are now reference-counted and copied only when modified; `s = s + x` and `s += x` append in place (they copied the whole string each time); the number of characters is cached, so `s.len()` and `s[i]` on ASCII text are O(1) (`s[i]` used to collect every character first).
- **Loops reuse their scope.** Every iteration of a loop allocated two environments (the loop variable's and the body's); they are now reused unless a closure or task captured them. Variable lookups hash names with FxHash instead of SipHash.
- **Tasks: pooled stacks, smaller frames.** Finished `fg` tasks give their stack back to a pool instead of an mmap/munmap per task (or per Socket.IO event). The interpreter's eval / exec / call dispatch keeps each kind of expression in its own function, so a call uses ~1.8 KB of native stack instead of ~3 KB: a parked task takes about half the memory.
- **Servers: no waiting for the program.** HTTP requests were only served between the program's calls, so a server kept alive with `time.delay(100ms)` answered up to 100 ms late; now the server wakes a task that serves them at once, and every handler runs in its own task. WebSocket connections are tasks on the shared runtime instead of a thread each polling every 2 ms, and `app.ws` uses the upgraded connection directly.
- **list.join(): one buffer.** join() built a temporary string per item and joined them; it now writes every item into one buffer.

## Reading the numbers

- **Fogu** runs with its Cranelift JIT (the default): hot functions and loops over numbers
  are compiled to machine code. **Fogu (no JIT)** is the tree-walking interpreter alone
  (`fogu --no-jit`), the semantic reference.
- Work that stays in the runtime (strings, maps, JSON, sorting) runs in Rust code in all
  modes; what matters there is the cost of the calls around it.
- Start-up: native modules are built on their first import, so a script starts in a few
  milliseconds.

## Run them yourself

```bash
cargo build --release
target/release/fogu bench/run.fg            # 5 runs per case (needs python3 and bun)
target/release/fogu bench/run.fg 3 fib loop  # fewer runs, some cases
python docs/tools/gen_bench.py              # refresh this page and docs/view
```

The programs are in `bench/` (`<name>.fg`, `<name>.py`, `<name>.js`).


## Servers and concurrency

Median of 3 rounds (each round runs every implementation once). bun 1.3.3, fogu 0.7.0, Python 3.10.12, oha 1.16.0. Machine load (1-minute average): 1.2 at the start, 2.1 at the end.

### HTTP throughput

| server | requests/s | p50 | p99 | non-200 |
|---|---:|---:|---:|---:|
| Fogu | 32,837 | 0.60 ms | 17.38 ms | 0 |
| Fogu (before) | 14,572 | 4.25 ms | 9.05 ms | 0 |
| Python (stdlib) | 960 | 50.06 ms | 82.90 ms | 0 |
| Python (aiohttp) | 6,526 | 9.14 ms | 18.42 ms | 0 |
| Bun | 22,774 | 2.45 ms | 8.87 ms | 0 |

### WebSocket echo

| server | messages/s | errors |
|---|---:|---:|
| Fogu | 29,287 | 0 |
| Fogu (before) | 20,970 | 0 |
| Python (websockets) | 14,766 | 0 |
| Bun | 126,071 | 0 |

### Memory per task (20,000 tasks)

| runtime | per task |
|---|---:|
| Fogu | 5.1 KB |
| Fogu (before) | 9.2 KB |
| Python | 2.2 KB |
| Bun | 1.5 KB |

What it counts (peak RSS with N tasks minus with none, divided by N):

- **Fogu:** a task is a coroutine with its own stack (not a future): the stack pages it touched (2 pages of 4 KB for this program; huge pages are turned off on task stacks), the closure and its environment, its scheduler entry and its wait entry on the channel.
- **Python:** the coroutine frame, the asyncio Task object and the waiter future on the Event.
- **Bun:** the async function's suspended state and its promises.
- **All:** plus indirect allocations and memory the allocator keeps after freeing: the number is what the operating system sees, not only the task's own structures.

### 10,000 concurrent waits of 100 ms

| runtime | total |
|---|---:|
| Fogu | 216 ms |
| Fogu (before) | 230 ms |
| Python | 368 ms |
| Bun | 131 ms |

### 100,000 channel round trips

| runtime | total |
|---|---:|
| Fogu | 129 ms |
| Fogu (before) | 152 ms |
| Python | 1,390 ms |
| Bun | 96 ms |

### Method

- HTTP: GET /hello returning a small JSON body; load from oha (64 connections, 5 s) on the same machine, so the load generator competes for the CPU too. Python is measured with its standard library (a thread per connection) and with aiohttp (asyncio).
- WebSocket: an echo server; the same Bun client for every server opens 50 connections, each sends an 89-byte message and waits for its echo, for 5 s.
- Memory per task: 20,000 tasks parked on a channel / event / promise, then released.
- Concurrent waits: 10,000 tasks each wait 100 ms; the ideal is 100 ms, the rest is scheduling cost.
- Ping-pong: 100,000 round trips between two tasks through channels (Python: asyncio.Queue; JavaScript has no channels, so a minimal promise queue).
- Fogu (before): the same benchmarks from bench/baseline_net.json, a run made before the server, WebSocket and task changes listed above, on the same machine.

```bash
fogu bench/run_net.fg            # needs Linux, bun, oha and bench/.venv (aiohttp, websockets)
python docs/tools/gen_bench.py
```
