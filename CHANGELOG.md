# Changelog

All notable changes to Fogu. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); before 1.0 there may
be incompatible changes (spec §75).

## [Unreleased]

## [0.7.2] - 2026-10-30

### Added

- JIT: loops over lists, maps and texts. `for x in list`, `xs[i]`, `m[k]`, `m[k] = v`, `xs.len()`,
  `m.len()` and `s.len()` of a text element compile inside hot loops; the element or value type
  is speculated from the first one and checked on every read. A failed check (another type, a
  missing key, an index out of range) leaves native code *before any side effect of that
  iteration*, restores the loop's variables and the interpreter redoes the iteration
  (`LoopOutcome::Deopt`); guards after a side effect, or inside a nested loop with side
  effects, make the loop bail instead (`tests/spec/jit_for_list.fg`, `jit_map_index.fg`,
  `docs/jit.md`). `--jit-stats` counts the deopts.
- Docs: how to create and publish a library (`docs/packages.md`, the guide, docs.html, the docs
  site, the skill), with the GitHub example `microaitec-deploy/fogulang_module` and the project
  `examples/github_lib` that installs it with `fogu add user/repo`.

### Changed

- Interpreter: block and call scopes keep their bindings in a small vector searched linearly
  (the hash map stays for globals, modules and scopes with more than 16 names); parameters are
  bound by their interned names (`FnValue::param_syms`) and declarations no longer allocate a
  `String` per execution. Measured (1M iterations, no JIT): locals 242 → 225 ms, block scopes
  450 → 415, calls 543 → 495.
- `cargo build --profile profiling --features pprof` + `FOGU_PPROF=1`: an in-process sampling
  profiler (no root) prints the hottest functions at exit.
- `fogu add`: when `git ls-remote` fails because a repository is missing or private (git only
  says "could not read Username" / "terminal prompts disabled"), the error now says so and what
  to do (check `user/repo` and the host prefix, log in, use a token or an SSH key).

- `import(path)` expression: loads a module at run time and returns it (computed paths, plugins);
  resolved from the calling file, shared with static imports, an `error` value when the module
  cannot be loaded; `build --compile` bundles literal paths (`tests/spec/dynamic_import.fg`).
- `fogu add <name>` (and `<name>@<version>`) installs a library published on the Fogu
  server (`FOGU_SERVER` or `~/.fogu/server`): `fogu-pkg/src/registry.rs` resolves the
  version through `<server>/api/libs/<name>` and pins the archive URL in `fogu.json`.
- `fogu <package>/<file>.fg` runs a file of an installed package (`fogu desktop/build.fg .`).
- `fogu desktop <create|dev|build|update|path>` and `fogu mcu <boards|ports|build|flash|run|monitor|detect|…>`
  (`toolkit/frameworks.rs`): front-ends for libs/desktop and libs/mcu; the library is found like an
  import or installed globally from the Fogu server on first use.
- `fogu build --compile --service` (and `--service-name`, `--display-name`, `--description`,
  `--start-type`, or `"service"` in `fogu.json`): the executable becomes a service —
  `app install [--start auto|manual|delayed] [--user …] [--log …] [-- args]`, `uninstall`,
  `start`, `stop`, `status`. On Windows it runs under the service manager (`windows-service`):
  output to `<name>.log` next to the `.exe`, a stop or shutdown raises `service-stop` and
  `SIGTERM` with a grace period (`stopTimeout`, 20 s), the exit code is reported with
  SERVICE_STOPPED. On Linux `install` writes and enables a systemd unit, and the build writes
  `dist/<name>.service` (`toolkit/service.rs`, `examples/windows_service.fg`).
- `Fogu.signal`: the `service-stop` event; `exit()`, `process.exit` and `Fogu.exit` all run
  the native cleanup.

### Changed

- Performance: loop scopes are really reused between iterations (they were re-created every
  time); `Value` is 24 bytes instead of 48; calls of Fogu functions, struct methods and most
  native functions skip the standard library's interception hooks. CPU time vs 0.7.1: mandel
  2.1×, strings 1.46×, sort 1.42×, maps 1.37×, records 1.21× faster.
- Variables are looked up by interned symbols (`runtime/symbol.rs`, `ast::Name`), not by
  name text; maps of Fogu values use `ahash` (fast, seeded); the binary uses mimalloc on
  Linux and macOS.
- JIT: `xs.push(number)` in hot loops is compiled (lists become loop slots); other uses of
  the list keep the loop interpreted (`jit_list_push.fg`).
- CPU time vs 0.7.1: sort 7.9× (now faster than Bun and Go), maps 1.9× (faster than Bun),
  strings 1.9×, records 1.8×.
- JIT: `xs.push(…)` of texts (`"item" + str(i)`, templates) and of list/map literals is
  compiled (`jit_push_values.fg`): strings 3.3× and records 2× faster than 0.7.1.
- `fogu:websocket`: 2.5× more messages per second (an oversized read buffer was zero-filled
  on every read). Scheduler: timers no longer pile up while a program waits and serves;
  `fogu build --compile --target <platform>` downloads a missing runtime from the Fogu server
  (`FOGU_SERVER` or `~/.fogu/server`, written by the install scripts), checks its SHA-256
  and caches it.

### Security

- `fogu:zip`: zip 2.4 → 9.1 (GHSA-m665-v7pm-7grj / CVE-2026-115905, RUSTSEC-2026-0367/0368/0369).
  `unzip` creates symlink entries only when their target stays inside the target folder (a
  file holding the target on Windows), refuses them otherwise, never writes through a link
  that leads outside, and keeps only the rwx permission bits (no setuid/setgid/sticky);
  `untar` refuses symlinks pointing outside the target folder (`zip_safety.fg`).
