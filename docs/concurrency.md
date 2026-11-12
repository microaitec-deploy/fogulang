# Concurrency in Fogu: `fg` tasks and workers

Fogu has two levels of concurrency, and they are meant to be used together:

> **Threads for parallelism across cores, light tasks for concurrency at scale.**

- An **`fg` task** is a light task of the runtime: cheap to start (about 9 KB),
  scheduled cooperatively by the thread it was started on. Thousands of them
  wait on sockets, channels and timers at the same time.
- A **`sync.Worker`** is a real OS thread of the same process, running a Fogu
  program with its own interpreter. Workers run in parallel, one per core.

Each worker runs its own `fg` tasks. A server on every core is therefore a few
workers, each with many tasks:

```
process
├── main thread ── interpreter · scheduler · reactor ── fg tasks …
├── worker 1 ───── interpreter · scheduler · reactor ── fg tasks …
├── worker 2 ───── interpreter · scheduler · reactor ── fg tasks …
└── …
```

| | `fg` task | `sync.Worker` |
|---|---|---|
| What it is | a task of the runtime, on its own small stack | an OS thread with its own interpreter |
| Runs in parallel with | nothing on its thread; with the tasks of other workers | the main thread and the other workers |
| Scheduled by | the Fogu scheduler of its thread (cooperative) | the operating system (preemptive) |
| Cost to start | ~9 KB and a few µs | a thread, an interpreter and the program's top level |
| Shares Fogu values with | every task of its thread (same heap) | nothing — values cross as JSON |
| Use it for | many concurrent waits: connections, requests, timers | CPU work on several cores; a server per core |

The design rule: **code may cross workers; state and execution context do not
cross implicitly.** A worker receives a program (and, from 0.8.0, a function's
source) and builds its own values from it; what workers exchange goes through
explicit interfaces.

## `fg` tasks

```fogu
const results = chan(3)
fg fn() {
    for i in 0..3 { results <- i * 10 }
    close(results)
}()
for v in results { print(v) }
```

- **Cooperative.** A task runs until it waits: a network or file operation,
  a channel send or receive, `select`, `time.delay`, `sync.Mutex.lock`,
  `w.call`. Then the scheduler resumes another task that is ready. A loop that
  never waits keeps the others of its thread from running — split long CPU
  work into a worker, or wait now and then.
- **Affinity.** A task stays on the thread that started it, for its whole life;
  it never moves to another core (its values are reference-counted, not
  thread-safe). Tasks started inside a worker run in that worker.
- **No data races in Fogu code.** Only one task of a thread runs at a time, so
  a variable shared by tasks changes only at waiting points. Code that reads,
  waits and then writes must still expect other tasks to run in between
  (`sync.Mutex` for that).
- **Waiting is cheap.** On Linux the scheduler waits on its sockets itself
  (an `epoll` reactor per thread): a task whose socket is not ready costs
  nothing until it is, and when no task can run the thread sleeps in the
  kernel. Elsewhere the I/O threads of the runtime wait for it.
- **Deadlocks are reported.** When every task waits on channels and nothing
  can wake them (no I/O in flight, no timer), the program stops with
  `deadlock: all tasks are blocked waiting on channels`.
- **The program ends with the main task**, as in Go: tasks still waiting are
  discarded. Wait for them with a channel or `sync.WaitGroup`.

## Workers

```fogu
import sync from "fogu:sync"

const w = sync.Worker(`
fn primes(limit) {
    let n = 0
    for i in 2..limit {
        let p = true
        for d in 2..i {
            if d * d > i { break }
            if i % d == 0 {
                p = false
                break
            }
        }
        if p { n = n + 1 }
    }
    return n
}
`, { core: 2 })?
print(w.call("primes", 100000)?)
w.close()
```

- **A thread, not a process.** Same process, same memory space, same open
  file limit — but a separate interpreter: its own heap of Fogu values, its
  own scheduler and reactor, its own JIT. Workers do not lock each other.
- **Created with a program** (a file or code). Its top level runs once, when
  the worker is created; errors there are the error of `sync.Worker(...)`.
- **`w.call(name, ...args)`** runs a global function of the worker and waits
  for the result *cooperatively*: the caller's other tasks keep running.
  Arguments and results are JSON values (numbers, texts, bools, nil, lists,
  maps); a failure comes back as an error value. Calls to one worker run one
  at a time, in order.
