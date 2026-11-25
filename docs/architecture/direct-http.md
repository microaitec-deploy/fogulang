# Design note: the HTTP server on the interpreter thread

Status: implemented (unreleased, after 0.8.3) · changes a fundamental (how the
server's I/O reaches the interpreter), so it follows `engineering-principles.md`:
this note, measurements before and after, the full suite.

## Problem

Until 0.8.3 every `http.serve` / `app.listen` ran on the shared tokio runtime
(hyper). A request crossed threads twice: the I/O thread parsed it and queued a
`PlainRequest`, set a flag and woke the interpreter; the interpreter started a
task for the handler, built the answer and sent a `PlainResponse` back through a
channel; the I/O thread wrote it. Measured on the `bench/net/http.fg` server
(64 keep-alive connections, same machine):

- the interpreter thread was idle 70% of the time while the server was at its
  limit: the limit was the hand-over, not the handler;
- every request allocated on one thread and freed on another (the headers, the
  body, the answer), which made the interpreter's allocations 3 to 4 times slower;
- one connection alone got 800–1400 requests/s (two thread wake-ups per request),
  where `fogu:net` — already on the interpreter's own reactor — got 5300;
- the process used 3 to 4 cores to serve what one interpreter can run.

## Decision

On Linux, a plain HTTP server (no TLS, no static mounts, no Socket.IO) is served
**by the interpreter thread itself** (`native/network/http_direct.rs`), on the
per-thread `epoll` reactor `fogu:net` already uses:

1. `start_server` binds the listener and queues it; the next call of the
   interpreter (`Host::before_call` → `http::serve_pending`), or
   `Fogu.signal.wait()` when it is the very next thing, starts its accept task.
   Each accepted connection is one `fg` task.
2. The connection task receives into a buffer shared by the thread, parses the
   request where it was received (`httparse`, the parser hyper uses), builds the
   `Request` object, **calls the handler in the same task**, writes the answer
   into the connection's output buffer and sends it. A request costs one `recv`
   and one `send`; a socket found empty is not asked again before the reactor saw
   something arrive (`reactor::take_readable`).
3. Nothing of a request is seen by another thread. The usual answer (a
   `Response` object) is written straight from the object, without an
   intermediate `PlainResponse`.
4. **Deadlines without a timer per request.** A connection publishes its phase
   and deadline (`Shared`); one sweeper task per thread looks at the open
   connections every quarter of the shortest configured timeout (10 ms to 1 s)
   and acts on the socket: `shutdown(read)` wakes a task waiting for a slow
   request (which closes, or answers 408 in the body phase); a handler past
   `handlerTimeout` gets its 504 written by the sweeper and the connection
   closed — the handler finishes in its task and its answer is dropped, as
   before. A timeout therefore fires up to a quarter late, never early.
5. **Streamed bodies** (`web.sse`, `web.stream`, a readable returned from a
   handler) keep their channel; the sending end now also sets a gate
   (`http::StreamReady`, set on every send and when the sender is dropped), so
   the connection task parks behind it instead of asking the channel on every
   scheduler pass. The sweeper notices a client that left an idle stream.
   `web.file` reads the file on the pool (`rt::offload`), 64 KiB at a time.
6. **WebSocket upgrades** answer the 101 here and hand a duplicate of the
   socket to the WebSocket code, which stays on the I/O threads (bytes the client
   sent right behind its request go with it).
7. **More cores** are the program's decision, as everywhere in Fogu: one server
   per `sync.Worker` on the same port with the new `reusePort: true` option
   (`bench/net/http_workers.fg`). Each worker has its own interpreter, reactor
   and connections; the kernel spreads the connections.
8. Servers with TLS, static mounts or Socket.IO, and every server on other
   systems, keep the previous path unchanged. `FOGU_HTTP=threads` keeps all
   servers on it (for comparisons, and as an escape hatch).

`fogu:web`'s dispatch became the larger part of a request on this path, so two
things left it: the per-request state of `fogu:topcoat` (`ctx.tc`, three maps) is
made when something first uses it (`topcoat::ctx_state`; the app is found through
the store the `ctx` carries), and `Host::intercept` looks for an app's dispatch
function first.

## Behaviour kept

The limits and their answers are the same: `maxHeaderBytes` (431), `headerTimeout`
(the connection closes; also the idle keep-alive time), `maxBody` (413, for a
declared length before reading it), `readTimeout` (408), `handlerTimeout` (504
`timeout`), the canonical path (400 `bad request: …`), a handler's `nil` (404),
errors (500, logged, never sent), HEAD without a body, `Expect: 100-continue`,
keep-alive for HTTP/1.1 and on request for HTTP/1.0.

Stricter than before, on purpose:

- a request with both `Content-Length` and `Transfer-Encoding`, with two
  different lengths, a length that is not plain digits, or an encoding that does
  not end in `chunked` is refused (400) — these are the request-smuggling shapes;
- a response header whose name is not a token or whose value has a line break is
  not sent (a header can never inject another one);
- `content-length` and `transfer-encoding` of an answer are always the server's
  (a HEAD answer may still announce the size its GET would have).

Different: the requests of one connection are handled one after the other in
the connection's task (HTTP/1.1 has no other order; pipelined requests were
already answered in order).

