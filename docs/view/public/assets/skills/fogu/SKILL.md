---
name: fogu
description: Write, run, test, debug and ship programs in the Fogu language (.fg files) with the `fogu` CLI. Use when the task mentions Fogu, .fg files, fogu.json, `fogu:*` modules, or asks for a script, server, CLI tool, automation or standalone executable to be written in Fogu.
---

# Fogu

Fogu is a small, dynamically typed scripting language with optional type
annotations checked at run time, errors as values (no exceptions), cooperative
concurrency (`fg`, `chan`, `select`) and a large standard library (`fogu:*`).
One binary, `fogu`, is the interpreter (with a JIT), REPL, test runner,
formatter, package manager and compiler to standalone executables.

The syntax looks like JavaScript/Go, but **it is neither**. Do not assume a
JavaScript or Python feature exists: check this file, `stdlib.md` (every
standard library function, with signatures) or run the code.

## Working loop

1. Write the code in a `.fg` file.
2. `fogu check file.fg` catches syntax, unknown names, type and `match` errors
   without running anything. Fix everything it reports.
3. `fogu file.fg [args]` runs it. Errors print `file:line`, a code (`E0100`…),
   the source line and often a `help:` hint. Read them; they are precise.
4. For anything non-trivial, add tests in `name.test.fg` and run `fogu test`.
5. `fogu fmt` formats the files (single style, nothing to configure).

Quick checks: `fogu -p '<expr>'` prints an expression's value,
`fogu -e '<code>'` runs a snippet. Programs that serve or loop forever (web
servers, workers) must be started in the background and tested with a client.

## Language essentials

```fogu
let count = 0                     // mutable;  `var` does NOT exist
const name = "Fogu"               // immutable binding
const msg = `Hi ${name}, ${count + 1}`   // template string
// types: nil bool int float str duration list map buffer fn struct chan error
const wait = 1500ms               // durations: 500ms 2s 5m 1h 1d; 1s + 500ms == 1500ms

if count == 0 { print("zero") } else if count < 0 { print("neg") } else { print("pos") }
while count < 3 { count += 1 }
loop { count -= 1; if count == 0 { break } }    // `loop` = `while true`: until break/return
for i in 0..3 { print(i) }                    // 0 1 2 (half-open range)
for i, item in ["a", "b"] { print(i, item) }  // index and item
for key, value in { b: 1, a: 2 } { print(key, value) }   // keys in sorted order

const size = match count {
    0 => "none"
    1 | 2 => "few"
    n if n > 100 => "many"
    _ => "some"                   // a match must be exhaustive
}

fn area(w: float, h: float = 1.0): float { return w * h }   // annotations are optional
const double = (x) => x * 2                                 // arrow function
const add = fn(a, b) { return a + b }                       // anonymous function
fn total(...xs) { return xs.reduce((a, b) => a + b, 0) }    // variadic
```

- **Only `false` and `nil` are falsy.** `0`, `""`, `[]` and `{}` are truthy.
- **No implicit conversions**: `"n=" + 1` is an error; write `"n=" + str(1)` or a
  template string. Convert with `int(x)`, `float(x)`, `str(x)`.
- **`int / int` is integer division**: `7 / 2 == 3`, but `7.0 / 2 == 3.5`. Use
  `float(a) / b` when a fraction is wanted. Arithmetic is checked: overflow and
  division by zero stop the program.
- `//` starts a comment (there is no `//` operator). `**` is power, `%` modulo.
- `==` compares lists and maps by content. Missing map keys read as `nil`; `a?.b?.c`
  stops at `nil`; `x ?? default` replaces `nil`.
- Lists: `push pop shift unshift insert remove first last contains index_of join
  reverse slice concat sort map filter each find any all reduce`, `xs[-1]`, `len(xs)`.
  Maps: `keys values entries has get set remove merge`; add a key with `m.k = v`.
  Strings: `split upper lower trim replace starts_with ends_with repeat lines chars slice`.
- Binary data is a `buffer` (mutable bytes), made with `bytes(...)`; network, zip,
  image, protobuf, serial and USB functions return buffers:

  ```fogu
  const b = bytes("héllo")               // also bytes("cafe", "hex"), bytes(4), bytes([1, 2])
  print(len(b), b[0], b.text(), b.hex())  // 6 104 héllo 68c3a96c6c6f
  b[0] = 72                               // ints 0..255
  const n = bytes(4)
  n.writeU16(0, 513)                      // little-endian; writeU16(0, 513, true) big-endian
  print(n.readU16(0), b.slice(0, 3).text(), typeof(b))   // 513 Hé buffer
  ```
- There is **no spread** in map/list literals (`{ ...a }` is a syntax error) and no
  tuple literal; build maps key by key or with `a.merge(b)`.
- No classes or inheritance: `struct` with fields, methods, an optional `new(...)`
  constructor and `embed` for composition; `interface` is structural.

