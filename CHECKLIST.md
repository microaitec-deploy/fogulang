# Fogu specification checklist

Status of every feature of the [Draft 0.4 specification](docs/spec.md) in the
current code (v0.7.0), followed by the implementation plan by phase.
Every new `[x]` item must have a test in `tests/spec/` (`cargo test`).

Legend: `[x]` done · `[~]` partial · `[ ]` to do. `§` refers to the section of the specification.

---

## 1. Lexical structure and literals (§4–7)

- [x] UTF-8 source, whitespace, optional `;`
- [x] Comments `//` and `/* */` (error if unterminated)
- [x] Identifiers (`_`, letters, digits; case sensitive)
- [~] Keywords — `interface` added; `panic` is a global function, not a keyword (`new`/`this` are handled by the parser)
- [x] Integers `42`, `1_000_000`, `0xff`, `0b1010` (64-bit int)
- [x] Floats `3.14`, `1e-9`
- [x] Strings with `"…"` and `'…'`
- [x] Template strings `` `Hello, ${name}` `` (§7.4) — `template.fg`
- [x] `true`, `false`, `nil`
- [x] Duration literals `500ms 2s 5m 1h` (also `ns us d` and `1.5s`) (§7.7) — `durations.fg`

## 2. Types and values (§8–12)

- [x] `nil bool int float str list map fn struct chan error`
- [x] `buffer` type (mutable bytes, not in the spec): `bytes(x, encoding)`, `b[i]`, `for`, `==`, `read*/write*` numbers, `buffer` annotation — `buffer.fg`
- [x] `duration` type: arithmetic, comparison, fields `ns us ms seconds minutes hours`; accepted by `time.delay/sleep`, `cron.every`, `timeout`
- [~] `tuple`: `return a, b` and `const [x, err] = f()` work, but there is no separate immutable `tuple` type
- [~] `typeof` — also returns `range`, `module`, `object`, `struct-type`, `interface`, which are not in the spec
- [x] Mutable lists: `push`, `pop`, `len` and more
- [x] Maps with `str` keys, `.x` and `["x"]` access, missing key → `nil`
- [x] Optional types `str | nil`, `int?` — checked at runtime — `types.fg`
- [x] Type annotations `a: int`, `: int`, `let x: T` checked at runtime (§9); unknown type names rejected before running — `types.fg`, `error_type_*.fg`, `error_unknown_type.fg`
- [x] No implicit coercions; `int()`, `float()`, `str()`; `int → float` promotion
- [x] Truthiness: only `false` and `nil` are falsy
- [x] Structural `==` for lists and maps
- [x] Structs compare by identity, or with an `equals(other)` method (§12) — `equals.fg`

## 3. Variables and scope (§13)

- [x] `let` / `const`; `const` does not freeze the object
- [x] Lexical block scope
- [x] Redeclaration in the same scope is an error (native functions may be redefined; the REPL allows redeclaring) — `error_redeclaration.fg`
- [x] Use before declaration and undefined variables detected **before running**, with a "did you mean" suggestion — `resolver_ok.fg`, `error_before_declaration.fg`, `error_undefined.fg`

## 4. Operators and expressions (§14–17)

- [x] Arithmetic, comparison, logic, right-associative `**`
- [x] Compound assignment `+= -= *= /= %=` (also to fields and indices)
- [x] Checked arithmetic (overflow, division by zero); NaN
- [x] Optional access `?.`, short-circuiting the whole chain (§15) — `optional.fg`
- [x] Coalescing `??` (§16)
- [x] Half-open ranges `a..b` (§17)

## 5. Control flow (§18–19)

- [x] `if/else` without parentheses
- [x] `for` over ranges, lists, strings and maps; `for k, v in map` and `for i, x in list` — `for_pairs.fg`
- [x] `while`, `break`, `continue`
- [x] `match` as an expression, with `|`, `if` guards and `_`
- [x] `match` exhaustiveness checked before running (a `_`/name arm, or `true` and `false`) — `error_match.fg`

## 6. Functions (§20–22)