- quick-xml 0.36 → 0.42, calamine 0.26 → 0.37, rust_xlsxwriter 0.79 → 0.102
  (RUSTSEC-2026-0194/0195). `xml.parse` joins text split around entity references.
- tonic 0.12 → 0.14; `rustls-pemfile` (unmaintained) replaced by the PEM API of
  `rustls-pki-types` (`fogu:grpc`, `fogu:kafka`, `fogu:security`).

## [0.7.1] - 2026-10-29

### Added

- `loop { … }` — repeats until `break` (or `return`); the same as `while true { … }`.
  `loop` is contextual: elsewhere it is still an ordinary name (`loop.fg`).
- `fogu:crypto` UUIDs: `crypto.uuidv4()`, `crypto.uuidv7()` (time-ordered, monotonic within
  the same millisecond), `crypto.uuidv5(namespace, name)` (namespaces `dns url oid x500` or a
  UUID), `crypto.uuid(version = 4, …)` and `crypto.uuidInfo(text)` → `{ version, variant,
  time }` (`uuid.fg`). `crypto` functions now also accept a `buffer` as data.
- `fogu:ffi`: `ffi.open` resolves the same name on Linux, macOS and Windows — a name without
  an extension gets the platform's prefix and suffix (`"native/x"` → `native/libx.so`,
  `native/libx.dylib`, `native/x.dll`) and another platform's extension is mapped to the
  native one; the error lists every file tried.

### Removed

- `fogu:array`, `fogu:dataframe`, `fogu:ml` — now the `datascience` lib (`libs/datascience`,
  over `fogu:ffi`): `import np from "datascience/array"`. Drops `ndarray`, `smartcore` and
  `sqlx` from the `ai` feature.
- `fogu:serial`, `fogu:modbus`, `fogu:usb`, `fogu:hdmi` and the `hardware` feature — now the
  `hardware` lib (`libs/hardware`): `import serial from "hardware/serial"`. Drops `serialport`
  and `nusb`.
- `fogu:face` — now the `face` lib (`libs/face`): `import face from "face"`. Drops
  `tract-onnx`, `rustface` and the embedded detector model.

- `buffer` — a built-in mutable byte sequence, made with the global `bytes(x, encoding =
  "utf8")` (a str in `utf8 hex base64 base64url latin1`, an int n → n zero bytes, a list of
  byte ints, or a buffer to copy). `typeof(b) == "buffer"`, `buffer` as a type annotation,
  `len(b)`, `b[i]` / `b[i] = n` (ints 0..255), `for x in b` / `for i, x in b`, `==` by content,
  printed as `<buffer 6 bytes: 68 c3 a9 6c 6c 6f>`, JSON as an array of ints. Methods: `text
  toString hex base64 toList copy clone slice get push append write concat indexOf includes
  startsWith endsWith split equals fill clear reverse`, `readU8`…`readF64` and
  `writeU8`…`writeF64` (offset, little-endian unless `bigEndian`). Binary data across the
  standard library now comes back as a buffer (`fogu:net`, `fogu:websocket`, `res.bytes()` of
  `fogu:http`, `zip`, `image`, `protobuf`, `serial`, `usb`, `kafka`); inputs still accept a str
  or a list of byte ints.

- `fogu:foguMQ-client` — client for the FlowMQ v6 wire protocol: numbered channels, pub/sub
  with `*`/`>` wildcards (`subscribe` → `next()`), `publish`/`publishSync`, RPC (`serve` →
  Request `reply`/`fail`, `request`), job queues (`registerWorker` → Job `ack`/`nack`/`extend`,
  `enqueue`), API keys, auto-reconnect. Runs on the shared tokio runtime and waits cooperatively.
- `fogu:grpc` — gRPC servers and clients from `fogu:protobuf` schemas (no code
  generation): unary, server/client streaming and bidirectional methods, metadata,
  deadlines, status codes (`grpc.status`), TLS (`listen(port, { cert, key })`, `https://`),
  `c.stream(...).next()`; each request runs as its own `fg` task (tonic framing + hyper).
- `fogu:socketio` — Socket.IO v5 servers (socketioxide) and clients (Engine.IO v4 over WebSocket),
  compatible with the official libraries: namespaces, rooms, broadcasts, acks
  (`emitWithAck`), auth, several arguments per event; standalone (`io.listen`) or on a
  `fogu:web` app's port (`app.listen(port, { socketio: io })`).
- `fogu:security` — TLS/certificate inspection, `selfSigned`, port scanning with banners,
  DNS/reverse DNS (own resolver, `{ server, timeout }`), HTTP security-header audit with a grade, password strength
  (zxcvbn), TOTP/2FA, secret scanning in text and folders, Ed25519 keys and signatures,
  constant-time comparison, CIDR helpers and a token-bucket rate limiter.
- `fogu:broker` (also `fogu:mq`) — the Fogu broker inside the runtime (KV with TTLs and
  redb persistence, channels, wildcard pub/sub, RPC, API keys), an inline client over an
  in-memory pipe (`b.client()`), clusters (`node`, `peers`, `secret`: messages, RPC routes
  and KV writes cross the nodes). Tested with the Bun, Go and Rust clients in `fogu/client/`
  and a new Python client (`fogu/client/python/fogu_client.py`).
- `fogu:kafka` — Apache Kafka/Redpanda in pure Rust (rskafka): topics, `send`/`sendBatch`
  (keys, headers, partitions, gzip/lz4/snappy/zstd), consumers with `next`/`poll`/`seek`,
  TLS and SASL (PLAIN, SCRAM). Consumer groups are not supported.
