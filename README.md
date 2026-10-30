# Fogu

**Fogu** (`.fg` files) is a small, dynamically typed scripting language with
optional type annotations, built for **automation, infrastructure, systems
integration and small services**. This repository is the reference
implementation in Rust: interpreter + **JIT compiler (Cranelift)**, REPL, CLI,
git-based package manager and standalone executables.

```fogu
import http from "fogu:http"
import time from "fogu:time"

http.serve(3000, fn(req) {
    if req.path == "/health" {
        return http.json({ status: "ok" })
    }
    return http.text("not found", { status: 404 })
})

while true { time.delay(100ms) }
```

> 📖 **Official documentation:** open [docs/docs.html](docs/docs.html) in a browser.
>
> Version **0.7.0** — in development. The full language specification
> (Draft 0.4) is in [docs/spec.md](docs/spec.md); the status of every feature
> is tracked in [CHECKLIST.md](CHECKLIST.md).

---

## Contents

- [Installation](#installation)
- [Getting started](#getting-started)
- [The language](#the-language)
- [Standard library](#standard-library)
- [CLI](#cli)
- [Packages](#packages)
- [Tests](#tests)
- [Standalone executables](#standalone-executables)
- [JIT](#jit)
- [For AI agents](#for-ai-agents)
- [Repository layout](#repository-layout)
- [Status and limitations](#status-and-limitations)
- [License](#license)

---

## Installation

The install scripts download the executable, put it in a folder and add that folder to the
`PATH` (`scripts/`):

```bash
# Linux / macOS — ~/.fogu/bin, added to your shell's PATH (--system: /usr/local/bin)
curl -fsSL https://<fogu-server>/install.sh | sh   # the Fogu website (server/) shows the exact command
sh scripts/install.sh --from target/release/fogu     # install a local build instead
sh scripts/install.sh --uninstall
```

```powershell
# Windows — %LOCALAPPDATA%\Fogu\bin, added to your user PATH (-System: Program Files + system PATH)
powershell -c "iex (irm https://<fogu-server>/install.ps1)"
.\scripts\install.ps1 -From target\release\fogu.exe
.\scripts\install.ps1 -Uninstall
```

### Build from source

Requires Rust (2024 edition) and `git` on the `PATH` (for packages).

```bash
cargo build --release
# binary: target/release/fogu  (Windows: target\release\fogu.exe)
cargo test --release      # CLI tests, conformance suite and the crates' unit tests
# smaller builds: the standard library areas are cargo features (all on by default)
cargo build --release --no-default-features                       # language + files, processes, sockets, data formats
cargo build --release --no-default-features --features network,database
```

Features: `network` (http, web, websocket, socketio, topcoat, jwt, html, mail,
security), `database` (sql, orm, kv, surrealkv, surrealdb, scylla, redis, etcd),
`messaging` (nats, mqtt, kafka, rabbitmq, bullmq, fogumq, broker), `ops` (ssh, docker,
service, playbook, s3), `ai` (ai/MCP, plot), `media` (image, webview), `rpc` (grpc, protobuf). `database`
turns on `rpc`; `ai` turns on `media` and `network`. The core modules (time, sync, cron,
log, regex, crypto, vm, cli, ffi, fs, path, process, system, shell, zip, csv/json/yaml…,
net) are always built. A module whose feature is off cannot be imported (unknown module).

Put the binary on your `PATH`, or use `cargo run -- <args>` while developing.

## Getting started

```bash
fogu                       # REPL
fogu examples/basics.fg    # run a file
fogu -p '1 + 2 * 3'        # evaluate and print: 7

fogu init -y               # create fogu.json, main.fg and .gitignore
fogu run                   # run the "entry" of fogu.json
fogu --watch main.fg       # restart when *.fg / fogu.json change
fogu check                 # check syntax, names and types without running
fogu build --compile       # standalone executable in dist/
fogu test                  # run the *.test.fg tests
```

---

## The language

### Variables and types

```fogu
let counter = 0           // mutable
const name = "Fogu"       // immutable binding (the object may still change)

print(typeof(42))         // int    (64-bit signed; 1_000_000, 0xff, 0b1010)
print(typeof(3.14))       // float  (IEEE 754; 1e-9)
print(typeof("txt"))      // str    ('...' or "...", UTF-8, immutable)
print(typeof(true))       // bool
print(typeof(nil))        // nil
print(typeof(500ms))      // duration (500ms 2s 5m 1h 1d; also 1.5s)
print(typeof(bytes("hi")))  // buffer (mutable bytes: b[0], b.hex(), b.readU16(0))

const v = 4
print(`Hello, ${name} v${v + 0.1}`)   // template string: Hello, Fogu v4.1
```

Types: `nil bool int float str duration list map buffer tuple fn struct chan error`.

- **No implicit coercions**: `"10" + 2` is an error. Use `int("10")`, `str(10)`,
  `float(10)`. The only automatic promotion is `int → float` in arithmetic.
- **Truthiness**: only `false` and `nil` are falsy (`0`, `""`, `[]`, `{}` are truthy).
- **Equality** `==` is structural for lists and maps; structs compare by identity.
- **Lexical scope**, no hoisting; each block creates a scope. Declaring the same
  name twice in the same scope is an error (an inner block may shadow it).
- **Durations**: `1s + 500ms == 1500ms`, `2s * 3`, `1m / 2s == 30.0`, `1m > 59s`;
  fields `d.ns d.us d.ms d.seconds d.minutes d.hours`.
- Comments `// line` and `/* block */`; semicolons are optional.

### Type annotations

Annotations are optional and **checked at runtime**; unknown type names are
reported before the program runs.

```fogu
fn add(a: int, b: int): int { return a + b }
fn half(x: float): float { return x / 2.0 }       // an int is accepted as float
fn find(xs: list<int>, v: int): int? { ... }      // int? = int | nil
fn parse(s: str): (int, error) { ... }            // multiple return values
let label: str | nil = nil

add(1, "2")
// error[E0207]: type mismatch: parameter 'b' expects int, got str
```

Generic arguments (`list<int>`) document the intent; only the outer kind is
checked. Annotated numeric functions are still compiled by the JIT.

### Operators

| Precedence (high → low) | |
| --- | --- |
| `()` `[]` `.` `?.` | call, index, field, optional access |
| `!` `-` `<-` | unary, channel receive |
| `**` | power (right-associative) |
| `*` `/` `%` · `+` `-` | **checked** arithmetic (overflow and division by zero are errors) |
| `<` `<=` `>` `>=` · `==` `!=` | comparison (always false with NaN) |
| `&&` · `\|\|` `??` | logical and nil-coalescing |
| `..` | half-open range `[a, b)` |
| `=` `+=` `-=` `*=` `/=` `%=` | assignment (also to fields and indices) |

### Control flow

```fogu
if age >= 18 { print("adult") } else { print("minor") }

while counter < 3 { counter += 1 }

loop {                        // the same as `while true`: until `break` or `return`
    counter -= 1
    if counter == 0 { break }
}

for i in 0..5 {
    if i == 2 { continue }
    if i == 4 { break }
    print(i)
}

for item in [1, 2, 3] { print(item) }
for key, value in { a: 1, b: 2 } { print(key, value) }   // alphabetical order
for i, item in ["x", "y"] { print(i, item) }             // index and item

const kind = match n {
    0 => "zero"
    1 | 2 => "small"
    x if x < 0 => "negative"
    _ => "large"
}
```

A `match` must be exhaustive: without a `_` (or plain name) arm it is rejected
before the program runs.

### Lists, maps and strings

```fogu
let xs = [5, 3, 8]
xs.push(1)
print(xs[0], xs[-1], len(xs))
print(xs.sort(), xs.map((x) => x * 2).join(","))

let u = { name: "John", age: 30 }
print(u.name, u["age"], u.country)      // missing key → nil
print(u.keys(), u.has("name"))
print(u?.address?.city ?? "?")          // ?.: if anything in the chain is nil, nil

print("a,b".split(","), "fogu".upper(), "  x ".trim())
```

Methods work as methods (`xs.push(1)`) or as functions (`push(xs, 1)`):

- **Lists**: `push pop shift unshift insert remove first last contains index_of join
  reverse slice concat sort map filter each find any all reduce`
- **Maps**: `keys values entries has get set remove merge`
- **Strings**: `split upper lower trim replace starts_with ends_with repeat lines chars slice`

### Functions

```fogu
fn sum(a, b) { return a + b }

fn connect(host = "localhost", port = 80) {            // default values
    return `${host}:${port}`
}

const double = fn(x) { return x * 2 }                   // anonymous, with closures
const triple = (x) => x * 3                             // arrow function
const mean = (...xs) => xs.reduce((a, b) => a + b, 0) / len(xs)   // variadic

fn first<T>(items: list<T>): T? {                       // generics (erased at runtime)
    if len(items) == 0 { return nil }
    return items[0]
}
```

### Errors as values

There is no `try/catch`. Functions return a `value, error` tuple:

```fogu
fn divide(a, b) {
    if b == 0 {
        return nil, error("division by zero")
    }
    return a / b, nil
}

fn compute(a, b): (int, error) {
    const x = divide(a, b)?         // on error, returns (nil, err) to the caller
    return x * 2, nil
}

const [r, err] = compute(1, 0)
if err != nil {
    print(err.message, err.trace)   // also: err.cause  (error(msg, cause))
}
```

`?` can only be used in functions whose declared return type includes `error`
(or that have no annotation). Operational errors of native functions (a file
that does not open, a refused connection, invalid SQL or JSON) are `error`
values too:

```fogu
const db = kv.open("data.redb")?            // propagates the error
const [conn, err] = net.tcpConnect("host:9000")
if err != nil { print("no connection:", err.message) }
```

`panic("msg")` signals unrecoverable failures; in an `fg` task it does not stop
the other tasks (use `--strict` to terminate the process).

Errors are shown with a code, position and source excerpt:

```text
error[E0100]: variable 'usr' is not defined
  --> main.fg:3
   |
 3 |     print(usr.name)
   |           ^^^
   = help: did you mean 'user'?
```

### Structs, interfaces and composition

No inheritance — reuse through `embed`; contracts through structural interfaces:

```fogu
struct Logger {
    prefix = "[log]"
    fn log(msg) { print(this.prefix, msg) }
}

struct Service {
    embed Logger              // Logger's fields and methods become available
    name = "api"
    fn start() { this.log(`service ${this.name} starting`) }
}

interface Startable { start() }
fn boot(s: Startable) { s.start() }   // any struct (or map of fns) with start()
boot(new Service())
```

Structs compare by identity; with an `equals(other)` method, `==` and `!=` use
it. `export struct Account { ... }` shares a type between modules; fields and
methods starting with `_` are only accessible in the module where the struct
was declared. Structs can be generic: `struct Stack<T> { ... }`, `new Stack<int>()`.

### Concurrency: `fg`, `chan`, `select`

```fogu
let c1 = chan(1)              // buffered channel (chan() = unbuffered / rendezvous)
let c2 = chan<int>(buffer: 1)

fg fn() { c1 <- 10 }()        // cooperative task
fg () => { c2 <- 20 }()

select {                      // picks a random ready case (like Go)
    case v = <-c1: print("c1:", v)
    case v = <-c2: print("c2:", v)
    case <-timeout(500ms): print("timed out")
}

close(c1)                     // receives drain the buffer and then end
```

The runtime detects deadlocks between tasks.

### Modules

```fogu
// util.fg
export fn sum(a, b) { return a + b }
export const PI = 3.14159
fn _helper(x) { return x * 2 }         // `_` prefix = private to the module

// main.fg
import { sum, PI } from "./util.fg"
import { sum as add } from "./util.fg"
import util from "./util.fg"           // namespace
import json from "fogu:json"           // standard library
import greet from "greet"              // package (see Packages)
```

Each module is evaluated once; cycles are a load error.

---

## Standard library

**Globals**: `print len typeof str int float bytes clock close timeout error panic`.

| Module | Contents | Example |
| --- | --- | --- |
| `fogu:math` | `sqrt abs floor ceil pow`, `PI`, `E` | [system.fg](examples/system.fg) |
| `fogu:json` | `parse`, `stringify` (sorted keys) | [modules.fg](examples/modules.fg) |
| `fogu:system` | `time()`, `exit()`, `info.{os arch family cores pid env() args() sleep_ms()}` | [system.fg](examples/system.fg) |
| `fogu:time` | `now()` (ms), `delay(d)`, `sleep(d)`, `Date()` with `getFullYear… toISOString diff addMs` | [time.fg](examples/time.fg) |
| `fogu:cron` | `every(d, fn)`, `schedule("*/5 * * * *", fn)`, `tick()`, `list()`, `clear()` | [cron.fg](examples/cron.fg) |
| `fogu:events` | `EventEmitter()` with `on once off emit listeners eventNames hasListeners removeAll` | [events.fg](examples/events.fg) |
| `fogu:net` | TCP: `tcpListen`, `tcpConnect` (`read readExact readLine readText readAll write writeLine`, data as `buffer`); UDP: `udpBind`; `resolve` — waits without blocking other tasks | [tcp_server.fg](examples/tcp_server.fg), [udp_server.fg](examples/udp_server.fg) |
| `fogu:http` | server `serve(port or "ip:port", fn(req))`, responses `text json html` (with `{status, headers}`); client `get post put delete` | [http_server.fg](examples/http_server.fg), [http_client.fg](examples/http_client.fg) |
| `fogu:web` | framework: routes with `:params`, middleware (`use`, per route, `after`, `onError`), `static` files and SPA builds, uploads (`req.form()`), `web.file`, SSE (`web.sse`), WebSocket routes (`app.ws`), `cors()`, `logger()` | [web_fullstack.fg](examples/web_fullstack.fg), [web_auth.fg](examples/web_auth.fg) |
| `fogu:kv` | embedded key-value store (redb): `open` → `set get has delete keys len clear close` | [kv.fg](examples/kv.fg) |
| `fogu:surrealkv` | versioned LSM key-value store (SurrealKV): MVCC transactions, `scan`, `getAt`/`history`, checkpoints, encryption at rest | [surrealkv.fg](tests/spec/surrealkv.fg) |
| `fogu:sql` | SQLite, PostgreSQL, MySQL/MariaDB (SQLx) by URL: `exec run query one begin/commit/rollback` | [sql.fg](examples/sql.fg) |
| `fogu:fs` | `readText readLines writeText append exists isFile isDir mkdir remove removeAll copy move list stat glob` | [files_processes.fg](examples/files_processes.fg) |
| `fogu:path` | `join dirname basename stem extname normalize isAbsolute absolute` | [files_processes.fg](examples/files_processes.fg) |
| `fogu:process` | `run(cmd, args, {cwd, env, input})` → `{ok, code, stdout, stderr}`, `shell`, `env`, `cwd`, `exit`, `pid` | [files_processes.fg](examples/files_processes.fg) |
| `fogu:sync` | `Mutex`, `WaitGroup`, `Once`, `Atomic` for `fg` tasks | [sync.fg](examples/sync.fg) |
| `fogu:test` | `test`, `skip`, `expect(v).toBe/toEqual/toContain/toBeError/…`, `.not` — run with `fogu test` | [test/math.test.fg](examples/test/math.test.fg) |
| `fogu:orm` | Prisma-style ORM over SQL: `create_client({schema, migrate: "auto"})`, `db.user.create/findMany/update/upsert/delete/count`, `where`, `include`, `select`, `orderBy` | [orm/native.fg](examples/orm/native.fg) |
| `fogu:surrealdb` | SurrealDB client (own JSON-RPC over WebSocket/HTTP): `query`, CRUD, `relate`, `run`, live queries, auth | [surrealdb.fg](examples/surrealdb.fg) |
| `fogu:ai` MCP | `ai.mcpServer` (tools, resources, prompts over stdio or HTTP — for Claude Desktop/Code) and `ai.mcpClient` (use any MCP server; `tools()` for agents) | [mcp.fg](tests/spec/mcp.fg) |
| `fogu:topcoat` | server-rendered reactive pages for `fogu:web` (Topcoat's model): `h`, `page`, signals, `@event`/`:bind`, shards, procedures, link navigation; UI kits `tc.ui` (Topcoat UI) and `tc.shadcn` (shadcn-rs), Tailwind CSS | [topcoat.fg](tests/spec/topcoat.fg) |
| `fogu:jwt` | `sign verify decode` (HS256/384/512) and `middleware(secret)` for `fogu:web` | [web_auth.fg](examples/web_auth.fg) |
| `fogu:ssh` | `connect` (also through a `jump` host) → `run upload download forward reverse mount close` (system OpenSSH; `mount` via sshfs) | — |
| `fogu:scylla` | ScyllaDB/Cassandra: `connect` → `execute query one use close`, prepared statements | — |
| `fogu:fogumq` | Fogu broker: `connect("fogu://token@host:port")` → `kv`, `channel(name)` (join send onMessage subscribe publish act call), `apikeys` | [fogumq.fg](examples/fogumq.fg) |
| `fogu:foguMQ-client` | FlowMQ v6 client: `connect("flowmq://token@host:port")` → `channelOpen`, `subscribe/publish/publishSync`, `serve/request`, `registerWorker/enqueue` (Job `ack nack extend`), `apikey*` | — |
| `fogu:ffi` | native libraries (C ABI): `open` → `func(symbol, argTypes, returnType)`, `alloc` buffers | — |
| `fogu:playbook` | Ansible-style configuration management over SSH or locally; also `fogu play` | [playbook/](examples/playbook/site.yml) |
| `fogu:bullmq` | BullMQ job queues on Redis (compatible with Node's BullMQ v5): `queue worker events`, retries, schedulers | [bullmq.fg](examples/bullmq.fg) |
| `fogu:vm` | runs Fogu code at run time (`node:vm` style): `runInNewContext createContext runInContext Script compileFunction`, timeouts | [vm.fg](examples/vm.fg) |
| `fogu:cli` | terminal: prompts, colors, spinners, progress bars, tables, argument parsing | [cli.fg](examples/cli.fg) |
| `fogu:websocket` | client (`ws://`, `wss://`) and server: `connect listen accept send next close`; binary frames as `buffer` | [websocket.fg](tests/spec/websocket.fg) |
| `fogu:mail` | SMTP (Gmail, Outlook, SES… presets) and Resend, SendGrid, Mailgun, Postmark, Brevo, MailerSend, Mailjet APIs | [mail.fg](examples/mail.fg) |
| `fogu:ai` | LLMs (OpenAI-compatible, Anthropic, Gemini): `chat stream models embed`, tools, `agent` | [ai.fg](examples/ai.fg) |
| `fogu:shell` | POSIX-style shell with built-in Linux commands: `run exec text lines json quote cd` | — |
| `fogu:s3` | S3-compatible storage: `put get list head copy delete presign upload download` | [s3.fg](examples/s3.fg) |
| `fogu:redis` | every Redis command as a method, `pipeline`, `transaction`, `subscribe` | [redis.fg](examples/redis.fg) |
| `fogu:nats` | `publish subscribe request`, JetStream streams and pull consumers; `nats.fogu()` (KV stores, `sub`/`pub`, services, commands) | [nats.fg](examples/nats.fg) |
| `fogu:rabbitmq` | AMQP 0-9-1: `exchange queue bind publish consume get`, acks | [rabbitmq.fg](examples/rabbitmq.fg) |
| `fogu:mqtt` | MQTT 3.1.1: `publish subscribe next`, QoS, retain, last will | [mqtt.fg](examples/mqtt.fg) |
| `fogu:regex` · `fogu:crypto` · `fogu:log` | regular expressions · hashes, HMAC, AES-GCM, argon2, random · structured logging | — |
| `fogu:csv` · `fogu:excel` · `fogu:yaml` · `fogu:toml` · `fogu:xml` | `parse stringify read write` (Excel: `read sheets write`) | — |
| `fogu:html` | parse + CSS selectors, Jinja templates, `sanitize`, HTML ↔ Markdown/text | — |
| `fogu:webview` | headless Chrome/Edge: `launch open goto eval click type screenshot pdf`, `render(url)` | — |
| `fogu:grpc` · `fogu:socketio` | gRPC servers/clients from `.proto` schemas · Socket.IO v5 servers (also on a `fogu:web` port) and clients | [grpc.fg](tests/spec/grpc.fg) |
| `fogu:security` | TLS/certificates, port scan, DNS, security headers, password strength, TOTP, secret scanning, Ed25519, CIDR, rate limiting | [security.fg](tests/spec/security.fg) |
| `fogu:broker` · `fogu:kafka` | the Fogu broker inside the runtime (inline client, clusters, persistence) · Apache Kafka producer/consumer | [broker.fg](tests/spec/broker.fg) |
| `fogu:image` · `fogu:plot` | images (resize, filters, drawing, text, SVG/HTML, GIF) · charts (line, bar, scatter, hist, pie) | — |
| `face` lib (not `fogu:*`) | face detection and recognition (`detect embed compare db`) — `libs/face`, over `fogu:ffi` | — |
| `datascience` lib (not `fogu:*`) | NumPy-style arrays, pandas-style frames, machine learning — `libs/datascience`, over `fogu:ffi` | — |
| `hardware` lib (not `fogu:*`) | serial ports, Modbus TCP/RTU, raw USB, HDMI-CEC — `libs/hardware`, over `fogu:ffi` | — |
| `fogu:docker` · `fogu:service` | Docker/Podman, compose, Swarm, Dockerfiles (also over SSH) · systemd units and `service.sh` (also over SSH) | — |
| `fogu:zip` · `fogu:protobuf` | ZIP/tar/gzip · Protocol Buffers from `.proto` files | — |

**The `Fogu` object** (no import needed): `Fogu.file(path)` (`text lines json
write append exists size remove`), `Fogu.env`, `Fogu.args`, `Fogu.run(cmd, args)`,
`Fogu.glob(pattern)`, `Fogu.hash(data, "sha256")`, `Fogu.sql(url)`, `Fogu.sleep(d)`,
`Fogu.exit(code)`, `Fogu.version`, `Fogu.os`.

**Signals and embedding**: `Fogu.signal.wait()` keeps a program (servers, tasks)
alive until Ctrl+C/SIGTERM; `Fogu.signal.on(name, fn)` handles signals and the
program's own events. `cargo ffi` builds `libfogu_rt` (`.so`/`.dll`/`.dylib`, header
`ffi/fogu.h`) to embed the runtime in other programs (C, Bun/Node FFI, Python ctypes…).

```fogu
const cfg = Fogu.file("config.json").json()?
const r = Fogu.run("git", ["rev-parse", "HEAD"])?
if r.ok { print("commit", r.stdout.trim()) }
for f in Fogu.glob("src/**/*.fg") { print(f) }
```

Fogu code runs on one thread: servers (`http`, `web`) answer requests whenever the
main program waits (`Fogu.signal.wait()`, `time.delay(...)`, a channel), each request
in its own task; `cron` jobs run while it waits in `time.delay(...)` or `cron.tick()`.

```fogu
import sql from "fogu:sql"

let db = sql.open("sqlite://demo.db")?
db.exec("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT)")
db.run("INSERT INTO users (name) VALUES (?)", ["Mary"])
for u in db.query("SELECT * FROM users") { print(u.name) }
db.close()
```

---

## CLI

```text
fogu                         REPL (history, multi-line, value echo)
fogu <file.fg> [args]        run (the args are in system.info.args())
fogu run [file|script]       run; without a target uses "entry" from fogu.json
fogu <script>                script from fogu.json (e.g. fogu dev)
fogu -e '<code>'             run code
fogu -p '<expr>'             evaluate and print the result
fogu --watch <file>          restart when *.fg / fogu.json change
fogu check [files]           syntax, names and types (no args: every .fg here)
fogu build --compile [file]  standalone executable in dist/ (--target linux-arm64|windows-x64|…|all)
fogu test [paths] [-t text]  run the tests of *.test.fg files
fogu fmt [paths] [--check]   format .fg files
fogu play <playbook.yml>     run a playbook (-i inventory, --check, -l, -t, -e)
fogu desktop <cmd>           desktop apps: create, dev, build (libs/desktop)
fogu mcu <cmd>               microcontrollers: boards, build, flash, run, monitor (libs/mcu)
fogu init | add | remove | install | update | list | pm
fogu --version | --help
```

| Option | Effect |
| --- | --- |
| `--no-jit` | interpreter only |
| `--jit-stats` | JIT statistics at the end |
| `--jit-debug` | log to stderr what the JIT compiles / why it gives up |
| `--jit-dump` | like `--jit-debug`, and print the IR (CLIF) |
| `--strict` | an error in an `fg` task terminates the process |

| Variable | Effect |
| --- | --- |
| `FOGU_HOME` | base of the package store (default `~/.fogu`) |
| `NO_COLOR` | disables colors |
| `FOGU_DOTENV=0` | do not load `./.env` (loaded by default; variables already set win) |
| `FOGU_JIT=0` | disables the JIT |
| `FOGU_JIT_CALLS` | calls before compiling a function (default 2) |
| `FOGU_JIT_LOOPS` | iterations before compiling a `while` (default 200) |

Details: [docs/cli.md](docs/cli.md).

---

## Packages

Packages come from **git repositories** (GitHub, GitLab, Bitbucket, Codeberg or
any URL), **`.tar.gz` archives** or **local folders**, and live in a global
store shared between projects:

```text
~/.fogu/fogu_modules/<name>@<commit>/
```

**`fogu.json`**

```json
{
  "name": "my-app",
  "version": "0.1.0",
  "entry": "main.fg",
  "scripts": { "start": "fogu run main.fg", "dev": "fogu --watch main.fg" },
  "dependencies": { "greet": "github:user/greet#semver:^1.2.0" }
}
```

**`fogu.lock`** pins, per package, the exact commit and the `sha256` checksum of
the tree. If the store does not match the lock, the installation fails.
Install scripts are **never** run.

| Source | Meaning |
| --- | --- |
| `user/repo` · `github:u/r` | GitHub |
| `gitlab:g/r` · `bitbucket:u/r` · `codeberg:u/r` | other hosts |
| `https://host/x.git` · `git@host:u/r.git` · `git+file:///path` | any git |
| `...#main` · `...#v1.0.0` · `...#<sha>` | branch / tag / commit |
| `...#semver:^1.2.0` or `u/r@^1.2.0` | highest `vX.Y.Z` tag that satisfies |
| `https://host/pkg.tar.gz` | archive |
| `./folder` · `file:../folder` | local folder |
| `mcu` · `mcu@0.1.0` | a library of the Fogu server |

Semver ranges: `^1.2.0`, `~1.2.3`, `1.x`, `>=1.0.0 <2.0.0`, `*`.

```bash
fogu add mcu                         # a library of the Fogu server (<server>/libs)
fogu add user/greet@^1.2.0           # add and install
fogu add alias@github:u/r            # import it as "alias"
fogu install --frozen                # CI: fail if the lock is out of date
fogu update                          # move to the latest commits/tags
fogu list                            # dependency tree
fogu pm path | ls | cache clean | hash <folder>
```

```fogu
import { hello } from "greet"            // package entry ("entry", main.fg or index.fg)
import extra from "greet/lib/extra"      // a file inside the package
```

Details: [docs/packages.md](docs/packages.md).

---

## Tests

```fogu
// math.test.fg
import { test, skip, expect } from "fogu:test"
import { sum, parse } from "./math.fg"

test("adds", () => {
    expect(sum(1, 2)).toBe(3)
    expect([1, 2]).not.toContain(3)
})

test("errors are values", (): error => {
    const n = parse("42")?            // a returned error fails the test
    expect(n).toBeGreaterThan(40)
    return nil
})
```

```text
$ fogu test
math.test.fg
  ✓ adds (0ms)
  ✓ errors are values (0ms)

2 passed — 1 file(s) [3ms]
```

`fogu test` finds every `*.test.fg` under the current folder (or the given
files/folders), runs each file in a fresh interpreter and exits with 1 if a
test fails. Matchers: `toBe`, `toEqual`, `toBeTruthy`, `toBeFalsy`, `toBeNil`,
`toBeError`, `toBeType`, `toContain`, `toHaveLength`, `toBeGreaterThan`,
`toBeLessThan`, `toBeCloseTo`, each negatable with `.not`.

---

## Standalone executables

```bash
fogu build --compile            # uses "entry" from fogu.json
fogu build --compile tool.fg -o out
```

Produces `dist/<name>` (`.exe` on Windows): a copy of the `fogu` runtime with the
program appended — the entry file, every module it imports and the packages it
uses. The executable runs anywhere without Fogu installed and passes every
command-line argument to the program. On first run the bundle is extracted to a
cache folder (`<temp>/fogu-bundles/<hash>`).

### Other platforms

```bash
fogu build --compile --target linux-arm64        # Raspberry Pi (64-bit OS)
fogu build --compile --target linux-x64,windows-x64
fogu build --compile --target all                # every platform with a runtime
fogu build --targets                             # platforms, aliases and runtimes found
```

The same bundle is appended to the Fogu runtime of each platform, giving
`dist/<name>-<target>` (`.exe` for Windows). Platforms: `linux-x64`,
`linux-arm64` (`raspberry`), `linux-armv7` (`raspberry-armv7`, no JIT),
`windows-x64`, `windows-x86` (`win32`), `macos-x64`, `macos-arm64` (`mac`); Rust triples work too.
A runtime is a `fogu-<target>` binary looked up in `$FOGU_RUNTIMES`, next to
the `fogu` executable (and its `runtimes/` folder), in `./dist` and in
`$FOGU_HOME/runtimes`, or given with `--runtime <path>`; the one of the
current machine is `fogu` itself. Build them with Docker:

```bash
docker build -f docker/Dockerfile --output dist .
# dist/fogu-linux-x64  fogu-linux-arm64  fogu-linux-armv7  fogu-windows-x64.exe
docker build -f docker/Dockerfile --output dist --build-arg TARGETS="linux-arm64" .
```

macOS runtimes need Apple's SDK (`--build-arg MACOS_SDK_URL=<MacOSX.sdk.tar.xz>`)
or a build on a Mac (`cargo build --release`, renamed to `fogu-macos-arm64`).
The ARM Linux runtimes also build from x86_64 Linux without Docker (OpenSSL is
vendored for ARM, `.cargo/config.toml` sets the linkers):

```bash
sudo apt install gcc-aarch64-linux-gnu gcc-arm-linux-gnueabihf
rustup target add aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf
cargo build --release --target aarch64-unknown-linux-gnu   # target/aarch64-unknown-linux-gnu/release/fogu
```

### As a Windows service (or a systemd unit)

```bash
fogu build --compile app.fg --target windows --service --service-name myapp --display-name "My App"
```

`app.exe install` (Administrator) registers the executable with the Windows service
manager; `start`, `stop`, `status` and `uninstall` control it; without a command it runs
in the console. As a service its output goes to `myapp.log` next to the `.exe` and a
stop arrives as `SIGTERM` / `service-stop` (`Fogu.signal`), with 20 s to finish. On
Linux `sudo ./app install` writes and enables a systemd unit. Settings can also be
`"service": {...}` in `fogu.json`; details in [docs/cli.md](docs/cli.md#services-windows-service-systemd).

---

## JIT

The interpreter is the semantic reference; the Cranelift JIT compiles a
numeric subset and, when something is not supported, the code keeps running in
the interpreter with identical results and error messages.

- **Functions**: after N calls, a specialization for the argument types
  (int/float/bool) is compiled. Direct and mutual recursion are supported.
- **Loops (OSR)**: hot `while` loops are compiled mid-execution; long `for a..b` and `for x in list` right away.
- **Lists, maps and texts in loops**: `xs.push(…)`, `xs[i]`, `m[k]`, `m[k] = v`, `.len()` and the texts used as
  keys or pushed. Element and value types are *speculated* from the first one and checked on every read; a failed
  check leaves native code before any side effect of that iteration and the interpreter redoes it (deopt).
- **Errors** identical to the interpreter's, with the correct line number.

| Benchmark (release) | Interpreter | JIT |
| --- | --- | --- |
| fib(32) | 3.02 s | 0.025 s |
| loop 50M | 16.7 s | 0.069 s |
| primes | 5.2 s | 0.051 s |
| mandelbrot | 3.5 s | 0.013 s |
| collatz | 16.5 s | 0.137 s |

Validated differentially against the interpreter on ~1100 random programs.
Benchmarks are in [benches/](benches/). Details: [docs/jit.md](docs/jit.md).

---

## For AI agents

[`skills/fogu/`](skills/fogu/SKILL.md) is a skill (Agent Skills format) that teaches
AI agents to write, check, test and ship Fogu code: [`SKILL.md`](skills/fogu/SKILL.md)
has the rules and common mistakes, [`stdlib.md`](skills/fogu/stdlib.md) every
standard library function (generated, with tested examples). [`llms.txt`](llms.txt)
points agents to them. Install it with `cp -r skills/fogu ~/.claude/skills/` — see
[skills/README.md](skills/README.md).

---

## Repository layout

A Cargo workspace:

```text
crates/
├── fogu-core/      the language: lexer, parser, resolver, interpreter, Cranelift JIT,
│                   `fg` scheduler (src/runtime/), values and native objects, built-ins
│                   (str/list/map/buffer methods, print len typeof bytes…, fogu:math,
│                   fogu:json, fogu:test, events), the shared tokio runtime and the
│                   `Host` trait (src/host.rs) the standard library plugs into
├── fogu-std/       the standard library, by area in src/native/<area>: base io network
│                   database messaging ops hardware aiml media rpc; `fogu_std::register()`
└── fogu-pkg/       package manager (sources, semver, installation, lock; HTTP downloads)
src/                the `fogu` package: the CLI (toolkit/: commands, REPL, diagnostics,
                    build), embed.rs, main.rs; re-exports the crates as fogu::runtime,
                    fogu::native and fogu::pkg
ffi/                libfogu_rt, the C API (fogu.h)
tests/
├── spec.rs         conformance suite runner (cargo test)
├── cli.rs          CLI tests (init, test, build --compile)
└── spec/           .fg programs + expected output (.out / .err)
examples/           example programs (see examples/README.md)
benches/            JIT benchmarks (.fg)
docs/
├── docs.html       official language documentation (HTML)
├── spec.md         language specification (Draft 0.4)
├── cli.md · packages.md · jit.md
├── analysis.md     history of fixes and known gaps
└── design/         design notes of the native modules
CHECKLIST.md        status of every feature of the spec, and the plan
CHANGELOG.md · CONTRIBUTING.md · LICENSE (MIT)
```

### Tests

```bash
cargo test --release
```

Each `tests/spec/<name>.fg` runs **with and without the JIT** and its output must
equal `<name>.out`. If `<name>.err` exists, the program must fail and stderr
must contain every line of that file. `_name.fg` files are helper modules.
`FOGU_BLESS=1 cargo test` regenerates the `.out` files (review them before
committing).

---

## Status and limitations

Implemented: the core language, template strings, durations, `?.`, arrow
functions, default and variadic parameters, type annotations checked at
runtime, structural interfaces, generics (erased), errors as values with `?`,
structs with `embed` and `equals`, exhaustive `match`, `fg`/`chan`/`select`/`timeout`,
modules with private `_x` names, the standard library above (files, paths,
processes, sync primitives, tests), the `Fogu` object, JIT, REPL, CLI, `fogu test`,
git package manager and standalone executables. The detailed status is in
[CHECKLIST.md](CHECKLIST.md).

Not implemented yet (planned in the [specification](docs/spec.md)):

- permissions (`--allow-net`, `--allow-fs`, …);
- `fogu audit`, `fogu publish`, `fogu build --target wasm`;
- the bytecode VM, `fogu:kafka`;
- a central package registry and signatures.

Known gaps:

- the JIT does not compile strings, lists, maps, structs, `?`, `error`, destructuring or `fg`;
- `const [a, b] = f()` destructures lists, so for native functions that return
  lists (`json.parse` of an array, `db.query`) use `?`;
- generic type arguments are not checked (only the outer kind);
- transitive dependencies are flat (the first version found wins, with a warning).

See [docs/analysis.md](docs/analysis.md) and the [CHANGELOG.md](CHANGELOG.md).

---

## License

[MIT](LICENSE) © 2026 Edmilson Soares. Contributions: see [CONTRIBUTING.md](CONTRIBUTING.md).
