# Analysis and fixes

## Fixed

- Lexer/parser rewritten with positions (line:column) in errors; hex/bin/exp
  literals; unterminated comments/strings.
- Assignment to fields/indices and compound operators (`+= -= *= /= %=`), `??`,
  `<-` in select, precedence of `-`/`(`/`[` across line breaks.
- Checked arithmetic (overflow, division by zero, `abs(MIN)`, negative pow);
  comparisons with NaN are false.
- Structs: definition environment, inheritance of embedded fields/methods.
- Channels: VecDeque queue, unbuffered rendezvous, `close`, `len`; `select`
  with cases evaluated once.
- `fg`: cooperative tasks (corosensei) with deadlock detection and stack limits.
- Modules: the `in_progress` stack is always released; deep recursion is an
  error instead of a crash.
- Ordered (deterministic) map iteration.

## Second round

- **Positioned errors**: `file:line: runtime error: ...` (also inside the JIT).
- **`error`, `?`, `panic`** (spec §28-31): `error(msg, cause)`,
  `.message/.cause/.trace`, `return a, b` (tuple), `const [x, err] = f()`;
  `expr?` returns `(nil, err)` to the caller.
- **Collections**: `push pop shift unshift insert remove first last contains
  index_of join reverse slice concat sort map filter each find any all reduce`
  (lists); `keys values entries has get set remove merge` (maps); `split upper
  lower trim replace starts_with ends_with repeat lines chars slice` (strings) —
  as methods (`xs.push(1)`) and as functions (`push(xs, 1)`).
- **`select`** picks a random ready case (like Go).
- **JIT**: mutual recursion; variables written back on error.
- **CLI and packages**: see docs/cli.md and docs/packages.md.

## Third round (phases A and B of CHECKLIST.md)

- **Tests**: `tests/spec/` + `tests/spec.rs` (`cargo test`), each test runs
  with and without the JIT.
- **Syntax**: template strings, durations (`500ms`, `duration` type), `?.` with
  chain short-circuit, arrow functions, default parameters, `...args`,
  `for k, v in map` / `for i, x in list`.
- **Semantics**: redeclaration in the same scope is an error; `equals` on
  structs; private `_x` in module imports; `timeout(d)` (the scheduler waits for
  the deadlines instead of reporting a deadlock).
- **Fix**: `json.stringify` sorts the keys (deterministic output).

## Fourth round (phase C and leftovers)

- **Privacy** of `_x` in structs: every top-level environment knows its module
  (`Environment::module_name`), every instance keeps the struct's module.
- **Static resolver** (`src/runtime/resolver.rs`): the same scopes as the
  interpreter; inside functions it accepts outer names declared later.
- **Native errors as values**: `io_err!` marks the message and `call_value`
  turns it into an `error`. Limitation: `const [a, b] = f()` destructures lists,
  so for functions that return lists use `?`.
- **Diagnostics** (`src/toolkit/diag.rs`) with codes, excerpt and help; the
  error's file is the module where the function was defined.
- **Fix**: `http.text/json/html` accept `{status, headers}`.

## Fifth round (phase D, build, English)

- **Types**: `TypeExpr` annotations kept in the AST. Parameter annotations are a
  `Stmt::CheckType` prologue; the return annotation is a `Stmt::ReturnType`
  first statement checked by `run_body`; `let x: T` checks after evaluating.
  Tuples with a non-nil `error` in the last position skip the other checks
  (`return nil, err`). `error` also accepts `nil`.
- **Interfaces** are structural (structs, or maps of functions, with the
  methods). **Generics** are parsed and erased.
- **Resolver**: unknown types, `?` restricted by the declared return type,
  non-exhaustive `match`, unused-variable warnings.
- **JIT**: numeric annotations are checked at compile time, so annotated
  numeric code is still compiled.
- **`fogu build --compile`**: copies the `fogu` executable and appends a JSON
  bundle (entry, imported modules, packages under `fogu_modules/`) followed by
  its length and a magic trailer; at startup the bundle is extracted once to
  `<temp>/fogu-bundles/<hash>` and the entry is run from there.
- **English**: messages, CLI, comments, docs, examples and tests.

