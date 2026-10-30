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

## Known gaps

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
- Generic type arguments and bounds are not checked; type mismatches of values
  are only found at runtime (no static inference).
- No central package registry, no signatures/`fogu audit`, no `fogu fmt`.
- `Fogu.watch` and `Fogu.serve/reply` are not implemented.
- Old dead-code warnings in native modules.
- `system.sleep_ms` blocks the thread (and with it every `fg` task); use `time.delay`.
- Connecting (`fogumq.connect`, `mqtt`, ssh tunnels) still blocks while the
  handshake runs; module functions of `fs` run inline.
- `fogu build --compile` does not embed data files the program reads at runtime.
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