- `nats.fogu(config)` / `nc.fogu()` — KV stores on JetStream with watches, `sub`/`pub`
  with handlers, `service` (with auth) and `command`, interoperable with the same API
  written with nats.js.
- `Fogu.signal` — `wait()` keeps the program alive until SIGINT/SIGTERM (or other signals
  and events), `on`/`off`/`emit` handlers for OS signals and the program's own events.
- Embedding: `cargo ffi` (= `cargo build --release -p fogu-ffi`) builds `libfogu_rt`
  (`.so`, `.dll`, `.dylib`) with a C API (`ffi/fogu.h`: `fogu_new eval spawn run_file call
  get set emit on_host free`), `fogu::embed::Runtime` for Rust hosts, `Fogu.host.emit` for
  events to the host. Tested from `bun:ffi`.
- `fogu:ssh`: `s.shell({ pty, cwd, env })` — persistent remote shells (`run` keeps the
  directory and variables; `send`/`expect`/`read`), `s.interactive()`; `FOGU_SSH` picks
  the ssh client.
- `fogu:vm`: `vm.spawn`, `script.spawn`, `ctx.spawn` (each run in its own task), `wait`,
  `cancel`, `vm.all`; contexts run one script at a time (`ctx.mutex` is a `sync.Mutex`).
- `fogu:sync`: `Semaphore(n)`.
- `fogu:http` client: `request`, `patch`, `head`, options (`headers query json form
  multipart body timeout redirects basicAuth bearer`), file uploads (`http.file`),
  `download`, `res.ok/url/bytes()/header()/cookies()`.

- `fogu:html` — HTML parsing with CSS selectors (scraper), Jinja templates (minijinja,
  with folder loaders, `extends`/`include` and a `markdown` filter), sanitizing (ammonia),
  `escape`/`unescape`, HTML → text / Markdown and Markdown → HTML (pulldown-cmark).
- `fogu:webview` — drives Chrome/Edge/Chromium through the DevTools protocol (own client
  over tungstenite): `launch`, `render`, pages with `goto eval click type fill press
  waitFor select screenshot pdf cookies setContent`; browsers are closed at exit.
- `fogu:image` — create, transform, draw (anti-aliased, tiny-skia), text (system fonts),
  SVG (resvg) and HTML rendering, PNG/JPEG/WebP/GIF/BMP/ICO/TIFF, animated GIFs.
- `fogu:plot` — line, scatter, area, grouped bar, histogram and pie/donut charts as SVG,
  PNG, JPEG or WebP.
- `fogu:face` — face detection (embedded SeetaFace model, rustface) and recognition
  (SFace ONNX via tract, downloaded once and checksum-verified), `compare`, face database.
- `fogu:array` (NumPy-style n-d arrays, ndarray), `fogu:dataframe` (pandas-style frames:
  expressions, groupBy, joins, describe, SQL over the frame, plots) and `fogu:ml`
  (smartcore: logistic, kNN, tree, forest, naive Bayes, SVM, linear/ridge/forest
  regression, k-means, split, metrics, scalers, model files).
- `fogu:serial` (serial ports, virtual pairs), `fogu:modbus` (TCP client/server, RTU
  client/slave over serial), `fogu:usb` (nusb: control/bulk/interrupt transfers) and
  `fogu:hdmi` (HDMI-CEC through the Linux kernel API or libCEC's cec-client).
- `fogu:docker` — Docker/Podman: containers, images, networks, volumes, compose projects
  (`services`, `status`, per-service restart/stop/start), Swarm services, Dockerfile and
  compose file writing/editing; `docker.remote(ssh)` runs it all on a server.
- `fogu:service` — systemd units and `service.sh` scripts, install/deploy/control/status,
  locally or over SSH (`{ ssh }`).
- `fogu:zip` — ZIP (AES passwords), tar/tar.gz and gzip; `fogu:protobuf` — `.proto`
  files compiled in pure Rust (protox) and dynamic encode/decode (prost-reflect).
- `fogu:ssh`: `s.mount(remote, local)` (sshfs / SSHFS-Win), through a tunnel or a bastion,
  and `ssh.connect({ jump })`; `fs.remote(ssh)` gives the `fogu:fs` functions on a server.
- A native object may describe itself for `print` (`__display__` field): arrays, data
  frames, figures and models print readably.
- Logo and favicon (`docs/assets/`), used by `docs/docs.html`.

- `http.serve` and `app.listen` accept `"ip:port"` as well as a port: `"0.0.0.0:8080"`,
  `"[::1]:8080"`, `":8080"`, `"localhost:8080"` (IPv6 hosts now bind correctly).
- `./.env` is loaded at startup (`dotenvy`) into `Fogu.env` and `process.env`;
  variables already set win, bad lines are warned about, `FOGU_DOTENV=0` turns it off.
- `fogu:bullmq` — BullMQ job queues on Redis, wire-compatible with BullMQ for Node.js v5
  (on `bullmq-rs`): `queue` (add, bulk, delays, priorities, retries with backoff,
  deduplication, job schedulers, counts, clean, obliterate), `worker` (jobs handed to the
  program: `complete`, `fail`, `progress`, `log`; concurrency, rate limits) and `events`.
- `fogu:vm` — runs Fogu source at run time, like `node:vm`: `runInNewContext`,
  `createContext`/`runInContext`, `runInThisContext`, `Script`, `compileFunction`, with
  `timeout` and `filename`; errors come back as `error` values.
- `fogu:cli` — terminal interaction: prompts (`input password confirm select multiSelect`,
  plain lines without a terminal), styles and colors, terminal control, spinners and
  progress bars, tables and argument parsing (`cli.parse`, on `clap`).
- `fogu build --compile --target <platform>` (`linux-x64 linux-arm64 linux-armv7
  windows-x64 macos-x64 macos-arm64`, aliases such as `raspberry`, or `all`) and
  `fogu build --targets`; `docker/Dockerfile` builds the runtimes into `dist/`.
- `skills/fogu/` — an Agent Skill that teaches AI agents to use Fogu (`SKILL.md`, and
  `stdlib.md` generated with tested examples by `gen_reference.py`), and `llms.txt`.
- `fogu:mqtt` speaks MQTT 5 too (`connect(url, { version: 5, sessionExpiry })`): message
  properties (`contentType responseTopic correlationData messageExpiry userProperties`),
  subscription options (`noLocal retainAsPublished retainHandling`), reason codes in errors,
  `m.version`.
- `fogu:etcd` watches and lease keep-alives are streams (`w.next(timeout?)`, `close()`).

- `fogu:surrealkv` — SurrealKV, an embedded versioned LSM key-value store: MVCC
  transactions (`begin`/`commit`, write conflicts as error values), `scan` by prefix/range,
  `getAt`/`history` of every key, `batch`, checkpoints and encryption at rest (`key`,
  `keyFile`). Stores left open are flushed at exit.

- `scripts/install.sh` (Linux, macOS) and `scripts/install.ps1` (Windows): download Fogu (or
  copy a local build with `--from`/`-From`), put it in `~/.fogu/bin` /
  `%LOCALAPPDATA%\Fogu\bin` and add it to the user's PATH (`--system`/`-System`: for every
  user), with `--uninstall`.