## Sixth round (phase E)

- **`fogu:fs`, `fogu:path`, `fogu:process`**: operational failures are `error`
  values (`io_err!`); a non-zero exit code of a process is not an error
  (`r.ok`, `r.code`). `fs.glob` supports `*`, `?` and `**` and skips dot files.
- **`fogu:sync`**: blocking methods declare a wait condition
  (`FoguObject::wait_for`) that the interpreter waits for cooperatively
  *before* calling the method, so no `RefCell` borrow is held while other tasks
  run. `Once.do(fn)` is intercepted by the interpreter (it calls Fogu code).
- **`fogu:test` / `fogu test`**: `test()` registers into a thread-local list;
  `runtime::run_test_file` loads the file and calls each registered function.
- **`Fogu` global**: a module value registered as a global; `glob`, `run` and
  `sql` reuse the module implementations.
- **Fixes**: `<-` at the start of a line; `?` in `fn f(): error` returns the
  error; `db.driver` for in-memory SQLite.

## Seventh round (cooperative I/O, smaller binary)

- **Scheduler wake-ups**: work running off the interpreter thread holds a
  `sched::IoGuard`; finishing it calls `sched::wake()`. The main task sleeps on
  a condvar until a wake or the next timer instead of polling every 2 ms, and
  in-flight work is not taken for a deadlock. A wake no longer drops a pending
  `timeout` deadline.
- **`native/rt.rs`**: one shared tokio runtime (sql used to build a full
  runtime per connection; scylla, bullmq and the http server had their own) and
  a reusable pool of blocking threads (`thread_poller` used to spawn a thread
  per call).
- **Blocking calls made cooperative** (other `fg` tasks run meanwhile; inline
  when there are none): `http.get/post/put/delete`, `process.run/exec`,
  `fogu:shell` programs and its builtin `sleep`, `ssh.run/upload/download`,
  `sql` statements. Inside a transaction (`begin` … `commit`/`rollback`) sql
  statements still do not yield, and `begin()` first waits for the statements
  other tasks have in flight.
- **Child processes**: `process.run({ input })` and `ssh.run` feed stdin from
  their own thread (a child writing a lot before reading could deadlock).
- **Dependencies**: `ureq` → `reqwest` (one TLS stack), trimmed features,
  release profile with strip + thin LTO, `dist` profile with fat LTO.

## Eighth round (desktop, networking, broker, embedding)

- `fogu:desktop` reviewed: one window only, bound calls ran inline (a slow call froze the
  window), any page (even a remote site navigated to) could call bound functions,
  `devtools()` was a no-op, `percent_decode` sliced UTF-8 at byte offsets (panic), `emit`
  before the page loaded was lost, `background` was ignored, `fogu desktop dev` left Vite
  running after Ctrl+C. Rewritten: a window registry, calls as `fg` tasks, trusted origins
  plus external links in the browser, queued emits, more window APIs; the CLI gained `.fg`
  restarts, Ctrl+C handling, a dev-URL probe and `doctor`.
- `fg` tasks did not keep their own current file/line: a task switch left the interpreter
  pointing at another task's position, so errors could name the wrong file (found by
  `fogu:vm` concurrency tests). The scheduler now saves/restores it per task (switch hook).
- `fogu:vm` deadlines were interpreter-wide; now per task (`Interpreter::deadline`),
  with cancellation (`VmTask.cancel`).
- `fogu:fogumq` client/broker id collision for forwarded RPC calls (found by a Python
  client); the old broker in `fogu/src` has the same issue.
- New: `fogu:grpc`, `fogu:socketio` (+ fogu:web mount), `fogu:security`, `fogu:broker`
  (embedded FoguMQ with clusters), `fogu:kafka`, `nats.fogu()`, `Fogu.signal`,
  `ssh.shell`, `vm.spawn`, `sync.Semaphore`, the richer HTTP client, and the C API
  (`ffi/`, `src/embed.rs`). Verified against real servers and official clients: Kafka 3.9
  (and its console tools), nats-server 2.10 + nats.js, socket.io 4.8 (server and client),
  Bun/Go/Rust/Python FoguMQ clients, `bun:ffi`.