- **`{ core: n }`** pins the worker's thread to CPU `n`.
- **Sharing.** Nothing of the Fogu heap is shared. To share state, pass it in
  calls, or keep it outside: a database or `fogu:kv` file, Redis, NATS, a
  socket. Channels belong to one thread and cannot be sent to a worker.
- **The worker's own tasks run while a call is in progress.** Between calls
  the worker's thread waits for the next call and its tasks do not advance —
  so a worker that serves (a listener, background tasks) does it inside a call
  that lasts as long as the service, as `bench/net/tcp_workers.fg` does with
  `serve()`. (Letting a worker's tasks run between calls is in the 0.8.0
  roadmap, #12e.)
- **Not a function, yet.** `sync.Worker` takes a file or code text, not a
  function value: a function is a value of its thread's heap, with the
  environment it captures. 0.8.0 adds `sync.Worker(fn)`, which sends the
  function's source and refuses functions that capture local variables (#12c).

## A server on every core

`net.tcpListen(addr, { reusePort: true })` lets several listeners share a port;
the kernel spreads new connections over them. One listener per worker gives a
server on every core, each worker serving its connections with `fg` tasks —
`bench/net/tcp_workers.fg`:

| workers | messages (3 s, 256 connections) | p50 ms | p99 ms |
|---:|---:|---:|---:|
| 1 | 204 964 | 3.43 | 65.58 |
| 2 | 352 555 | 2.00 | 7.20 |
| 4 | 711 116 | 0.95 | 3.29 |
| 8 | 1 412 363 | 0.31 | 4.80 |

6.9× from 1 to 8 workers (86% of linear). The kernel does not spread
connections exactly evenly: run the example with a duration
(`fogu tcp_workers.fg PORT 8 10`) to see how many each worker accepted. On the
measuring machine (8 cores, 16 threads) the load generator ran on the same
cores; `bench/run_net.fg scaling` pins the server and the generator to
separate CPUs and measures Go on the same matrix.

### Verified: one scheduler per worker, tasks confined to their thread

In the code:

- All the scheduler's state is per thread (`thread_local!` in
  `runtime/sched.rs`: the run queue, the parked tasks, the timers, the task
  stacks, the current task) and so is the socket reactor (`runtime/reactor.rs`).
  Each worker thread therefore has its own scheduler and its own `epoll`.
- A task cannot move: a task is a `corosensei` coroutine, which is not `Send`
  (it holds a `PhantomData<*mut ()>`), and Fogu values are `Rc` — the Rust
  compiler refuses any code that would hand either to another thread.

In a run (`strace -f`, `tcp_workers.fg` with 4 workers, 64 connections):

| thread | `epoll_create1` | `recvfrom` | `sendto` | `epoll_wait` | `accept4` |
|---|---:|---:|---:|---:|---:|
| worker A | 1 (+1 tokio's) | 2 564 | 2 545 | 154 | 0 |
| worker B | 1 | 2 667 | 2 654 | 213 | 0 |
| worker C | 1 | 3 958 | 3 937 | 216 | 0 |
| worker D | 1 | 4 660 | 4 648 | 395 | 0 |
| 5 I/O threads | 0 | 0 | 0 | 77 | 68 |

Connections accepted per worker: 12, 13, 19, 20 — the kernel spreads them, not
evenly, and each worker's share of the traffic follows its share of the
connections. All the reads and writes of a connection happen on one worker
thread; the runtime's I/O threads only accept new connections (that goes
through tokio, and is rare). The main thread does no socket work.

One thing is still shared between threads: the wake-up signal for work that
finishes off-thread (`sched::wake`) is process-wide, so a completion on one
worker also wakes the others that are asleep, to look and find nothing. It
costs no correctness, only some idle wake-ups — a candidate for the p99 rise
from 4 to 8 workers, to measure in 0.8.0 (#12c).

In 0.8.0 the servers of the standard library (`http.serve`, `web`, `websocket`,
`net`) take `{ workers }` and do this themselves (`docs/roadmap-0.8.0.md`, #12b).

## Choosing

- Many things waiting (connections, requests, timers, subprocesses): `fg`.
- CPU-heavy work, or a server that must use more than one core: workers,
  each with `fg` tasks inside.
- Code that must be stopped after a time: `fogu:vm` with `{ timeout }` (it
  stops compiled code too), in a task or a worker.