## Memory

A connection is a task (its stack is pooled and only touched pages are
resident), two small buffers that are empty between requests (the receive
buffer is the thread's; the output buffer is dropped when an answer was large)
and an entry in the thread's connection table. No per-request task, channel,
timer or `PlainRequest`.

## Measurements

Same machine (WSL2, 16 logical CPUs, other work running), `bench/.bin/netbench
load-http`, keep-alive connections, 3–4 s, interleaved runs; the load generator
runs on the same machine. `web`: `bench/net/http.fg` (`GET /hello`, a small JSON
from a `fogu:web` route — the case of `bench/run_net.fg`); `raw`: the same
answer from `http.serve`. Requests/s; CPU seconds and peak RSS of the server at
64 connections over 4 s.

| server | 1 conn | 16 | 64 | 256 | 1024 | CPU s | RSS |
|---|---|---|---|---|---|---|---|
| Go `net/http` (all cores) | 3.7k | | 64–69k | 88k | 98k | 22 | 24–28 MB |
| 0.8.3 web | 2.4k | | 37–43k | 43–44k | 38–41k | 12.9 | 25–26 MB |
| new web, one interpreter | 9.5k | | 46–50k | 42.5k | 33.5k | 3.7 | 16–18 MB |
| 0.8.3 raw | | 33–35k | 63–69k | 75–78k | | 15 | 22–25 MB |
| new raw, one interpreter | | 55–58k | 55–67k | 49–52k | | 3.7 | 15–19 MB |
| new web, 2 workers | | | 82–84k | | | 7.4 | 20 MB |
| new web, 4 workers | | | 113–114k | 97k | 79k | 14 | 28 MB |
| new web, 8 workers | | | 169k | | | 21 | 45 MB |

One interpreter now serves on one core what 0.8.3 needed 3 to 4 cores for, and
a request no longer waits for two thread wake-ups: 4 times the throughput on
one connection, 1.7 times on 16, p99 at 64 connections 3.7–4.2 ms instead of
10–13 ms. Per CPU second it handles about 4 times what Go does; 4 workers give
1.7 times Go's throughput on 64 connections with two thirds of its CPU, 8
workers 2.5 times with the same CPU.

## The cost, accepted

One interpreter is one core, and that core now also does the kernel's work for
every socket, which 0.8.3 spread over the I/O threads. While the handler is the
larger part of a request this does not show; with a trivial handler and many
connections it does:

- `fogu:web`, one interpreter: equal at 256 connections (−2%), **−15% at 1024**;
- `http.serve` with a trivial handler, one interpreter: −5% at 64 connections,
  **−33% at 256**.

The cost of a request grows with the number of connections (each connection's
task has its own stack and the kernel its own socket state: more cache and TLB
misses), on one core. This was put to the project's owner with these numbers
and accepted as the default, because every other measure improves (latency,
CPU, memory, few connections, the official benchmark) and the answer to many
connections is the one Fogu already gives for cores: a server per `sync.Worker`
with `reusePort` (4 workers at 1024 connections: 79k against 38–41k). A program
that wants the previous behaviour sets `FOGU_HTTP=threads`.

What could remove part of it later: an idle keep-alive connection that holds no
task (its next request would run on a stack that is still in the cache).

## Not done

- TLS, static mounts and Socket.IO on this path (they keep the I/O threads).
- `sendfile` for `web.file`.
- An automatic worker per core: a second interpreter is a decision of the
  program (`sync.Worker`), not of `listen`.