## Ninth round (own protocol clients)

- Six client crates replaced by code on the runtime's networking (726 → 660 crates):
  Socket.IO client (Engine.IO v4 over `websocket::dial`), DNS (UDP + EDNS0, TCP on
  truncation; hickory failed on large TXT answers), etcd (tonic + the official protos via
  `protobuf::compile_sources`; the old watch/keepAlive stopped after the first event and a
  second tokio runtime was built), SMTP + MIME, MQTT 3.1.1/5 and AMQP 0-9-1.
- Socket.IO server: socketioxide spawns a task per message, so handlers could run out of
  order; an extractor numbers messages per socket and the interpreter reorders them.
- Gap: the Socket.IO client has no long-polling transport; BullMQ still uses the `redis`
  crate (porting it onto `fogu:redis` would save ~12 more crates).

## Tenth round (SurrealKV, Topcoat, broker admin)

- `fogu:surrealkv`: waiting for a commit inside `FoguObject::call` panicked ("already
  borrowed") when another task used the same store; commits are pollers now. Unclosed
  stores lost their last writes at exit (`native::cleanup` closes them).
- `Fogu.signal.wait()` never answered HTTP requests: they are served at `call_value`
  boundaries, and the wait idled in native code. `Interpreter::serve_http` is called from the
  wait loop and the server thread calls `sched::wake()`.
- HTML attributes from a Fogu map came out in `HashMap` order (different per run): native
  code that walks a map must sort it.
- `fogu:topcoat` reimplements Topcoat's model (the Rust crate is macro-based and cannot serve
  Fogu scripts); its browser runtime evaluates expressions with `new Function` + `with`, so a
  strict CSP without `unsafe-eval` blocks it. Tested with happy-dom, not yet in a real browser
  in CI.
- Gaps: shards have no streaming/suspense; `tc.shadcn` covers the components with CSS in
  shadcn-rs 0.1 (no calendar/command/carousel); Tailwind's browser build needs the CDN.

## `ssh.mount` review

- Works on Linux with sshfs 3.7.1 + OpenSSH 8.9 (key auth, read/write through the mount,
  `readonly`, missing remote folder as an error value, unmount, cleanup at exit). Fixed:
  unmount waited inside `call`; SIGINT/SIGTERM skipped `native::cleanup` (mounts stayed) —
  `signal::arm_cleanup` now runs it; Windows accepted paths, custom keys and `readonly` that
  SSHFS-Win cannot honour. macOS (macFUSE) and Windows (SSHFS-Win) are not tested here.
- A killed process (SIGKILL) still leaves the mount: `fusermount -u <dir>` removes it.

## Eleventh round (performance)

- Benchmarks against Python and Bun (`bench/`, `docs/benchmarks.md`). With the JIT, numeric code
  is close to Bun and 15–40× faster than CPython; the interpreter alone is slower than CPython.
- Fixed: start-up built all 70 native modules (~50 ms → ~8 ms, lazy modules); `list.sort()` was
  O(n²); JSON stringify/parse (allocations, invalid escapes); `join`.
- Gap: strings, maps and JSON are still slower than CPython. The interpreter code suggests why (to be confirmed with a profiler):
  every variable read clones the value, and `Value::Str(String)` copies the whole
  string; environments are a chain of `HashMap<String, _>` (SipHash per lookup). Next steps:
  `Rc<str>` strings, resolved variable slots, a faster hasher.

## Twelfth round (buffers, networking on the runtime, interpreter speed)

- `fogu:net`: `readLine` dropped whatever had been read past the line
  (two lines in one packet gave only the first), and `read` decoded the bytes as lossy UTF-8
  (binary data was corrupted); `readLine` gave `""` at the end of the stream, like an empty
  line. Rewritten on the shared tokio runtime with one read buffer per stream; data comes back
  as a `buffer` (new built-in type, `bytes(...)`), and the end of the stream is `nil`. Waiting
  operations no longer block the other `fg` tasks.
- `fogu:websocket` used an OS thread per connection and polled every 2 ms; connections are
  now tasks on the shared runtime that wake the scheduler, and `app.ws` runs on the upgraded
  HTTP connection instead of a loopback socket bridge. Echo: 15.1k → 28.8k messages/s.