```fogu
struct Point {
    x = 0
    y = 0
    new(x, y) {                   // constructor: `new Point(3, 4)`
        this.x = x
        this.y = y
    }
    fn dist2() { return this.x * this.x + this.y * this.y }
}
const p = new Point(3, 4)
print(p.dist2())                  // 25
```

## Errors are values

There is no `try`/`catch`/`throw`. A function that can fail returns
`value, error`; the caller destructures it or propagates it with `?`.

```fogu
import fs from "fogu:fs"

fn loadConfig(path) {
    if !fs.exists(path) {
        return nil, error(`no config at ${path}`)
    }
    const text = fs.readText(path)?        // on error: return nil, err to the caller
    return text, nil
}

const [cfg, err] = loadConfig("app.json")
if err != nil {
    print("failed:", err.message)        // err.message, err.cause, err.trace
}
```

- `?` returns the error to the caller. At the **top level** an error reaching `?`
  stops the program with that message, which is fine for scripts. Use
  `const [v, err] = …` where the program must continue.
- Native functions return `error` **values** for operational failures (missing
  file, refused connection, bad SQL, invalid JSON) but **stop the program** on
  programming errors (wrong argument type, `int("abc")`, unknown method). Validate
  untrusted input before converting it, e.g.
  `if regex.test(s, "^-?[0-9]+$") { n = int(s) }`.
- `panic("msg")` is for unrecoverable states only.

## Modules and the standard library

```fogu
import json from "fogu:json"                 // standard library module
import { readText, writeText } from "fogu:fs" // named imports
import { sum } from "./util.fg"              // local file (in util.fg: export fn sum…)
import greet from "greet"                    // package from fogu.json
```

