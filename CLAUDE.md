# CLAUDE.md

Guide for working in this repository: the reference implementation (Rust) of
the **Fogu** language (`.fg`) — tree-walking interpreter + Cranelift JIT, CLI,
REPL, git-based package manager and standalone executables.

**Everything is in English**: documentation, error messages, CLI output, code
comments, examples and tests. Keep it that way (the user may talk to you in
Portuguese; the project itself is English). Author: Edmilson Soares (MIT).

## Commands

```bash
cargo build --release                 # binary in target/release/fogu(.exe)
cargo run -- examples/basics.fg       # run a file
cargo run -- --no-jit file.fg         # interpreter only (semantic reference)
cargo run -- --jit-debug file.fg      # what the JIT compiles/rejects (stderr)
cargo run -- check examples/*.fg      # syntax, names, types (+ warnings)
cargo run -- build --compile          # standalone executable in dist/
cargo run -- build --compile --target linux-arm64   # other platforms (fogu-<target> runtimes)
docker build -f docker/Dockerfile --output dist .  # the runtimes: linux x64/arm64/armv7, windows
cargo run -- test                     # run *.test.fg files (fogu:test)
cargo run -- fmt --check              # formatting (toolkit/fmt.rs)
cargo run -- play site.yml -i inv.yml # playbooks (fogu-std native/ops/playbook.rs)
cargo ffi                             # libfogu_rt .so/.dll/.dylib (ffi/, header ffi/fogu.h)
cargo run --                          # REPL
cargo test --release                  # unit tests of every crate + tests/spec (with and without JIT)
cargo build --release --no-default-features --features network   # a smaller binary (features below)
cargo build --release --target aarch64-unknown-linux-gnu        # ARM64 from x86_64 (OpenSSL vendored)
cargo build --profile profiling --features pprof && FOGU_PPROF=1 target/profiling/fogu --no-jit x.fg   # hottest functions at exit (no root)
FOGU_BLESS=1 cargo test --release     # regenerate the .out files (review the diff!)
```

**Tests**: `tests/cli.rs` covers `init`, `test` and `build --compile` on temp
projects. `tests/spec.rs` runs each `tests/spec/<name>.fg` with and without
the JIT (low thresholds, to force compilation) and compares stdout with
`<name>.out`; if `<name>.err` exists the program must fail and stderr must
contain every line. `_name.fg` are helper modules; `*.tmp.*` data files are
deleted after each test. Every new feature gets a test here, with the expected
output written by hand (not blessed without review). Network/HTTP/web examples
(`examples/`) loop forever — run them in the background and test with a client
on `127.0.0.1`.

Environment: `FOGU_HOME` (default `~/.fogu`), `NO_COLOR`, `FOGU_JIT=0`,
`FOGU_JIT_DEBUG`, `FOGU_JIT_CALLS` (default 2), `FOGU_JIT_LOOPS` (default 200),
`FOGU_DOTENV=0` (skip loading `./.env`, done by `toolkit::load_dotenv` at startup).

## Architecture (Cargo workspace)

- `crates/fogu-core` — **the language**. `lib.rs` defines `io_err!` and the
  modules `runtime`, `native`, `host`.
  - `runtime/`: `lexer.rs`, `token.rs`, `parser.rs`, `ast.rs` (errors always with
    `line:column`); `resolver.rs` (static checks before running: names, types, `?`,
    `match`, unused-variable warnings); `interpreter.rs` — **semantic reference**;
    `environment.rs` (lexical scope, FxHash, each top-level env knows its module;
    loop bodies reuse their scope via `LoopScope` unless a closure captured it);
    `value.rs` (`Value` uses `Rc<RefCell<..>>` → **not `Send`**); `fstr.rs` (`Str`, the
    string of `Value::Str`: Rc copy-on-write, O(1) clone, cached char count —
    `s = s + x` appends in place); `ops.rs` (checked arithmetic and **the error
    messages shared** by the interpreter and the JIT); `jit.rs` (Cranelift; only the
    numeric subset — int/float/bool, if/while/for a..b, match, JIT-able calls, basic
    `math.*`; see `docs/jit.md`); `sched.rs` (cooperative `fg` tasks on corosensei,
    pooled stacks — 32 MB virtual, 2 MB on 32-bit —, wake-ups from I/O threads,
    channels, deadlock detection, `timeout`, `--strict`); `module.rs` (loading/cache:
    `./x.fg`, `fogu:*`, packages).
  - `native/`: what every program has — `object.rs` (`FoguObject`, `NativeObjectRef`),
    `function.rs`, `context.rs` (`NativeContext`: the language globals `print len close
    timeout typeof str int float clock bytes`, the lazy module registry; the host adds
    the rest), `collections.rs` (list/map/str methods, `error`/`panic`), `buffer.rs`
    (the `buffer` type: `bytes(...)` and its methods), `math.rs`, `json.rs`, `test.rs`,
    `events.rs`, `rt.rs` (the shared tokio runtime), `util.rs` (`nfn_var`, `str_arg`,
    `map`, `bytes_of`…).
  - `host.rs`: the `Host` trait the standard library implements — `install` (modules and
    the `Fogu` global), `intercept` / `intercept_method` (calls that need the interpreter:
    `fogu:vm`, servers running Fogu handlers, `cron.tick`, `Agent.run`…) and
    `before_call` (the HTTP server serves queued requests). The interpreter never names
    a stdlib module.