- HTTP requests were served only between the program's calls, so the documented
  `while true { time.delay(100ms) }` loop added up to 100 ms per request (1.2k req/s); they are
  now dispatched while the program waits, each handler in its own task (24.9k req/s; 27.1k
  with `Fogu.signal.wait()`).
- The string gap of the eleventh round: strings are now `Rc` copy-on-write, `s += x` appends in
  place (was O(n) per append), `s[i]` collected every character on each call (now O(1) on
  ASCII, the char count is cached), loop bodies reuse their scope, lookups use FxHash, and
  finished tasks return their stack to a pool.
- `fogu:desktop` was removed (with `fogu init --desktop`, `fogu desktop` and the `desktop`
  cargo feature).
- Split into a workspace: `fogu-core` (language, built-ins, tokio runtime, `Host` trait),
  `fogu-std` (standard library by area, each area but `base`/`io` a cargo feature),
  `fogu-pkg` and the CLI package. A build without a feature drops its optional dependencies;
  importing one of its modules fails as an unknown module.

## Dependency audit (osv.dev, 0.7.1)

- Fixed: zip 2.4 → 9.1 (zip cloaking, SUID bits, symlinks outside the target, permissions
  through symlinks). `fogu:zip` never relied on the crate's `extract`, but its own `unzip` now
  also creates only symlinks that stay inside (checked before anything is created), refuses
  to write through links leading outside and strips setuid/setgid/sticky; `untar` checks
  symlink targets too (the `tar` crate refuses `..` names and writing through outside links,
  not creating such links). quick-xml 0.42 (calamine 0.37, rust_xlsxwriter 0.102);
  rustls-pemfile replaced by rustls-pki-types (tonic 0.14).
- Left: zstd 0.13.3 / zstd-safe 7.3.0 (RUSTSEC-2026-0391, -0395..0404) — rskafka 0.6.0 and
  surrealkv 1.1.1 (the latest) require zstd ^0.13; fixed only in zstd 0.14.3 / zstd-safe 8.2.
  rsa 0.9.10 (Marvin, no fix): only sqlx-mysql uses it, to encrypt the password with the
  server's public key (`caching_sha2_password` without TLS) — no private-key operation, so the
  timing attack does not apply. block-buffer 0.10.4: every RustCrypto 0.10 hash (sha2, sha1,
  md-5, hmac) pins it. ttf-parser (ab_glyph), unsafe-libyaml (serde_yaml) and paste
  (in the lock only, not built) are unmaintained with no drop-in replacement.

## Thirteenth round (JIT speculation, 0.7.2)

- The JIT only compiled numeric loops; `strings` and `maps` spent 60–80% of their time
  in the interpreter walking lists and maps (`for p in back { total + p.len() }`,
  `m["k" + str(i)] = i`, `sum + m[k]`). Lists, maps and text elements now live in loop
  slots as pointers; reads *speculate* the type of the first element/value and every
  helper checks it. A failed check sets `ctx.deopt`: the loop restores the variables as
  they were at the start of that iteration (committed copies in the prologue of every
  iteration of the compiled loop, a few register moves), returns the iteration index and
  the interpreter redoes it. Soundness rule: a guard may only run before any side effect
  of the iteration (push, map write, print, call), otherwise the loop bails at compile
  time; a nested loop whose body has both guards and effects bails too (its later
  iterations would guard after effects of earlier ones). A loop that deopts finishes in
  the interpreter: it is not recompiled with another speculation.
- Measured (median of 11, wall ms): maps 357 → 189 (Bun 275, Go 177), strings 88 → 68
  (Bun 52, Go 39); the other benchmarks unchanged. Of what remains in `strings`, the
  split/join and the pushes are native; in `maps` the hashing and growth of a 300k-entry
  `HashMap<String, Value>` dominate.
- The map value speculation uses the map's first value in iteration order, which is
  arbitrary for a hash map: a map with mixed value types may bail with "would change
  type" (correct, slower) depending on which value comes first.

## Fourteenth round (interpreter scopes, profile)