- [x] `fn` declarations, recursion, closures, first-class functions
- [x] Anonymous functions `fn(x) { … }`
- [x] Arrow functions `(x) => x * 2` and `(x) => { ... }` (§21) — `arrow.fg`
- [x] Parameters with default values, also in methods and constructors (§20) — `params.fg`
- [x] Variadic functions `...args` (§22) — `params.fg`

## 7. Structs, composition and interfaces (§23–27)

- [x] `struct` with fields, default values and methods; `new`, `this`
- [x] `new(...)` constructor
- [x] Composition with `embed` (fields and methods)
- [x] Structural `interface` (§25): structs (and maps of functions) satisfy it when they have the methods — `types.fg`, `error_interface.fg`
- [x] Visibility (§26): `_x` private in modules (imports, `module._x`) and structs (fields and methods, read and write) — `error_private*.fg`, `private_struct.fg`
- [x] `export struct` / `export interface` to share types between modules
- [~] Generics `fn f<T>`, `struct Stack<T>`, `new Stack<int>()`, bounds `T: Interface` (§27) — parsed and erased at runtime; generic arguments and bounds are not checked — `types.fg`

## 8. Errors (§28–31)

- [x] Errors as values; no `try/catch`
- [x] `error(msg, cause)` with `.message`, `.cause` and `.trace`
- [x] `?` propagation operator
- [x] `?` only in functions whose declared return type includes `error` (checked before running) — `error_try_no_error.fg`; at the top level it terminates with E0206
- [x] Operational errors of native functions (IO, network, SQL, JSON, HTTP) are `error` values; programming errors still stop the program — `native_errors.fg`
- [x] `panic(msg)`
- [x] `panic` in a task does not stop the others; `--strict` terminates the process (§31) — `task_panic.fg`

## 9. Concurrency (§32–37, §57, §68)

- [x] `fg` — cooperative tasks (corosensei), with deadlock detection
- [x] `chan(n)`, `c <- v`, `<-c`, `close`, `len`
- [x] `chan<int>(buffer: 1)` syntax (§34) — `concurrency.fg`
- [x] `select` with `case`, `default` and a random choice among ready cases
- [x] `timeout(1s)` in `select` and `<-timeout(d)` (§36) — `timeout.fg`
- [ ] Value transfer model between tasks (§35)
- [x] `fogu:sync` — `Mutex`, `Atomic`, `WaitGroup`, `Once` (§37); blocking operations yield cooperatively and deadlocks are reported — `sync.fg`, `error_mutex_deadlock.fg`
- [ ] M:N multi-threaded scheduler, `FOGU_THREADS`, I/O that releases the thread (§57)
- [ ] Data race prevention (§68)

## 10. Modules and packages (§38–41)

- [x] `import { a, b as c } from "./x.fg"`, `import m from …`, `export`
- [x] `fogu:*` modules
- [x] Evaluated once per process, with a cache; cycles are an error
- [x] `fogu.json` and `fogu.lock` (commit + sha256)
- [x] No install scripts; integrity check
- [~] git, tar and folder sources work; there is no central registry, so `"^1.2.0"` alone does not resolve
- [ ] Signed packages (§41)
- [ ] `fogu audit` (§41)

## 11. Permissions and security (§42, §66–67)

- [ ] No permissions by default
- [ ] `--allow-net[=host:port]`, `--allow-fs[=path]`, `--allow-run`, `--allow-ffi`, `--allow-device`
- [ ] `"permissions"` section in `fogu.json`

## 12. Runtime and standard library (§43–53)

Fundamental API (§44):

- [x] `print len int float str typeof error panic`, plus `close`, `clock` and `timeout`

`Fogu.*` global object (§45) — `fogu_global.fg`:

- [x] `Fogu.file(path)` — `text lines json write append exists size remove`, fields `path name ext dir`
- [ ] `Fogu.serve()` and `Fogu.reply()` (use `fogu:http` / `fogu:web`)
- [x] `Fogu.env` (map) and `Fogu.args` (list), plus `Fogu.version`, `Fogu.os`
- [x] `Fogu.sleep()`, `Fogu.exit()`, `Fogu.hash()` (sha256/sha512), `Fogu.glob()`
- [x] `Fogu.watch()` (`Fogu.watch(paths).next()` events)
- [x] `Fogu.sql()` (same as `sql.open`)
- [x] `Fogu.run()` — external processes (§52)