- `crates/fogu-std` — the standard library, `src/native/<area>/`: `base` (time sync cron
  log regex crypto vm `fogu_global` signal host cli ffi + the generated `ffi_calls.rs`),
  `io` (fs path process system shell watch zip formats net `ws_client`), and the
  feature-gated areas `network` (http http_client web websocket socketio topcoat jwt
  html mail security dns), `database` (sql orm kv surrealkv surrealdb scylla redis
  etcd + `proto/`), `messaging` (nats mqtt kafka rabbitmq amqp bullmq fogumq
  `foguMQ-client.rs` broker broker_admin), `ops` (ssh ssh_mount fs_remote docker
  service playbook s3), `hardware` (serial modbus usb hdmi), `aiml` (ai ai_mcp
  mcp_client ml array dataframe plot face; feature `ai`), `media` (image webview), `rpc`
  (grpc protobuf). `native/mod.rs` re-exports every module (and the core ones), so
  paths stay `crate::native::<name>`; `host.rs` implements `Host`; `register()` installs
  it.
- `crates/fogu-pkg` — package manager: `source.rs` (git/tar/folder sources), `semver.rs`,
  `install.rs` (global store + `fogu.lock` with sha256), `net.rs` (the blocking HTTP
  client, also re-exported by `util`).
- Root package `fogu` — the CLI: `src/lib.rs` re-exports `runtime`, `native`, `pkg`,
  `host`, `register`; `main.rs` only calls `fogu::toolkit::run()` (which calls
  `register()`); `embed.rs` (the runtime of `ffi/`) registers too. `toolkit/` — Bun-style
  CLI (`mod.rs`: flags and dispatch; `commands.rs`: `run/init/add/remove/install/update/
  list/pm/check`; `build.rs`: `build --compile` and running an embedded bundle;
  `testing.rs`: `fogu test`; `diag.rs`: diagnostics; `repl.rs` with rustyline; `ui.rs`).
- Features (root, forwarded to fogu-std): `network database messaging ops hardware ai
  media rpc`, all default; `database` enables `rpc`, `ai` enables `media` and `network`.
  A gate is `#[cfg(feature = "...")]` in fogu-std; check
  `cargo check --no-default-features [--features x]` and `--target wasm32-wasip1`
  after touching registrations or cross-area calls.

## Conventions and pitfalls

- **The interpreter is the source of truth.** Any new JIT behaviour must produce
  exactly the same output and error messages (`file:line: runtime error: ...`).
  When in doubt, bail in the JIT.
- New AST constructs must be handled in `jit.rs` (`bail`, plus
  `expr_name`/`stmt_name`/`collect_names`) **and** in `resolver.rs`.
- Desugarings in the parser: default parameters are a `p = p ?? value` prologue;
  parameter annotations a `Stmt::CheckType` prologue; the return annotation is a
  `Stmt::ReturnType` that must be the **first** statement of the body
  (`Interpreter::run_body` checks it). Variadic params are stored as `...name`
  (`ast::rest_param`) and the JIT refuses them. Generic parameters are erased
  to `TypeExpr::Any`.
- Control sentinels travel as errors (`PROPAGATE` for `?`, `OPT_NIL` for `?.`):
  never let them reach the user.
- Declarations use `Interpreter::declare` (error if the name already exists in
  the same scope); `Environment::define` is for internal bindings (params,
  loops, imports).
- Errors are values: `return value, error(msg)`, `const [x, err] = f()`, `expr?`.
  In native functions, **operational** errors (IO, network, SQL, JSON) use
  `io_err!(...)` (becomes an `error` value); programming errors (wrong argument
  type) use `format!` and stop the program.
- Error message formats: `file: line L:C: <kind> error: msg` (kinds: lexical,
  syntax, name, type) or `file:L: runtime error: msg`; `toolkit::diag` parses
  them and adds code/excerpt/help (the code table is `diag::classify`).
- Native methods that must *block* (e.g. `Mutex.lock`) implement
  `FoguObject::wait_for`, returning a condition over shared `Rc<Cell<…>>`
  state; the interpreter waits for it cooperatively before calling the method.
  Never wait inside `call` (the object is borrowed there). Network operations
  implement `FoguObject::start_async`, returning a poller the interpreter
  calls cooperatively until it yields the result (`net.rs`: `rt::spawn` a future,
  the poller takes its `Pending`). A poller that waits for data from elsewhere
  holds a `sched::IoGuard` (it is not a deadlock; the producer calls `sched::wake()`)
  and only `add_timer`s its real deadline — no short polling timers
  (`websocket::wait_poller`).