- `Fogu.cores()` / `Fogu.cores(n)` / `Fogu.cores([cpus])`, `--cores`, `FOGU_CORES`: how many
  threads the runtime uses, and pinning to chosen CPUs (Linux, Windows).
- `time.monotonic()` (durations that the wall clock's jumps do not disturb).
- Benchmarks of servers and concurrency (`bench/run_net.fg`): HTTP throughput (with `oha`),
  WebSocket echo, memory per task, concurrent waits and channel round trips, against Python
  (stdlib, aiohttp, websockets) and Bun; the runners record the machine's load.
- MCP (Model Context Protocol) in `fogu:ai`: `ai.mcpServer({ name, version })` with tools
  (`ai.tool` values), resources and prompts, served over stdio (`serveStdio()`, for Claude
  Desktop/Code) or Streamable HTTP (`listen(port, { token })`, origin checks), and `handle()` for
  tests; a failing tool is reported (`isError`) without stopping the server. `ai.mcpClient`
  (stdio `{ command, args }` or HTTP `{ url }`, JSON or SSE answers): `listTools callTool
  listResources readResource listPrompts getPrompt`, and `tools()` gives `ai.tool` values for
  `ai.agent`. Tested with the official TypeScript SDK (server) and
  `@modelcontextprotocol/server-filesystem` (client). `system.readLine()` reads stdin lines.
- `fogu:surrealdb` — a SurrealDB client on the runtime's networking (no SurrealDB crate):
  JSON-RPC over WebSocket (sessions, live queries: `live`/`listen`, `next`, `kill`) or HTTP
  (`POST /rpc`, stateless); `query` (one value per statement, statement errors as error
  values), `select create insert insertRelation update upsert merge patch delete relate run`,
  `signin signup authenticate invalidate use set unset`, `define: true` for SurrealDB 3.
  Tested with SurrealDB 3.3.4.
- `fogu:topcoat` — server-rendered reactive pages for `fogu:web` in the model of Topcoat
  (tokio-rs): `h`/`raw`/`page`, signals (`tc.signal`, tracked server reads rerun the page or
  shard), `@event` handlers, `:attr` bindings and live text in the browser, shards
  (`tc.shard`, `render`, `app.shard`) swapped in place with a DOM morph, procedures
  (`app.procedure` + `call()` in the browser), link navigation, toasts; Topcoat's ndjson
  frame protocol. Its own ~18 KB browser runtime, no build step.
- UI kits: `tc.ui` (Topcoat UI's neutral theme as plain CSS: buttons, cards, stats, forms,
  tables, alerts, tabs, an app shell…) and `tc.shadcn` (the markup of shadcn-rs with its
  stylesheet: also tabs, dialog, accordion, dropdown, tooltip, pagination, avatar,
  breadcrumb, switch). Tailwind CSS: `page({ tailwind: true | "/app.css" })`, the theme
  tokens as utilities (`tc.tailwindTheme`).
- `tc.ui` kit: 35 icons, the Fogu logo and favicon, sparklines, stat cards with an icon, a
  tone and a trend line, empty states, a live indicator, a light/dark switch remembered per
  browser (`page({ theme })`, applied before the first paint); `ask()` confirmation dialogs and
  `copy()` in the browser; icons and a footer in `shell`, `grid({ min })`, `table({ header })`.

- `fogu:broker`: `b.admin(port, { user, password })` — a web management UI built with
  fogu:web + fogu:topcoat: a sign-in page with a session cookie (basic auth for scripts), a
  live overview with traffic per second and trend lines, channels and topics, a key-value
  editor, API keys, confirmation dialogs, light/dark themes and the Fogu logo;
  `b.inspect()` and `b.keys({ prefix })`; traffic counters.

### Changed

- The project is a Cargo workspace: `crates/fogu-core` (lexer, parser, resolver,
  interpreter, JIT, scheduler, the value model and the built-ins every program has:
  str/list/map/buffer methods, the globals, `fogu:math`, `fogu:json`, `fogu:test`, events,
  the shared tokio runtime; a `Host` trait for the standard library), `crates/fogu-std` (the
  standard library by area in `src/native/<area>`, plugged in by `fogu_std::register()`),
  `crates/fogu-pkg` (the package manager) and the root `fogu` package (the CLI, re-exporting
  them as `fogu::runtime`/`fogu::native`/`fogu::pkg`, so the C API and embedders are
  unchanged). The areas are cargo features, all on by default: `network database messaging
  ops hardware ai media rpc` (`base` and `io` are always built); `cargo build --release
  --no-default-features` builds the language with files, processes, sockets and data
  formats only. A module whose feature is off is not importable.
- `fogu:net` rewritten on the shared tokio runtime: `accept`, reads, writes, `recv` and
  `tcpConnect` wait cooperatively, so they no longer block the other `fg` tasks.
  **Breaking:** `read(n = 64 KiB)`, `readExact(n)`, `readAll()` and `recv(n)` return a
  `buffer` (they returned a lossy UTF-8 str), `read`/`readLine` return `nil` at the end of the
  stream (`readLine` returned `""`), `UdpPacket.data` is a buffer (`text` has the str, `from`
  the sender). New: `readExact`, `readText(n)`, `tcpConnect(addr, timeout?)`,
  `accept(timeout?)`, `write`/`writeLine` return the bytes written, `setTimeout(d)` takes a
  duration or int ms; fields `peer localAddr open`, listener `port`.
- `fogu:websocket` rewritten: every connection is a task on the shared tokio runtime (no OS
  thread per connection, no 2 ms polling); `next`/`accept` are woken when data arrives.
  **Breaking:** a binary message's `data` and `bytes` are buffers (`data` was a str and
  `bytes` a list of ints); `text` is always a str. `send(buffer)` sends a binary frame.
  `app.ws` routes of `fogu:web` run the WebSocket on the upgraded HTTP connection itself (no
  loopback socket bridge).
