# Fogu 0.8.0 — roadmap and issues

The milestone that turns [FOGU_VISION.md](../FOGU_VISION.md) into the language,
the runtime and the site. Five stages, in order; inside a stage the issues are
independent. Each issue lists its scope and what "done" means. Sources: the
open items of `CHECKLIST.md` and "Known gaps" / "Concurrency audit" in
`docs/analysis.md`.

Labels: `identity`, `language`, `runtime`, `concurrency`, `stdlib`, `pkg`,
`security`, `ffi`, `desktop`, `mcu`, `tooling`, `docs`, `site`, `release`.

Standing rules for every issue: a spec test (with and without the JIT) for
behaviour; docs, `CHANGELOG.md` and the site in the same change; no benchmark of
`bench/` slower than 0.7.5, none above 1.5× of Go.

---

## Stage 1 — Identity & Foundation

**#1 Publish the vision** · `identity` `docs`
Adopt `FOGU_VISION.md`; README opens with the new message and the six domains.
Done: README, `docs/docs.html` intro, `skills/fogu/SKILL.md` and `llms.txt` say the same thing.

**#2 Language reference, written from the code** · `language` `docs`
A reference of what Fogu *is* today (execution model, types and inference, errors as values,
functions, structs, modules, concurrency, memory and resources, compatibility) — separate from
the aspirational `docs/spec.md`. Done: every example in it runs (extracted and executed in CI).

**#3 Audit spec vs code** · `language` `docs`
Walk `docs/spec.md` and `CHECKLIST.md`; mark each item done / partial / not yet; remove claims
the code does not back. Done: no feature is documented that does not exist.

**#4 Compatibility policy** · `language` `docs`
What may change before 1.0, how deprecations are announced (warning in `fogu check` for one
minor version), what a package can rely on. Done: written in the reference and the changelog.

**#5 Known semantic traps** · `language`
Decide and fix or document: `const [v, err] = f()` when `f` returns a list; exports as values,
not live bindings. Done: chosen behaviour has a spec test and a `fogu check` warning where useful.

## Stage 2 — Runtime & Language

**#6 Value representation** · `runtime`
`BoundMethod` / `StructBoundMethod` without inline `String`, a `size_of::<Value>()` test, typed
runtime errors (`code, message, help, line`) instead of `String`. Done: benchmarks equal or better.

**#7 Interpreter: slots instead of hash maps** · `runtime`
Resolver assigns local slots and upvalues; the tree-walker reads slots (the first step of the
bytecode VM in `docs/bytecode.md`). Done: interpreted calls ≥ 30% faster, `--no-jit` suite green.

**#8 JIT coverage document** · `runtime` `docs`
`docs/jit.md` lists exactly what compiles, what bails and why, generated from the bail messages.

**#9 Memory per task** · `runtime` `concurrency`
Below 9 KB per parked `fg` task, or a documented reason why not; lift the ~30 000 task limit
(`vm.max_map_count`). Done: `bench/run_net.fg tasks` shows it.

**#10 Concurrency model, made predictable** · `concurrency`
Cancellation of a task and of its pending operation; `close()` cancels pending `accept` / `read`
of other tasks; a second SIGINT forces exit from a CPU-bound loop; per-thread `IN_FLIGHT` so a
worker cannot hide a deadlock. Done: one spec test per case.

**#11 Backpressure** · `concurrency` `stdlib`
Bounded queues (with a documented policy: block, drop oldest, error) for WebSocket in/out,
SSE, the watcher, `fogumq` / `nats` / `mqtt` subscriptions and the broker. Done: a slow consumer
test that keeps memory flat.

**#12 Network reactor on every platform** · `runtime`
The Linux `epoll` reactor (0.7.5) for macOS (`kqueue`) and Windows (IOCP or `WSAPoll`); TLS and
WebSocket streams on it too. Done: `bench/run_net.fg` within 1.5× of Go at 64 connections on each.

## Stage 3 — Ecosystem

**#13 Standard library conventions** · `stdlib`
One document of argument order, options maps, error style, durations, buffers and sized
numbers; a lint over `fogu-std` exports. Done: every module follows it or has an issue.

**#14 Integration modules, verified** · `stdlib`
For HTTP, SQL, Redis, NATS, Kafka, MQTT, SSH, Docker, mail, LLMs and MCP: a test against a real
service (containers in CI), and the tested versions listed. `foguMQ-client` against a real server.

**#15 fogu-pkg** · `pkg`
Workspaces, `fogu outdated`, a lockfile format version, private git sources, offline installs
from the store. Done: documented in `docs/packages.md` with tests in `tests/cli.rs`.

**#16 Permissions** · `security`
`--allow-net[=host:port]`, `--allow-fs[=path]`, `--allow-run`, `--allow-ffi`, `--allow-device`
and a `"permissions"` section in `fogu.json`; default for `fogu run` decided and documented.
Done: every module that touches the outside checks it; spec tests for denials.

**#17 FFI and embedding** · `ffi`
Direct calls for text and buffer arguments and void results; the `libfogu_rt` C API versioned,
with Go and Bun embedding examples tested in CI.

## Stage 4 — Platforms & Developer Experience

**#18 Desktop** · `desktop`
Stable bridge between the Fogu backend and the web UI; Linux host built in CI; signed Windows
builds; one documented way to package and distribute.

**#19 Microcontrollers: supported means tested** · `mcu`
A support table per board — compiled, emulated, flashed and run on hardware — with the date and
version. Today: AVR (Uno/Nano) and RP2040 (Pico) run in emulators; STM32 and ESP32 build;
Mbed OS output is checked against a mock. Done: no board listed beyond its real status.

**#20 Diagnostics and `fogu check`** · `tooling`
Imported names checked; every runtime error with a code, an excerpt and a hint; `fogu check`
in the editor extension.

**#21 Formatter and tests** · `tooling`
`fogu fmt` stable (idempotence tests over every example), `fogu test` with coverage of Fogu code.

**#22 Install and CI** · `tooling` `release`
CI on Linux / macOS / Windows (fmt, clippy, both engines, minimal features); install scripts
tested; release pipeline from a tag.

**#23 The site, reorganised** · `site`
Home page with the vision and the six domains; navigation Overview · Language · Runtime ·
Libraries · Applications · Devices · Tools · Learn · Community; the 8-lesson tour kept;
performance as a technical page (method, versions, code) instead of a home-page headline.

## Stage 5 — Release

**#24 Reproducible benchmarks** · `release`
`bench/run.fg` and `bench/run_net.fg` with published method, machine, versions and source;
startup, interpreter, JIT, memory, tasks, network, compile time and executable size.

**#25 Release candidate** · `release`
Full suite, regressions against 0.7.5 resolved, `0.8.0-rc.1` published, then `0.8.0`.