- `rt.rs` (fogu-core): the **one shared tokio runtime** (`rt::tokio()`, used by sql,
  scylla, bullmq and the http/web server — never build another) and a pool of
  blocking threads. `util::thread_poller` runs on that pool. In *module
  functions* (no object borrowed) use `rt::offload(work)` / `rt::block_on(fut)`:
  the caller waits cooperatively, or runs inline when there is no other task.
  Work off the interpreter thread holds a `sched::IoGuard`; dropping it calls
  `sched::wake()`, so the scheduler sleeps on a condvar instead of polling.
- Since `Value` is not `Send`, modules with threads (`http`, `web`, `cron`)
  never call Fogu code off the main thread: they talk through channels. The HTTP
  server sets an atomic flag and wakes the scheduler; `Host::before_call` and a pump
  task (`http::serve_pending`) start each request's handler in its own `fg` task, so
  requests are served even while the program waits in `time.delay` or on a channel.
- Binary data is a `buffer` (`Value::Buffer`): return `Value::buffer(vec)` /
  `util::bytes_value`, accept `util::bytes_of` (buffer, str or list of byte ints).
- Strings: `Value::Str(Str)`; build with `Value::str(x)` / `x.into()`, read through
  `Deref` (`&str`, `&String`); `.to_string()` / `.into_string()` to own a `String` (a
  `Str` is `Rc`, never move one into another thread).
- New native module: create `crates/fogu-std/src/native/<area>/<name>.rs` with
  `pub fn build() -> ModuleValue`, declare it in `<area>/mod.rs`, re-export it in
  `native/mod.rs` and register it in `install` (fogu-std `host.rs`), with the area's
  `#[cfg(feature)]` gate.
- Map iteration (and `json.stringify`) is ordered — do not break that. Native code that
  walks a `Value::Map` (a `HashMap`) itself must sort the keys (e.g. HTML attributes).
- `fogu:topcoat` (`native/network/topcoat.rs`, assets in `native/network/topcoat/`: `runtime.js`, `ui.css`,
  `shadcn.css`, `shadcn.rs`): `fogu:web` serves `/_tc/*`, runs shards/procedures and page
  reruns in `web::dispatch`; `shard.render` is intercepted in `call_value`. The broker's
  admin UI is Fogu code (`native/messaging/broker_admin.fg`, run by `broker_admin::intercept`).
- `fogu:vm` runs code on the same interpreter: its calls are intercepted at the top of
  `Interpreter::call_value` (through `Host::intercept`, `native::vm::intercept`), and `Interpreter::vm_run` sets a
  `deadline` (checked in loops and calls; the JIT is skipped while it is set).
- Packages: never run install scripts; always verify the lock checksum.
- Source files use CRLF line endings; keep them — except `scripts/*.sh` (LF: shells refuse CRLF).
- `scripts/install.sh` / `install.ps1` install Fogu and set the PATH (test install.sh with a
  temporary `HOME`; do not run install.ps1 against the real Windows PATH).
- Benchmarks: `bench/run.fg` (compute) and `bench/run_net.fg` (servers, concurrency) write
  `bench/results*.json`; `docs/tools/gen_bench.py` turns them into `docs/benchmarks.md`, the
  docs.html section and the docs/view page. Measure on a quiet machine (the runners record the
  load); time with `time.monotonic()`, not `clock()`.

## Documentation

- `docs/spec.md` — the language **specification** (Draft 0.4). It is
  aspirational: permissions `--allow-*`, `fogu test/fmt/audit`, the bytecode VM
  and more **do not exist yet**. The real status is in `CHECKLIST.md`; check the
  code before assuming anything.
- `docs/cli.md`, `docs/packages.md`, `docs/jit.md` — reflect the current state.
- `docs/analysis.md` — history of fixes and **known gaps**; `CHANGELOG.md` —
  changes per version. Update both.
- `docs/docs.html` — official user documentation (HTML + Tailwind); keep its
  examples in line with what the language really does (extract and run them).
  The standard library reference inside it is **generated**: edit
  `docs/tools/reference/*.py` and run `python docs/tools/gen_reference.py` (it runs
  every example marked `run: True` and shows the real output; `--check` only runs them).
- `docs/view/` — the documentation site (Vite + React). Keep it current with every code
  change. Module sections rendered by `RefSection` come from
  `src/data/reference.generated.ts`, written by `gen_reference.py` (do not edit it);
  the other pages are hand-written TSX. Check with `bun run build`.
- `docs/design/*.md` — design notes of the native modules (may be outdated).
- `README.md` — overview for users; keep it in sync with the CLI.
- `skills/fogu/` — the skill that teaches AI agents to use Fogu: `SKILL.md` is
  written by hand (update it when the language or CLI changes and run its
  examples); `stdlib.md` is generated by `gen_reference.py`. `llms.txt` points to them.
- `CHECKLIST.md` — status of every feature of the spec and the plan by phase;
  mark `[x]` when something is implemented.