- HTTP server (`fogu:web` `app.listen`, `fogu:http` `serve`): requests are served as soon as
  they arrive, also while the program waits in `time.delay(...)` or on a channel (before: only
  between calls, so a `while true { time.delay(100ms) }` loop added up to 100 ms per request),
  and each request's handler runs in its own task, so a handler that waits (a query, an HTTP
  call, `time.delay`) does not hold the others. 64 keep-alive clients: with
  `Fogu.signal.wait()` 11.9k → 27.1k req/s (p50 4.95 → 1.46 ms); with the
  `time.delay(100ms)` loop 1.2k → 24.9k req/s (p99 112 → 15 ms). WebSocket echo (50
  connections): 15.1k → 28.8k messages/s.
- Interpreter: strings are reference-counted copy-on-write (reading a variable, passing an
  argument or iterating no longer copies the text); `s = s + x` / `s += x` appends in place;
  `s[i]` and `s.len()` are O(1) on ASCII text; loop bodies reuse their scope between
  iterations; scope lookups use FxHash; finished `fg` tasks give their stack back to a pool
  (no mmap/munmap per task or per Socket.IO event); on 32-bit targets a task reserves 2 MB of
  stack instead of 32 MB. 100k string appends 1.74 s → 0.04 s, 40k string indexing 3.36 s →
  0.03 s, iterating a 200k list ×10 3.98 s → 0.98 s, 100k `fg` spawn + finish 2.13 s → 0.18 s.
- Memory per parked `fg` task: ~9.3 KB → ~5.2 KB (100k parked tasks: 925 MB → 525 MB). The
  interpreter's `eval` / `exec` / `call_value` are small dispatchers and each kind of
  expression, statement and callee has its own non-inlined method, so a call costs ~1.8 KB
  of native stack instead of ~3 KB (`eval` 1208 → 488 bytes); recursion inside a task goes
  ~11k → ~19k levels deep. No measurable change in speed.
- Cross builds for ARM Linux from x86_64: OpenSSL is vendored for the ARM Linux targets and
  `.cargo/config.toml` sets the linkers, so `cargo build --release --target
  aarch64-unknown-linux-gnu` (or `armv7-unknown-linux-gnueabihf`) works with only
  `gcc-aarch64-linux-gnu` / `gcc-arm-linux-gnueabihf` installed.
- Memory per `fg` task: ~25 KB → ~9 KB. Task stacks opt out of transparent huge pages
  (`MADV_NOHUGEPAGE`, Linux), so a first fault no longer commits a 2 MB page, and the JIT
  answers calls of functions it already rejected before entering its compile path (which
  touched an extra stack page in every task).
- `fogu:broker` stores KV and API keys in SurrealKV (a folder) instead of redb; writes go in
  order through a background writer and are flushed when the program exits. A redb file
  from an earlier version is converted on open (kept as `<db>.redb.bak`).
- Benchmarks against Python and Bun (`bench/`: the same 8 programs in `.fg`, `.py`, `.js`,
  `fogu bench/run.fg` checks their outputs and times them; `docs/benchmarks.md` and the docs
  site page are generated from the results by `docs/tools/gen_bench.py`).
- Guides in the docs site: 13 step-by-step guides (CLI tool, REST API, Topcoat app,
  concurrency, errors, storage, realtime, broker, automation, AI, embedding, deploy); their
  programs live in `examples/guides/` and are run by `docs/tools/gen_guides.py`.
- Native modules are built on their first import instead of at startup: running a script
  went from ~50 ms to ~9 ms (Python 3.10: ~20 ms, Bun: ~19 ms on the same machine), and the
  spec suite from 26 s to 16 s.