- Profile of a plain `while` loop and of calls (`FOGU_PPROF=1`, no JIT), self time: the
  `?` plumbing of `Result<Value, String>` 15%, `get_sym` (walk up the scopes, clone) 13%,
  `eval_binary` 12%, `eval_assign` 6%, `Value::clone` 5%, `ok_or_else` of `Expr::Ident` 5%,
  `assign_sym` 5%, `eval` dispatch 5%. Variable access is about a quarter of the time,
  the `Result` moves a fifth, dispatch a sixth: the tree-walker is near its structural
  limit (≈ 240 ns per simple loop iteration, 500 ns per call, 10× a bytecode VM).
- Small scopes in a vector and parameters bound by interned symbols: 5–9% on locals, block
  scopes and calls. The next real step is Phase 4 (the bytecode VM with slots resolved at
  compile time), not more tuning of the tree-walker.

## Fifteenth round (`fogu check`)

- `fogu check` accepts folders, reports every error of a file (the resolver recovers per
  statement; `resolver::check`, used before running, still returns the first), and has
  `--help`, `--json`, `--strict`/`--deny-warnings`, `-q`, `--watch`.
- A conservative type inference in the resolver (`Ty`, `binop` mirrors `eval_binary`): the
  messages are those of the interpreter, so a static error is exactly the error the code would
  hit when it runs. Too many arguments are an error; missing ones are not (they are `nil`).
- New warnings: unused imports, unreachable code after `return`/`break`/`continue`,
  shadowing inside the same function. Unused parameters are not reported (callbacks often
  ignore theirs).
- `tests/spec/error_type_param.fg` and `error_type_return.fg` now pass values the inference
  cannot see (`[x][0]`), so they still cover the run-time checks.

## Sixteenth round (exports and data imports, 0.7.3)

- New export forms (`ExportKind::LetList/Names/All/Default/DefaultDecl`) and
  `ImportKind::Combined`. The default export lives under the key `default` of the module's
  exports (a keyword, so no declared name collides); `import x from` binds it when present and
  the module object otherwise, so every existing `import mod from "./x.fg"` keeps working.
  `export *` is applied when the module ends (`Interpreter::export_stars`), so the module's own
  exports win. Duplicate exports are a parse error (with the line of the first one); exporting an
  undefined name is a resolver error; the interpreter has the same messages.
- `ModuleCache::resolve` returns `Imported` (a module, or a data file's value). `.json` is parsed
  by the core; `.json5 .yaml .toml .xml .csv` go through the new `Host::load_data` (fogu-std's
  `formats::load_data`), since fogu-core cannot depend on fogu-std.

## `fogu:webrtc` (0.7.3)

- Library: str0m 0.24 (sans-IO ICE/DTLS/SCTP, small, actively maintained) over webrtc-rs (a full
  async stack with many more crates). Crypto backend `rust-crypto` (dimpl DTLS on RustCrypto, no
  OpenSSL). Its self-signed DTLS certificate comes from rcgen, which `str0m-rust-crypto` builds with
  dimpl's `aws-lc-rs` feature: aws-lc-sys (C, built with `cc`; it cross-builds for
  x86_64-pc-windows-gnu here) is a new native dependency. Release binary: 72.3 MB against 65.2 MB for the previous
  release build of the main checkout (roughly +7 MB; not measured on the exact same base).
