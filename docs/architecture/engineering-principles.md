# Engineering principles

> **Evolve without losing character. Add without compromising. Improve without losing the essence.**

This document protects the decisions that make Fogu what it is. It complements
the fundamental rule of [FOGU_VISION.md](../../FOGU_VISION.md) (no change may
make performance worse): performance is one property to protect; the
architecture that produces it is another. It is permanent from 0.8.0 on.

## The Fogu Architectural Preservation Principle

Fogu must evolve without losing the principles that define its architecture.
No feature, optimization or change may compromise, without justification,
performance, memory efficiency, isolation, the concurrency model, the
simplicity of the runtime or the identity of the language.

The existing architectural decisions are the foundation of evolution, not
obstacles to remove. Any change to the fundamentals must be technically
justified, measured, documented and validated.

When a feature cannot respect these principles, it is redesigned or left out,
however convenient it would be.

## Four permanent commitments

| Commitment | Purpose |
|---|---|
| **Architectural Preservation** | Keep the identity and the fundamentals of the runtime (below). |
| **Performance Integrity** | No unjustified performance regression (the fundamental rule of `FOGU_VISION.md`). |
| **Compatibility Discipline** | Control incompatible changes and keep releases predictable: what may change before 1.0, deprecation warnings for one minor version, written in the changelog (roadmap #4). |
| **Reliability Evidence** | Tests for reliability, safety, long-running stability (soak tests of servers and workers) and recovery from failures (a worker that panics, a peer that vanishes, a full disk) — run before each release (roadmap #26). |

Fogu does not call itself "production-ready", "enterprise-grade" or similar.
Those words are not evidence. Trust is built, not announced: by keeping what
is documented, what is implemented and what is measured consistent with each
other, release after release, and by writing down the limits. These
principles do not by themselves make Fogu ready for every production
environment; that takes comprehensive tests, documented limitations, error
handling, security, compatibility and real use. They are how it gets there.

## The fundamentals

| Fundamental | What is preserved | Where it lives |
|---|---|---|
| **Runtime** | Fogu programs run on Fogu's own runtime: interpreter (the semantic reference), JIT, scheduler, standard library. Libraries used inside it (Cranelift, tokio for the I/O threads) are implementation details, never part of what a program depends on. | `crates/fogu-core`, `crates/fogu-std` |
| **Concurrency** | Light `fg` tasks, scheduled cooperatively and locally by the thread that created them. | `runtime/sched.rs` |
| **Parallelism** | Workers are real OS threads (`sync.Worker`), one interpreter each. | `fogu-std` `native/base/sync.rs` |
| **Isolation** | Each worker has its own heap of Fogu values, scheduler and reactor. Code may cross workers; state and execution context do not cross implicitly. | `docs/concurrency.md` |
| **Memory** | Values are reference-counted (`Rc`), with Rust's guarantees; no tracing garbage collector or other mechanism without a measured need. Compact values (`Value` is 24 bytes, short strings inline). | `runtime/value.rs`, `runtime/fstr.rs` |
| **I/O** | Local reactors (one per interpreter thread), direct non-blocking system calls, no hand-over per operation on the hot path. | `runtime/reactor.rs`, `native/io/net.rs` |
| **Language** | Its own syntax and semantics: errors as values, no exceptions, no hoisting, one meaning across interpreter, JIT, `fogu:vm` and executables. | `docs/spec.md`, the language reference |
| **Simplicity** | Coherent features; no artificial complexity; one way to do a thing where possible. | — |

These are not changed casually to make room for a feature.

## Five questions for every change

1. Does it respect the current execution model?
2. Does it keep the runtime's isolation and efficiency?
3. Does it keep or improve performance? (Measured against the previous version.)
4. Is the complexity it adds really justified?
5. Is it still coherent with the philosophy and identity of Fogu?

A proposal that fails one of them is revised before it is accepted.

## Three kinds of change

| Kind | Examples | What it needs |
|---|---|---|
| **Normal evolution** — adds capabilities on the existing design | new libraries, APIs, modules, protocols, tools, syntax improvements | the usual: spec tests on both engines, docs, changelog, benchmarks against the previous version |
| **Architectural change** — modifies a fundamental | the scheduler, the memory model, worker isolation, how tasks run, the value representation | a written design note (problem, alternatives, decision) in `docs/architecture/`, measurements before and after, the compatibility impact, and the full test suite; reviewed before merging |
| **Rejected** — compromises the identity | a slower solution, or one that destroys a fundamental property, without a technical benefit that justifies it | it does not enter; it is redesigned or dropped |

## How it is checked

- `cargo test --release` (unit tests, the spec suite with and without the JIT,
  CLI tests) before every commit.
- `bench/run.fg` and `bench/run_net.fg` with `FOGU_PREV=<previous fogu>`: the
  new build and the previous release side by side, with every run recorded,
  the spread and the machine load. Accept / investigate / reject as in
  `FOGU_VISION.md`.
- The published binaries are measured, not only a local build.
- Architectural changes leave a note in `docs/architecture/`, and
  `docs/analysis.md` records what was tried, measured and decided.