- Dependency versions aligned so the tree no longer carries two copies of the same crates:
  `scraper` 0.29 (one html5ever with `ammonia`), `aes-gcm` 0.11 (shared with SurrealKV),
  `ndarray` 0.17 (shared with tract), `resvg` 0.48 (the image codecs of `image`): 616 → 599
  crates. Data encrypted with `crypto.encrypt` before still decrypts.
- `Fogu.signal.wait()` answers `fogu:web`/`fogu:http` requests while it waits (it used to
  leave them hanging); the HTTP server thread wakes the scheduler when a request arrives.
- Fewer dependencies (726 → 660 crates on Linux): the Socket.IO client (rust_socketio),
  DNS (hickory-resolver), etcd (etcd-client), SMTP/MIME (lettre), MQTT (rumqttc) and AMQP
  (lapin) are now Fogu's own code on the runtime's networking (`websocket::dial`, the
  shared tokio runtime, tonic + the official etcd protos). The Fogu APIs are the same;
  each was checked against a real server (socket.io 4.8, public DNS, etcd 3.5, a Python
  SMTP server with STARTTLS/TLS/AUTH, Mosquitto 2.0 with v3 and v5, RabbitMQ 3.13).
- The Socket.IO client uses the WebSocket transport only (`transport: "polling"` is an
  error); like the JavaScript client, the url's path is the namespace.

### Fixed

- `fogu:net`: `readLine` lost the data buffered after the line (two lines in one packet gave
  only the first); `read` corrupted binary data (it was decoded as UTF-8).
- WebSocket connections waited up to 15 ms to send each message (the I/O thread only sent
  between reads with a timeout) and did not wake the program when one arrived: on Unix the
  thread now sleeps in `poll()` on the socket and a wake-up pipe. Echo throughput went from
  ~1,600 to ~19,000 messages/s (50 connections).
- `fogu:web`: requests given to `app.handler()` (tests) are now the same `Request` object as
  real HTTP ones, so `ctx.req.json()`, `form()` and `bytes()` work in tests too (a map body is
  sent as JSON); a handler or middleware that does `return value, error(...)` goes to
  `onError` instead of answering `[null, "<error>"]`.
- `list.sort()` was a binary insertion sort: O(n²) element moves, so sorting 300k numbers took
  minutes. It is now a stable merge sort with the same results and comparator rules; lists of
  only integers, only strings or only numbers use the standard library's sorts.
- `json.stringify` wrote invalid JSON for control characters (`\u{1b}` instead of `\u001b`)
  and for NaN/infinity (now `null`); `json.parse` accepted any character where an object key's
  opening quote belongs and dropped `\ud83d\ude00`-style surrogate pairs. Both were rewritten:
  one output buffer, a byte parser without copying the text (JSON about 30% faster).
- `list.join()` writes into one buffer.
- Ctrl+C / SIGTERM left sshfs mounts mounted, browser processes running and the last
  SurrealKV/broker writes unflushed: once such a resource exists, the signal runs the
  cleanup before the program ends (exit 130/143); a program that handles the signal with
  `Fogu.signal` keeps control. The `Fogu.signal` exit path cleans up too.
- `ssh.mount`: `unmount()` no longer waits while holding the object (another task using it
  panicked); on Windows the local side must be a drive letter, a key other than
  `~/.ssh/id_rsa` and `readonly` are refused (SSHFS-Win would ignore them); IPv6 hosts
  without a user are bracketed. Tested with sshfs 3.7.1 against OpenSSH 8.9.
- `fogu:surrealkv`: stores left open are flushed at exit (writes were lost); commits wait
  outside `call`, so several tasks can use one store.
- `fogu:socketio` server handlers ran out of order under load (socketioxide spawns each
  one); messages are numbered when they arrive and dispatched in order.
- `fogu:etcd` `watch`/`keepAlive` returned only the first event, and the module built its
  own tokio runtime.
- `security.dns` failed for large answers (TXT of big domains): truncated UDP answers are
  retried over TCP.
- `fogu:rabbitmq`: a wrong password is reported as `ACCESS_REFUSED` instead of a reset
  connection.

- The current file and line of each `fg` task are kept across task switches: an error in
  the main program after a task yielded could be reported with the task's file and line.
- `fogu:vm` timeouts were global: a `vm` run with a timeout that yielded made every other
  task time out (and lose the JIT); deadlines are now per task.
- `http.post(url, map)` sent JSON as `text/plain`.
- `fogu:fogumq`: a forwarded RPC call whose id matched a pending request of the client was
  taken for the reply (the broker now numbers forwarded calls apart, and the client treats
  `CALL` frames as calls).

### Removed

- `fogu:desktop`, `fogu init --desktop`, `fogu desktop dev|build|doctor`, the `desktop`
  cargo feature and the wry/tao dependencies (they were never in a release).
  `fogu:webview` (the Chrome DevTools driver) stays.

## [0.7.0] — 2026-10-10

### Added

- `fogu fmt` — formats `.fg` files in a single style (indentation, spacing, blank lines);
  strings and comments are kept, and every file is verified against the lexer before it
  is written. `--check` for CI, `-` for stdin.
- `Fogu.watch(paths, { recursive, debounce })` — file system events (`create modify remove
  rename`) through `w.next(timeout?)`.
- `fogu:ffi` — calls C functions of native libraries (`ffi.open("c").func("strlen", ["str"],
  "usize")`), with buffers for out-parameters.
- `fogu:playbook` and `fogu play` — Ansible-style configuration management: inventories,
  plays, idempotent tasks (`shell command copy template file lineinfile package service
  debug set_fact fail wait_for`), `when`, `loop`, `register`, handlers, `--check`, tags,
  limits, facts; over SSH or locally (also on Windows), hosts in parallel.

## [0.6.0] — 2026-10-10

### Added