- Each peer: one tokio UDP socket per local address (default: the default-route address and
  127.0.0.1, so local peers connect offline), one reader task per socket and a driver task that
  feeds packets/timeouts to the `Rtc`. The program mutates the `Rtc` under the same mutex and
  drains `poll_output` before releasing it (str0m's single-mutation contract), then pokes the
  driver (`Notify`) since the next timeout may have moved. `on` handlers run from a pump task
  started by the intercepted `webrtc.peer` (it holds an `IoGuard` only while handlers exist).
- STUN is a small binding request of our own (str0m gathers nothing); browser mDNS candidates
  (`*.local`) are dropped from SDP and `addCandidate` — browsers are reached through
  peer-reflexive candidates. The signaling helpers are Fogu code (`webrtc.fg`, run like
  `ai_mcp.fg`); `for msg in ch` is a `DataChannel` arm in the interpreter's `for`.
- Browser interop checked with headless Windows Chrome (CDP) against a WSL process: WebSocket
  rooms with Chrome as caller and as callee, and the HTTP POST variant; text and binary echo,
  nominated pair `succeeded`; and `examples/webrtc_chat.fg` end to end (page → Fogu → page).

## Dependency audit (cargo audit, 0.7.3)

`cargo audit` on 2026-11-06, 764 dependencies, 7 advisories:

| Advisory | Crate | Fix |
| --- | --- | --- |
| RUSTSEC-2026-0436 | calamine 0.37 (OOM on a malformed CFB/OLE2 file) | 0.38.0 |
| RUSTSEC-2026-0391 | zstd 0.13.3 (data of an abandoned stream leaks into the next) | 0.13.4 |
| RUSTSEC-2026-0398, -0403 and 9 unsound warnings | zstd-safe 7.3 (Vec length past capacity, uninitialized memory, use-after-free…) | 7.4.0 |
| RUSTSEC-2026-0433 | pulldown-cmark 0.13.5 (quadratic wikilinks) | 0.13.6 |
| RUSTSEC-2026-0387 | crypto-bigint 0.5 (timing variability) via str0m → dimpl → ecdsa 0.16 | str0m 0.25 (dimpl 0.9, ecdsa 0.17, crypto-bigint 0.7); no code change |
| yanked | clipboard-win 5.4.1 (rustyline, Windows) | 5.4.3 |
| RUSTSEC-2023-0071 | rsa 0.9 (Marvin attack) via sqlx-mysql | **open**: no patched release (0.10 is a release candidate; sqlx 0.9 pins it but RustSec lists no fixed version). Exposure: a MySQL login without TLS (`caching_sha2_password` sends the password RSA-encrypted). Ignored in `.cargo/audit.toml` with the reason |

Unmaintained: `unsafe-libyaml` via serde_yaml → **replaced** by `serde_norway` 0.9 (the maintained fork: same API, parser `unsafe-libyaml-norway`; `serde-yaml-ng` and `serde_yml` still use the unmaintained one). Left as warnings, no fix available: `paste` (a proc-macro helper pinned by dependencies), `ttf-parser` (via ab_glyph for `fogu:image` text; the maintained replacement, skrifa, would need a new text renderer).

## Seventeenth round (sized numbers, runtime cost, 0.7.4)

- **Sized numbers** are ranges and conversions over `int` / `float`, not new run-time types: every
  native keeps matching `Value::Int` / `Value::Float`. `ops::Sized` holds the ranges and Go's
  wrapping; the interpreter, the resolver, the JIT, `fogu:types`, `fogu:ffi` and the buffer accessors
  share it. Arithmetic does not wrap by itself (it stays checked 64-bit): `uint8(a + b)`.
- **JIT errors with a program-specific message** (`parameter 'x' expects int8…`) go through
  `ops::custom_code` (interned, leaked once per distinct message).
- **The line of a JIT error** is a register value passed to the cold error block; `JitCtx::line` is 0
  when native code failed before its first statement, and the interpreter then keeps its own line
  (the call), exactly as when it runs the parameter checks itself.
- **Task stacks**: guarded stacks up to `GUARDED_MAX` (8192), then `sched::slab`. Slab stacks have
  no guard page — their protection is the software stack limit (`STACK_MARGIN` = 512 KB) checked by
  the interpreter and by compiled code on every call. A finished slab stack gives all its pages back
  unless it is one of the first 256 free ones.
- **Start-up**: of ~6 ms for `print(1)`, the program itself takes ~1.5 ms (0.5 ms registering the
  lazy module table and `Fogu`); the rest is process creation, page faults of the 70 MB image and,
  where transparent huge pages are set to `always`, zeroing the allocator's first 2 MB pages.

## Eighteenth round (deadlines in native code, direct ffi, workers, 0.7.4)

- **Deadlines in native code**: one mechanism for stack overflow and timeouts — the stack limit
  lives in `jit::Watch` (an `Arc` shared with a watchdog thread), read with an **atomic load**
  (a plain load was hoisted out of a nested loop by Cranelift's redundant-load elimination: the
  loop then never saw the limit change). Arming and firing hold `Watch::armed`, so a deadline
  cannot stop the native code of another run. Functions and loops compiled for a run with a
  deadline are separate specializations (`FnKey::checked`, `LoopKey::checked`): code without a
  deadline has no check in its loops.
- **Direct ffi**: while native code runs, the interpreter is marked (`NativeGuard`): a Fogu
  callback run by a C function called from compiled code finds the interpreter through
  `interpreter::native_interpreter()`, and the JIT is not re-entered until the native code returns.
- **False deadlock**: see the changelog; found because `surrealkv.fg` failed about once in 20 runs.
- **Workers**: the worker's top level runs once on its thread; calls are served one at a time
  (its own `fg` tasks only run during a call).

## Concurrency audit (0.7.4)

Three read-only reviews of every module that hands work between threads; the confirmed findings
that were fixed are in the changelog. **Found and not fixed yet** (each has its interleaving in the
review; none is new in 0.7.4):

- `fogu:net`: a `setTimeout` that fires in the middle of `readLine` / `readExact` / `readAll` drops
  the bytes already read (they live in the cancelled future); `close()` does not cancel a pending
  `accept` / `read` of another task (the port stays bound until it returns).
- `fogu:websocket`: the connection task does not read while it flushes a write — two Fogu peers
  that both send more than the socket buffers hold, without reading, block each other.
- No backpressure: WebSocket inbox / outbox, SSE `send`, the watcher, `fogumq` / `nats` / `mqtt`
  subscriptions and the broker's per-client queue grow without bound while the consumer is slow.
- `Fogu.signal`: once the program listens to SIGINT / SIGTERM, Ctrl+C only acts when the program
  waits cooperatively — a CPU-bound loop cannot be interrupted (a second signal should force the exit).
- `fogumq` / `nats` clients that are dropped without `close()` keep their reader thread and socket.
- Broker: a call whose handler never answers keeps the caller's connection open after the caller
  left; a connection accepted in the same instant as `close()` can outlive it.
- `sched::IN_FLIGHT` is process-wide: an operation in flight on one interpreter thread (a
  `sync.Worker`) can hide a deadlock on another until it finishes (the worker's own call is
  discounted with `sched::set_served`).

## Known gaps

- **Not in 0.7.4**: a bytecode VM for non-numeric code (interpreted calls are still ~150–300 ns:
  the tree-walker is at its limit; `docs/bytecode.md` has the design). A single `fg` task still
  cannot be moved to another core (values are `Rc`): several cores are used through `sync.Worker`
  (separate interpreters, JSON between them). `fogu:ffi` functions that take text or buffers, or
  return nothing, are still called through the interpreter. The bundle of a standalone executable
  is parsed and hashed at every start (not measurable for small programs; it grows with embedded
  assets) — changing it needs a trailer the prebuilt desktop hosts can still read.

- Exports are values taken when the `export` runs, not live bindings: `export let n = 0` (or
  `export { n }`) followed by `n += 1` does not change what importers see. A WebAssembly bundle
  serves imported *text* data files from its in-memory file system, but binary ones (images,
  `.bin`) are assets, which a WASI runtime only gets when the host gives access to a folder.
  `fogu check` reports missing relative imports but does not check that imported names exist.

- `fogu:foguMQ-client` (FlowMQ v6) is tested against an in-process mock broker only (Rust unit
  test); it was not run against a real FlowMQ server. `apikeyGet`/`apikeyList` return the raw
  JSON text. The original draft's callbacks were no-ops; they now feed the inbox objects.

- Memory per `fg` task is ~9 KB (Python asyncio ~2 KB, Bun ~1 KB). It was ~25 KB: with
  transparent huge pages set to `always`, some task stacks got a whole 2 MB page on their first
  fault (now `madvise(MADV_NOHUGEPAGE)` on every stack), and the JIT entered its compile path on
  every call of an already-rejected function, touching a third stack page in each task (rejected
  keys are now answered before that path). What is left: 2 stack pages (a parked task sits ~3.8 KB
  deep, so the 4 KB page boundary is crossed during the call) plus ~1 KB of heap. Freeing the
  pages below the stack pointer at park time is not safe with that little room: corosensei saves
  the suspended context just below it. Each task has its own stack (`DefaultStack`), separate
  from the main thread and from the Tokio workers, which never run Fogu code. Each stack takes 2
  kernel memory maps, so with Linux's `vm.max_map_count` (65,530) a program cannot hold much more
  than ~30,000 tasks at once ("could not create the task stack"). Under 4 KB per task needs
  stackless tasks (a body that parks would be compiled into a state machine), as in Python and
  Bun, or interpreter frames small enough for a task to fit in one page.
- WebSocket connections run on the shared runtime (28.8k echo messages/s with 50
  connections), but every message is still handed to the interpreter thread and its handler
  code is interpreted.
- `const [v, err] = f()` destructures a list result itself: when `f` returns a list (e.g.
  `json.parse("[1, 2]")`), `err` is its second item, not `nil`. Check `typeof(v) == "error"` for
  functions that may return lists. Found in the MCP server (batches) and the broker admin.

- The JIT does not compile strings/lists/maps/structs.
- Generic type arguments and bounds are not checked. The static inference of the resolver
  is deliberately small: literals, annotated parameters/variables, variables never
  reassigned anywhere in the file (by name) and the signatures of `fn` declarations of the
  same file. Fields, call results of methods/modules/imports, struct types and generics are
  `Ty::Any` and never reported, so most type mismatches are still found at runtime.
- No central package registry, no signatures/`fogu audit`, no `fogu fmt`.
- `Fogu.watch` and `Fogu.serve/reply` are not implemented.
- Old dead-code warnings in native modules.
- `system.sleep_ms` blocks the thread (and with it every `fg` task); use `time.delay`.
- Connecting (`fogumq.connect`, `mqtt`, ssh tunnels) still blocks while the
  handshake runs; module functions of `fs` run inline.
- `fogu build --compile` does not embed data files the program reads at runtime.
- TLS (`fogu:tls`, HTTPS in `fogu:web`/`fogu:http`) uses rustls; the HTTP client still uses
  native-tls (OpenSSL / SChannel), so its `ca` option goes through a different stack. The
  standalone `websocket.listen` and `io.listen` (Socket.IO) servers have no TLS option yet (serve
  them through `app.listen(port, { tls })` or a proxy); HTTPS is HTTP/1.1 only. A TLS read that
  ends without the peer's `close_notify` is an error value, not `nil`.
- `fogu:webrtc` has data channels only (no audio/video tracks), no TURN relay (TURN urls in
  `iceServers` are skipped: peers behind symmetric NATs will not connect), no ICE restart or
  renegotiation (channels added after the connection need no SDP), IPv4 STUN only, and no
  `maxPacketLifeTime`/`maxRetransmits` reported back for channels the other side opened.
  `webrtc.connect` pairs two peers per room (the newcomer calls the first member).
- `fogu:kafka` has no consumer groups (rskafka does not implement the group protocol):
  consumers keep their own offsets.
- `fogu:grpc` receives every request of a bidirectional call before running the handler
  (no interleaved streaming); no server reflection.
- `fogu:broker` clusters copy KV writes made while linked (no catch-up of older keys, last
  write wins); members/channel lists are per node.
- The C API serializes calls per runtime (each waits for the previous one) and passes
  values as JSON.
- `Fogu.signal` has no `exit` event (code that runs when the program ends).
- `fogu:dataframe` group/aggregation columns come out in alphabetical order (maps are
  unordered); `fogu:array` holds float64 only.
- `fogu:face` aligns faces by their box (no landmarks): fine for frontal photos, weaker
  for tilted faces. `fogu:hdmi`, `fogu:usb` and the Swarm part of `fogu:docker` have no
  hardware/cluster in the test suite.
- `ssh.mount` uses the system `sshfs` (Linux/macOS) or SSHFS-Win (Windows); there is no
  built-in FUSE implementation.
- Binary size grew with the new modules (image, tract, smartcore…); Polars was rejected
  for `fogu:dataframe` (+70 MB on its own).
