# Design note: streams

Status: implemented (unreleased, next version) · `crates/fogu-std/src/native/io/stream.rs`.
Follows `engineering-principles.md`: this note, measurements, tests on the three engines.

## Problem

A program that handled a large file, download or response had to hold it whole:
`fs.readText`, `res.bytes()`, `web.file` (which read the file into memory before sending it) and
a handler returning a string. Memory grew with the data, and a slow client made the server keep
everything it had produced. Node and Bun solve this with streams; Fogu had them only for
sockets (`fogu:net`) and Server-Sent Events.

## Decision

1. **Two shapes, the same methods everywhere.** A *readable* (`ReadStream` from
   `fs.createReadStream`, `HttpStream` from `http.stream`) has `read(n)`, `readLine()`,
   `readAll()`, `readText()`, `pipe(w)`, `close()`; a *writable* (`WriteStream` from
   `fs.createWriteStream`, `ResponseStream` from `web.stream`) has `write`, `writeLine`,
   `end` / `close`. These are the names `TcpStream` already uses.
2. **`for` reads a readable.** `for chunk in r` gives buffers (lines with `{ lines: true }`)
   until the end. Errors are values: an error ends the loop and is kept in `r.error`, it is not
   raised. Interpreter and VM call the same `Interpreter::stream_next`.
3. **Waiting never blocks the thread.** A read that would wait runs on the pool (an async
   method: the interpreter polls it, the other `fg` tasks run). A read whose answer is already in
   the stream's buffer returns at once — the buffer is looked at, never filled, on the
   interpreter's thread: filling it could wait on the network, and the data may come from a task
   of this very thread (a server and its client in one program).
4. **`pipe` does not touch values.** Source and sink move to a pool thread; the bytes go from one
   to the other without becoming Fogu buffers.
5. **Backpressure for responses.** Each chunk of a `ResponseStream` is a `Bytes` that owns its
   memory and, when the server has written it to the socket and drops it, takes its size off a
   shared counter (`Bytes::from_owner`). `write` waits (cooperatively, `wait_for`) while the
   counter is above `highWaterMark` (1 MiB). The response type of the server did not change: the
   accounting travels with the chunks.
6. **Files are sent a chunk at a time.** `web.file` (and a `ReadStream` returned from a handler)
   streams 64 KiB chunks; `web.file` sets `content-length`.

## Alternatives considered

- *A stream type in the language core*: the core keeps its primitives (tasks, channels);
  streams are objects of the standard library built on them.
- *Bounded channels for the response body*: a different receiver type for the server, and SSE
  would have needed the same change; owning chunks keep one body type.
- *Reading on the interpreter thread and relying on the reactor*: works for sockets (which
  `fogu:net` already does) but not for files or the blocking HTTP client.

## Measured

- 200 MB served to a client reading at 50 MB/s: peak RSS of the server 22 MB with `web.stream`,
  with a `ReadStream` returned from the handler and with `web.file`; `web.file` of 0.8.2 (the
  released binary) reached 244 MB.
- 400 MB received with `http.stream` (a `for` over chunks and a `pipe` to a file): client peak
  26–30 MB on the three engines.

## How it is checked

- `tests/spec/streams.fg`: file streams (lines, chunks, ranges, append, pipe, errors), a
  `web.stream` response, a readable returned from a handler, `web.file`, `http.stream` with
  lines, chunks and `pipe`, a non-2xx status — interpreter, JIT and VM.
- Unit tests of `stream.rs`: chunking, lines, limits, the response queue giving its bytes back.

## Not done

- Value streams (`fogu:stream`: `map`, `filter`, `take`, cancellation over channels) — a
  separate design, because they touch iteration and cancellation in the language.
- Request bodies as streams (`ctx.req` is read whole, within `maxBody`).
- `pipe` into a `TcpStream`: write each chunk (`for c in r { conn.write(c)? }`).