- `fogu:web` serves **static files and SPA builds** (`app.static(prefix, dir, { spa, maxAge })`:
  ETag/304, `Cache-Control`, `index.html` fallback for React/Vue/Svelte routes), **uploads**
  (`ctx.req.form()` with multipart files: `save(path)`, `text()`, `bytes()`), **downloads**
  (`web.file(path, { download })`), **Server-Sent Events** (`web.sse()`) and **WebSocket**
  routes (`app.ws(path, …middlewares, (ws, ctx) => …)`, built on `fogu:websocket`).
  `app.listen(port, { host, maxBody, quiet })`; request bodies are binary-safe.
- `fogu:websocket` — client (`ws://`, `wss://`) and standalone server.
- `fogu:mail` — SMTP (Gmail, Outlook, Yahoo, iCloud, SES… presets) and the APIs of
  Resend, SendGrid, Mailgun, Postmark, Brevo, MailerSend and Mailjet; attachments, inline
  images, a `console` provider.
- `fogu:ai` — chat, streaming, tools, agents, models and embeddings for OpenAI-compatible
  APIs (OpenAI, Groq, OpenRouter, DeepSeek, Mistral, xAI, Ollama, LM Studio…), Anthropic and
  Gemini.
- `fogu:shell` — a POSIX-style shell (pipes, redirections, `$VAR`, `$(…)`, globs) with the
  usual Linux commands built in, so scripts run the same on Windows.
- `fogu:s3` (AWS S3, MinIO, R2… with Signature V4, presigned URLs), `fogu:redis`
  (every command as a method, pipelines, transactions, pub/sub), `fogu:nats` (with
  JetStream), `fogu:rabbitmq` (AMQP 0-9-1, publisher confirms), `fogu:mqtt`.
- `fogu:regex`, `fogu:crypto` (hashes, HMAC, AES-256-GCM, argon2 passwords, random,
  base64/hex), `fogu:log` (levels, fields, child loggers, text/JSON, files).
- Data formats: `fogu:csv`, `fogu:excel` (xlsx read/write), `fogu:yaml`, `fogu:toml`,
  `fogu:xml` — text or files to Fogu values and back.
- `fogu build --compile` embeds data files (`"embed"` in fogu.json, `--embed <glob>`);
  `Fogu.asset(path)` finds them (also when running from the sources).
- `Fogu.write(...)` (print without a newline).
- `fogu:kv` stores maps.
- The standard library reference in `docs/docs.html` lists every function with its
  parameters and examples with their real output (`docs/tools/gen_reference.py`).

### Changed

- Waiting I/O no longer stops the other `fg` tasks: `http` client calls,
  `process.run`, `fogu:shell` (and its `sleep`), `ssh.run/upload/download` and `sql`
  statements run on a shared tokio runtime / thread pool while the task waits.
  Programs without `fg` run them inline, as before. The scheduler is woken by
  finished I/O instead of polling every 2 ms.
- Smaller binary and fewer dependencies: `ureq` replaced by `reqwest` (one TLS stack,
  native-tls; `rustls`/`ring` are gone), `full` features of tokio/hyper/hyper-util and
  the default features of reqwest/sqlx (macros, migrate, http2) dropped, `notify` 8.
  `[profile.release]` strips symbols and uses thin LTO (49 MB → 35 MB on linux-x64);
  `cargo build --profile dist` (fat LTO, one codegen unit) gives the smallest binary.
  Debug builds keep only line tables (faster linking).
- `fogu:fogumq` now speaks the Fogu broker protocol (17-byte header, JSON
  payloads, `fogu://token@host:port`): `kv`, channels (`join send onMessage
  members`), pub/sub, RPC (`act`/`call`) and `apikeys`. The v3 API (`fogumq://`,
  `request serve enqueue worker`, `matchTopic`) was removed.
- `time.delay`, `time.sleep` and `Fogu.sleep` let the other `fg` tasks run while they wait
  (they used to block every task).
- Dependencies are compiled with optimizations in debug builds.

### Fixed

- `http.serve` crashed on every response after the web server changes of this version.
- `fogu:nats` disconnected after 5 idle seconds on Windows (a socket timeout was set on a
  cloned handle).

## [0.5.0] — 2026-10-10

### Added

- `fogu:orm` — a native Prisma-style ORM over SQLite, PostgreSQL and MySQL:
  `create_client({ schema: { provider, url, models }, migrate: "auto", log })`;
  per model `create createMany findMany findFirst findUnique update updateMany
  upsert delete deleteMany count`; `where` (`equals not in notIn lt lte gt gte
  contains startsWith endsWith mode AND OR NOT`), `select`, `orderBy`, `take`,
  `skip`, nested `include` of one-to-many / many-to-one / one-to-one relations
  (one query per level); defaults `"now"` / `"uuid"`; `{ increment, decrement,
  multiply, divide, set }` updates; `migrate: "auto"` creates tables with
  foreign keys and adds new columns.
- `fogu:web` middleware: `use(fn)`, `use(prefix, fn)`, per-route middleware
  `get(path, mw…, handler)`, `after((ctx, res) => …)`, `onError((ctx, err) => …)`;
  a middleware returning a response stops the request; `ctx.set.status` /
  `ctx.set.headers`; `ctx.query`, `ctx.headers`, `ctx.body`; `web.cors()`,
  `web.logger()`, `web.redirect()`; `head`/`options` routes.
- `fogu:jwt` — `sign`, `verify` (`issuer audience subject algorithms leeway
  maxAge`), `decode` with HS256/HS384/HS512, and `jwt.middleware(secret)` for
  `fogu:web` (`ctx.user`, or 401).
