# Fogu — One language. From automation to devices.

This document says what Fogu is for, what it values and how it decides. It
guides the roadmap (see [docs/roadmap-0.8.0.md](docs/roadmap-0.8.0.md)), the
documentation and the official site. When a proposal conflicts with it, either
the proposal changes or this document does — explicitly, in a commit of its own.

## What Fogu is

Fogu is an independent programming language and runtime for **connecting
software, systems, services and devices**. One language — its syntax, its
errors, its concurrency, its standard library — from a configuration script to
a service, a desktop tool or the firmware of a board.

Fogu is not positioned as a replacement for Rust, Go, Python or JavaScript.
It is for the work *around* the code people already have: reading and checking
configuration, automating operations, gluing APIs and databases together,
administering hosts, shipping small tools, and talking to hardware — the work
that today is split between shell, Python, YAML, a bit of Go and a vendor SDK.

The name comes from **Fôgu** — *fire* in Forro, the creole of São Tomé and
Príncipe.

## The six domains

These organise the identity, the documentation and the site. They describe
what Fogu already does; they are not six projects.

| Domain | What it covers | Where it lives today |
|---|---|---|
| **Configuration** | Read, validate, transform and write data and environments | `fogu:formats` (YAML, TOML, JSON, CSV, ENV), `fogu:types`, `fogu:fs`, `fogu play` |
| **Automation** | Tasks, processes and workflows | `fg`, `chan`, `select`, `fogu:shell`, `fogu:process`, `fogu:cron`, `fogu:time` |
| **Integration** | APIs, databases, queues and services | `fogu:http`, `fogu:web`, `fogu:sql`, `fogu:redis`, `fogu:nats`, `fogu:mqtt`, `fogu:kafka`, MCP |
| **Infrastructure** | Hosts, containers and services | `fogu:ssh`, `fogu:docker`, `fogu:service`, playbooks, `fogu build --service` |
| **Applications** | Tools, services and desktop apps | `fogu build --compile` (standalone executables), `fogu:web`, the `desktop` library |
| **Devices** | Microcontrollers and hardware | the `mcu` library (AVR, RP2040, STM32, ESP32, Mbed OS), `fogu:serial`, `fogu:modbus`, `gpio` |

Each domain is documented with the boards, systems and services that are
**actually tested** — nothing is listed as supported on a promise.

## Principles

1. **One coherent language.** A feature belongs in Fogu when it is the same
   idea everywhere it appears: errors are values in a script, a server and on a
   board; `fg` tasks behave the same over sockets and over pins.
2. **Errors are values.** `return value, error(msg)`, `const [x, err] = f()`,
   `expr?`. No exceptions. Programming mistakes stop the program; operational
   failures (I/O, network, data) are values the program decides about.
3. **Concurrency you can read.** Threads for parallelism across cores
   (`sync.Worker`), light tasks for concurrency at scale (`fg`), channels,
   `select`, timeouts and cancellation (`docs/concurrency.md`). Predictable before clever: no data races in
   Fogu code, deadlocks reported, every wait can be bounded.
4. **Batteries that agree with each other.** The standard library is one
   design, not a collection: the same argument conventions, the same error
   style, the same types (`buffer`, durations, sized numbers) across modules.
   Fewer modules that work together beat more modules that do not.
5. **The interpreter is the meaning.** Every way of running a program — the
   interpreter, the JIT, `fogu:vm`, a standalone executable, a worker — gives
   the same output and the same errors. The interpreter is the reference.
6. **Honest about what works.** The specification may be ahead of the code;
   the documentation may not. Status lives in `CHECKLIST.md`, limits are
   written down (`docs/analysis.md`, "Known gaps").
7. **Performance is a property, not the pitch.** Fogu is fast because the
   work it does — starting a script, waiting on many sockets, a hot loop —
   should not need another language. Internally the bar stays where it is:
   every program of `bench/` within 1.5× of its Go version, and no release
   slower than the one before (`CLAUDE.md`). Publicly, numbers are technical
   information with their method, versions and code, not a headline.

## The fundamental rule

> **No feature, change or improvement may compromise Fogu's performance, efficiency or
> architectural integrity.** First, keep what works well. Then improve without compromising.
> A new feature is progress only when it adds value without sacrificing the fundamental
> qualities of the language.

This rule is permanent, from 0.8.0 on, and applies to every change — features, fixes,
refactors, dependencies.

**Every change is checked against:**

1. **Performance** — no regression in execution time, throughput or latency.
2. **Memory** — no needless consumption, copies, allocations or duplicated state.
3. **Concurrency** — the efficiency of workers, `fg` tasks, schedulers and reactors is kept.
4. **Architecture** — no added complexity or global synchronization without a reason.
5. **Safety** — the guarantees and the isolation of the runtime are kept.
6. **Simplicity** — no complex mechanism where a simpler, efficient solution exists.

**Deciding:**

| Result | Decision |
|---|---|
| Performance improves or stays the same | **Accept** — the change adds value without costing the existing properties |
| A measurable regression | **Investigate** — analyse, optimise, measure again under controlled conditions |
| The regression cannot be removed | **Reject** — the change does not enter the release, however convenient |

**Proof, not claims.** Every relevant change is compared with the previous version through
benchmarks (`bench/run.fg`, `bench/run_net.fg` with `FOGU_PREV`) and regression tests
(`cargo test --release`, both engines). Saying an implementation is faster is not enough: the
measurement shows it, with its runs, spread and machine load.

The architecture that produces these properties is protected the same way:
[docs/architecture/engineering-principles.md](docs/architecture/engineering-principles.md)
— the fundamentals (runtime, concurrency, parallelism, isolation, memory, I/O,
language, simplicity), five questions for every change, and what an
architectural change requires.

## What Fogu is not

- **Not a systems language.** No manual memory management, no unsafe code in
  Fogu programs. Rust is the foundation of the implementation, not part of the
  identity.
- **Not a garbage-collector project.** Values are reference-counted. A tracing
  collector is added only if a measured, real program needs it.
- **Not everything for everyone.** A module, a target or a board enters the
  standard set when it fits the domains above and can be tested.

## How the runtime is shaped

- **Tree-walking interpreter** — the semantic reference.
- **Cranelift JIT** — numeric and loop-heavy code, with speculation and
  deoptimization back to the interpreter; identical results.
- **Cooperative scheduler** — `fg` tasks on their own stacks; on Linux the
  interpreter thread waits on its sockets itself (an `epoll` reactor).
- **Isolation** — `fogu:vm` contexts with deadlines that also stop compiled
  code; `sync.Worker` for other cores.
- **Distribution** — standalone executables (`fogu build --compile`), an
  embeddable runtime (`libfogu_rt`, C ABI), WebAssembly (the site's tour).

## How decisions are made

- A change that users can see updates the documentation, `CHANGELOG.md` and
  the site in the same release.
- A new feature comes with a test in `tests/spec/` that runs with and without
  the JIT, its expected output written by hand.
- A dependency of the runtime must be pure Rust (existing ones stay; unused
  ones are removed).
- Before a release: the full test suite, the benchmarks against the previous
  release and Go, and the published binaries measured — not a local build.

## Public message

> **Fogu — One language. From automation to devices.**
>
> Fogu is an independent programming language and runtime for connecting
> software, systems, services and devices. From configuration and automation
> to infrastructure, applications and embedded development, it gives one
> coherent way to build and integrate solutions across domains — with its own
> language design, execution model and concurrency, built on Rust.
