# Design note: streams of values (`fogu:stream`)

Status: implemented (0.8.3; WebSockets and `pipe` in 0.8.4) · `crates/fogu-core/src/native/vstream.rs`.
Follows `engineering-principles.md`: this note, measurements, tests on the three engines.
The streams of bytes of `fs`, `http` and `web` are in `streams.md`.

## Problem

Fogu has tasks and channels for communication between tasks, but no way to describe *a
sequence of values consumed a piece at a time*: transforming the lines of a large file, the
readings of a sensor or the results of a paginated API meant either a list (all of it in memory)
or a hand-written loop with its own buffering, ending and error handling. What was asked for:
backpressure, finite and infinite streams, ending and errors, cancellation, composition,
integration with `fg` and channels, bounded memory — never accumulating everything.

## Decision

1. **Pulled, not pushed.** A stream is a chain of stages; `map`, `filter`, `take`, `skip`,
   `takeWhile`, `chunk`, `tap` and `enumerate` return a new stage and do nothing until the
   consumer asks for a value. Each request walks up the chain once. So a chain holds one value
   per stage, an infinite source is fine, and the consumer sets the pace — that *is* the
   backpressure, for every stage, with no buffer to size.
2. **One buffer, where tasks meet.** `stream.create({ buffer })` is the bridge from tasks that
   push values: `write` waits (cooperatively) while `buffer` values are unread, the consumer
   waits while there are none. It is the only place values queue.
3. **No hidden tasks.** The operators run on the consumer's task, when it pulls. Nothing is left
   blocked when a consumer stops early, there is no task per stage to schedule, and the order of
   effects is the order of the program.
4. **Ending and errors.** A source ends (`close()`, the end of a list or range, `nil` from a
   generator). `fail(err)` — or an error value returned by a function of the chain
   (`return nil, error(...)`) — ends the stream with that error: `for` stops and `s.error` has
   it; the terminals (`toList`, `reduce`, `count`, `forEach`, `first`, `find`) return it as
   their error. A runtime error in a function of the chain stops the program, as anywhere.
5. **Cancellation goes upstream.** `cancel()` ends a stage and every stage before it: a writer
   of a `create` stream gets an error value (`stream: cancelled by its consumer`) instead of
   waiting forever, a file or HTTP stream is closed. `take(n)`, `takeWhile`, `first` and `find`
   cancel their upstream once they have what they need.
6. **Sources from what exists.** `stream.from` takes a list, a range, a text, a channel, a
   readable of `fs` / `http` (so lines of a file are a stream), a WebSocket (its messages until it
   closes; `for m in ws` is the same pull) or a stream; `stream.of(...)`,
   `stream.range(start, end?, step?)` (no end: infinite), `stream.generate(fn(i))` (until it
   returns nil), `stream.interval(d)`.
7. **Sinks are a terminal, too.** `s.pipe(dest, { end = true })` pulls every value and hands it
   to a chan (`chan_send`: it waits while the chan is full — the chan's buffer is the
   backpressure), a WebSocket (`send`) or a writable (`write`), then closes `dest` unless
   `end: false`. It runs on the consumer's task like every terminal: no task is started, and
   an error of `dest` ends the stream with it and cancels upstream. So channels and sockets
   meet through streams in both directions without a new primitive: `stream.from(ch).pipe(ws)`,
   `stream.from(ws).map(f).pipe(ch)`.
8. **A module, not the core.** The core keeps its primitives (tasks, channels, the iteration of
   `for`); `fogu:stream` is a module built on them. `for` and the method calls know a stream
   only through two calls (`Interpreter::stream_next`, `call_bound`), like the other objects
   that need the interpreter.

## Alternatives considered

- *A task per stage, connected by channels*: concurrency the program did not ask for, a buffer
  to size at every stage, and tasks left blocked (or sends on closed channels, which stop the
  program) when the consumer stops early.
- *Operators on lists* (`xs.map(f).filter(g)`, as today): builds every intermediate list —
  the measurement below.
- *Writing the module in Fogu*: possible, but `for x in s` over a Fogu value needs an
  iteration protocol for structs, a change to the language that is not needed for this.

## Measured

A chain of 2 million values, `map` then `filter` then a count (lite build):

| | time | peak RSS |
|---|---:|---:|
| lists (`xs.map(…).filter(…)`) | 0.62 s | 214 MB |
| `stream.range(0, 2000000).map(…).filter(…).count()` | 0.56 s | 11 MB |

The lines of a 200 000-line file, filtered, parsed and summed through a stream: 0.21 s, 11 MB.

## How it is checked

`tests/spec/value_streams.fg` on the interpreter, the JIT and the VM: the example of the
request, an infinite chain with `take`, a producer task and a consumer with a buffer of 2 (the
buffer never holds more), every terminal and operator, errors from a function of the chain and
from `fail`, a writer stopped by cancellation, `interval`, a channel and a file as sources.