`fogu:*` modules (§46–53, §50):

| Status | Module |
| --- | --- |
| `[x]` | `fogu:math`, `fogu:json`, `fogu:time`, `fogu:net` (TCP/UDP), `fogu:http`, `fogu:cron` |
| `[x]` | extras not in the spec: `fogu:system`, `fogu:events`, `fogu:kv`, `fogu:surrealkv`, `fogu:sql`, `fogu:web` |
| `[~]` | `fogu:net` without TLS; `fogu:http` without HTTP/2 or the `http.serve({ port, fetch })` form |
| `[~]` | `fogu:cron` requires calling `cron.tick()`; it does not run in the background |
| `[x]` | `fogu:fs`, `fogu:path` (`fs_path.fg`), `fogu:process` (`process.fg`), `fogu:sync` (`sync.fg`), `fogu:test` (`tests/cli.rs`) |
| `[x]` | extras: `fogu:orm` (Prisma-style ORM, `orm.fg`), `fogu:jwt` (`jwt.fg`), web middleware (`web_middleware.fg`), `fogu:ssh` (ssh/scp/tunnels), `fogu:scylla`, `fogu:fogumq` — the last three tested by hand against real servers |
| `[ ]` | `fogu:context` |
| `[x]` | `fogu:crypto`, `fogu:regex`, `fogu:log` (`regex_crypto_log.fg`), `fogu:yaml`, `fogu:toml`, `fogu:csv`, `fogu:excel`, `fogu:xml` (`formats.fg`) |
| `[x]` | `fogu:websocket`, `fogu:mail` (SMTP + HTTP APIs, `mail.fg`), `fogu:ai` (`ai.fg`), `fogu:shell` (`shell.fg`), `fogu:s3` (`s3.fg`) |
| `[x]` | DNS lookups — in `fogu:security` (`dns`, `reverseDns`, hickory) |
| `[x]` | `fogu:mqtt`, `fogu:nats` (+ JetStream), `fogu:redis`, `fogu:rabbitmq` (AMQP) — tested by hand against real servers |
| `[x]` | `fogu:kafka` (rskafka; `kafka.fg` offline, tested by hand against Kafka 3.9) — no consumer groups |
| `[x]` | `fogu:ffi` (§51, `ffi.fg`; without `--allow-ffi` yet), `fogu:playbook` + `fogu play` (Ansible-style, `playbook.fg`) |
| `[x]` | `fogu:bullmq` (BullMQ v5 compatible, `bullmq.fg`), `fogu:vm` (`node:vm` style, `vm.fg`), `fogu:cli` (prompts, styles, spinners, tables, `clap` arguments; `cli.fg`) |
| `[x]` | serial, Modbus, USB, HDMI-CEC moved to `libs/hardware` (over `fogu:ffi`) |
| `[x]` | `fogu:html` (`html.fg`), `fogu:image` (`image.fg`), `fogu:plot` (`plot.fg`); arrays, data frames and ML moved to `libs/datascience` |
| `[x]` | `fogu:webview` — tested by hand (browser, display); face detection moved to `libs/face` |
| `[x]` | `fogu:docker` (`docker_files.fg`; live commands tested by hand), `fogu:service` (`service.fg`), `fogu:zip`, `fogu:protobuf`, `ssh.mount`, `fs.remote` |
| `[x]` | `fogu:grpc` (`grpc.fg`), `fogu:socketio` (`socketio.fg`; official JS client/server by hand), `fogu:security` (`security.fg`), `fogu:broker` / `fogu:mq` (`broker.fg`; Bun/Go/Rust/Python clients by hand; SurrealKV storage, `b.admin` web UI), `fogu:topcoat` (`topcoat.fg`; browser runtime tested with happy-dom), MCP in `fogu:ai` (`mcp.fg`; official TS SDK client and server-filesystem by hand), `fogu:surrealdb` (`surrealdb.fg` offline; SurrealDB 3.3.4 by hand, ws and http) |
| `[x]` | `nats.fogu()` (`nats_fogu.fg` offline; nats-server + nats.js by hand), `Fogu.signal` (`signal.fg`), `ssh.shell` (`tests/cli.rs`), `vm.spawn` + `sync.Semaphore` (`vm_concurrency.fg`), HTTP client uploads (`http_client.fg`) |
| `[x]` | C API / `.so` `.dll` (`ffi/`, `cargo ffi`; `bun:ffi` by hand) |

