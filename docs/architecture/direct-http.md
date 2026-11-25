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
2. **A connection between requests holds no task.** It is an entry in a table
   of the thread (`IDLE`, by descriptor) and a flag in the reactor
   (`reactor::notify_readable`): when something arrives the reactor puts its
   descriptor in a list (`take_ready`) and opens one gate (`ready_gate`).
   **Worker tasks** wait behind that gate: a worker takes the connections with
   something to read one after the other, receives into a buffer shared by the
   thread, parses the request where it was received (`httparse`, the parser
   hyper uses), builds the `Request` object, **calls the handler in the same
   task**, writes the answer and sends it; a connection with nothing more to
   read goes back to the table (its output buffer goes to a small pool). A
   request costs one `recv` and one `send`. Before a worker enters a request it
   makes sure another one is free (it starts one if not), because the handler
   may wait (`time.delay`, a query, a slow client): the tasks alive are the
   requests being handled at once plus two, not the connections. So every
   request of a busy server runs on the same warm stack, and ten thousand idle
   keep-alive connections cost a table entry each.
3. Nothing of a request is seen by another thread. The usual answer (a
   `Response` object) is written straight from the object, without an
   intermediate `PlainResponse`.
4. **Deadlines without a timer per request.** A connection publishes its phase
   and deadline (`Shared`); one sweeper task per thread looks at the open
   connections every quarter of the shortest configured timeout (10 ms to 1 s)
   and acts on the socket: `shutdown(read)` makes an idle or slow connection
   readable (it then closes, or answers 408 in the body phase); a handler past
   `handlerTimeout` gets its 504 written by the sweeper and the connection
   closed — the handler finishes in its task and its answer is dropped, as
   before. A timeout therefore fires up to a quarter late, never early.
5. **Streamed bodies** (`web.sse`, `web.stream`, a readable returned from a
   handler) keep their channel; the sending end now also sets a gate
   (`http::StreamReady`, set on every send and when the sender is dropped), so
   the worker parks behind it instead of asking the channel on every
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
the same task (HTTP/1.1 has no other order; pipelined requests were
already answered in order).

## The reactor

`runtime/reactor.rs` gained the three functions above (`notify_readable`,
`take_ready` / `ready_gate`, `kick_ready`): a descriptor may be watched without
a gate of its own. Nothing changes for descriptors that use gates (`fogu:net`):
the event loop has one more branch, taken only when no reader is parked.
`bench/net/tcp.fg` and `udp.fg` measure the same before and after (54–64k and
112–125k msg/s on the loaded machine of the table below, in both builds).

## Memory

An idle connection is its `Conn` (the socket, its limits, two empty buffers)
and an entry in two tables. No stack, no per-request task, channel, timer or
`PlainRequest`. With 1024 keep-alive connections under load the server peaks at
16–18 MB (0.8.3: 95 MB; the first version of this server, a task per
connection: 23–25 MB).

## Measurements

Same machine (WSL2, 16 logical CPUs, other work running: compare within a row
group, not with other documents), `bench/.bin/netbench load-http`, keep-alive
connections, 3 s, interleaved runs; the load generator runs on the same
machine. `web`: `bench/net/http.fg` (`GET /hello`, a small JSON from a
`fogu:web` route — the case of `bench/run_net.fg`); `raw`: the same answer from
`http.serve`. Requests/s.

| server | 1 conn | 64 | 256 | 1024 |
|---|---|---|---|---|
| Go `net/http` (all cores) | 3.7k | 63k | | 91k |
| 0.8.3 web | 2.4–2.5k | 39–40k | 35–37k | 32–33k |
| a task per connection, web | 9.1–9.6k | 39–40k | 32–34k | 26–27k |
| **workers, web (this note)** | 9.2–9.3k | 56–59k | 46–50k | 36–41k |
| 0.8.3 raw | | 55–65k | 65–66k | |
| a task per connection, raw | | 46–60k | 38–44k | |
| **workers, raw** | | 69–72k | 54–61k | |
| workers, web, 4 `sync.Worker` | | 104k | | 111k |
| workers, web, 8 `sync.Worker` | | 148k | | 146k |

CPU seconds of the server over 3 s at 64 connections: 2.9 (0.8.3: 10.4; Go:
16). Peak RSS: 15–16 MB (0.8.3: 25 MB; Go: 23 MB).

One interpreter serves on one core more than 0.8.3 did with three to four, at
every connection count measured for `fogu:web`; a request no longer waits for
two thread wake-ups (4 times the throughput on one connection, p99 at 64
connections 3.4 ms instead of 13).

## What is left

One interpreter is one core, and that core also does the kernel's work for
every socket, which 0.8.3 spread over the I/O threads. It still shows in one
case measured: `http.serve` with a trivial handler at 256 connections, 54–61k
against 65–66k (−8 to −18%). The answer to many connections on many cores is
the one Fogu gives everywhere: a server per `sync.Worker` with `reusePort`
(1024 connections: 111k with 4 workers, 146k with 8; Go 91k).
`FOGU_HTTP=threads` keeps the previous server.

## Not done

- TLS, static mounts and Socket.IO on this path (they keep the I/O threads).
- `sendfile` for `web.file`.
- An automatic worker per core: a second interpreter is a decision of the
  program (`sync.Worker`), not of `listen`.