- `fogu:ssh` — commands, scp `upload`/`download`, local (`-L`) and remote (`-R`)
  tunnels over the system OpenSSH client, with key or password authentication.
- `fogu:scylla` — ScyllaDB/Cassandra client: prepared statements with typed
  parameters, collections, UDTs, address translation for nodes behind Docker.
- `fogu:fogumq` — FoguMQ broker client (protocol v3): pub/sub with `*`/`>`
  wildcards, request/reply, job queues (`ack nack extend`), API keys.
- Native objects can implement asynchronous methods (`FoguObject::start_async`),
  polled cooperatively so other `fg` tasks keep running.
- Any keyword can be a map key: `{ select: …, default: … }`.

### Changed

- `app.get(...)` and the other `App` methods return the same app, so chained
  calls register on it (they used to modify a copy). Several apps can serve at
  once (each handler is bound to its app); `app.store` is shared between requests.
- Middleware errors are no longer ignored: they go to `onError` or become a 500.

## [0.4.0] — 2026-10-10

### Added

- `fogu:fs` — `readText readLines writeText append exists isFile isDir mkdir
  remove removeAll copy move list stat glob cwd tempDir`.
- `fogu:path` — `join dirname basename stem extname normalize isAbsolute absolute`.
- `fogu:process` — `run(cmd, args, {cwd, env, input})` → `{ok, code, stdout, stderr}`,
  `shell`, `env`, `setEnv`, `cwd`, `chdir`, `exit`, `pid`, `os`, `arch`, `args`.
- `fogu:sync` — `Mutex`, `WaitGroup`, `Once`, `Atomic`; blocking operations
  yield cooperatively to the other `fg` tasks (deadlocks are reported).
- `fogu:test` and the `fogu test` command: `*.test.fg` files, `test`, `skip`,
  `expect(v).toBe/toEqual/toContain/…`, `.not`, `-t` filter.
- The global `Fogu` object (spec §45): `file env args sleep exit hash glob run sql version os`.
- Arrow functions accept a return type: `(x): int => x * 2`.
- CLI integration tests (`tests/cli.rs`) for `init`, `test` and `build --compile`.

### Changed

- `?` keeps the declared return type: in `fn f(): error` it returns just the
  error instead of the `(nil, err)` pair.

### Fixed

- A `<-` at the start of a line is a receive that begins a new statement (it
  was parsed as a send continuing the previous line).
- `db.driver` for `sqlite::memory:` is `sqlite`.

## [0.3.0] — 2026-10-10

### Added

- **Syntax**: template strings `` `Hello, ${name}` ``; durations `500ms 2s 5m 1h 1d`
  (`duration` type); optional access `?.` short-circuiting the whole chain; arrow
  functions `(x) => x * 2`; default parameter values; variadic parameters
  `...args`; `for k, v in map` and `for i, x in list`; `export struct`.
- **Types**: annotations on parameters, return types and `let`/`const`, checked
  at runtime (`int?`, `A | B`, `(int, error)`, `list<T>`…); structural
  `interface`; generics `fn f<T>`, `struct S<T>`, `new S<int>()` (erased).
- **Semantics**: `equals(other)` on structs for `==`/`!=`; `_x` names private to
  the module (imports, `module._x`, struct fields and methods); `timeout(d)` for
  `select`.
- **Errors**: operational errors of native functions (IO, network, SQL, JSON,
  HTTP) are `error` values — they work with `?` and `const [v, err] = f()`.
- **Static checks** before running and in `fogu check`: undefined variables (with
  a "did you mean" suggestion), use before declaration, unknown types, `?` in a
  function whose return type has no `error`, non-exhaustive `match`; unused
  variables are reported as warnings by `fogu check`.
- **Diagnostics** in the spec §58 format: `error[E0201]`, `--> file:line:column`,
  excerpt with `^^^` and `= help:`.
- `fogu build --compile`: standalone executable with the program, its modules
  and its packages embedded.
- `fogu init` also creates a `.gitignore`.
- `--strict`: an error in an `fg` task terminates the process.
- `http.text/json/html` and `web.text/json/html` accept `{ status, headers }`.
- Conformance suite `tests/spec/` (`cargo test`), each test runs with and without the JIT.
- Official HTML documentation ([docs/docs.html](docs/docs.html)), `LICENSE` (MIT),
  `CONTRIBUTING.md`, this `CHANGELOG.md`.

### Changed

- The whole project (messages, CLI, documentation, examples, tests, comments) is in English.
- Redeclaring a name in the same scope is an error (the REPL still allows it).
- Constructors accept fewer arguments (the rest are `nil`/default).
- `?.` is now optional access; to propagate and then access use `(f()?).field`.
- Errors inside functions of another module point to that module's file.
- Standard Rust project layout: `src/lib.rs` + `src/main.rs`, tests in
  `tests/`, benchmarks in `benches/`, specification in `docs/spec.md`, design
  notes in `docs/design/`. The package was renamed to `fogu`.

### Fixed

- `json.stringify` writes the keys in order (deterministic output).
- `http.text("…", { status: 404 })` answered 500.
- `fogu:sql` uses a single persistent connection: transactions (`begin`/`commit`/`rollback`)
  now apply to the following statements, and `sqlite::memory:` keeps its tables.

## [0.2.0]

- Interpreter with positioned errors, `error`/`?`/`panic`, collections with
  methods, random `select`, cooperative `fg` tasks with deadlock detection.
- Cranelift JIT for numeric code (functions and OSR loops).
- Bun-style CLI, REPL and git-based package manager with `fogu.lock`.
- Modules `fogu:math json system time net cron events kv sql http web`.

Detailed history: [docs/analysis.md](docs/analysis.md).