## 13. Implementation (§54–58)

- [~] Pipeline Lexer → Parser → AST → Resolver → tree-walking interpreter + Cranelift JIT. The spec asks for Bytecode → VM
- [x] Resolver (static name, type-name, `?` and `match` checks) — `src/runtime/resolver.rs`, runs before execution
- [ ] Bytecode and VM (§55)
- [~] Memory management with `Rc`: reference cycles are not collected; no GC (§56)
- [x] Diagnostics (§58): `error[E0201]: …`, `--> file:line:column`, excerpt with `^^^`, `= help:` — `src/toolkit/diag.rs`, `error_diagnostic.fg`
- [x] Errors inside functions of another module point to that module's file — `error_other_module.fg`

## 14. Toolchain / CLI (§59–65)

- [x] `fogu <file>`, `fogu run`, `fogu repl` (REPL)
- [x] `fogu init` (creates `fogu.json`, `main.fg` and `.gitignore`), `add`, `remove`, `install`, `update`, `list`, `pm`
- [x] Extras: `-e`, `-p`, `--watch`, `fogu.json` scripts, JIT flags
- [x] CLI integration tests (`tests/cli.rs`): `init`, `test`, `build --compile`
- [x] `fogu check` (§62) — syntax, undeclared names, use before declaration, unknown types, `?` misuse, non-exhaustive `match`, and unused-variable warnings; type mismatches of values are only detected at runtime
- [x] `fogu test` with `*.test.fg` files and `fogu:test` (`test`, `skip`, `expect(v).toBe/…`, `.not`), `-t` filter (§63) — `tests/cli.rs`
- [x] `fogu fmt` (§61) — token-based, verified against the lexer; `tests/cli.rs`
- [x] `fogu build --compile` — standalone executable in `dist/` with the program, its modules and its packages (§64)
- [x] `fogu build --compile --target <platform>|all` — cross-platform executables from `fogu-<target>` runtimes (`docker/Dockerfile` builds linux-x64/arm64/armv7 and windows-x64; macOS with the Apple SDK)
- [x] `fogu build --compile --service` — the executable installs/controls itself as a Windows service (`install uninstall start stop status`, SCM stop → `SIGTERM`/`service-stop`, log file) or a systemd unit (`toolkit/service.rs`)
- [ ] `fogu build --target wasm` with the Core/Host layers (§65)
- [ ] `fogu publish`, `fogu audit`, `fogu doc`

## 15. Quality and process (§70–76)

- [~] Conformance suite `tests/spec/` (`.fg` + `.out` + `.err`) (§70) — core, types, kv, sql, tcp/udp; http/web tested by hand
- [x] `cargo test` runs the suite with and without the JIT and compares (`tests/spec.rs`)
- [ ] Fuzzing with `cargo-fuzz` (lexer, parser, VM) (§71)
- [ ] Formal grammar `spec/grammar.ebnf`
- [ ] Workspace split into crates (§72) — optional, may be left for the end
- [ ] Startup of an empty program < 10 ms (measure) (§74)
- [ ] FEP process documented (§76)

---

## Plan by phase

Each phase leaves the project working and is validated with examples and tests
before moving on. The order follows the specification's roadmap (§77), adapted
to what already exists.

### Phase A — Test base ✅

- [x] `tests/spec/*.fg` + `*.out` / `*.err` and a `cargo test` runner
- [x] Differential test comparing the output with and without `--no-jit`
- [x] Tests for kv, sql, net and websocket (`kv.fg`, `sql.fg`, `net.fg`, `websocket.fg`); http/web by hand (the client would block its own server)

### Phase B — Complete the core syntax ✅

- [x] Template strings, `?.`, arrow functions, default parameters, `...args`
- [x] Duration literals and the `duration` type
- [x] Redeclaration is an error
- [x] `chan<T>(buffer: n)`, `timeout(d)` in `select`, `for k, v in`
- [x] Structs: identity + `equals`; private `_x` in modules and structs