Globals (no import): `print len typeof str int float bytes clock close timeout error
panic`, and the `Fogu` object: `Fogu.file(path).text()/.json()/.write(s)`,
`Fogu.env.NAME`, `Fogu.args`, `Fogu.run(cmd, args)`, `Fogu.glob("src/**/*.fg")`,
`Fogu.sleep(d)`, `Fogu.exit(code)`, `Fogu.os`, `Fogu.asset(path)`,
`Fogu.signal.wait()` (keep a server alive until Ctrl+C/SIGTERM),
`Fogu.signal.on(name, fn)` / `emit(name, data)` (OS signals and the program's events).

| Need | Module |
| --- | --- |
| files, paths, processes, shell | `fogu:fs` `fogu:path` `fogu:process` `fogu:shell` |
| JSON, CSV, Excel, YAML, TOML, XML | `fogu:json` `fogu:csv` `fogu:excel` `fogu:yaml` `fogu:toml` `fogu:xml` |
| time, scheduling, events | `fogu:time` `fogu:cron` `fogu:events` |
| HTTP client/server (uploads: `multipart` + `http.file`), web framework, WebSocket, TCP/UDP | `fogu:http` `fogu:web` `fogu:websocket` `fogu:net` (`conn.read()` → buffer, `readLine()` → str, both `nil` at end of stream) |
| interactive server-rendered UIs (signals, shards, procedures; Topcoat UI / shadcn-rs kits, Tailwind) | `fogu:topcoat` with `fogu:web` |
| Socket.IO (server, also on a `fogu:web` port; client), gRPC | `fogu:socketio` `fogu:grpc` (+ `fogu:protobuf` schemas) |
| TLS/cert checks, port scan, DNS, security headers, passwords, TOTP, secret scanning, signatures, rate limits | `fogu:security` |
| databases | `fogu:sql` (SQLite/Postgres/MySQL) `fogu:orm` `fogu:kv` `fogu:surrealkv` (versioned, transactions) `fogu:surrealdb` `fogu:redis` `fogu:scylla` `fogu:etcd` |
| messaging, job queues | `fogu:nats` (`nats.fogu()`: KV, sub/pub, services) `fogu:kafka` `fogu:rabbitmq` `fogu:mqtt` `fogu:fogumq` `fogu:foguMQ-client` (FlowMQ v6) `fogu:bullmq` |
| an embedded message broker (KV, channels, pub/sub, RPC, clusters) | `fogu:broker` (`broker.start(addr)`, inline `b.client()`, web admin `b.admin(port)`) |
| cloud, mail, LLMs, SSH | `fogu:s3` `fogu:mail` `fogu:ai` `fogu:ssh` |
| MCP servers (tools/resources/prompts for Claude) and MCP clients | `fogu:ai` (`ai.mcpServer`, `ai.mcpClient`) |
| regex, crypto, JWT, logging | `fogu:regex` `fogu:crypto` `fogu:jwt` `fogu:log` |
| terminal UI and argument parsing | `fogu:cli` |
| run Fogu code at run time (also concurrently: `vm.spawn`), native C libraries | `fogu:vm` `fogu:ffi` |
| embed Fogu in another program (C API, `.so`/`.dll`) | `cargo ffi` → `libfogu_rt`, `ffi/fogu.h`; `Fogu.host.emit` |
| remote shells (persistent `sh.run`, expect-style) | `fogu:ssh` `s.shell()` |
| configuration management (Ansible-like) | `fogu:playbook`, `fogu play` |
| tests | `fogu:test` |
| HTML parsing/scraping, Jinja templates, Markdown | `fogu:html` |
| real browser automation, screenshots, PDF | `fogu:webview` |
| images, charts | `fogu:image` `fogu:plot` (faces: the `face` lib, `import face from "face"`) |
| arrays, data frames, machine learning | the `datascience` lib (`import np from "datascience/array"`, `datascience/dataframe`, `datascience/ml`) — not in the standard library |
| serial ports, Modbus, USB, HDMI-CEC | the `hardware` lib (`import serial from "hardware/serial"`, `hardware/modbus`, `hardware/usb`, `hardware/hdmi`) — not in the standard library |
| Docker/Podman, systemd services (local or over SSH) | `fogu:docker` `fogu:service` (`docker.remote(ssh)`, `{ ssh }`) |
| ZIP/tar/gzip, Protocol Buffers | `fogu:zip` `fogu:protobuf` |

**Before using a module, look up its exact functions, argument order and
return values in [stdlib.md](stdlib.md).** Do not guess names from other
languages (e.g. it is `fs.readText`, `regex.test(text, pattern)`).

## Concurrency and servers

Fogu runs one thread of Fogu code with **cooperative** tasks:

```fogu
import time from "fogu:time"

const results = chan(10)
for i in 0..3 {
    fg fn(n) {                       // start a task
        time.delay(100ms)            // cooperative wait: other tasks run
        results <- n * n
    }(i)
}
for i in 0..3 { print(<-results) }

select {
    case v = <-results: print(v)
    case <-timeout(1s): print("timed out")
}
```

- Waiting calls (`time.delay`, `chan` receive, network and HTTP calls,
  `process.run`, `shell.run`, `sql` statements, `w.next()` of queues and
  watchers) let other tasks run; a busy `while true {}` does not. Run slow
  calls in `fg` tasks to overlap them.
- Servers (`fogu:http`, `fogu:web`) and `fogu:cron` do their work **while the
  main program waits**; each request runs in its own task, so a handler that waits
  does not hold the others. `app.listen(port)` returns at once: keep the program
  alive with `Fogu.signal.wait()` (until Ctrl+C) or a waiting loop, or it exits. The address is a port (127.0.0.1)
  or a string: `app.listen("0.0.0.0:3000")`, `"[::1]:3000"`, `":3000"`.
  A deadlock between tasks is detected and reported.

```fogu
import web from "fogu:web"

const app = web.app()
app.get("/hello/:name", (ctx) => ({ hello: ctx.params.name }))   // a returned map is a JSON response
app.listen(8080)?                       // host 127.0.0.1; { host: "0.0.0.0" } for other machines
Fogu.signal.wait()                      // serve until Ctrl+C / SIGTERM
```

## Projects, tests and shipping

```bash
fogu init -y                 # fogu.json (name, entry, scripts, dependencies), main.fg
fogu run                     # run "entry"; `fogu <script>` runs a script of fogu.json
fogu add user/repo@^1.2.0    # dependency from git (also gitlab:, tar.gz, ./folder)
fogu test [-t name]          # run *.test.fg
fogu fmt --check             # formatting check for CI
fogu build --compile         # standalone executable in dist/ (no Fogu needed to run it)
fogu build --compile --target linux-arm64,windows-x64   # other platforms
```

```fogu
// math.test.fg
import { test, expect } from "fogu:test"
import { area } from "./math.fg"

test("area", () => {
    expect(area(2.0, 3.0)).toBe(6.0)
    expect([1, 2]).not.toContain(3)
})
```

Matchers: `toBe toEqual toBeTruthy toBeFalsy toBeNil toBeError toBeType
toContain toHaveLength toBeGreaterThan toBeLessThan toBeCloseTo`, each with `.not`.

## Common mistakes

| Wrong | Right |
| --- | --- |
| `var x = 1` | `let x = 1` |
| `try { … } catch` | `const [v, err] = f()` or `f()?` |
| `throw new Error("x")` | `return nil, error("x")` |
| `"total: " + n` | `` `total: ${n}` `` or `"total: " + str(n)` |
| `7 / 2` expecting `3.5` | `7.0 / 2` or `float(7) / 2` |
| `{ ...a, b: 1 }` | `a.merge({ b: 1 })` |
| `new Point({ x: 1 })` without `new(...)` | declare `new(x, y) { … }` in the struct |
| `x.length`, `xs.size()` | `len(x)` |
| `async`/`await`, `Promise` | plain calls; `fg fn() { … }()` for tasks |
| `app.listen(port)` as the last line | add `Fogu.signal.wait()` after it |
| `if x return y` (no braces) | `if x { return y }` — braces are always required |
| `"\d+"` in a regex (invalid escape) | `"\\d+"` — write backslashes twice in strings |
| `int(userInput)` on untrusted text | validate first (`regex.test`) — `int` stops the program on bad input |

When unsure, write a three-line probe and run it with `fogu -e` or a scratch
file. The interpreter's error messages are the source of truth.