### Phase C — Errors and diagnostics ✅

- [x] Native functions return `error` values → `?` works with them
- [x] Complete diagnostics (excerpt, `^^^`, `Exxxx` code, `help:`)
- [x] `panic` isolated per task + `--strict`
- [x] Fix: `http.text/json/html(body, {status, headers})` (the 404 returned 500); same in `fogu:web`

### Phase D — Static types and `fogu check` ✅ (almost)

- [x] Annotations kept in the AST and checked at runtime (parameters, return, `let`)
- [x] Resolver: undeclared names, unknown types, unused variables (warnings)
- [x] Structural `interface`
- [~] Generics parsed and erased (arguments and bounds not checked)
- [x] Static `match` exhaustiveness
- [x] `?` restricted by the declared return type
- [ ] Static type inference (detect `add(1, "2")` before running)

### Phase E — Runtime and system library ✅ (core done)

- [x] `Fogu.*` object (`file env args sleep exit hash glob run sql version os write asset watch`)
- [x] `fogu:fs`, `fogu:path`, `fogu:process`
- [x] `fogu:sync` (`Mutex`, `Atomic`, `WaitGroup`, `Once`)
- [x] `fogu:test` + `fogu test` (moved forward from phase G)
- [x] `fogu:log`, `fogu:regex`, `fogu:crypto`
- [ ] `http.serve({ port, fetch })`; TLS in `net`/`http`

### Phase F — Permissions (medium)

- [ ] Capability model in the runtime (no access by default)
- [ ] `--allow-*` flags with per-resource granularity + `"permissions"` in `fogu.json`
- [ ] Checks in every native module (`fs`, `net`, `http`, `sql`, `kv`, `process`)

### Phase G — Toolchain (medium–high)

- [x] `fogu test` + `fogu:test`
- [x] `fogu fmt` (single style, no configuration)
- [ ] `fogu doc`
- [x] `fogu build --compile` (binary with the code and the dependencies embedded)

### Phase H — Real concurrency (high)

- [ ] M:N multi-threaded scheduler, `FOGU_THREADS`, non-blocking I/O
- [ ] Transfer model between tasks and data race prevention
- [ ] `cron` and HTTP servers without a manual `delay`/`tick` loop

### Phase I — Bytecode VM and memory (very high)

- [ ] AST → bytecode compiler, VM, closures in bytecode
- [ ] Integrate the JIT with the VM
- [ ] GC (tracing, or reference counting with cycle collection)
- [ ] Fuzzing of the lexer, parser, bytecode loader and VM

### Phase J — Extensibility and ecosystem (high)

- [x] `fogu:ffi` (C ABI: up to 4 mixed or 8 integer arguments, buffers)
- [ ] `--allow-ffi`; Rust extensions with a versioned ABI
- [ ] Package registry, `fogu publish`, signatures, `fogu audit`

### Phase K — Messaging, automation and devices (medium, per module)

- [x] `ssh` (`fogu:ssh`: run, scp upload/download, local `-L` and remote `-R` tunnels)
- [x] `fogumq` (Fogu broker: key/value, channels, pub/sub, RPC, API keys)
- [x] `foguMQ-client` (FlowMQ v6 protocol: channels, pub/sub, RPC, job queues, API keys)
- [x] `scylla` (ScyllaDB/Cassandra CQL client)
- [x] `websocket`, `mqtt`, `nats`, `redis`, `amqp` (`fogu:rabbitmq`), `smtp` (`fogu:mail`), `yaml`, `toml`, `csv`, `excel`, `xml`, `s3`, `shell`, `ai`
- [ ] `kafka`, `dns`, `context`
- [x] `serial`, `modbus`, `usb`, `hdmi` (CEC) — [ ] `--allow-device`
- [x] `docker`/`podman`, `service` (systemd), `ssh.mount`, `fs.remote`

### Phase L — 1.0

- [ ] `fogu build --target wasm` (Core/Host)
- [ ] Frozen grammar, `spec/grammar.ebnf`, complete conformance suite
- [ ] Startup < 10 ms, complete documentation, FEP process
