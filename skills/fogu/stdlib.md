# Fogu standard library

Generated from `docs/tools/reference/*.py` by `docs/tools/gen_reference.py` — do not edit.
Outputs marked `output` were produced by running the example. Signatures read
`call(args) → result — notes`; `value · error` means the call returns an `error`
value on operational failures (use `?` or `const [v, err] = …`).

- [Overview](#overview)
- [Global functions](#global-functions)
- [Strings, lists, maps and buffers](#strings-lists-maps-and-buffers)
- [The Fogu object](#the-fogu-object)
- [fogu:math](#fogumath)
- [fogu:json](#fogujson)
- [fogu:time](#fogutime)
- [fogu:system](#fogusystem)
- [fogu:fs](#fogufs)
- [fogu:path](#fogupath)
- [fogu:process](#foguprocess)
- [fogu:sync](#fogusync)
- [fogu:test](#fogutest)
- [fogu:log](#fogulog)
- [fogu:regex](#foguregex)
- [fogu:crypto](#fogucrypto)
- [fogu:csv](#fogucsv)
- [fogu:excel](#foguexcel)
- [fogu:yaml](#foguyaml)
- [fogu:toml](#fogutoml)
- [fogu:xml](#foguxml)
- [fogu:kv](#fogukv)
- [fogu:sql](#fogusql)
- [fogu:orm](#foguorm)
- [fogu:surrealkv](#fogusurrealkv)
- [fogu:redis](#foguredis)
- [fogu:s3](#fogus3)
- [fogu:scylla](#foguscylla)
- [fogu:surrealdb](#fogusurrealdb)
- [fogu:etcd](#foguetcd)
- [fogu:http](#foguhttp)
- [fogu:web](#foguweb)
- [fogu:topcoat](#fogutopcoat)
- [fogu:jwt](#fogujwt)
- [fogu:socketio](#fogusocketio)
- [fogu:grpc](#fogugrpc)
- [fogu:security](#fogusecurity)
- [fogu:websocket](#foguwebsocket)
- [fogu:net](#fogunet)
- [fogu:events](#foguevents)
- [fogu:cron](#fogucron)
- [fogu:nats](#fogunats)
- [fogu:rabbitmq](#fogurabbitmq)
- [fogu:mqtt](#fogumqtt)
- [fogu:broker](#fogubroker)
- [fogu:kafka](#fogukafka)
- [fogu:fogumq](#fogufogumq)
- [fogu:foguMQ-client](#fogufogumq-client)
- [fogu:bullmq](#fogubullmq)
- [fogu:mail](#fogumail)
- [fogu:ai](#foguai)
- [fogu:ssh](#fogussh)
- [fogu:shell](#fogushell)
- [fogu:ffi](#foguffi)
- [fogu:playbook](#foguplaybook)
- [Embedding Fogu (C API, .so / .dll)](#embedding-fogu-c-api-so--dll)
- [fogu:vm](#foguvm)
- [fogu:cli](#fogucli)
- [fogu:html](#foguhtml)
- [fogu:webview](#foguwebview)
- [fogu:image](#foguimage)
- [fogu:plot](#foguplot)
- [fogu:docker](#fogudocker)
- [fogu:service](#foguservice)
- [fogu:zip](#foguzip)
- [fogu:protobuf](#foguprotobuf)

## Overview

Modules are imported with `import name from "fogu:name"` (or by name: `import { parse } from "fogu:json"`). Each section below lists every function with its parameters and return value, and shows an example with its **real output** (the examples are run when this page is generated). Core [`fogu:math`](#fogumath) [`fogu:json`](#fogujson) [`fogu:time`](#fogutime) [`fogu:system`](#fogusystem) [`fogu:fs`](#fogufs) [`fogu:path`](#fogupath) [`fogu:process`](#foguprocess) [`fogu:sync`](#fogusync) [`fogu:test`](#fogutest) [`fogu:log`](#fogulog) [`fogu:regex`](#foguregex) [`fogu:crypto`](#fogucrypto)
Data formats [`fogu:csv`](#fogucsv) [`fogu:excel`](#foguexcel) [`fogu:yaml`](#foguyaml) [`fogu:toml`](#fogutoml) [`fogu:xml`](#foguxml)
Storage [`fogu:kv`](#fogukv) [`fogu:surrealkv`](#fogusurrealkv) [`fogu:surrealdb`](#fogusurrealdb) [`fogu:sql`](#fogusql) [`fogu:orm`](#foguorm) [`fogu:redis`](#foguredis) [`fogu:s3`](#fogus3) [`fogu:scylla`](#foguscylla) [`fogu:etcd`](#foguetcd)
Web & network [`fogu:http`](#foguhttp) [`fogu:web`](#foguweb) [`fogu:jwt`](#fogujwt) [`fogu:topcoat`](#fogutopcoat) [`fogu:websocket`](#foguwebsocket) [`fogu:socketio`](#fogusocketio) [`fogu:grpc`](#fogugrpc) [`fogu:net`](#fogunet) [`fogu:security`](#fogusecurity)
Messaging & events [`fogu:nats`](#fogunats) [`fogu:kafka`](#fogukafka) [`fogu:rabbitmq`](#fogurabbitmq) [`fogu:mqtt`](#fogumqtt) [`fogu:broker`](#fogubroker) [`fogu:fogumq`](#fogufogumq) [`fogu:foguMQ-client`](#fogufoguMQ-client) [`fogu:events`](#foguevents) [`fogu:cron`](#fogucron)
Integrations [`fogu:mail`](#fogumail) [`fogu:ai`](#foguai) [`fogu:ssh`](#fogussh) [`fogu:shell`](#fogushell) [`fogu:ffi`](#foguffi) [`fogu:playbook`](#foguplaybook)

### Conventions

**Errors are values.** Operational failures — a missing file, a refused connection, a server error, malformed input — come back as an `error` value: handle them with `?` (propagate) or `const [value, err] = f()`. Programming mistakes — a wrong argument type, an unknown option — stop the program with a message. **Options** are passed as a last map argument: `f(x, { timeout: 5s })`. **Durations** accept a duration literal (`500ms`, `2s`, `1h`) or an int number of milliseconds. Network operations wait cooperatively: other `fg` tasks keep running while one waits.

```fogu
import json from "fogu:json"

const [cfg, err] = json.parse("{ broken")      // an error value, not a crash
if err != nil {
    print("invalid config:", err.message)
}

fn load(text: str): error {
    const data = json.parse(text)?              // ? returns the error to the caller
    print("name:", data.name)
    return nil
}
print(load("{\"name\": \"fogu\"}"), load("nope") != nil)
```

output:

```text
invalid config: json.parse: invalid JSON at position 2: Some('b')
name: fogu
nil true
```

## Global functions

Available everywhere, without an import.

- `print(...values)` → nil — Writes the values separated by spaces and a newline (`Fogu.write` writes without them).
- `len(x)` → int — Length of a `str` (characters), `list`, `map`, `buffer` (bytes) or `chan` (queued items).
- `typeof(x)` → str — `int float str bool nil list map buffer fn duration chan error struct …`
- `str(x)` → str — Text form of any value (the same text `print` shows).
- `int(x)` → int — From a float (truncates), a numeric str or a bool. Invalid text stops the program.
- `float(x)` → float — From an int or a numeric str.
- `bytes(x, encoding = "utf8")` → buffer — A new [buffer](#values): from a str (encodings `utf8 hex base64 base64url latin1`), an int `n` (n zero bytes), a list of byte ints, or a buffer (a copy).
- `clock()` → float — Seconds since the UNIX epoch, with fractions — use differences to measure time.
- `error(message, cause?)` → error — A new error value; `err.message`, `err.cause`.
- `panic(...values)` → — — Stops the program with a message (for impossible situations).
- `timeout(d)` → chan — A channel that receives once after `d` (use with `select`).
- `close(ch)` → nil — Closes a channel: receivers get `nil` once it is empty.

```fogu
print(len("olá"), len([1, 2, 3]), len({ a: 1 }))
print(typeof(1), typeof(1.5), typeof("x"), typeof(nil), typeof([]), typeof({}), typeof(2s), typeof(bytes(2)))
print(str(42) + "!", int("17") + 1, int(3.9), float("2.5") * 2)
const e = error("disk full")
print(typeof(e), e.message)
const t0 = clock()
print(clock() - t0 < 1)
```

output:

```text
3 3 1
int float str nil list map duration buffer
42! 18 3 5.0
error disk full
true
```

## Strings, lists, maps and buffers

Methods of the built-in values. Indexes start at 0; negative indexes count from the end in `slice`.

### str

- `s.len()` → int — Number of characters.
- `s.upper() · s.lower()` → str — Case conversion.
- `s.trim()` → str — Without leading and trailing whitespace.
- `s.split(sep)` → list — Parts between `sep`.
- `s.lines() · s.chars()` → list — Lines (without line endings) · single characters.
- `s.contains(t) · s.starts_with(t) · s.ends_with(t)` → bool — Substring tests.
- `s.index_of(t)` → int — Character index of the first `t`, or `-1`.
- `s.replace(old, new)` → str — Every `old` replaced.
- `s.repeat(n)` → str — `s` repeated `n` times.
- `s.slice(start, end?)` → str — Characters from `start` up to (not including) `end`.

```fogu
const s = "  Fogu, fast & small  "
print(s.trim().upper(), s.trim().split(", "))
print("a-b-c".replace("-", "+"), "ab".repeat(3), "olá mundo".slice(4))
print("main.fg".ends_with(".fg"), "fogu".index_of("g"), "x\ny".lines(), "abc".chars())
print(`template: ${1 + 1} items`)
```

output:

```text
FOGU, FAST & SMALL [Fogu, fast & small]
a+b+c ababab mundo
true 2 [x, y] [a, b, c]
template: 2 items
```

### list

- `l.push(x) · l.pop()` → nil · value — Add to / remove from the end.
- `l.unshift(x) · l.shift()` → nil · value — Add to / remove from the start.
- `l.insert(i, x) · l.remove(i)` → nil · value — At an index.
- `l.first() · l.last()` → value — `nil` when empty.
- `l.contains(x) · l.index_of(x)` → bool · int — Search by value.
- `l.join(sep)` → str — Items as text joined with `sep`.
- `l.reverse() · l.sort(fn?)` → list — New list; `sort` takes an optional comparator `(a, b) => int`.
- `l.slice(start, end?) · l.concat(other)` → list — Copies.
- `l.map(fn) · l.filter(fn) · l.find(fn)` → list · list · value — Functional helpers.
- `l.any(fn) · l.all(fn) · l.each(fn)` → bool · bool · nil
- `l.reduce(fn, initial)` → value — `fn(acc, item)` over the items.
- `l.clear() · l.len()` → nil · int

```fogu
const nums = [5, 3, 8, 1]
nums.push(10)
print(nums, nums.sort(), nums.sort((a, b) => b - a))
print(nums.map((n) => n * 2), nums.filter((n) => n > 4), nums.find((n) => n > 6))
print(nums.reduce((acc, n) => acc + n, 0), nums.any((n) => n > 9), nums.all((n) => n > 0))
print(nums.slice(1, 3), nums.first(), nums.last(), nums.join(" | "))
```

output:

```text
[5, 3, 8, 1, 10] [1, 3, 5, 8, 10] [10, 8, 5, 3, 1]
[10, 6, 16, 2, 20] [5, 8, 10] 8
27 true true
[3, 8] 5 10 5 | 3 | 8 | 1 | 10
```

### map

- `m.keys() · m.values() · m.entries()` → list — Entries are `[key, value]` pairs.
- `m.has(key)` → bool
- `m.get(key, default?)` → value — The value, or `default` (nil).
- `m.set(key, value) · m[key] = value` → nil
- `m.remove(key)` → value — Removes and returns the value.
- `m.merge(other)` → map — A new map; `other` wins on equal keys.
- `m.len() · m.clear()` → int · nil

```fogu
const user = { name: "Ana", age: 30 }
user.city = "Lisboa"
print(user.keys(), user.has("age"), user.get("email", "none"))
print(user.merge({ age: 31, admin: true }))
for entry in user.entries() { print(entry[0], "=", entry[1]) }
```

output:

```text
[age, city, name] true none
{admin: true, age: 31, city: Lisboa, name: Ana}
age = 30
city = Lisboa
name = Ana
```

### buffer

A mutable sequence of bytes, for binary data: files, network packets, protocols. Built with `bytes(...)`; `typeof` gives `"buffer"` and `buffer` works as a type annotation. `b[i]` reads an int 0..255 and `b[i] = n` writes one (any other value stops the program); `for x in b` and `for i, x in b` walk the bytes; `==` compares the bytes; `print` shows the first 32 bytes in hex and `json.stringify` writes an array of ints. The network modules (`fogu:net`, `fogu:websocket`, `res.bytes()` of `fogu:http`) and `zip image protobuf serial usb kafka` return binary data as buffers; their inputs also accept a str or a list of byte ints.

- `b.len() · b.isEmpty()` → int · bool
- `b.text(enc = "utf8") · b.toString(enc)` → str — Decoded (`utf8 hex base64 base64url latin1`; invalid UTF-8 becomes U+FFFD).
- `b.hex() · b.base64() · b.toList()` → str · str · list
- `b.get(i)` → int · nil — The byte, or `nil` out of range.
- `b.push(byte...) · b.append(x...) · b.write(x...)` → buffer — Add to the end, in place, and return the buffer (chainable); `append`/`write` take a str, a buffer, a list or byte ints.
- `b.concat(x...)` → buffer — A new buffer: `b` followed by the arguments.
- `b.slice(start, end?) · b.copy() · b.clone()` → buffer — Copies; negative indexes count from the end.
- `b.indexOf(needle, from = 0) · b.includes(needle)` → int · bool — `needle`: a byte, a str or a buffer; `-1` when missing.
- `b.startsWith(x) · b.endsWith(x) · b.equals(x)` → bool
- `b.split(sep)` → list — Buffers between `sep`.
- `b.fill(byte, start?, end?) · b.reverse() · b.clear()` → buffer · buffer · nil — In place.
- `b.readU8 · readI8 · readU16 · readI16 · readU32 · readI32 · readI64 · readF32 · readF64 (offset, bigEndian = false)` → int · float — A number at `offset`, little-endian unless `bigEndian`.
- `b.writeU8 … b.writeF64(offset, value, bigEndian = false)` — Writes a number at `offset`.

```fogu
const b = bytes("héllo")
print(typeof(b), len(b), b[0], b.text(), b.hex(), b.base64())
print(b)
print(bytes("cafe", "hex").toList(), bytes(3), bytes([104, 105]).text())

const msg = bytes(0)
msg.push(1).append("ok").append(bytes([0, 255]))
print(msg.hex(), msg.indexOf("ok"), msg.slice(1, 3).text(), msg.startsWith(bytes([1])))
print(bytes("a,b,c").split(",").map((p) => p.text()))

const header = bytes(8)
header.writeU16(0, 513)                 // little-endian by default
header.writeU32(2, 3000000000, true)    // big-endian
print(header.readU16(0), header.readU16(0, true), header.readU32(2, true), header.hex())

fn checksum(data: buffer): int {
    let sum = 0
    for x in data { sum = (sum + x) % 256 }
    return sum
}
print(checksum(bytes("abc")), bytes("x") == bytes("x"))
```

output:

```text
buffer 6 104 héllo 68c3a96c6c6f aMOpbGxv
<buffer 6 bytes: 68 c3 a9 6c 6c 6f>
[202, 254] <buffer 3 bytes: 00 00 00> hi
016f6b00ff 1 ok true
[a, b, c]
513 258 3000000000 0102b2d05e000000
38 true
```

## The Fogu object

Runtime helpers available everywhere, without an import.

- `Fogu.version · Fogu.os` → str — Fogu's version · `windows`, `linux`, `macos`…
- `Fogu.cores() · Fogu.cores(n) · Fogu.cores([cpus])` → int · list · error — How many threads the runtime uses for networking and blocking work (default: the machine's CPUs) · use `n` · run only on those CPUs (Linux, Windows). Set it at the top of the program — once the runtime has started it is an error value. Also `--cores 4`, `--cores 0,2-3`, `$FOGU_CORES`.
- `Fogu.env` → map — Environment variables.
- `Fogu.args` → list — Arguments after the script name (`fogu run app.fg a b`).
- `Fogu.file(path)` → File — `text() lines() json() write(s) append(s) exists() size() remove()`, fields `path name ext dir`.
- `Fogu.asset(path)` → str — A project file: relative to the project root — and inside `fogu build --compile` executables, the extracted embedded files.
- `Fogu.run(cmd, args?, opts?)` → map — Runs a program → `{ ok, code, stdout, stderr }` (see [fogu:process](#foguprocess)).
- `Fogu.glob(pattern)` → list — Files matching `*`, `?` and `**`.
- `Fogu.hash(data, algo?)` → str — Hex digest, `sha256` (default) or `sha512`.
- `Fogu.sql(url)` → Database — Same as `sql.open(url)`.
- `Fogu.sleep(d)` → nil — Waits (other tasks keep running).
- `Fogu.signal.wait(names = ["SIGINT", "SIGTERM"], { timeout })` → str · nil — Keeps the program alive (servers — fogu:web/http requests are answered while it waits — and `fg` tasks keep running) until one of the signals or events happens; returns its name, `nil` after `timeout`.
- `Fogu.signal.on(name, fn(data)) · off(name) · emit(name, data?) · last · names` — Handlers for OS signals (`SIGINT SIGTERM SIGHUP SIGQUIT SIGUSR1 SIGUSR2`; Windows: `SIGINT SIGBREAK SIGTERM`) or the program's own events; they run as `fg` tasks. With no handler and nobody waiting, SIGINT/SIGTERM still end the program (exit code 130/143) — after releasing sshfs mounts, browsers and SurrealKV/broker stores. `emit` also simulates a signal.
- `Fogu.host.embedded · Fogu.host.emit(name, data)` → bool — When the runtime is embedded through the C API ([libfogu_rt](#embed)): calls the host's callback; `false` without one.
- `Fogu.write(...values)` → nil — Like `print`, without spaces or newline (streaming output).
- `Fogu.watch(path | [paths], { recursive = true, debounce = 50ms })` → Watcher · error — File system events: `w.next(timeout?)` → `{ kind, path, paths }` (`create modify remove rename`), `tryNext()`, `close()`.
- `Fogu.exit(code?)` → — — Ends the program.

```fogu
Fogu.file("notes.txt").write("one\ntwo")?
const f = Fogu.file("notes.txt")
print(f.name, f.ext, f.exists(), f.size(), f.lines()?)
print(Fogu.hash("abc"))
print(typeof(Fogu.env), typeof(Fogu.args), Fogu.asset("data/logo.png").ends_with("data/logo.png"))
Fogu.write("no newline: ", 1, 2)
print()
```

output:

```text
notes.txt .txt true 7 [one, two]
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
map list true
no newline: 12
```

Signals and the program's events (`emit` stands in for a real Ctrl+C here):

```fogu
const seen = []
Fogu.signal.on("job.done", fn(d) { seen.push(d.id) })
Fogu.signal.emit("job.done", { id: 7 })
fg fn() {
    Fogu.sleep(10ms)
    Fogu.signal.emit("SIGINT")          // a real one comes from Ctrl+C / kill -INT
}()
print("stopping on", Fogu.signal.wait())
print(seen, Fogu.signal.wait("never", { timeout: 20ms }))
```

output:

```text
stopping on SIGINT
[7] nil
```

Watching files — rebuild on every change:

```fogu
const w = Fogu.watch("src")?
while true {
    const ev = w.next()?
    print(ev.kind, ev.path)
    Fogu.run("fogu", ["test"])
}
```

output:

```text
modify src/app.fg
create src/new.fg
remove src/old.fg
```

## fogu:math

```fogu
import math from "fogu:math"
```

- `math.PI · math.E` → float — Constants.
- `math.abs(x)` → number — Absolute value (int stays int).
- `math.sqrt(x)` → float — Square root.
- `math.pow(base, exp)` → number — Power.
- `math.floor(x) · math.ceil(x)` → int — Rounding down / up.

```fogu
import math from "fogu:math"
print(math.abs(-4), math.sqrt(16), math.pow(2, 10), math.floor(2.7), math.ceil(2.1))
print(math.PI)
```

output:

```text
4 4.0 1024.0 2 3
3.141592653589793
```

## fogu:json

```fogu
import json from "fogu:json"
```

- `json.parse(text)` → value · error — JSON text → maps, lists, str, int, float, bool, nil. Invalid JSON is an error value.
- `json.stringify(value)` → str — Compact JSON (map keys in order).

```fogu
import json from "fogu:json"
const data = json.parse("{\"name\": \"Ana\", \"tags\": [\"a\", \"b\"], \"age\": 30}")?
print(data.name, data.tags, typeof(data.age))
print(json.stringify({ ok: true, list: [1, 2.5, nil] }))
```

output:

```text
Ana [a, b] int
{"list":[1,2.5,null],"ok":true}
```

## fogu:time

```fogu
import time from "fogu:time"
```

- `time.now()` → int — Milliseconds since the UNIX epoch.
- `time.monotonic()` → float — Seconds from a fixed point; never goes backwards (the wall clock can jump): measure durations with it.
- `time.Date()` → Date — The current instant (UTC).
- `time.delay(d) · time.sleep(seconds)` → nil — Waits; other `fg` tasks keep running.

### Date

- `d.year · d.month · d.day` → int — Month 1–12.
- `d.hour · d.minute · d.second · d.millis · d.weekday` → int — Weekday 0 = Sunday.
- `d.getTime()` → int — Milliseconds since the epoch (also `d.ms`).
- `d.toISOString()` → str — `2026-10-10T12:00:00.000Z`
- `d.toString()` → str — Human readable.
- `d.addMs(ms)` → nil — Moves the date by `ms` (in place).
- `d.diff(other)` → int — Milliseconds between the dates.
- `getFullYear() getMonth() getDate() getDay() getHours() getMinutes() getSeconds() getMilliseconds()` → int — JavaScript-style getters (month 0–11).

```fogu
import time from "fogu:time"
const start = time.now()
time.delay(20ms)
print(time.now() - start >= 20)
const d = time.Date()
print(d.year > 2000, d.month >= 1, d.toISOString().ends_with("Z"))
const later = time.Date()
later.addMs(24 * 3600 * 1000)
print(later.diff(d) >= 86400000)
```

output:

```text
true
true true true
true
```

## fogu:system

```fogu
import system from "fogu:system"
```

- `system.args` → list — Script arguments (`fogu run app.fg a b` → `[a, b]`).
- `system.time()` → float — Seconds since the epoch.
- `system.readLine()` → str · nil — The next line of stdin without its ending; `nil` at the end. Other `fg` tasks keep running while it waits.
- `system.exit(code)` → — — Ends the program.
- `system.info.os · .arch · .family` → str — `windows`/`linux`/`macos` · `x86_64`/`aarch64` · `windows`/`unix`.
- `system.info.cores · .pid` → int — Logical CPUs · process id.
- `system.info.env(name)` → str · nil — An environment variable.
- `system.info.sleep_ms(ms)` → nil — Blocks the whole program (prefer `time.delay`).

```fogu
import system from "fogu:system"
const info = system.info
print(typeof(info.os), info.cores > 0, typeof(info.pid), system.args)
print(info.env("FOGU_DOES_NOT_EXIST"), system.time() > 0)
```

output:

```text
str true int []
nil true
```

## fogu:fs

```fogu
import fs from "fogu:fs"
```

Files and folders. Failures (missing file, permission) are error values.

- `fs.readText(path)` → str · error — Whole file as UTF-8 text.
- `fs.readLines(path)` → list · error — Lines without line endings.
- `fs.writeText(path, text)` → nil · error — Creates or replaces.
- `fs.append(path, text)` → nil · error — Adds to the end (creates the file).
- `fs.exists(path) · fs.isFile(path) · fs.isDir(path)` → bool
- `fs.mkdir(path)` → nil · error — Creates the folder and its parents.
- `fs.list(path)` → list · error — Names inside a folder, sorted.
- `fs.stat(path)` → map · error — `{ size, isFile, isDir, modified }` (modified: ms since the epoch).
- `fs.copy(from, to) · fs.move(from, to)` → nil · error
- `fs.remove(path)` → nil · error — A file or an empty folder.
- `fs.removeAll(path)` → nil · error — A folder and everything inside.
- `fs.glob(pattern)` → list — Paths matching `*`, `?`, `**`, sorted.
- `fs.cwd() · fs.tempDir()` → str — Working folder · system temp folder.
- `fs.remote(ssh | "user@host", { sudo })` → RemoteFs — The same functions on a server through [fogu:ssh](#fogussh) (plus `upload(local, remote)` and `download(remote, local)`); every call is an SSH command.

```fogu
import fs from "fogu:fs"
fs.mkdir("data/logs")?
fs.writeText("data/a.txt", "hello")?
fs.append("data/a.txt", "\nworld")?
print(fs.readLines("data/a.txt")?, fs.stat("data/a.txt")?.size)
fs.copy("data/a.txt", "data/logs/b.txt")?
print(fs.list("data")?, fs.glob("data/**/*.txt"))
const [_, err] = fs.readText("missing.txt")
print(err != nil)
fs.removeAll("data")?
print(fs.exists("data"))
```

output:

```text
[hello, world] 11
[a.txt, logs] [data/a.txt, data/logs/b.txt]
true
false
```

## fogu:path

```fogu
import path from "fogu:path"
```

- `path.join(...parts)` → str — Joined with `/` and normalized.
- `path.dirname(p) · path.basename(p)` → str — Folder · last component.
- `path.stem(p) · path.extname(p)` → str — Name without extension · extension with the dot.
- `path.normalize(p)` → str — Resolves `.` and `..`.
- `path.isAbsolute(p)` → bool
- `path.absolute(p)` → str — Absolute path from the working folder.
- `path.sep` → str — The system separator.

```fogu
import path from "fogu:path"
print(path.join("src", "lib", "../main.fg"))
print(path.dirname("a/b/c.txt"), path.basename("a/b/c.txt"), path.stem("a/b/c.tar.gz"), path.extname("a/b/c.txt"))
print(path.normalize("a/./b/../c"), path.isAbsolute("/etc"), path.isAbsolute("etc"))
```

output:

```text
src/main.fg
a/b c.txt c.tar .txt
a/c true false
```

## fogu:process

```fogu
import process from "fogu:process"
```

A program that exits with a non-zero code is not an error: check `ok` and `code`. Not being able to start it is an error value. For pipes and Linux commands see [fogu:shell](#fogushell).

- `process.run(cmd, args?, { cwd, env, input })` → map · error — `{ ok, code, stdout, stderr }`.
- `process.shell(command)` → map — Through `sh -c` / `cmd /C`.
- `process.env(name) · process.setEnv(name, value)` → str · nil
- `process.cwd() · process.chdir(path)` → str · nil
- `process.pid · process.os · process.arch · process.args`
- `process.exit(code)` → —

```fogu
import process from "fogu:process"
const r = process.run("git", ["--version"])?
print(r.ok, r.code, r.stdout.starts_with("git version"))
const [_, err] = process.run("no-such-program-xyz")
print(err != nil)
```

output:

```text
true 0 true
true
```

## fogu:sync

```fogu
import sync from "fogu:sync"
```

Coordination of `fg` tasks. Waiting operations yield to the other tasks; waiting forever is reported as a deadlock.

- `sync.Mutex()` → Mutex — `lock()` `unlock()` `tryLock()` → bool, field `locked`.
- `sync.WaitGroup()` → WaitGroup — `add(n = 1)` `done()` `wait()`, field `count`.
- `sync.Once()` → Once — `do(fn)` runs `fn` only the first time.
- `sync.Atomic(value)` → Atomic — `get()` `set(v)` `add(n = 1)` → new value, `compareAndSwap(old, new)` → bool.
- `sync.Semaphore(n)` → Semaphore — `acquire()` (waits for a permit) `tryAcquire()` → bool `release()`, fields `available size`. Also limits [fogu:vm](#foguvm) tasks (`{ semaphore }`).

```fogu
import sync from "fogu:sync"
const wg = sync.WaitGroup()
const total = sync.Atomic(0)
const once = sync.Once()
for i in 1..5 {
    wg.add(1)
    fg fn() {
        once.do(() => print("first task"))
        total.add(i)
        wg.done()
    }()
}
wg.wait()
print(total.get())

// at most two tasks inside at a time
const sem = sync.Semaphore(2)
let inside = 0
let peak = 0
const done = sync.WaitGroup()
for i in 0..5 {
    done.add(1)
    fg fn() {
        sem.acquire()
        inside += 1
        if inside > peak { peak = inside }
        Fogu.sleep(5ms)
        inside -= 1
        sem.release()
        done.done()
    }()
}
done.wait()
print("peak", peak, "free", sem.available)
```

output:

```text
first task
10
peak 2 free 2
```

## fogu:test

```fogu
import { test, skip, expect } from "fogu:test"
```

Used in `*.test.fg` files, run with `fogu test`. A test fails when an expectation fails, when it raises an error or returns an error value.

- `test(name, fn)` → nil — Declares a test.
- `skip(name, fn)` → nil — A test that is reported but not run.
- `expect(value)` → Expectation — Then a matcher; `.not` negates it.
- `toBe(x) · toEqual(x)` — Equality (`toEqual` compares lists and maps deeply).
- `toBeTruthy() · toBeFalsy() · toBeNil() · toBeError()`
- `toBeType("int") · toContain(x) · toHaveLength(n)`
- `toBeGreaterThan(n) · toBeLessThan(n) · toBeCloseTo(x, eps?)`

```fogu
import { test, expect } from "fogu:test"

test("math", () => {
    expect(1 + 2).toBe(3)
    expect([1, 2]).toContain(2)
    expect("abc").not.toHaveLength(2)
    expect(0.1 + 0.2).toBeCloseTo(0.3)
})
```

fogu test:

```text
$ fogu test
math.test.fg
  ✓ math (0ms)

1 passed — 1 file(s) [1ms]
```

## fogu:log

```fogu
import log from "fogu:log"
```

Structured logs to stderr (default), stdout or a file. The level comes from `FOGU_LOG` (default `info`).

- `log.trace/debug/info/warn/error(message, fields?)` → nil — `fields`: a map added as `key=value` (or JSON keys).
- `log.setLevel(level)` → nil — `trace debug info warn error off`.
- `log.level()` → str — Current level.
- `log.setFormat("text" | "json")` → nil — One line per entry.
- `log.setOutput("stderr" | "stdout" | path)` → nil · error — A file is appended to.
- `log.setTime(bool)` → nil — Show the time stamp (default true).
- `log.child(fields)` → Logger — A logger that adds `fields` to every line; same methods + `child`.

```fogu
import log from "fogu:log"
log.setOutput("stdout")
log.setTime(false)
log.info("server started", { port: 3000 })
log.debug("not shown at the info level")
const db = log.child({ module: "db" })
db.warn("slow query", { ms: 812, sql: "select *" })
log.setFormat("json")
log.error("failed", { code: 23 })
```

output:

```text
INFO  server started port=3000
WARN  slow query module=db ms=812 sql="select *"
{"level":"error","msg":"failed","code":23}
```

## fogu:regex

```fogu
import regex from "fogu:regex"
```

Rust `regex` syntax (linear time, no backtracking). Positions are character indexes. In Fogu strings write `\\d` for `\d`. An invalid pattern is an error value.

- `regex.compile(pattern, { ignoreCase, multiline, dotAll, verbose })` → Regex · error
- `re.test(text)` → bool — Matches somewhere.
- `re.matches(text)` → bool — The whole text matches.
- `re.find(text)` → map · nil — `{ text, start, end, groups, named }`.
- `re.findAll(text)` → list — Every match.
- `re.replace(text, repl) · re.replaceAll(text, repl)` → str — `$1`, `${name}`, `$$` in `repl`.
- `re.split(text, limit?)` → list
- `re.source · re.groups · re.names` — Pattern · group count · group names.
- `regex.test/find/findAll/replace/replaceAll/split/matches(text, pattern, …)` — One-off shortcuts.
- `regex.escape(text)` → str — A literal pattern.

```fogu
import regex from "fogu:regex"
const email = regex.compile("(?P<user>[\\w.]+)@(?P<host>[\\w.]+)")?
print(email.test("write to ana@fogu.dev"))
const m = email.find("contact: ana@fogu.dev")
print(m.text, m.start, m.named.host, m.groups)
print(email.replaceAll("a@x.io, b@y.io", "${user} (at ${host})"))
print(regex.split("one, two;three", "\\s*[,;]\\s*"))
print(regex.findAll("v1.2 and v3.10", "\\d+\\.\\d+").map((x) => x.text))
```

output:

```text
true
ana@fogu.dev 9 fogu.dev [ana, fogu.dev]
a (at x.io), b (at y.io)
[one, two, three]
[1.2, 3.10]
```

## fogu:crypto

```fogu
import crypto from "fogu:crypto"
```

- `crypto.hash(algo, data, { base64 })` → str — `md5 sha1 sha256 sha384 sha512`; hex by default.
- `crypto.hmac(algo, key, data, { base64 })` → str
- `crypto.encrypt(data, key)` → str — AES-256-GCM; `key` is a password (argon2id) or 64 hex chars. Base64 output with salt and nonce.
- `crypto.decrypt(box, key, { bytes })` → str · error — Wrong key or altered data → error value.
- `crypto.hashPassword(password)` → str — argon2id PHC string, for storing passwords.
- `crypto.verifyPassword(password, hash)` → bool
- `crypto.randomBytes(n) · crypto.randomHex(n)` → list · str — Secure random.
- `crypto.randomInt(min, max)` → int — Inclusive, unbiased.
- `crypto.uuid()` → str — Random UUID v4.
- `crypto.base64(data, { url }) · crypto.fromBase64(text, { bytes })` → str — `data`: str or list of bytes.
- `crypto.hex(data) · crypto.fromHex(text, { bytes })` → str
- `crypto.equal(a, b)` → bool — Constant-time comparison.

```fogu
import crypto from "fogu:crypto"
print(crypto.hash("sha256", "abc"))
print(crypto.hmac("sha256", "key", "The quick brown fox jumps over the lazy dog"))
const box = crypto.encrypt("card 4242", "my secret")
print(crypto.decrypt(box, "my secret")?)
const [_, err] = crypto.decrypt(box, "wrong")
print(err)
const h = crypto.hashPassword("hunter2")
print(crypto.verifyPassword("hunter2", h), crypto.verifyPassword("hunter3", h))
print(crypto.base64("olá"), crypto.fromBase64("b2zDoQ=="), crypto.hex("hi"), len(crypto.uuid()))
```

output:

```text
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8
card 4242
error: crypto.decrypt: wrong key or corrupted data
true false
b2zDoQ== olá 6869 36
```

## fogu:csv

```fogu
import csv from "fogu:csv"
```

Rows with a header become maps; cells are typed (`42` → int, `3.5` → float, `true` → bool, empty → nil; codes like `007` stay text). The delimiter is guessed (`, ; tab |`).

- `csv.parse(text, { header = true, delimiter, types = true, trim })` → list · error — Maps (or lists without header).
- `csv.stringify(rows, { header = true, delimiter = ",", columns })` → str — Rows: maps (a header line is written) or lists.
- `csv.read(path, options?) · csv.write(path, rows, options?)` → list · nil — The same, from/to a file.

```fogu
import csv from "fogu:csv"
const people = csv.parse("name;age;code;city\nAna;30;007;Lisboa\nBob;25.5;12;\n")?
print(people[0])
print(people[1].age, people[1].city)
print(csv.stringify([{ name: "Ana", note: "says \"hi\", ok" }, { name: "Bob", age: 3 }]))
csv.write("out.csv", people, { columns: ["name", "age"] })?
print(csv.read("out.csv")?)
```

output:

```text
{age: 30, city: Lisboa, code: 007, name: Ana}
25.5 nil
name,note,age
Ana,"says ""hi"", ok",
Bob,,3

[{age: 30, name: Ana}, {age: 25.5, name: Bob}]
```

## fogu:excel

```fogu
import excel from "fogu:excel"
```

Reads `.xlsx .xlsm .xls .xlsb .ods`; writes `.xlsx` (bold, frozen header row, column widths fitted). Dates become ISO text.

- `excel.read(path, { sheet, header = true })` → list · map · error — The first sheet (or `sheet`: name or index) as rows; `sheet: "*"` → `{ name: rows }`.
- `excel.sheets(path)` → list · error — Sheet names.
- `excel.write(path, rows | { Sheet: rows }, { header = true, columns })` → nil · error — One sheet per map key.

```fogu
import excel from "fogu:excel"
const products = [
    { id: 1, name: "Café", price: 4.5, stock: true },
    { id: 2, name: "Pão", price: 0.8, stock: false },
]
excel.write("shop.xlsx", { Products: products, Notes: [["checked", true]] })?
print(excel.sheets("shop.xlsx")?)
print(excel.read("shop.xlsx")?)
print(excel.read("shop.xlsx", { sheet: "Notes", header: false })?)
```

output:

```text
[Notes, Products]
[]
[[checked, true]]
```

## fogu:yaml

```fogu
import yaml from "fogu:yaml"
```

- `yaml.parse(text)` → value · error
- `yaml.parseAll(text)` → list · error — Every document of a `---` stream.
- `yaml.stringify(value)` → str
- `yaml.read(path) · yaml.write(path, value)` → value · nil

```fogu
import yaml from "fogu:yaml"
const cfg = yaml.parse("server:\n  host: 0.0.0.0\n  ports: [80, 443]\ndebug: false\n")?
print(cfg.server.ports, cfg.debug)
print(yaml.stringify({ app: "fogu", tags: ["web", "cli"] }))
```

output:

```text
[80, 443] false
app: fogu
tags:
- web
- cli
```

## fogu:toml

```fogu
import toml from "fogu:toml"
```

Dates become text. TOML has no null: `nil` values are left out when writing; the top level must be a map.

- `toml.parse(text)` → map · error
- `toml.stringify(map)` → str
- `toml.read(path) · toml.write(path, map)` → map · nil

```fogu
import toml from "fogu:toml"
const t = toml.parse("title = \"app\"\n[db]\nurl = \"sqlite://x.db\"\npool = 5\n[[users]]\nname = \"ana\"\n")?
print(t.title, t.db.pool, t.users[0].name)
print(toml.stringify({ name: "fogu", version: 5, server: { port: 8080 } }))
```

output:

```text
app 5 ana
name = "fogu"
version = 5

[server]
port = 8080
```

## fogu:xml

```fogu
import xml from "fogu:xml"
```

An element is `{ name, attrs, children, text }`; `children` holds elements (maps) and text (str); `text` joins the element's own text.

- `xml.parse(text, { whitespace })` → map · error — The root element.
- `xml.stringify(element, { pretty = true, declaration = true })` → str
- `xml.read(path) · xml.write(path, element, options?)` → map · nil

```fogu
import xml from "fogu:xml"
const feed = xml.parse("<feed lang=\"pt\"><item id=\"1\">Olá &amp; adeus</item><item id=\"2\"/></feed>")?
print(feed.name, feed.attrs.lang, feed.children.map((c) => c.attrs.id), feed.children[0].text)
print(xml.stringify({ name: "note", attrs: { to: "Ana" }, children: [{ name: "body", text: "1 < 2" }] }))
```

output:

```text
feed pt [1, 2] Olá & adeus
<?xml version="1.0" encoding="UTF-8"?>
<note to="Ana">
  <body>1 &lt; 2</body>
</note>
```

## fogu:kv

```fogu
import kv from "fogu:kv"
```

An embedded key-value store in a single file (redb, ACID). Values are any Fogu value (stored as JSON).

- `kv.open(path)` → Store · error — Creates the file if needed.
- `store.set(key, value) · store.get(key)` → nil · value — `get` → nil when missing.
- `store.has(key) · store.delete(key)` → bool
- `store.keys() · store.len() · store.clear()` → list · int · nil
- `store.path · store.close()` → str · nil

```fogu
import kv from "fogu:kv"
const db = kv.open("app.db")?
db.set("user:1", { name: "Ana", roles: ["admin"] })
db.set("visits", 41)
db.set("visits", db.get("visits") + 1)
print(db.get("user:1").name, db.get("visits"), db.has("nope"), db.keys(), db.len())
db.close()
```

output:

```text
Ana 42 false [user:1, visits] 2
```

## fogu:sql

```fogu
import sql from "fogu:sql"
```

SQLite, PostgreSQL and MySQL/MariaDB by URL: `sqlite://app.db`, `sqlite::memory:`, `postgres://user:pass@host/db`, `mysql://user:pass@host/db`. Parameters: `?` (SQLite/MySQL) or `$1` (PostgreSQL).

- `sql.open(url)` → Database · error
- `db.run(sql, params?)` → int · error — One statement with parameters; returns the affected rows (alias `execute`).
- `db.query(sql, params?)` → list · error — Rows as maps.
- `db.one(sql, params?)` → map · nil · error — The first row.
- `db.exec(sql)` → nil · error — Raw SQL without parameters — may hold several statements (migrations).
- `db.begin() · db.commit() · db.rollback()` → nil · error — Transactions.
- `db.driver · db.url · db.close()`

```fogu
import sql from "fogu:sql"
const db = sql.open("sqlite::memory:")?
db.exec("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, age INT)")?
const n = db.run("INSERT INTO users (name, age) VALUES (?, ?)", ["Ana", 30])?
db.run("INSERT INTO users (name, age) VALUES (?, ?)", ["Bob", 25])?
print("inserted:", n)
print(db.query("SELECT * FROM users WHERE age > ? ORDER BY name", [20])?)
print(db.one("SELECT COUNT(*) AS n FROM users")?.n, db.driver)
const [_, err] = db.query("SELECT * FROM nope")
print(err != nil)
```

output:

```text
inserted: 1
[{age: 30, id: 1, name: Ana}, {age: 25, id: 2, name: Bob}]
2 sqlite
true
```

## fogu:orm

```fogu
import { create_client } from "fogu:orm"
```

A Prisma-style ORM for SQLite, PostgreSQL and MySQL. Models are described in a map; `migrate: "auto"` creates tables (with foreign keys) and adds new columns. Each model is reached as `db. `. Relations load with one query per level.

### Client and schema

- `create_client({ schema, migrate, log })` → Client · error — `schema: { provider, url, models }`; `log: true` prints every SQL statement.
- `field: { type, primary, autoincrement, required, unique, defaultValue }` — Types `Int BigInt String Bool Float DateTime Json`; defaults `"now"`, `"uuid"` or a value.
- `relation: { model, kind, foreignKey, references }` — `one-to-many`, `many-to-one`, `one-to-one`.
- `db.query(sql, params?) · db.execute(sql, params?)` → list · int — Raw SQL.
- `db.begin() · db.commit() · db.rollback() · db.migrate() · db.disconnect()`

### Model methods

`where`: `{ field: value }`, `{ field: { equals, not, in, notIn, lt, lte, gt, gte, contains, startsWith, endsWith, mode: "insensitive" } }`, `AND`/`OR`/`NOT`, and `nil` for IS NULL. `orderBy`: `{ field: "asc" | "desc" }` or a list of them.

- `create({ data, include, select })` → map — Missing required fields stop the program; a duplicate is an error value.
- `createMany({ data: [...] })` → { count }
- `findMany({ where, select, include, orderBy, take, skip })` → list
- `findFirst(opts) · findUnique(opts)` → map · nil
- `update({ where, data })` → map · error — `data` values may be `{ increment, decrement, multiply, divide, set }`.
- `updateMany({ where, data }) · deleteMany({ where })` → { count }
- `upsert({ where, create, update })` → map
- `delete({ where })` → map · error — The deleted record.
- `count({ where })` → int

```fogu
import { create_client } from "fogu:orm"

const db = create_client({
    schema: {
        provider: "sqlite",
        url: "sqlite::memory:",
        models: {
            User: {
                fields: {
                    id:    { type: "Int", primary: true, autoincrement: true },
                    email: { type: "String", required: true, unique: true },
                    role:  { type: "String", defaultValue: "user" },
                },
                relations: { posts: { model: "Post", kind: "one-to-many", foreignKey: "authorId" } }
            },
            Post: {
                fields: {
                    id:       { type: "Int", primary: true, autoincrement: true },
                    title:    { type: "String", required: true },
                    authorId: { type: "Int", required: true },
                },
                relations: { author: { model: "User", kind: "many-to-one", foreignKey: "authorId" } }
            }
        }
    },
    migrate: "auto",
})?

const ana = db.user.create({ data: { email: "ana@x.com" } })?
db.post.createMany({ data: [{ title: "Hello", authorId: ana.id }, { title: "Fogu", authorId: ana.id }] })?
print(ana)
const users = db.user.findMany({ include: { posts: { select: { title: true } } } })?
print(users[0].posts)
print(db.post.findFirst({ where: { title: { startsWith: "Fo" } }, include: { author: true } })?.author.email)
print(db.user.update({ where: { id: ana.id }, data: { role: "admin" } })?.role, db.post.count()?)
```

output:

```text
{email: ana@x.com, id: 1, role: user}
[{title: Hello}, {title: Fogu}]
ana@x.com
admin 2
```

## fogu:surrealkv

```fogu
import surrealkv from "fogu:surrealkv"
```

SurrealKV: an embedded, versioned LSM key-value store in a folder, with MVCC transactions (snapshot isolation, write conflicts as error values), the history of every key, checkpoints and encryption at rest (AES-256-GCM). Keys are strings, values any Fogu value (stored as JSON); times are ms since the epoch, like `time.now()`. Stores left open are flushed when the program exits.

- `surrealkv.open(dir, { versions, retention, sync, key, keyFile, vlog })` → SurrealKV · error — `versions` keeps every version (for `retention`, a duration; 0 = forever); `sync` waits for fsync on each commit; `key` (64 hex digits) or `keyFile` encrypt the store. A folder already open is an error.
- `db.get(key) · db.has(key) · db.set(key, value, { at })` → value · nil · bool — `at` writes a version for a past time (versioned stores).
- `db.delete(key) · db.purge(key) · db.deleteRange(start, end) · db.deletePrefix(prefix)` → nil · error — In a versioned store `delete` leaves a tombstone in the history; `purge` erases every version.
- `db.scan({ prefix, start, end, limit, reverse, keys })` → list — `[{ key, value }]` in key order (`end` excluded); `keys: true` → only the keys.
- `db.keys(prefix?) · db.count(prefix?)` → list · int
- `db.getAt(key, at) · db.history(key, { limit, deleted })` → value · list — Versioned stores. `history` → `[{ value, at, deleted }]`, newest first.
- `db.batch({ set: { key: value }, delete: [keys] })` → nil · error — One atomic commit.
- `db.begin({ readOnly }) → tx · tx.get/set/delete/scan/keys/count/getAt/history · tx.commit() · tx.rollback()` → Txn — Reads see a snapshot plus the transaction's own writes; `commit` → error value on a write conflict.
- `db.checkpoint(dir) · db.restore(dir) · db.flush() · db.path · db.closed · db.close()`

```fogu
import surrealkv from "fogu:surrealkv"
const db = surrealkv.open("shop.db", { versions: true })?
db.set("user:1", { name: "Ana" })?
db.set("user:2", { name: "Bia" })?
print(db.scan({ prefix: "user:" }))
db.set("price", 10, { at: 1000 })?
db.set("price", 12, { at: 2000 })?
print(db.getAt("price", 1500), db.get("price"), db.history("price").len())

const t1 = db.begin()
const t2 = db.begin()
t1.set("stock", 5)
t2.set("stock", 7)
t1.commit()?
const [_, err] = t2.commit()
print(err != nil, db.get("stock"))
db.close()
```

output:

```text
[{key: user:1, value: {name: Ana}}, {key: user:2, value: {name: Bia}}]
10 12 2
true 5
```

## fogu:redis

```fogu
import redis from "fogu:redis"
```

Redis, Valkey and KeyDB. **Every Redis command is a method** (`r.lpush(…)`, `r.xadd(…)`) or goes through `r.command("CMD", …)`; maps become `field value` pairs. Redis errors (`WRONGTYPE`, `NOAUTH`) are error values.

- `redis.connect(url | { host, port, username, password, db, tls, timeout, name })` → Redis · error — `redis://[user:pass@]host[:port][/db]`, `rediss://` for TLS, default `$REDIS_URL`.
- `r.set(key, value, { ex, px, exat, nx, xx, keepTtl, get })` → bool · value — `false` when `nx`/`xx` prevented it.
- `r.get(key) · r.del(keys) · r.incr(key) · r.expire(key, s) · r.ttl(key)`
- `r.hset(key, map) · r.hget(key, f) · r.hgetall(key)` → int · str · map
- `r.zrange(key, start, stop, { withScores, rev, byScore })` → list — With scores: `[{ member, score }]`.
- `r.setJson(key, value, opts?) · r.getJson(key)` → bool · value — JSON values.
- `r.pipeline([[cmd, …args], …])` → list — One round trip; a failed command is an error value in its place.
- `r.transaction([...])` → list · error — `MULTI … EXEC`.
- `r.subscribe(...channels) · r.psubscribe(...patterns)` → Subscription — Its own connection: `next(timeout?)` → `{ channel, message, pattern? }`, `tryNext()`, `close()`.
- `r.publish(channel, message)` → int — Receivers.
- `r.close()` → nil

```fogu
import redis from "fogu:redis"
const r = redis.connect("redis://127.0.0.1:6379/3")?
print(r.set("name", "Ana", { ex: 60 })?, r.get("name")?, r.ttl("name")?)
print(r.set("name", "x", { nx: true })?)              // exists: not set
r.hset("user:1", { name: "Ana", age: 30 })?
print(r.hgetall("user:1")?)
r.zadd("rank", [10, "ana", 25, "bob"])?
print(r.zrange("rank", 0, -1, { withScores: true, rev: true })?)
print(r.pipeline([["INCR", "n"], ["INCR", "n"], ["GET", "n"]])?)
const sub = r.subscribe("news")?
r.publish("news", "hello")?
print(sub.next(1s)?)
```

output (with a Redis server):

```text
true Ana 60
false
{age: 30, name: Ana}
[{member: bob, score: 25.0}, {member: ana, score: 10.0}]
[1, 2, 2]
{channel: news, message: hello}
```

## fogu:s3

```fogu
import s3 from "fogu:s3"
```

AWS S3 and compatible storage (MinIO, Cloudflare R2, Backblaze B2, DigitalOcean Spaces, Wasabi) with AWS Signature V4. Credentials default to `$AWS_ACCESS_KEY_ID`, `$AWS_SECRET_ACCESS_KEY`, `$AWS_REGION`, `$AWS_ENDPOINT_URL`.

Every method also takes `{ bucket }` in its options.

- `s3.connect({ endpoint, region, accessKey, secretKey, sessionToken, bucket, pathStyle, timeout })` → S3 — Path-style URLs when an `endpoint` is given.
- `store.put(key, body, { contentType, metadata, cacheControl, acl })` → { etag } · error — `body`: str, list of bytes, or any value (JSON).
- `store.upload(path, key?, opts?)` → { etag } · error — A local file.
- `store.get(key) · store.getBytes(key) · store.download(key, path)` → str · list · nil
- `store.head(key) · store.exists(key)` → map · nil · bool — `{ size, etag, contentType, lastModified, metadata }`.
- `store.list(prefix?, { delimiter, max })` → list — `{ key, size, etag, lastModified }`; folders `{ key, dir: true }`.
- `store.copy(from, to, { fromBucket }) · store.delete(key | keys)`
- `store.presign(key, { method = "GET", expires = 1h })` → str — A temporary URL that needs no credentials.
- `store.url(key) · store.withBucket(name)` → str · S3
- `store.createBucket(name?) · store.deleteBucket(name?) · store.listBuckets()`

```fogu
import s3 from "fogu:s3"
const store = s3.connect({ endpoint: "http://127.0.0.1:9000", accessKey: "fogu", secretKey: "secret", bucket: "docs" })?
store.put("notes/hello.txt", "Olá, S3!", { metadata: { author: "ana" } })?
print(store.get("notes/hello.txt")?)
print(store.head("notes/hello.txt")?.metadata, store.exists("nope")?)
print(store.list("notes/")?.map((o) => o.key))
print(store.presign("notes/hello.txt", { expires: 15m }).contains("X-Amz-Signature="))
```

output (with an S3 server):

```text
Olá, S3!
{author: ana}
false
[notes/hello.txt]
true
```

## fogu:scylla

```fogu
import scylla from "fogu:scylla"
```

ScyllaDB and Apache Cassandra (CQL). Statements with parameters are prepared once and cached; parameters are converted to the column types (collections, UDTs, uuid, timestamp…).

- `scylla.connect("host:port" | { hosts, keyspace, user, password, singleNode, addressMap })` → Scylla · error — `addressMap` translates addresses of nodes behind Docker/NAT.
- `db.execute(cql, params?)` → nil · error
- `db.query(cql, params?)` → list · error — Rows as maps.
- `db.one(cql, params?)` → map · nil · error
- `db.use(keyspace) · db.close()`

```fogu
import scylla from "fogu:scylla"
const db = scylla.connect({ hosts: ["127.0.0.1:9042"] })?
db.execute("CREATE KEYSPACE IF NOT EXISTS shop WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}")?
db.use("shop")?
db.execute("CREATE TABLE IF NOT EXISTS users (id int PRIMARY KEY, name text, tags set<text>)")?
db.execute("INSERT INTO users (id, name, tags) VALUES (?, ?, ?)", [1, "Ana", ["admin"]])?
print(db.one("SELECT * FROM users WHERE id = ?", [1])?)
```

output (with a ScyllaDB server):

```text
{id: 1, name: Ana, tags: [admin]}
```

## fogu:surrealdb

```fogu
import surrealdb from "fogu:surrealdb"
```

A SurrealDB client written on the runtime's networking (no SurrealDB crate): its JSON-RPC protocol over WebSocket (`ws://`, `wss://`: a session, live queries) or HTTP (`http://`, `https://`: `POST /rpc`, the session kept by the client). Tested with SurrealDB 3.3. Record ids are strings (`"person:ana"`); dates come back as ISO strings.

- `surrealdb.connect(url?, { namespace, database, user, pass, auth, token, define, timeout = 10s })` → SurrealDB · error — Default `$SURREAL_URL` or `ws://127.0.0.1:8000` (`/rpc` is added); also `$SURREAL_NS $SURREAL_DB $SURREAL_USER $SURREAL_PASS`. `user`/`pass` sign in as a root user; `auth` is passed to `signin` as it is (`{ ns, db, ac, … }` for other levels and record access). `define: true` creates the namespace and database if needed (SurrealDB 3).
- `db.query(sql, vars?) · db.queryRaw(sql, vars?)` → list · error — One value per statement; a failed statement is an error value (`statement 2: …`). `queryRaw` keeps `{ status, result, time }`.
- `db.select(thing) · create(thing, data?) · insert(table, data) · insertRelation(table, data) · update(thing, data?) · upsert(thing, data?) · merge(thing, data) · patch(thing, ops, diff?) · delete(thing)` → value · error — `thing`: a table (`"person"`) or a record (`"person:ana"`).
- `db.relate(from, relation, to, data?) · db.run(fn, args?)` → value · error — `run("fn::total", [1, 2])` or a built-in (`"string::uppercase"`).
- `db.live(table, { diff }) → live · db.listen(id) · live.next(timeout?) · live.tryNext() · live.kill() · live.id · live.done` → Live — WebSocket only. Events `{ action: CREATE | UPDATE | DELETE | KILLED, record, result }`, one at a time; `nil` after the timeout. `listen` follows a `LIVE SELECT` run with `query`.
- `db.use(ns, db) · signin(params) · signup(params) · authenticate(token) · invalidate() · set(name, value) · unset(name)` — `signin`/`signup` return the token and use it from then on; `set` defines a `$name` session variable.
- `db.info() · db.version() · db.ping() · db.close() · db.url · db.transport · db.namespace · db.database · db.token · db.closed`

```fogu
import surrealdb from "fogu:surrealdb"
const db = surrealdb.connect("ws://127.0.0.1:8000", { namespace: "shop", database: "main", user: "root", pass: "root", define: true })?
db.create("person:ana", { name: "Ana", age: 30 })?
db.insert("person", [{ id: "bia", name: "Bia", age: 17 }, { id: "caio", name: "Caio", age: 41 }])?
const [adults, total] = db.query("SELECT name FROM person WHERE age >= $min ORDER BY name; RETURN count(SELECT * FROM person)", { min: 18 })?
print(adults, total)
const live = db.live("person")?
db.merge("person:bia", { age: 18 })?
const ev = live.next(2s)?
print(ev.action, ev.record, ev.result.age)
```

output (with a SurrealDB server):

```text
[{name: Ana}, {name: Caio}] 3
UPDATE person:bia 18
```

## fogu:etcd

```fogu
import etcd from "fogu:etcd"
```

etcd v3 over gRPC (the official protos, no code generation): keys, ranges by prefix, leases, transactions, watches and cluster status. Calls go to every endpoint in turn; with `user` the token is renewed when it expires.

- `etcd.connect(url? | { endpoints, user, password, tls, timeout = 15s, connectTimeout = 5s })` → Etcd · error — Default `$ETCDCTL_ENDPOINTS` or `http://127.0.0.1:2379`; `user` may be `"name:pass"` (also `$ETCDCTL_USER`/`$ETCDCTL_PASSWORD`).
- `db.get(key, { prefix, rangeEnd, limit, revision, keysOnly, countOnly, serializable, sort, sortTarget })` → { kvs, count, more, revision } — `kvs`: `[{ key, value, version, createRevision, modRevision, lease }]`.
- `db.put(key, value, { lease, prevKv, ignoreValue, ignoreLease }) · db.delete(key, { prefix, rangeEnd, prevKv })` → { revision, prevKv } · { deleted, prevKvs }
- `db.grant(ttl) · db.revoke(id) · db.ttl(id) · db.keepAlive(id)` → { id, ttl } · … · Stream — `keepAlive` refreshes the lease until `close()`; `next(timeout?)` gives `{ id, ttl }`.
- `db.txn({ compare: [{ key, op, field, value }], success: [ops], failure: [ops] })` → { succeeded, responses, revision } — `field`: `value version createRevision modRevision lease`; ops `{ type: put | get | delete, key, … }`.
- `db.watch(key, { prefix, rangeEnd, prevKv, revision, progressNotify })` → Watch — `w.next(timeout?)` → `{ type: put | delete, kv, prevKv }`, one event at a time · `w.close()`.
- `db.status() · db.memberList() · db.compact(revision) · db.endpoints · db.close()`

```fogu
import etcd from "fogu:etcd"
const db = etcd.connect("http://127.0.0.1:2379")?
db.put("/app/a", "1")?
db.put("/app/b", "2")?
print(db.get("/app/", { prefix: true })?.count)
const w = db.watch("/app/", { prefix: true })?
db.put("/app/a", "3")?
const ev = w.next(2s)?
print(ev.type, ev.kv.key, ev.kv.value)
w.close()
print(db.delete("/app/", { prefix: true })?.deleted)
```

output (with an etcd server):

```text
2
put /app/a 3
2
```

## fogu:http

```fogu
import http from "fogu:http"
```

A minimal HTTP server (one handler for every request) and an HTTP client. For routing, middleware, static files, uploads, SSE and WebSocket use [fogu:web](#foguweb).

### Client

- `http.get(url, opts?) · http.delete(url, opts?) · http.head(url, opts?)` → Response · error — Network failures are error values; HTTP 4xx/5xx are normal responses (`res.ok` is false).
- `http.post(url, body?, opts?) · http.put(…) · http.patch(…)` → Response · error — `body`: a str (text/plain), a map or list (JSON), bytes (octet-stream).
- `http.request({ method, url, ...opts })` → Response · error
- `opts: { headers, query, json, form, multipart, body, timeout, redirects = true, basicAuth: [user, pass], bearer }` — `form`: urlencoded fields. `multipart`: a map of fields — text values, `http.file(path)` (streamed from disk), `{ data, filename, type }` bytes, or a list (a repeated field). Map keys are sent in alphabetical order.
- `http.file(path, { filename, type })` → HttpFile · error — A file to upload; the type comes from the extension.
- `http.download(url, path, opts?)` → int · error — Streams the body to a file; returns the bytes written (non-2xx is an error).
- `res.status · res.ok · res.url · res.headers · res.body` — `headers`: the first value of each (lower case).
- `res.json() · res.bytes() · res.header(name) · res.cookies()` — `bytes()`: the body as a [buffer](#values); `header` joins repeated values.

### Server

- `http.serve(port | "ip:port", fn(req))` → nil — Requests are answered as soon as they arrive while the program is alive — waiting in `Fogu.signal.wait()`, `time.delay`, on a channel… — and each handler runs in its own task, so one that waits does not hold the others. The address is an int port (on 127.0.0.1) or `"0.0.0.0:8080"`, `"[::1]:8080"`, `":8080"`.
- `req.method · req.path · req.query · req.headers · req.body` — `query` is the raw query string.
- `req.header(name) · req.json() · req.form() · req.bytes()` — `form()`: urlencoded and multipart bodies.
- `http.text(body, opts?) · http.json(value, opts?) · http.html(body, opts?)` → Response — `opts: { status, headers }`.

```fogu
import http from "fogu:http"
import time from "fogu:time"

http.serve(8080, (req) => {
    if req.path == "/hello" {
        return http.json({ hello: "world", method: req.method })
    }
    return http.text("not found", { status: 404 })
})
while true { time.delay(50ms) }
```

```fogu
import http from "fogu:http"
const r = http.get("http://127.0.0.1:8080/hello")?
print(r.status, r.headers["content-type"], r.json())
print(http.get("http://127.0.0.1:8080/nope")?.status)
const [_, err] = http.get("http://127.0.0.1:1/")
print(err != nil)
```

output:

```text
200 application/json {hello: world, method: GET}
404
true
```

Forms, file uploads, headers and downloads (here against a [fogu:web](#foguweb) app in the same program):

```fogu
import http from "fogu:http"
import web from "fogu:web"
import fs from "fogu:fs"

fs.writeText("report.txt", "hello file\n")?
const app = web.app()
app.post("/echo", (ctx) => ({ type: ctx.headers["content-type"].split(";")[0], body: ctx.body }))
app.post("/upload", (ctx) => {
    const f = ctx.req.form()?
    return { title: f.title, tags: f.tags, file: f.file.filename, text: f.file.text(), avatar: f.avatar.bytes().len() }
})
app.listen(18471, { quiet: true })

let done = false
fg fn() {
    const base = "http://127.0.0.1:18471"
    print(http.post(base + "/echo", { name: "Ana" }, { query: { page: 2 } })?.json())
    print(http.post(base + "/echo", nil, { form: { user: "ana", pass: "a b&c" } })?.json())
    const up = http.post(base + "/upload", nil, { multipart: {
        title: "report", tags: ["a", "b"],
        file: http.file("report.txt"),
        avatar: { data: [1, 2, 3, 4], filename: "a.png" },
    } })?
    print(up.ok, up.json())
    done = true
}()
while !done { Fogu.sleep(10ms) }
```

output:

```text
{body: {"name":"Ana"}, type: application/json}
{body: pass=a+b%26c&user=ana, type: application/x-www-form-urlencoded}
true {avatar: 4, file: report.txt, tags: [a, b], text: hello file
, title: report}
```

## fogu:web

```fogu
import web from "fogu:web"
```

A web framework: routes with parameters, middleware, static files and SPA builds, uploads, downloads, Server-Sent Events and WebSocket — on one port. Requests are served as soon as they arrive while the program waits (`Fogu.signal.wait()`, `time.delay`, a channel), each in its own task: a handler that waits on a query or an HTTP call does not hold the others.

### App

- `web.app()` → App — Methods return the app, so calls can be chained.
- `app.get/post/put/patch/delete/head/options/all(path, ...middlewares, handler)` → App — `/users/:id` parameters, `*` wildcard.
- `app.use(fn) · app.use(prefix, fn)` → App — Middleware for every request, or under a path prefix.
- `app.after((ctx, res) => …)` → App — After the handler; returning a value replaces the response.
- `app.onError((ctx, err) => response)` → App — For runtime errors and returned `error` values (default: 500).
- `app.static(dir) · app.static(prefix, dir, { spa, index, maxAge, dotfiles })` → App — Files served by the server thread: ETag/304, `Cache-Control`; `spa: true` serves `index.html` for unknown HTML routes (React, Vue, Svelte builds).
- `app.ws(path, ...middlewares, (ws, ctx) => …, { protocols })` → App — WebSocket routes; the handler runs in its own task with a [WebSocket](#foguwebsocket) on the upgraded HTTP connection.
- `app.set(key, value) · app.store` → App · map — Shared state (`ctx.store`).
- `app.listen(port | "ip:port", { host = "127.0.0.1", maxBody = 10 MB, quiet })` → nil · error — `app.listen("0.0.0.0:3000")` (or `host: "0.0.0.0"`) accepts other machines; IPv6 as `"[::1]:3000"`. A host in the string wins over `host`.
- `app.handler()` → fn — Calls the app without a network (tests): `h({ method, path, headers, body })` → Response.
- `app.routes()` → list

### Context and responses

- `ctx.method · ctx.path · ctx.params · ctx.query · ctx.headers · ctx.body` — `query` is a map; header names are lower case.
- `ctx.req.json() · ctx.req.form() · ctx.req.bytes()` — `form()`: fields as str, files as `UploadedFile`.
- `ctx.set.status · ctx.set.headers` — Adjust the final response from middleware or handlers.
- `ctx.store` → map — The app's shared map.
- `handler result` — A str (text or HTML), any value (JSON), a response, or nil (204).
- `web.json/text/html(body, { status, headers })` → Response
- `web.redirect(url, status = 302)` → Response
- `web.file(path, { download, contentType, status })` → Response · error — Sent by the server thread (binary safe).
- `web.sse({ retry, headers })` → SseStream — Return it from the handler; `send(data, { event, id })`, `comment(text)`, `close()`, field `open`.
- `web.cors({ origin, methods, headers, credentials, maxAge })` → middleware — Answers preflight requests.
- `web.logger()` → middleware — `GET /path 200 0.4ms`.
- `UploadedFile: filename, contentType, size, text(), bytes(), json(), save(path)` — `save("uploads/")` keeps a sanitized file name.

```fogu
import web from "fogu:web"
import time from "fogu:time"

const app = web.app()
app.use(web.logger())
app.use("/api", (ctx) => {
    if ctx.headers["x-key"] != "secret" { return web.json({ error: "forbidden" }, { status: 403 }) }
})
app.get("/api/users/:id", (ctx) => ({ id: ctx.params.id, page: ctx.query.page ?? "1" }))
app.post("/api/upload", (ctx) => {
    const form = ctx.req.form()?
    return { saved: form.file.save("uploads/")?, size: form.file.size }
})
app.get("/events", (ctx) => {
    const stream = web.sse()
    fg fn() {
        for i in 1..4 { stream.send({ n: i }, { event: "tick" }); time.delay(1s) }
        stream.close()
    }()
    return stream
})
app.ws("/chat", (ws, ctx) => {
    while true {
        const m = ws.next()?
        if m == nil { break }
        ws.send(`echo: ${m.data}`)
    }
})
app.static("/", "dist", { spa: true, maxAge: 1d })
app.listen(3000)?
while true { time.delay(50ms) }
```

Without a network, through `app.handler()`:

```fogu
import web from "fogu:web"
const app = web.app()
app.use((ctx) => { ctx.set.headers["x-served-by"] = "fogu" })
app.get("/users/:id", (ctx) => ({ id: ctx.params.id, sort: ctx.query.sort }))
app.get("/old", (ctx) => web.redirect("/new"))
app.get("/fail", (ctx) => error("boom"))
app.onError((ctx, err) => web.json({ error: err.message }, { status: 500 }))
const h = app.handler()
const show = (path) => {
    const r = h({ method: "GET", path: path })
    print(r.status, r.body, r.headers["x-served-by"], r.headers["location"])
}
show("/users/7?sort=name")
show("/old")
show("/fail")
show("/missing")
```

output:

```text
200 {"id":"7","sort":"name"} fogu nil
302  fogu /new
500 {"error":"boom"} fogu nil
404 not found fogu nil
```

## fogu:topcoat

```fogu
import tc from "fogu:topcoat"
```

Server-rendered reactive pages for [fogu:web](#foguweb), in the model of [Topcoat](https://github.com/tokio-rs/topcoat): HTML is built on the server; signals hold browser state; `@event` handlers, `:attr` bindings and live text update in the browser without a round-trip; shards re-render on the server when their inputs change and are swapped in place (keeping focus and input state); procedures are server functions called from the browser. The browser runtime (~18 KB, no build step) and the stylesheets are served by every fogu:web app under `/_tc/`. Two UI kits: `tc.ui` (the look of Topcoat UI) and `tc.shadcn` (the components of [shadcn-rs](https://github.com/hughdbrown/shadcn-rs)), plus Tailwind CSS.

### Pages and markup

- `tc.h(tag, attrs?, ...children)` → Html — Children: strings (escaped), `Html`, lists, numbers, signals and expressions (live text); `nil`/`false` render nothing. Attributes: `true` → bare, `false`/`nil` → left out, `class` may be a list or a `{ name: on }` map, `style` a map.
- `@event: "js" · :attr: "js" | signal` — `@click` runs JavaScript statements (`e` is the event, signals are in scope by name, `await` works); modifiers `.prevent .stop .once .self .enter .escape .debounce[.ms]`. `:value`, `:hidden`, `:class`, `:disabled`… follow an expression. Written as `tc-on:click` / `tc-bind:hidden` in raw HTML.
- `tc.page({ title, head, body, lang = "en", theme = "light" | "dark" | "system", ui = true | "shadcn" | false, tailwind, icon, bodyClass })` → Html — A whole document with the runtime, the stylesheet and the Fogu favicon (`icon`: another URL, or `false`). The theme the user picked with `ui.themeToggle()` is applied before the first paint. Return an `Html` from a handler: it is sent as HTML with the page's signals.
- `tc.raw(html) · tc.fragment(...children) · tc.escape(str) · html.html` → Html · str
- `tc.script() · tc.styles() · tc.tailwind(true | href)` → Html — For documents written by hand.

### Signals, shards and procedures

- `tc.signal(ctx, name, initial)` → Signal — Browser state named `name` (a JS identifier). `s.value` / `s.get()` read it on the server and make it tracked : a change in the browser renders the page (or the shard that read it) again with the new value. `s.peek()` reads without tracking. Values sent by the browser are user input: validate them.
- `in the browser: s.get() · set(v) · update(f) · increment(n) · decrement(n) · toggle() · push(x)` — Plus `call(name, ...args)`, `navigate(url)`, `toast(title, { description, type, duration })`, `await ask(message, { title, confirm, cancel, danger })` (a confirmation dialog → bool), `copy(text, label)`, `toggleTheme()` and `tc.signals`.
- `tc.expr(js, initial?)` → Expr — A browser expression: live text, an `:attr` value or a shard prop.
- `tc.shard(name, fn(ctx, props)) · shard.render(ctx, props) · app.shard(shard)` → Shard · Html — Props are fixed values or signals/expressions; when one changes, `POST /_tc/s/ ` renders it again and the browser morphs it in. The first page that renders a shard registers it; `app.shard` does it up front. Check access inside the shard: page middleware scoped to a path does not run for it.
- `app.procedure(name, fn(ctx, ...args))` → App — `await call(name, ...args)` in the browser; an `error` value (or `return nil, error(…)`) rejects the call with its message.
- `ui.link(children, href) · <a tc-link>` → Html — Navigation without a full reload (prefetched on hover, history kept).
- `protocol` — Topcoat's frames, one JSON object per line (`application/x-ndjson`): `snapshot` (a rerun: `POST` of the signals to the page URL with `x-topcoat: rerun`), `swap`, `redirect`, `error`. `ctx.tc`: `{ signals, input, rerun, page }`.

### UI kits and Tailwind

- `tc.ui.button(children, { variant, size, href }) · badge · card(content, { title, description, footer, action }) · stat(label, value, { hint })` → Html — Topcoat UI's look (its neutral theme tokens, plain CSS: `/_tc/ui.css`). Variants `secondary outline ghost destructive link`; badges also `success warning`; sizes `sm lg icon`. Extra options become attributes (`@click`, `class`…).
- `tc.ui.icon(name, { size = 16 }) · logo({ size, text }) · sparkline(values, { width, height, area }) · empty(title, { description, icon, action }) · live(text) · themeToggle()` → Html — 35 built-in stroke icons (`activity alert arrow-right check chevron-right clock copy database edit eye flame hash home key layers lock logout message moon network plus radio refresh search send server settings shield sun terminal trash users x zap`); `stat(label, value, { icon, tone, trend: [numbers], hint })` draws a trend line; tones `primary success warning destructive info`. `shell` nav items take an `icon`, and `footer` goes under the nav; `grid(children, { min })`, `table(…, { header: false })`.
- `tc.ui.input · textarea · select(options, { value }) · checkbox · switch · label · field(label, control, { description, error })` → Html
- `tc.ui.table(columns, rows, { empty, caption }) · alert(title, text, { variant }) · progress(value) · skeleton · spinner · separator · kbd · tabs · link · shell({ brand, nav, title, actions }, content) · row · stack · grid · container` → Html — `columns`: names or `{ key, label, align }`; rows: maps or lists; cells may be `Html`.
- `tc.shadcn.button · badge · card · stat · alert · input · textarea · select · checkbox · switch(signal) · label · field · table` → Html — The markup and class names of shadcn-rs, styled by its stylesheet (`page({ ui: "shadcn" })`). Button variants `default primary secondary destructive outline ghost link`, sizes `xs sm md lg xl`.
- `tc.shadcn.tabs(signal, [{ value, label, content }]) · dialog(signal, { title, description, footer, trigger }, content) · accordion([{ title, content, open }], { multiple }) · dropdown(trigger, items) · tooltip(trigger, text) · pagination(page, pages, { href }) · progress · skeleton · separator · avatar · kbd · breadcrumb · shell · toaster` → Html — Interactive parts run in the browser: tabs and dialogs follow a signal; `pagination` takes a signal or an `href` with `{page}`.
- `tc.page({ tailwind: true }) · tc.page({ tailwind: "/app.css" }) · tc.tailwindTheme` — `true` loads Tailwind v4's browser build (development) with the theme tokens as utilities (`bg-primary`, `text-muted-foreground`, `border-border`, `dark:`); for production build a stylesheet with the Tailwind CLI (`@import "tailwindcss"; @source "./**/*.fg";` plus `tc.tailwindTheme`) and pass its URL.

```fogu
import web from "fogu:web"
import tc from "fogu:topcoat"
import json from "fogu:json"
const h = tc.h
const ui = tc.ui

const results = tc.shard("results", fn(ctx, props) {
    const found = ["apple", "banana", "mango"].filter(fn(f) { return f.contains(props.query ?? "") })
    return ui.table(["fruit"], found.map(fn(f) { return { fruit: f } }), { empty: "Nothing found" })
})
const app = web.app()
app.procedure("double", fn(ctx, n) { return n * 2 })
app.get("/", fn(ctx) {
    const count = tc.signal(ctx, "count", 0)
    const query = tc.signal(ctx, "query", "")
    return tc.page({ title: "Demo", body: ui.container([
        h("p", nil, "Count: ", count),
        ui.button("+1", { "@click": "count.increment()" }),
        ui.button("double on the server", { variant: "outline", "@click": "count.set(await call('double', count.get()))" }),
        ui.input({ placeholder: "Search", ":value": query, "@input": "query.set(e.target.value)" }),
        results.render(ctx, { query: tc.expr("query.get()", query.peek()) }),
    ])})
})

// what the browser does, without a network:
const call = app.handler()
print(call({ method: "GET", path: "/" }).body.contains("data-tc-text=\"count.get()\""))
const swap = json.parse(call({ method: "POST", path: "/_tc/s/results", body: "{\"props\":{\"query\":\"an\"}}" }).body)
print(swap.t, swap.html.contains("banana"), swap.html.contains("apple"))
print(call({ method: "POST", path: "/_tc/p/double", body: "[21]" }).body)
// app.listen(3000) and Fogu.signal.wait() to serve it
```

output:

```text
true
swap true false
{"ok":42}
```

## fogu:jwt

```fogu
import jwt from "fogu:jwt"
```

JSON Web Tokens with HMAC (HS256/HS384/HS512). A token that fails verification is an error value.

- `jwt.sign(payload, secret, { algorithm, expiresIn, notBefore, issuer, audience, subject, jwtid, header, noTimestamp })` → str — `iat` is added; durations or seconds for times.
- `jwt.verify(token, secret, { algorithms, issuer, audience, subject, leeway, maxAge, ignoreExpiration })` → map · error — The claims.
- `jwt.decode(token)` → map · error — `{ header, payload }` without verifying.
- `jwt.middleware(secret, { property = "user", optional, …verify options })` → middleware — For `fogu:web`: `Authorization: Bearer` → `ctx.user`, otherwise 401.

```fogu
import jwt from "fogu:jwt"
const token = jwt.sign({ sub: "42", role: "admin" }, "s3cret", { expiresIn: 1h, issuer: "fogu" })
const claims = jwt.verify(token, "s3cret", { issuer: "fogu" })?
print(claims.sub, claims.role, claims.exp - claims.iat)
print(jwt.decode(token)?.header)
const [_, e1] = jwt.verify(token, "wrong")
const [_, e2] = jwt.verify(jwt.sign({ a: 1 }, "s3cret", { expiresIn: -10 }), "s3cret")
print(e1, e2)
```

output:

```text
42 admin 3600
{alg: HS256, typ: JWT}
error: jwt: invalid signature error: jwt: token expired
```

## fogu:socketio

```fogu
import socketio from "fogu:socketio"
```

Socket.IO (protocol v5) servers and clients, compatible with the official JavaScript, Python and Go libraries. The server is socketioxide (WebSocket and long polling); the client speaks Engine.IO v4 over WebSocket; it runs standalone or on the port of a [fogu:web](#foguweb) app. Handlers run as `fg` tasks in the order the messages arrived; keep the program alive meanwhile (`Fogu.signal.wait()`, a loop…).

### Server

- `socketio.server({ path = "/socket.io", pingInterval, pingTimeout, ackTimeout, maxPayload })` → SocketIoServer
- `io.on("connection", fn(socket)) · io.of("/ns").on("connection", fn)` → nil — Namespaces.
- `io.listen(port | "host:port") · app.listen(port, { socketio: io })` → int · nil — Standalone, or on a fogu:web app (same port).
- `io.emit(event, ...args) · io.to(room).emit(…) · io.except(room) · io.of(ns).emit(…) · io.close()` — Several arguments travel as several arguments.
- `socket.on(event, fn(...args, ack?)) · socket.on("disconnect", fn(reason)) · socket.off(event)` → nil — `ack` is passed when the client asked for one: `ack(value)`.
- `socket.emit(event, ...args) · socket.emitWithAck(event, data, { timeout })` → nil · value
- `socket.join(room) · leave(room) · to(room).emit(…) · broadcast.emit(…) · disconnect()` — `to`/`broadcast` exclude the socket itself.
- `socket.id · ns · rooms · auth · handshake · connected` — `auth`: what the client sent; `handshake`: headers and query.

### Client

- `socketio.connect(url, { namespace, auth, headers, path = "/socket.io", reconnect = true, timeout = 10s })` → SocketIoClient · error — Returns once connected. Like the JavaScript client, the url's path is the namespace (`http://host:3000/admin`). A refused connection (auth middleware) is an error value.
- `c.on(event, fn(...args)) · c.on("connect" | "disconnect" | "error", fn) · c.off(event)` → nil — Handlers registered right after `connect` see the first messages.
- `c.emit(event, ...args) · c.emitWithAck(event, data, { timeout = 5s }) · c.disconnect()`

```fogu
import socketio from "fogu:socketio"
const log = []
const io = socketio.server()
io.on("connection", fn(socket) {
    socket.join("lobby")
    socket.on("chat", fn(msg, ack) {
        io.to("lobby").emit("chat", "echo:" + msg)
        if ack != nil { ack({ ok: true }) }
    })
    socket.on("disconnect", fn(reason) { log.push("bye") })
})
const port = io.listen("127.0.0.1:0")

const got = []
const c = socketio.connect("http://127.0.0.1:" + str(port), { auth: { token: "t1" } })?
c.on("chat", fn(m) { got.push(m) })
c.emit("chat", "hi")
print(c.emitWithAck("chat", "ack me", { timeout: 2s })?)
Fogu.sleep(200ms)
c.disconnect()
Fogu.sleep(200ms)
print(got, log)
io.close()
```

output:

```text
{ok: true}
[echo:hi, echo:ack me] [bye]
```

## fogu:grpc

```fogu
import grpc from "fogu:grpc"
```

gRPC servers and clients without code generation: the messages are described by a [fogu:protobuf](#foguprotobuf) schema (`.proto` compiled at run time) and travel as maps. Unary, server streaming, client streaming and bidirectional methods (the requests of a bidirectional call are all received before the handler runs). Each request runs as its own `fg` task.

- `grpc.server(schema)` → GrpcServer
- `srv.handle(method, fn(req, call)) · srv.handle({ method: fn }) · srv.service("pkg.Service", { Method: fn })` → nil — Methods: `"pkg.Service/Method"`, `"pkg.Service.Method"` or a unique `"Method"`. Client streaming: `req` is a list.
- `call.send(msg) · call.metadata · call.peer · call.method · call.cancelled` — `send` for server streaming (or return a list).
- `srv.listen(port | "host:port", { cert, key })` → int · error — The port. `cert`/`key`: PEM files for TLS.
- `srv.serve() · srv.close() · srv.port · srv.address · srv.methods` — `serve` runs until `close` (start it with `fg`).
- `return nil, grpc.status("NOT_FOUND", "msg")` — Fails the call with that code (any other error is `UNKNOWN`); `grpc.codes` lists them.
- `grpc.connect(url, schema, { timeout, metadata, ca, domain })` → GrpcClient — `https://` uses TLS (system roots or `ca`). Connects on the first call.
- `c.call(method, req, { timeout, metadata })` → value · error — A list for server streaming; a list of requests for client streaming. Failures: `grpc: NOT_FOUND: msg`, `grpc: DEADLINE_EXCEEDED: …`.
- `c.stream(method, req, opts) → s.next() · s.close() · s.done · s.received` → GrpcStream — One message at a time (`nil` at the end).

```fogu
import grpc from "fogu:grpc"
import protobuf from "fogu:protobuf"
const schema = protobuf.parse("syntax = \"proto3\"; package demo;
message HelloRequest { string name = 1; }
message HelloReply { string message = 1; }
message Num { int32 n = 1; }
service Greeter {
  rpc SayHello (HelloRequest) returns (HelloReply);
  rpc Count (Num) returns (stream Num);
}")?
const srv = grpc.server(schema)
srv.handle("demo.Greeter/SayHello", fn(req, call) {
    if req.name == "" { return nil, grpc.status("INVALID_ARGUMENT", "name is required") }
    return { message: "Hello, " + req.name }
})
srv.handle("Count", fn(req, call) { for i in 0..req.n { call.send({ n: i }) } })
const port = srv.listen("127.0.0.1:0")?
fg srv.serve()

const c = grpc.connect("127.0.0.1:" + str(port), schema)?
print(c.call("SayHello", { name: "Ana" })?)
print(c.call("SayHello", { name: "" }))
print(c.call("Count", { n: 3 })?)
srv.close()
```

output:

```text
{message: Hello, Ana}
error: grpc: INVALID_ARGUMENT: name is required
[{n: 0}, {n: 1}, {n: 2}]
```

## fogu:security

```fogu
import security from "fogu:security"
```

Security and network auditing: TLS and certificates (rustls, x509-parser, rcgen), port scanning, DNS (own resolver: UDP with EDNS0, TCP fallback), HTTP security headers, password strength (zxcvbn), TOTP two-factor codes, secret scanning, Ed25519 signatures, IP ranges (ipnet) and rate limiting. Scan only hosts you are allowed to test.

- `security.tls(host, { port = 443, sni, ca, alpn, timeout = 5s })` → map · error — `{ valid, error, protocol, cipher, alpn, daysLeft, handshakeMs, chain }` — the chain is inspected even when it is not trusted.
- `security.parseCert(pem | der) · security.selfSigned(names, { days = 365 })` → map · { cert, key } — `subject commonName issuer names serial notBefore notAfter daysLeft expired selfSigned ca signature keyType keyBits sha256`.
- `security.scan(host, ports = "common", { timeout = 500ms, concurrency = 200, banner })` → list · error — Ports: an int, a list, a range or `"22,80,8000-8100"`. → `[{ port, service, banner }]`.
- `security.dns(name, type = "A", { server, timeout = 3s }) · security.reverseDns(ip, opts?)` → list · error — `A AAAA MX TXT NS CNAME SOA CAA SRV PTR…` → `[{ type, value, ttl }]`. Servers: `server`, `$FOGU_DNS`, the system's, then 1.1.1.1/8.8.8.8.
- `security.headers(url, { timeout })` → map · error — HSTS, CSP, X-Content-Type-Options, X-Frame-Options, Referrer-Policy, Permissions-Policy → `{ grade, score, present, missing, warnings }` (leaky `server`, unsafe CSP, cookies without Secure/HttpOnly/SameSite…).
- `security.passwordStrength(pw, [userInputs])` → map — `{ score 0..4, guesses, crackTime, crackTimeOnline, warning, suggestions }`.
- `security.totpSecret() · totp(secret, opts) · verifyTotp(secret, code, { window = 1 }) · otpauthUrl(secret, account, { issuer })` — RFC 6238 (SHA-1, 6 digits, 30 s); the URL goes into a QR code for authenticator apps.
- `security.scanSecrets(text) · security.scanFiles(path)` → list — AWS keys, GitHub/GitLab/Slack/Stripe/OpenAI/Anthropic tokens, private keys, JWTs, passwords in URLs, `password = "…"` → `[{ file?, rule, line, column, match }]` (redacted).
- `security.keypair() · sign(privateKey, msg) · verify(publicKey, msg, sig)` — Ed25519, base64 keys and signatures.
- `security.constantTimeEq(a, b)` → bool
- `security.cidr(c) · inCidr(ip, c | [c]) · isPrivate(ip) · hosts(c, { limit })`
- `security.rateLimiter(ratePerSecond, burst) → allow(key?, cost?) · remaining(key) · reset(key?)` → RateLimiter — A token bucket per key (e.g. the client IP).

```fogu
import security from "fogu:security"
const pair = security.selfSigned(["localhost", "127.0.0.1"], { days: 30 })
const cert = security.parseCert(pair.cert)?
print(cert.commonName, cert.names, cert.daysLeft, cert.selfSigned)
print(security.passwordStrength("password123").score, security.passwordStrength("correct horse battery staple 9!").score)
const secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
print(security.totp(secret, { time: 59, digits: 8 }), security.verifyTotp(secret, "287082", { time: 59 }))
for s in security.scanSecrets("aws_key = \"AKIAIOSFODNN7EXAMPLE\"") { print(s) }
const k = security.keypair()
print(security.verify(k.publicKey, "hello", security.sign(k.privateKey, "hello")))
print(security.cidr("192.168.1.77/24").network, security.inCidr("10.1.2.3", "10.0.0.0/8"), security.isPrivate("8.8.8.8"))
const rl = security.rateLimiter(1, 2)
print(rl.allow("ip"), rl.allow("ip"), rl.allow("ip"))
```

output:

```text
localhost [localhost, 127.0.0.1] 29 true
0 4
94287082 true
{column: 12, line: 1, match: AKIA…LE, rule: aws-access-key}
true
192.168.1.0 true false
true true false
```

```fogu
import security from "fogu:security"
const t = security.tls("example.com")?
print(t.valid, t.protocol, t.daysLeft > 0, t.chain[0].issuer)
print(security.scan("127.0.0.1", "22,80,443,5432")?)
print(security.dns("gmail.com", "MX")?[0].type)
const h = security.headers("https://github.com")?
print(h.grade, h.missing.map(fn(m) { return m.header }))
```

output (with network access):

```text
true TLSv1.3 true C=US, O=SSL Corporation, CN=Cloudflare TLS Issuing ECC CA 3\n[{banner: nil, port: 22, service: ssh}]\nMX\nB [permissions-policy]
```

## fogu:websocket

```fogu
import websocket from "fogu:websocket"
```

WebSocket client (`ws://`, `wss://`) and a standalone server. The same connection object is used by `app.ws` in `fogu:web`. Each connection runs on the shared runtime (no thread per connection): `next` and `accept` wait cooperatively and wake up when a message or a client arrives.

- `websocket.connect(url, { headers, protocols, timeout })` → WebSocket · error — A rejected handshake (401, 404…) is an error value.
- `websocket.listen(port, { host })` → Server · error — `port 0` picks a free port; `server.accept(timeout?)` → WebSocket, fields `port url`.
- `ws.send(data)` → nil · error — str → text frame, [buffer](#values) or list of bytes → binary frame, other values → JSON text.
- `ws.next(timeout?)` → Message · nil — `{ type: "text" | "binary", data, text, bytes, size, json() }`: `data` is a str for text frames and a buffer for binary ones, `text` always a str, `bytes` always a buffer. nil on timeout or when closed.
- `ws.tryNext() · ws.ping() · ws.close(code = 1000, reason)` — `tryNext`: a queued message or nil, without waiting.
- `ws.open · ws.closeCode · ws.closeReason · ws.path · ws.url · ws.headers · ws.protocol · ws.pending · ws.error` — `pending`: messages queued; `error`: why the connection failed, or nil.

```fogu
import websocket from "fogu:websocket"
const server = websocket.listen(0)?
fg fn() {
    const conn = server.accept()?
    while true {
        const m = conn.next()?
        if m == nil { break }
        if m.type == "binary" {
            conn.send(m.data)                   // a buffer: echoed as a binary frame
        } else {
            conn.send(`echo ${conn.path}: ${m.data}`)
        }
    }
}()
const ws = websocket.connect(server.url + "/room?x=1")?
ws.send("hello")
print(ws.next(2s)?.data)
ws.send({ kind: "json" })
print(ws.next(2s)?.data)
ws.send(bytes([1, 2, 255]))
const m = ws.next(2s)?
print(m.type, m.data, m.size, typeof(m.text))
ws.close()
```

output:

```text
echo /room?x=1: hello
echo /room?x=1: {"kind":"json"}
binary <buffer 3 bytes: 01 02 ff> 3 str
```

## fogu:net

```fogu
import net from "fogu:net"
```

TCP and UDP sockets on the shared runtime: an operation that waits (`accept`, reads, writes, `recv`, `tcpConnect`) lets the other `fg` tasks run. Data is read as a [buffer](#values) (`read readExact readAll recv`) or as text (`readLine readText`); writes take a str, a buffer or a list of byte ints. Network failures are error values. Timeouts are durations or int milliseconds.

- `net.tcpListen("host:port")` → TcpListener · error — Port 0 picks a free one. `accept(timeout?)` → TcpStream, `for conn in listener`, `setTimeout(d)`, `close()`; fields `addr localAddr port`.
- `net.tcpConnect("host:port", timeout?)` → TcpStream · error
- `stream.read(n = 64 KiB)` → buffer · nil · error — What arrived (up to `n` bytes); `nil` at the end of the stream.
- `stream.readExact(n) · stream.readAll()` → buffer · error — Exactly `n` bytes · everything up to the end of the stream.
- `stream.readLine() · stream.readText(n)` → str · nil · error — A line without `\n`/`\r\n` · up to `n` bytes as text; `nil` at the end of the stream.
- `stream.write(data) · stream.writeLine(data)` → int · error — Bytes written.
- `stream.setTimeout(d) · stream.close()` — `setTimeout` bounds every later operation (a timeout is an error value).
- `stream.peer · stream.localAddr · stream.open`
- `net.udpBind("host:port")` → UdpSocket · error — `sendTo(addr, data)`, `recvFrom(n?)` → `{ data, text, from, len }` (`data` is a buffer), `connect(addr)`, `send(data)`, `recv(n?)` → buffer, `setTimeout(d)`, `close()`; field `addr`.
- `net.resolve(host)` → list · error — IP addresses.

```fogu
import net from "fogu:net"
const server = net.tcpListen("127.0.0.1:0")?
fg fn() {
    while true {
        const conn = server.accept()?
        fg fn() {                               // one task per connection
            while true {
                const data = conn.read()?       // a buffer · nil at end of stream
                if data == nil { break }
                conn.write(data)?
            }
            conn.close()
        }()
    }
}()

const c = net.tcpConnect(server.addr, 2s)?
c.setTimeout(2s)                                // bounds every later operation
c.write("one\ntwo\n")?
print(c.readLine()?, "|", c.readLine()?)
c.write(bytes([0, 255, 10]))?
const raw = c.readExact(3)?
print(typeof(raw), raw.hex(), raw[1])
c.close()

const a = net.udpBind("127.0.0.1:0")?
const b = net.udpBind("127.0.0.1:0")?
a.sendTo(b.addr, "hello udp")?
const p = b.recvFrom()?
print(p.text, p.len, typeof(p.data), p.from == a.addr)
```

output:

```text
one | two
buffer 00ff0a 255
hello udp 9 buffer true
```

## fogu:events

```fogu
import events from "fogu:events"
```

- `events.EventEmitter()` → Emitter
- `e.on(event, fn) · e.once(event, fn)` → nil — Listeners receive every emitted argument (declare them all).
- `e.off(event, fn?) · e.removeAll(event?)` → nil
- `e.emit(event, ...args)` → bool — `true` when someone listened.
- `e.listeners(event) · e.eventNames() · e.hasListeners(event)`

```fogu
import events from "fogu:events"
const bus = events.EventEmitter()
bus.on("order", (id, total) => print("order", id, "total", total))
bus.once("order", (id, total) => print("first order only:", id))
print(bus.emit("order", 1, 9.9))
print(bus.emit("order", 2, 15))
print(bus.emit("nobody-listens"), bus.eventNames())
```

output:

```text
order 1 total 9.9
first order only: 1
true
false
false []
```

## fogu:cron

```fogu
import cron from "fogu:cron"
```

Jobs run when the program calls `cron.tick()` (typically in its main loop).

- `cron.every(interval, fn)` → nil — A job every `interval` (duration or ms).
- `cron.schedule(expression, fn)` → nil — 5-field cron expression (`"*/5 * * * *"`); an invalid one stops the program.
- `cron.tick()` → int — Runs the jobs that are due; returns how many ran.
- `cron.list()` → list — Job names (`every(10ms)`, the cron expression…).
- `cron.clear()` → nil — Removes every job.

```fogu
import cron from "fogu:cron"
import time from "fogu:time"
let beats = 0
cron.every(10ms, () => { beats = beats + 1 })
cron.schedule("0 3 * * *", () => print("nightly backup"))
for i in 0..6 {
    cron.tick()
    time.delay(10ms)
}
print(beats >= 3, cron.list())
```

output:

```text
true [every(10ms), schedule(0 3 * * *)]
```

## fogu:nats

```fogu
import nats from "fogu:nats"
```

NATS core protocol plus JetStream (streams, acknowledged publishing, pull consumers).

- `nats.connect(url | { url, user, pass, token, name, timeout })` → Nats · error — `nats://[user:pass@|token@]host:4222`, default `$NATS_URL`.
- `nc.publish(subject, data?, { headers, reply })` → nil · error — `data`: str, or any value as JSON.
- `nc.subscribe(subject, { queue, max })` → Subscription — Wildcards `*` and `>`; queue groups split the messages.
- `sub.next(timeout?) · sub.tryNext() · sub.unsubscribe()` → Message · nil
- `nc.request(subject, data?, { timeout, headers })` → Message · error — No responders → error value.
- `nc.flush() · nc.close() · nc.connected · nc.server`
- `msg.subject · .data · .headers · .reply · .json() · .respond(data)`

### JetStream

- `nc.jetstream({ prefix, timeout })` → JetStream
- `js.addStream({ name, subjects, storage, max_msgs, max_age, … })` → map · error — NATS stream config (snake_case keys).
- `js.updateStream(cfg) · js.streamInfo(name) · js.deleteStream(name) · js.purgeStream(name)`
- `js.publish(subject, data, { msgId, headers, timeout })` → { stream, seq, duplicate } — `msgId` deduplicates.
- `js.consumer(stream, { durable, filter, ackWait, deliver, maxDeliver })` → Consumer · error — A durable pull consumer.
- `consumer.fetch(batch = 1, expires = 5s)` → list — Up to `batch` messages.
- `msg.ack() · msg.nak(delay?) · msg.term() · msg.inProgress()` → nil — `msg.seq`, `msg.delivered`, `msg.stream`.

```fogu
import nats from "fogu:nats"
const nc = nats.connect("nats://127.0.0.1:4222")?
const sub = nc.subscribe("orders.*")
nc.publish("orders.created", { id: 1 })
print(sub.next(1s)?.json())

const svc = nc.subscribe("math.double")
fg fn() { const req = svc.next()?; req.respond(str(int(req.data) * 2)) }()
print(nc.request("math.double", "21")?.data)

const js = nc.jetstream()
js.addStream({ name: "TASKS", subjects: ["tasks.>"] })?
print(js.publish("tasks.email", { to: "ana@x.com" }, { msgId: "t1" })?)
const c = js.consumer("TASKS", { durable: "workers" })?
for msg in c.fetch(10, 1s)? { print(msg.subject, msg.seq); msg.ack() }
```

output (with a NATS server, -js):

```text
{id: 1}
42
{duplicate: false, seq: 1, stream: TASKS}
tasks.email 1
```

### fogu() — the high-level layer

The same API as the usual `nats.js` wrapper (KV stores, `sub`/`pub` with handlers, services with auth, commands); it interoperates with it — same KV buckets, same `service: ` subjects and error payloads. Data: nil is sent empty, a string as is, the rest as JSON; received data is parsed as JSON when it can be. Handlers run as `fg` tasks.

- `nats.fogu(url | config) · nc.fogu()` → NatsFogu · error — Connects (or wraps an open connection).
- `f.store.createKV(name, fn?(entry), { history = 5, ttl })` → NatsKv · error — A JetStream KV bucket (`KV_name`). With `fn`: a watch — the last value of each key, then every change: `{ bucket, key, value, operation: PUT|DEL|PURGE, revision }`; returning `false` stops it.
- `f.store.kv(name) → create(key, data) · set · get · del · keys` — `create` → false when the key exists. Using a store before `createKV` is an error value.
- `f.sub({ topic }, fn(data), { queue }) · f.pub({ topic }, data)` → nil
- `f.service(name, fn(input), auth?(headers), { queue })` → nil — On `service:name`. `headers.get(n) · has(n)`. A falsy auth answers `{ error: "unauthorized", data: nil }`; a failing handler `{ error: "internal_error", data: nil }`; `nil` answers `{}`.
- `f.command(name, data = {}, headers = {}, { timeout = 5000 ms })` → value · error — No service → error value.
- `f.connect(newConfig?) · f.close() · f.connection` — `connection` is the underlying `Nats`.

```fogu
import nats from "fogu:nats"
const f = nats.fogu("nats://127.0.0.1:4222")?
f.store.createKV("config", fn(e) { print("watch", e.operation, e.key, e.value) })?
const kv = f.store.kv("config")
kv.set("theme", { dark: true })?
print(kv.get("theme")?, kv.create("theme", 1)?, kv.keys()?)
f.sub({ topic: "orders.*" }, fn(data) { print("order", data) })
f.pub({ topic: "orders.new" }, { id: 1 })
f.service("math.double", fn(x) { return x * 2 }, fn(h) { return h.get("token") == "s3cret" })
print(f.command("math.double", 21, { token: "s3cret" })?)
print(f.command("math.double", 21)?)
```

output (with nats-server -js):

```text
watch PUT theme {dark: true}
{dark: true} false [theme]
order {id: 1}
42
{data: nil, error: unauthorized}
```

## fogu:rabbitmq

```fogu
import rabbitmq from "fogu:rabbitmq"
```

RabbitMQ (AMQP 0-9-1, own client on the runtime's networking; `amqps://` uses TLS). Publishing waits for the broker's confirmation. A channel closed by an error (for example declaring a queue with other options) is reopened on the next call.

- `rabbitmq.connect(url?)` → RabbitMQ · error — `amqp://user:pass@host:5672/vhost`, default `$AMQP_URL`.
- `mq.exchange(name, kind = "direct", { durable, autoDelete, internal, arguments })` → nil · error — `direct fanout topic headers`.
- `mq.queue(name = "", { durable, exclusive, autoDelete, ttl, maxLength, deadLetter, passive })` → { name, messages, consumers } — "" gives a server-named queue.
- `mq.bind(queue, exchange, key = "") · mq.unbind(…)` → nil · error
- `mq.publish(exchange, key, data, { persistent, headers, contentType, correlationId, replyTo, messageId, expiration, priority, mandatory })` → nil · error — Maps are sent as JSON.
- `mq.send(queue, data, opts?)` → nil · error — Through the default exchange.
- `mq.get(queue, { noAck })` → Message · nil
- `mq.consume(queue, { prefetch = 10, noAck, exclusive, tag })` → Consumer — `next(timeout?)`, `tryNext()`, `cancel()`.
- `msg.data · .json() · .routingKey · .exchange · .headers · .redelivered · …` — Properties: `contentType correlationId replyTo messageId type persistent`.
- `msg.ack() · msg.nack({ requeue = true }) · msg.reject({ requeue })` → nil
- `mq.purge(queue) · mq.deleteQueue(name) · mq.deleteExchange(name) · mq.close()`

```fogu
import rabbitmq from "fogu:rabbitmq"
const mq = rabbitmq.connect("amqp://guest:guest@127.0.0.1:5672/%2f")?
mq.exchange("shop", "topic", { durable: true })?
mq.queue("emails", { durable: true })?
mq.bind("emails", "shop", "order.*")?
mq.publish("shop", "order.created", { id: 7 }, { persistent: true })?
const worker = mq.consume("emails", { prefetch: 5 })?
const msg = worker.next(2s)?
print(msg.routingKey, msg.json(), msg.persistent)
msg.ack()
```

output (with RabbitMQ):

```text
order.created {id: 7} true
```

## fogu:mqtt

```fogu
import mqtt from "fogu:mqtt"
```

MQTT 3.1.1 and 5 (own client on the runtime's networking) with automatic reconnection (subscriptions are restored). QoS 0/1/2, retained messages, wills; with `version: 5` also message properties, subscription options and reason codes.

- `mqtt.connect(url?, { clientId, username, password, keepAlive, clean = true, will, timeout, version = 3, sessionExpiry })` → Mqtt · error — `mqtt://` or `mqtts://`; `will: { topic, payload, qos, retain }`; `version: 5` speaks MQTT 5. A refused connection is an error with the broker's reason.
- `m.publish(topic, data, { qos = 0, retain })` → nil · error — QoS 1/2 wait for the broker's acknowledgement.
- `MQTT 5 publish options: { contentType, responseTopic, correlationData, messageExpiry, userProperties }` — Ignored on 3.1.1.
- `m.subscribe(filter | [filters], { qos, noLocal, retainAsPublished, retainHandling }) · m.unsubscribe(…)` → nil · error — `+` one level, `#` the rest; the last three are MQTT 5.
- `m.next(timeout?) · m.tryNext()` → Message · nil — `{ topic, data, qos, retain, json(), matches(filter) }`; MQTT 5 adds `contentType responseTopic correlationData messageExpiry userProperties`.
- `m.connected · m.clientId · m.version · m.subscriptions · m.close()`
- `mqtt.matches(filter, topic)` → bool

```fogu
import mqtt from "fogu:mqtt"
const m = mqtt.connect("mqtt://127.0.0.1:1883", { clientId: "monitor" })?
m.subscribe("home/+/temperature", { qos: 1 })?
m.publish("home/kitchen/temperature", "21.5", { qos: 1, retain: true })?
const msg = m.next(2s)?
print(msg.topic, msg.data, msg.qos, msg.matches("home/#"))
print(mqtt.matches("a/+/c", "a/b/c"))
```

output (with an MQTT broker):

```text
home/kitchen/temperature 21.5 1 true
true
```

## fogu:broker

```fogu
import broker from "fogu:broker"   // also "fogu:mq"
```

The Fogu broker inside the runtime: key/value store with TTLs, channels, pub/sub with wildcard topics (`chat.*`, `chat.**`), RPC (`act`/`call`), API keys with scoped permissions, persistence (SurrealKV), clusters and a web management UI (`b.admin`). It speaks the FoguMQ wire protocol, so the Bun, Go, Rust and Python clients (`fogu/client/`) and [fogu:fogumq](#fogufogumq) talk to it. The inline client has the same API as a fogumq client, over an in-memory pipe.

- `broker.start(addr = "127.0.0.1:7222", { token, db, node, peers, secret, log, purgeEvery = 60s })` → Broker · error — `token`: the root token (default `$FOGU_ROOT_TOKEN` or a random one). `db`: a SurrealKV folder (KV and API keys survive restarts, written in order by a background writer); in memory without it. A redb file of an earlier version is converted (kept as ` .redb.bak`). Also `broker.broker(…)` / `mq.broker(…)`.
- `b.client({ token, requestTimeout })` → FoguMQ — The inline client (root by default): `kv`, `channel(n).join/send/onMessage/sub/pub/act/call`, `apikeys`.
- `b.admin(port | "host:port", { user = "admin", password = b.token, host, title, theme })` → { url, port } · error — The management UI (fogu:web + [fogu:topcoat](#fogutopcoat)): a sign-in page (session cookie; HTTP basic auth also works for scripts), a live overview with traffic per second and trend lines, channels with their members, a key-value browser/editor and API keys (create, copy the token once, revoke), light/dark themes. `theme`: `system` (default), `light`, `dark`. Keep the program alive with `Fogu.signal.wait()`.
- `b.inspect() · b.keys({ prefix, limit = 200 })` → map · list — `inspect`: `{ node, address, started, clients, channels: [{ name, members }], topics: [{ pattern, subscribers }], actions, remoteActions: [{ method, node }], peers, keys, apiKeys, messages, publications, calls, pendingCalls, persistent }`. `keys` → `[{ key, value, ttl }]` (ttl in seconds).
- `b.stats() · b.close() · b.url · b.address · b.port · b.node · b.token` — `stats()`: `{ node, clients, channels, topics, actions, remoteActions, peers, keys }`. `close()` ends every connection and releases the db.
- `broker.connect(url, opts)` → FoguMQ · error — A remote broker (same as `fogumq.connect`).
- `cluster: { node, peers: [addr…], secret }` — Nodes link to the `peers` they are given (one side is enough) and authenticate with `secret` (default: the token). Channel messages and publications reach every node; `call` goes to the node that registered the method; KV writes made while the nodes are linked are copied to every node (last write wins; `incr` is atomic per node; there is no catch-up of older keys).

```fogu
import broker from "fogu:broker"
const b = broker.start("127.0.0.1:0", { token: "root1", node: "n1" })?
const c = b.client()
c.kv.set("visits", 10)?
print(c.kv.incr("visits")?)
const room = c.channel("chat")
const svc = room.act("add")?
fg fn() {
    while true {
        const [r, err] = svc.next()
        if err != nil { break }          // the broker closed
        r.reply(r.params.a + r.params.b)
    }
}()
print(room.call("add", { a: 1, b: 2 })?)
const sub = room.sub("news")?
room.pub("news", { id: 1 })?
print(sub.next(1s))

// a second node: RPC, publications and KV writes cross the link
const n2 = broker.start("127.0.0.1:0", { token: "root1", node: "n2", peers: [b.address] })?
Fogu.sleep(300ms)
const c2 = n2.client()
c.kv.set("shared", "from n1")?          // written while the nodes are linked
Fogu.sleep(100ms)
print(c2.channel("chat").call("add", { a: 20, b: 22 })?, c2.kv.get("shared")?)
print(n2.stats().remoteActions)
n2.close()
b.close()
```

output:

```text
11
3
{data: {id: 1}, topic: chat/news}
42 from n1
[chat/add]
```

## fogu:kafka

```fogu
import kafka from "fogu:kafka"
```

Apache Kafka (and Redpanda) in pure Rust (rskafka): topics, producing and consuming, compression, TLS and SASL. Values: a string is sent as UTF-8, other values as JSON; received records have `value` as text. Without a `partition`, records with a key go to `hash(key) % partitions` (same key, same order), the others round-robin. Consumers keep their own offsets — Kafka consumer groups (coordinated rebalancing, offsets stored in Kafka) are not supported.

- `kafka.connect(brokers, { clientId, tls: true | { ca }, sasl: { mechanism, username, password }, timeout = 10s })` → Kafka · error — `brokers`: a list or `"h1:9092,h2:9092"`. SASL: `PLAIN SCRAM-SHA-256 SCRAM-SHA-512`.
- `k.topics() · k.createTopic(name, { partitions = 1, replication = 1 }) · k.deleteTopic(name) · k.offsets(topic)` — `createTopic` → false if it existed; `offsets` → `{ partition: { earliest, latest } }`.
- `k.send(topic, value, { key, headers, partition, compression })` → { partition, offset } · error — `compression`: `none gzip lz4 snappy zstd`.
- `k.sendBatch(topic, [value | { value, key, headers }], { partition, compression })` → list · error
- `k.consumer(topic, { from = "latest" | "earliest" | offset, partitions, maxWait = 500ms, maxBytes })` → KafkaConsumer · error
- `c.next(timeout = 1s) · c.poll(timeout = 1s) · c.seek(partition, offset) · c.offsets · c.close()` — `next` → a record or nil; `poll` → the records that arrived.
- `record.topic · partition · offset · key · value · headers · timestamp · json() · bytes()`

```fogu
import kafka from "fogu:kafka"
const k = kafka.connect(["localhost:9092"])?
k.createTopic("orders", { partitions: 3 })?
const c = k.consumer("orders", { from: "earliest" })?
print(k.send("orders", { id: 1, total: 9.5 }, { key: "client-7", headers: { source: "fogu" } })?)
const r = c.next(3s)?
print(r.key, r.json(), r.headers)
```

output (with Kafka 3.9):

```text
{offset: 0, partition: 2}\nclient-7 {id: 1, total: 9.5} {source: fogu}
```

## fogu:fogumq

```fogu
import fogumq from "fogu:fogumq"
```

Client of the Fogu broker (binary protocol, JSON payloads): key/value store, channels, pub/sub, RPC and API keys. Methods that wait for the server let the other `fg` tasks run.

- `fogumq.connect("fogu://token@host:port", { timeout, requestTimeout })` → FoguMQ · error — Fields `connected authenticated isRoot url`; `ping() close()`.
- `mq.kv.get(key) · set(key, value, ttl?) · del(key) · exists(key) · incr(key, n?) · decr(key, n?)` → value · error — `get` gives nil for a missing key.
- `mq.channel(name)` → Channel — `join(alias?) leave() send(data) members() list()`.
- `channel.onMessage()` → Listener — `next(timeout?)` → `{ channel, data, ... }`; `tryNext() close()`.
- `channel.subscribe(topic) · channel.publish(topic, data)` → Subscription · error — Topics live under the channel: `name/topic`. Short forms: `sub`, `pub`.
- `channel.act(method)` → Service · error — `next()` → Request with `method params reply(result) fail(msg)`.
- `channel.call(method, params?, timeout?)` → value · error
- `mq.apikeys.create(name, { channels, kv, topics, rpc }) · delete(id) · get(id) · list()` → value · error — Root token only.

```fogu
import fogumq from "fogu:fogumq"
const mq = fogumq.connect(Fogu.env.FOGUMQ_URL)?
mq.kv.set("visits", 1)?
print(mq.kv.incr("visits")?)
const room = mq.channel("calc")
const svc = room.act("sum")?
fg fn() { const r = svc.next(); r.reply(r.params.reduce((a, n) => a + n, 0)) }()
print(room.call("sum", [1, 2, 3])?)
```

output (with a Fogu broker):

```text
2
6
```

## fogu:foguMQ-client

```fogu
import mqc from "fogu:foguMQ-client"
```

Client of the FlowMQ v6 wire protocol (length-prefixed binary frames): numbered channels, pub/sub with wildcard topics (`*` one token, `>` the rest), RPC, job queues and API keys. Each method sends exactly one protocol frame. The socket runs on the shared I/O runtime; methods that wait for the broker let the other `fg` tasks run, so one task can answer the requests another makes. Messages, requests and jobs are queued in `Subscription` / `Service` / `Worker` objects. Network and broker failures are `error` values. Payloads: a str is sent as is, any other value as JSON; received payloads are strs (use `json.parse`). `FLOWMQ_DEBUG=1` prints protocol errors to stderr.

### Connection and channels

- `mqc.connect("flowmq://token@host:7222?namespace=…", { timeout, requestTimeout, clientId, autoReconnect })` → FoguMQClient · error — URL query: `clientId namespace requestTimeout connectTimeout reconnectBackoff` (ms) `maxFrame maxChannels autoReconnect`. Defaults: port 7222, timeouts 5s. With `autoReconnect` the client reconnects and re-opens its channels, subscriptions, services and workers.
- `mq.connected · namespace · authMode · keyId · sessionId · url` — From the broker's HELLO_OK. `authMode`: `master`, an API key mode, or `unknown`.
- `mq.channelOpen(ch) · mq.channelClose(ch)` → nil · error — Channels are ints 1..65535 and must be open before use; opening one twice is an error.
- `mq.close()` → nil — Sends CLOSE and ends the connection.

### Pub/sub

- `mq.subscribe(ch, pattern, group?)` → Subscription · error — `next(timeout?)` → `{ topic, group, data }` (nil after the timeout); `tryNext()`; fields `pattern group channel pending`; `close()` unsubscribes.
- `mq.unsubscribe(ch, pattern, group?)` → nil · error
- `mq.publish(ch, topic, data)` → nil · error — Fire and forget.
- `mq.publishSync(ch, topic, data)` → nil · error — Waits for the broker's ACK.

### RPC

- `mq.serve(ch, service, topic)` → Service · error — `next(timeout?)` → Request with `service topic data answered` and `reply(data) fail(msg)`. A request dropped without an answer fails on the caller's side. `close()` unserves.
- `mq.unserve(ch, service, topic)` → nil · error
- `mq.request(ch, service, topic, data)` → str · error — The reply payload; a `fail` or a missing handler is a broker error.

### Job queues

- `mq.registerWorker(ch, queue, { filter = ">", prefetch = 1, visibility = 30s })` → Worker · error — `next(timeout?)` → Job with `id attempt deadline queue type data channel done`. `close()` sends WORKER_OFF.
- `job.ack() · job.nack({ requeue = true, delay, reason }) · job.extend(duration)` → nil — A job dropped without an answer is nacked and requeued.
- `mq.workerOff(ch, queue)` → nil · error
- `mq.enqueue(ch, queue, type, data, { jobId, priority, delay, retries, ttl })` → { jobId, seq } · error — `delay` and `ttl`: durations or ms.

### API keys

- `mq.apikeyCreate(ch, name, permissions, labels?)` → { id, token, permissions } · error — `permissions` is sent as JSON, e.g. `{ channels, services, queues, publish, subscribe, worker, enqueue }`; the key is created in the client's namespace.
- `mq.apikeyRevoke(ch, id) · mq.apikeyGet(ch, id) · mq.apikeyList(ch)` → nil · str · str · error — `get`/`list` return the broker's JSON text. All API key operations need a master token.

```fogu
import mqc from "fogu:foguMQ-client"
import json from "fogu:json"
const mq = mqc.connect(Fogu.env.FLOWMQ_URL)?
mq.channelOpen(1)?
mq.channelOpen(2)?
const sub = mq.subscribe(2, "orders.>")?
mq.publishSync(1, "orders.created", { id: 42 })?
print(json.parse(sub.next(2s).data))
const svc = mq.serve(2, "users", "get")?
fg fn() { const r = svc.next(); r.reply("user " + r.data) }()
print(mq.request(1, "users", "get", "42")?)
const jobs = mq.registerWorker(2, "emails")?
mq.enqueue(1, "emails", "welcome", "ana@example.com")?
const job = jobs.next(5s)
print(job.type, job.data)
job.ack()
mq.close()
```

output (with a FlowMQ v6 broker):

```text
{id: 42}
user 42
welcome ana@example.com
```

## fogu:bullmq

```fogu
import bullmq from "fogu:bullmq"
```

Job queues on Redis, wire-compatible with [BullMQ](https://docs.bullmq.io) for Node.js v5 (on the `bullmq-rs` crate): Fogu and Node producers and workers can share the same queues, and dashboards such as Bull Board see them. The worker owns locks, stalled-job recovery, retries with backoff, concurrency and rate limits; each job is handed to the program, which finishes it with `complete` or `fail`. Redis failures are `error` values.

### Queue

- `bullmq.queue(name, { connection, prefix = "bull" })` → BullQueue · error — `connection` is a `redis://` / `rediss://` URL (default `$REDIS_URL` or `redis://127.0.0.1:6379`).
- `q.add(name, data, opts?)` → job · error — Options: `delay attempts backoff: { type: "fixed" | "exponential", delay, max } priority jobId ttl deduplication: { id, ttl }`.
- `q.addBulk([{ name, data, opts }])` → list · error
- `q.getJob(id) · q.getJobs(states, start = 0, end = -1, asc = false)` → job | nil · list — A job: `{ id name data state timestamp attemptsMade progress processedOn finishedOn failedReason returnvalue opts }`.
- `q.counts() · q.count()` → map · int — Per state: `wait active delayed prioritized completed failed paused waiting-children`.
- `q.upsertJobScheduler(id, { every | pattern, tz, limit, immediately }, { name, data, opts })` → job · error — Repeatable jobs (cron `pattern` or `every` duration).
- `q.removeJobScheduler(id)` → bool · error
- `q.pause() · resume() · isPaused() · drain() · remove(id) · clean(grace, limit, state) · obliterate({ force }) · getJobLogs(id)`

### Worker and events

Limitation: the worker of `bullmq-rs` publishes `completed` events with an empty return value; `q.getJob(id).returnvalue` has the value given to `complete`.

- `bullmq.worker(name, { connection, prefix, concurrency = 1, lockDuration, maxStalledCount, limiter: { max, duration } })` → BullWorker · error
- `w.next(timeout?) · w.tryNext()` → BullJob | nil · error — Waits cooperatively (other `fg` tasks keep running); nil on timeout or after `close()`.
- `job.complete(value?) · job.fail(reason)` → nil — `fail` retries the job while `attempts` remain. A job dropped without an answer fails.
- `job.progress(value) · job.log(text)` → nil — Fields are those of the job map, plus `done`.
- `w.pause() · w.resume() · w.close() · w.running · w.pending`
- `bullmq.events(name, { connection, prefix, lastEventId })` → BullEvents · error — `next(timeout?)` → `{ event, jobId, id, … }` (`added waiting active progress completed failed delayed stalled removed`).

```fogu
import bullmq from "fogu:bullmq"
const conn = { connection: "redis://127.0.0.1:6379" }
const q = bullmq.queue("emails", conn)?
q.add("welcome", { to: "ana@x.com" }, { attempts: 3, backoff: { type: "exponential", delay: 1s } })?
const w = bullmq.worker("emails", { connection: conn.connection, concurrency: 4 })?
while true {
    const job = w.next()?
    print(job.name, job.data.to, job.attemptsMade)
    job.progress(50)
    job.complete({ sent: true })          // or job.fail("smtp down") → retried
}
```

output (with Redis):

```text
welcome ana@x.com 0
```

## fogu:mail

```fogu
import mail from "fogu:mail"
```

E-mail over SMTP (presets for Gmail, Outlook, Office 365, Yahoo, iCloud, Zoho, Fastmail, AOL, GMX, Amazon SES, Mailtrap, Mailpit) or the HTTP APIs of Resend, SendGrid, Mailgun, Postmark, Brevo, MailerSend and Mailjet. The SMTP client and the MIME builder are Fogu's own (STARTTLS, implicit TLS, AUTH PLAIN/LOGIN, attachments, inline images, UTF-8 headers). The `console` provider only prints (development and tests).

- `mail.connect({ provider, … } | url)` → Mailer — No connection is made yet.
- `SMTP: { provider: "gmail" | "smtp" | …, host, port, secure, user, password, region }` — `secure`: `tls` (465), `starttls` (587), `none`; URLs `smtp://user:pass@host:587`, `smtps://…`.
- `APIs: { provider: "resend" | "sendgrid" | …, apiKey, domain, region, baseUrl }` — `apiKey` defaults to `$RESEND_API_KEY`, `$SENDGRID_API_KEY`…; Mailgun needs `domain`, Mailjet `apiSecret`.
- `common: { from, timeout }` — Default sender.
- `mailer.send(message)` → { id, provider } · error — A rejected message, a bad address or a network failure is an error value.
- `mailer.sendMany([messages])` → list · error
- `mailer.verify()` → bool · error — SMTP: connects and authenticates.
- `mailer.render(message)` → str — The RFC 5322 text that SMTP would send.
- `message: { from, to, cc, bcc, replyTo, subject, text, html, attachments, headers }` — Addresses: `"a@x.com"`, `"Ana "`, `{ name, email }` or a list.
- `attachment: { filename, content | path, contentType, cid, encoding }` — `content`: str or list of bytes; `cid` makes it inline (` `).

```fogu
import mail from "fogu:mail"
const mailer = mail.connect({ provider: "console", from: "Shop <shop@example.com>" })?
const r = mailer.send({
    to: ["ana@example.com", { name: "Bob", email: "bob@example.com" }],
    subject: "Your order",
    text: "Thanks for buying!",
    attachments: [{ filename: "invoice.csv", content: "item,price\nbook,10\n" }],
})?
print(r)
const gmail = mail.connect({ provider: "gmail", user: "me@gmail.com", password: "app-password" })?
print(gmail.host, gmail.port, gmail.secure)
const [_, err] = mailer.send({ to: "not-an-address", subject: "x", text: "x" })
print(err)
```

output:

```text
--- mail ---
From: Shop <shop@example.com>
To: ana@example.com, Bob <bob@example.com>
Subject: Your order
Attachment: invoice.csv (text/csv, 19 bytes)

Thanks for buying!
---
{id: console-1, provider: console}
smtp.gmail.com 587 starttls
error: mail: invalid address 'not-an-address' in 'to'
```

## fogu:ai

```fogu
import ai from "fogu:ai"
```

LLMs over HTTP: OpenAI-compatible APIs (OpenAI, Groq, OpenRouter, DeepSeek, Mistral, xAI, Together, Fireworks, Perplexity, Ollama, LM Studio), Anthropic and Google Gemini — chat, streaming, tools, agents, embeddings, and MCP: serve tools, resources and prompts to Claude and other clients, or use any MCP server's tools in an agent. Keys default to `$OPENAI_API_KEY`, `$ANTHROPIC_API_KEY`, `$GEMINI_API_KEY`…

### Client

- `ai.client({ provider, model, apiKey, baseUrl, timeout, headers, maxTokens } | "provider/model")` → AiClient — Provider `compatible` + `baseUrl` for any OpenAI-style server.
- `client.chat(prompt | { messages, prompt, system, model, temperature, maxTokens, topP, stop, tools, toolChoice, json })` → map · error — `{ text, toolCalls, finishReason, usage: { inputTokens, outputTokens }, model, message }`.
- `client.stream(same)` → AiStream · error — `next(timeout?)` → the next piece of text or nil at the end; `collect()` → like `chat`; fields `text done result`.
- `client.models()` → list · error — `[{ id, name }]`.
- `client.embed(text | [texts], { model })` → list · error — Vectors (OpenAI-style and Gemini).
- `message: { role: "system" | "user" | "assistant" | "tool", content, toolCalls, toolCallId, name }` — `result.message` can be appended to continue the conversation.

### Tools and agents

A tool that returns an `error` value sends `error: …` back to the model, which can recover.

- `ai.tool(name, description, parameters, run?)` → map — `parameters` is a JSON schema; also a plain map `{ name, description, parameters, run }`.
- `ai.agent({ client, model, instructions, tools, maxSteps = 10, temperature, maxTokens, memory, onStep })` → Agent — Tools need `run(args)`.
- `agent.run(input)` → map · error — Calls the model and the tools until it answers: `{ text, steps, messages, usage, finishReason }`.
- `agent.reset() · agent.history` — With `memory: true` the conversation continues across runs.

```fogu
import ai from "fogu:ai"
const llm = ai.client({ provider: "anthropic" })?          // $ANTHROPIC_API_KEY
print(llm.chat("Say hi in Portuguese, one word")?.text)

const s = llm.stream("Count to 3")?
while true { const t = s.next()?; if t == nil { break }; Fogu.write(t) }
print()

const weather = ai.tool("weather", "Current temperature of a city",
    { type: "object", properties: { city: { type: "string" } }, required: ["city"] },
    (args) => ({ city: args.city, celsius: 21 }))
const agent = ai.agent({ client: llm, instructions: "Use the tools", tools: [weather] })
const answer = agent.run("Is it warm in Lisbon?")?
print(answer.text)
print(answer.steps[0].toolCalls[0].name, answer.steps[0].toolCalls[0].arguments)
```

output (the model's words vary):

```text
Olá
1, 2, 3
Yes — it is 21 °C in Lisbon, a pleasant temperature.
weather {city: Lisbon}
```

### MCP servers and clients

- `ai.mcpServer({ name, version, title, instructions })` → McpServer — A [Model Context Protocol](https://modelcontextprotocol.io) server (protocol 2025-06-18, also 2025-03-26 and 2024-11-05) that Claude Desktop, Claude Code, IDEs and agents can use.
- `server.tool(ai.tool(…)) · server.tool(name, description, schema, run)` → McpServer — A tool; `run(args)` returns a value (sent as text, maps also as `structuredContent`) or an `error` (sent as `isError`). A runtime error in a tool is reported to the client, the server keeps running.
- `server.resource(uri, { name, description, mimeType }, fn() → text) · server.prompt(name, { description, arguments: [{ name, required }] }, fn(args) → text | [{ role, text }])` → McpServer
- `server.serveStdio()` → nil — Serves on stdin/stdout until stdin closes — the transport of local servers. Do not `print` anything else in this mode (use `log` on stderr).
- `server.listen(port, { path = "/mcp", host = "127.0.0.1", token, origins })` → int · error — Streamable HTTP (JSON answers). `token` requires `Authorization: Bearer`; requests from browsers are only accepted from `localhost` or `origins`.
- `server.handle(message)` → map · list · nil — One JSON-RPC message (str or map) → the answer: tests, or another transport.
- `ai.mcpClient({ command, args, env, cwd, stderr } | { url, headers }, { timeout = 30s })` → McpClient · error — Starts a local server (stdio) or connects to a remote one (Streamable HTTP, JSON or SSE answers); fields `serverInfo protocolVersion capabilities instructions closed`.
- `mcp.listTools() · callTool(name, args) · listResources() · readResource(uri) · listPrompts() · getPrompt(name, args) · ping() · close()` — `callTool` → the structured result, else the text; a tool's `isError` is an error value.
- `mcp.tools()` → list — The server's tools as `ai.tool` values: `ai.agent({ client, tools: mcp.tools()? })` lets a model use any MCP server.

```fogu
import ai from "fogu:ai"
import json from "fogu:json"

const mcp = ai.mcpServer({ name: "notes", version: "1.0.0" })
mcp.tool("add", "Adds two numbers", { type: "object", properties: { a: { type: "number" }, b: { type: "number" } } },
    (args) => args.a + args.b)
mcp.resource("notes://today", { name: "Today" }, () => "- ship MCP")

// what a client sends (mcp.serveStdio() or mcp.listen(8000) serve it for real)
const call = (method, params) => mcp.handle({ jsonrpc: "2.0", id: 1, method: method, params: params })
print(call("initialize", { protocolVersion: "2025-06-18" }).result.serverInfo)
print(json.stringify(call("tools/call", { name: "add", arguments: { a: 2, b: 3 } }).result.content))
print(call("resources/read", { uri: "notes://today" }).result.contents[0].text)
```

output:

```text
{name: notes, version: 1.0.0}
[{"text":"5","type":"text"}]
- ship MCP
```

## fogu:ssh

```fogu
import ssh from "fogu:ssh"
```

Commands, file copies and tunnels over the system's OpenSSH client (key or password).

- `ssh.connect("user@host:port" | { host, user, port, key, password, strictHostKey, knownHosts, timeout, options, jump })` → Ssh · error — Checks the connection; `strictHostKey` defaults to `accept-new`; `jump` (another connection or `"user@host"`) goes through a bastion with that connection's key and settings.
- `s.run(command, { input })` → { ok, code, stdout, stderr } · error — A non-zero exit code is not an error.
- `s.upload(local, remote) · s.download(remote, local)` → nil · error — scp, folders included.
- `s.forward(localPort | 0, host, port)` → Tunnel · error — `-L`: a local port that reaches `host:port` through the server.
- `s.reverse(remotePort, host, localPort)` → Tunnel · error — `-R`: a port on the server that reaches a local service.
- `tunnel.localPort · .remotePort · .alive() · .close()`
- `s.mount(remote, local, { readonly, keep, options, tunnel, jump })` → Mount · error — Like sshfs: the remote folder appears as a local folder. Uses the connection's user, port, key (or password, sent on stdin), `knownHosts` and `strict`; `options` are extra `-o` sshfs options. Needs `sshfs` (Linux: `apt install sshfs`; macOS: macFUSE + sshfs; or `$FOGU_SSHFS`). `tunnel`: through an open `s.forward(…)`; `jump`: through a bastion. Unmounted when the program ends — also on Ctrl+C/SIGTERM — unless `keep`. Windows: SSHFS-Win (+ WinFsp), the local side is a drive letter (`"X:"`), only `%USERPROFILE%\.ssh\id_rsa` or a password, no `readonly`.
- `mount.path · .remote · .host · .mounted · .unmount()` — A missing remote folder, a refused key or a busy mount point is an error value.
- `s.shell({ pty, shell = "sh", cwd, env })` → SshShell · error — One persistent remote shell: `sh.run(cmd, { timeout })` → `{ ok, code, stdout, stderr }` keeps the working directory and variables between commands (one command at a time per shell).
- `sh.send(text) · sh.expect(text | regex, { timeout = 30s }) · sh.read({ timeout = 200ms }) · sh.alive() · sh.close()` — Expect-style control of programs that ask questions; with `pty: true` the remote side gets a terminal (sudo prompts…).
- `s.interactive(command?)` → int · error — Hands the user's terminal to the remote shell (`ssh -t`); returns its exit code.
- `FOGU_SSH` — The ssh client to use (another build, a wrapper).
- `s.close()` → nil

```fogu
import ssh from "fogu:ssh"
import fs from "fogu:fs"
const s = ssh.connect("deploy@server.example.com:22", { key: "~/.ssh/id_ed25519" })?
print(s.run("uname -s")?.stdout.trim())
s.upload("dist/app", "/srv/app")?
const db = s.forward(0, "127.0.0.1", 5432)?          // the server's PostgreSQL, locally
print("postgres://app@127.0.0.1:" + str(db.localPort) + "/app")
db.close()
const www = s.mount("/var/www", "./www", { readonly: true })?   // a local folder backed by the server
print(fs.list("./www")?)
www.unmount()?
s.close()
```

output (with an SSH server):

```text
Linux
postgres://app@127.0.0.1:53110/app
[index.html, static]
```

```fogu
import ssh from "fogu:ssh"
const s = ssh.connect("deploy@server.example.com")?
const sh = s.shell({ cwd: "/srv/app", env: { APP_ENV: "prod" } })?
sh.run("git pull -q && export VERSION=$(git rev-parse --short HEAD)")?
print(sh.run("echo $APP_ENV $VERSION; pwd")?.stdout)
const t = s.shell({ pty: true })?
t.send("sudo -k; sudo systemctl restart app\n")
t.expect("password")?
t.send(Fogu.env.SUDO_PASS + "\n")
print(t.expect("\\$ ")?.contains("restart"))
```

output (with an SSH server):

```text
prod 3f9c2a1
/srv/app

true
```

## fogu:shell

```fogu
import shell from "fogu:shell"
```

A small POSIX-style shell: pipes, `&&` `||` `;`, redirections (`> >> 2>&1 &>`), `$VAR`, `$(…)`, quotes, globs and `~`. The usual Linux commands are built in, so scripts behave the same on Windows: `echo cat ls pwd cd mkdir rm cp mv touch grep head tail wc sort uniq cut tr tee basename dirname which env export true false exit sleep seq`. Other commands run as programs.

- `shell.run(command, { cwd, env, input, native = true })` → { ok, code, stdout, stderr } — A non-zero exit is not an error; `native: false` uses system programs only.
- `shell.exec(command, opts?)` → same — Output goes to the terminal as it happens.
- `shell.text(command, opts?)` → str · error — stdout without the final newline; a failure is an error value.
- `shell.lines(command, opts?) · shell.json(command, opts?)` → list · value
- `shell.quote(text)` → str — Safe to interpolate into a command.
- `shell.cd(dir) · shell.cwd()` → nil · str — The shell's own working folder.
- `shell.which(program) · shell.builtins` → str · list

```fogu
import shell from "fogu:shell"
shell.run("mkdir -p data && echo banana > data/fruits.txt && echo apple >> data/fruits.txt")?
print(shell.text("cat data/fruits.txt | sort | head -n 1")?)
print(shell.lines("ls data")?, shell.text("cat data/*.txt | wc -l")?)
const name = "file with spaces.txt"
shell.run(`echo ok > data/${shell.quote(name)}`)?
print(shell.text("echo \"$WHO has $(cat data/fruits.txt | wc -l) fruits\"", { env: { WHO: "Ana" } })?)
const r = shell.run("grep kiwi data/fruits.txt || echo 'no kiwi'")?
print(r.ok, r.stdout.trim())
const [_, err] = shell.text("cat missing.txt")
print(err)
```

output:

```text
apple
[fruits.txt] 2
Ana has 2 fruits
true no kiwi
error: shell.text: exit code 1: cat: missing.txt: No such file or directory
```

## fogu:ffi

```fogu
import ffi from "fogu:ffi"
```

Calls functions of native libraries (C ABI): `.dll`, `.so`, `.dylib`. **Unsafe by nature**: a wrong signature can crash the program. Supported signatures: up to 4 arguments mixing integers and floats, or up to 8 integer/pointer arguments.

- `ffi.open(name)` → Library · error — A path or a name; `"c"` and `"m"` are the C runtime and the math library. The same name works on every platform: without an extension it gets the platform's prefix and suffix (`"native/mylib"` → `native/libmylib.so` on Linux, `native/libmylib.dylib` on macOS, `native/mylib.dll` on Windows), and another platform's extension is mapped to the native one (`"mylib.dll"` on Linux also tries `libmylib.so`). The error lists every file tried.
- `lib.func(symbol, [argTypes], returnType = "void")` → fn · error — A Fogu function that calls the symbol.
- `lib.has(symbol) · lib.name` → bool · str
- `types` — `i8 i16 i32 i64 u8 u16 u32 u64 isize usize bool f64 ptr str buffer`; results also `f32 void`. `str` passes a NUL-terminated copy; a `str` result reads a C string.
- `ffi.alloc(size)` → Buffer — Memory for out-parameters: `text() bytes(start?, len?) write(offset, str | bytes) i32/u32/i64/f64(offset)`, fields `size address`.
- `ffi.types · ffi.suffix` → list · str

```fogu
import ffi from "fogu:ffi"
const libc = ffi.open("c")?
const strlen = libc.func("strlen", ["str"], "usize")?
const atoi = libc.func("atoi", ["str"], "i32")?
print(strlen("hello"), strlen("olá"), atoi("42abc"))
const pow = ffi.open("m")?.func("pow", ["f64", "f64"], "f64")?
print(pow(2.0, 10))
const strcpy = libc.func("strcpy", ["buffer", "str"], "ptr")?
const buf = ffi.alloc(16)
strcpy(buf, "copied")
print(buf.text(), buf.bytes(0, 3))
```

output:

```text
5 4 42
1024.0
copied [99, 111, 112]
```

## fogu:playbook

```fogu
import playbook from "fogu:playbook"
```

Configuration management in the style of Ansible: an **inventory** of hosts and **playbooks** of idempotent tasks, run over SSH (the system's OpenSSH client) or on the local machine (through [fogu:shell](#fogushell), so it also works on Windows). Hosts run in parallel; the output and the `PLAY RECAP` follow Ansible's. From the command line: `fogu play site.yml -i inventory.yml [--check] [-l hosts] [-t tags] [--skip-tags t] [-e key=value] [-f forks] [-v]` (exit code 2 when a host failed or was unreachable).

### Inventory

`vars` for every host; `hosts`: name → `{ host, port, user, key, password, become_password, connection: "local", …vars }`; `groups`: name → list of hosts or `{ hosts, vars }`. Patterns in `hosts:` — `all`, a host, a group, `web:db`, `web:!web2`.

```yaml
hosts:
  web1: { host: 192.0.2.10, user: deploy, key: ~/.ssh/id_ed25519 }
  laptop: { connection: local }
groups:
  web: { hosts: [web1], vars: { port: 8080 } }
```

### Playbooks and tasks

Templates: `{{ expr }}` with variables (`item`, `facts`, `inventory_hostname`, `groups`, registered results), `. []`, `+ - * / % ~`, comparisons, `and or not in`, `is defined`, and the filters `default upper lower trim length join first last replace split int float string bool to_json basename dirname`.

- `play: { name, hosts, vars, become, gather_facts = true, tags, tasks, handlers }` — Facts: `facts.os arch hostname distribution distribution_version pkg_mgr service_mgr user`.
- `task keys: name, when, loop, register, notify, ignore_errors, changed_when, check_mode, become, tags` — `when` is an expression; `loop` sets `item`; `register` keeps the result.
- `shell / command: "cmd" | { cmd, chdir, creates, removes, stdin }` — Skipped in check mode (unless `check_mode: false`); `{ rc, stdout, stderr, stdout_lines }`.
- `copy: { content | src, dest, mode } · template: { src, dest, mode }` — Written only when the content or mode differs; `template` renders `{{ }}`.
- `file: { path, state: directory | absent | touch | file, mode }`
- `lineinfile: { path, line, regexp, state, create, mode }` — Replaces the last line matching `regexp`, or appends.
- `package: { name, state: present | absent, update_cache, use }` — apt, dnf, yum, zypper, apk, pacman (from the facts).
- `service: { name, state: started | stopped | restarted | reloaded, enabled }` — systemd.
- `debug: { msg } | { var } · set_fact: { … } · fail: { msg } · wait_for: { port | path, host, timeout }`

```yaml
- name: Web servers
  hosts: web
  become: true
  vars: { app: shop }
  tasks:
    - name: App folder
      file: { path: "/srv/{{ app }}", state: directory, mode: "0755" }
    - name: Config
      template: { src: templates/app.conf, dest: "/srv/{{ app }}/app.conf" }
      notify: restart nginx
    - name: Packages
      package: { name: [curl, nginx], state: present }
      when: facts.os == 'linux'
  handlers:
    - name: restart nginx
      service: { name: nginx, state: restarted }
```

### From Fogu

- `playbook.run(plays | path, { inventory, vars, check, limit, tags, skipTags, forks, verbose, quiet })` → map · error — `{ ok, failed: [hosts], recap: { host: { ok, changed, unreachable, failed, skipped, ignored } } }`.
- `playbook.inventory(path | map)` → map — The hosts and groups it describes.
- `playbook.render(text, vars)` → str · error — The template engine on its own.

```fogu
import playbook from "fogu:playbook"
const plays = [{
    hosts: "all",
    gather_facts: false,
    vars: { dir: "site", pages: ["index", "about"] },
    tasks: [
        { name: "pages", copy: { content: "<h1>{{ item | upper }}</h1>", dest: "{{ dir }}/{{ item }}.html" }, loop: "{{ pages }}" },
        { debug: { msg: "{{ pages | length }} pages in {{ dir }}" } },
    ],
}]
const first = playbook.run(plays)?
const again = playbook.run(plays, { quiet: true })?
print(first.ok, again.recap.localhost.changed)
print(playbook.render("{{ name | default('anon') | upper }}: {{ n * 3 }}", { n: 7 }))
```

output:

```text
PLAY [all] *************************************************************

TASK [pages] ***********************************************************
changed: [localhost] => (item=index)
changed: [localhost] => (item=about)

TASK [debug] ***********************************************************
ok: [localhost] => 2 pages in site

PLAY RECAP *************************************************************
localhost : ok=2    changed=1    unreachable=0    failed=0    skipped=0    ignored=0
true 0
ANON: 21
```

## Embedding Fogu (C API, .so / .dll)

The runtime builds as a dynamic library that other programs load: `cargo ffi` (= `cargo build --release -p fogu-ffi`) writes `target/release/libfogu_rt.so` (`fogu_rt.dll` on Windows — `cargo ffi --target x86_64-pc-windows-gnu` from Linux — and `libfogu_rt.dylib` on macOS); the header is `ffi/fogu.h`. A runtime runs on its own thread: code evaluated by the host shares its globals, and servers, `fg` tasks and `Fogu.signal` handlers keep running between calls. Values cross as JSON; every call returns `{"ok":true,"value":…}` or `{"ok":false,"error":"…"}`, freed with `fogu_free_string`. Rust programs can use `fogu::embed::Runtime` directly.

- `fogu_new() · fogu_free(rt) · fogu_version() · fogu_free_string(s)`
- `fogu_eval(rt, code)` → char* — Runs code in the global scope → the value of its last expression.
- `fogu_spawn(rt, code) · fogu_run_file(rt, path)` → char* — `spawn`: as a background task (servers, loops), returns at once.
- `fogu_call(rt, name, argsJson) · fogu_get(rt, name) · fogu_set(rt, name, json)` → char* — Global functions and variables.
- `fogu_emit(rt, name, dataJson)` → char* — Fires the program's `Fogu.signal.on(name, fn)` handlers.
- `fogu_on_host(rt, cb, user)` → char* — `void cb(char *name, char *json, void *user)` — called by `Fogu.host.emit(name, data)` on the runtime's thread; it owns both strings (`fogu_free_string`), so asynchronous hosts can read them later.

```fogu
import { dlopen, FFIType, CString, JSCallback } from "bun:ffi";
const { symbols: F } = dlopen("target/release/libfogu_rt.so", {
  fogu_new: { args: [], returns: FFIType.ptr },
  fogu_eval: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.ptr },
  fogu_call: { args: [FFIType.ptr, FFIType.cstring, FFIType.cstring], returns: FFIType.ptr },
  fogu_emit: { args: [FFIType.ptr, FFIType.cstring, FFIType.cstring], returns: FFIType.ptr },
  fogu_on_host: { args: [FFIType.ptr, FFIType.function, FFIType.ptr], returns: FFIType.ptr },
  fogu_free_string: { args: [FFIType.ptr], returns: FFIType.void },
  fogu_free: { args: [FFIType.ptr], returns: FFIType.void },
});
const c = (t: string) => Buffer.from(t + "\\0");
const take = (p: any) => { const r = JSON.parse(new CString(p).toString()); F.fogu_free_string(p); if (!r.ok) throw new Error(r.error); return r.value; };

const rt = F.fogu_new();
take(F.fogu_eval(rt, c("fn greet(name, n) { return { msg: \"hi \" + name, twice: n * 2 } }")));
console.log(take(F.fogu_call(rt, c("greet"), c('["bun", 21]'))));      // { msg: "hi bun", twice: 42 }

const cb = new JSCallback((name, json) => {
  console.log(new CString(name).toString(), new CString(json).toString());   // done {"id":7}
  F.fogu_free_string(name); F.fogu_free_string(json);
}, { args: [FFIType.ptr, FFIType.ptr, FFIType.ptr], returns: FFIType.void, threadsafe: true });
take(F.fogu_on_host(rt, cb.ptr, null));
take(F.fogu_eval(rt, c('Fogu.signal.on("job", fn(d) { Fogu.host.emit("done", d) })')));
take(F.fogu_emit(rt, c("job"), c('{"id": 7}')));
await Bun.sleep(100);
F.fogu_free(rt);
```

## fogu:vm

```fogu
import vm from "fogu:vm"
```

Runs Fogu source code at run time, in the style of Node's `node:vm`. The code runs on the same interpreter, so its functions can be called from the program and vice versa. A new context only sees the global natives plus the keys of its sandbox map (it may still `import` modules); like `node:vm`, it is **not a security boundary**. The value of the last expression is returned; syntax, name, runtime and timeout errors come back as `error` values.

- `vm.runInNewContext(code, sandbox?, options?)` → value · error — Options: `{ filename = " ", timeout }` or a filename. Variables the code declares are copied into the sandbox map.
- `vm.createContext(sandbox?) · vm.isContext(x)` → VmContext · bool — A context keeps its variables between runs: `ctx.sandbox`, `ctx.name` reads a variable, `ctx.run(code, options?)`.
- `vm.runInContext(code, ctx, options?)` → value · error — Changes to the sandbox map are seen by the next run.
- `vm.runInThisContext(code, options?)` → value · error — Sees the program's globals; its declarations persist between calls.
- `vm.Script(code, options?)` → VmScript · error — Parsed once: `runInContext(ctx) runInNewContext(sandbox?) runInThisContext()`, fields `source filename`.
- `vm.compileFunction(code, params = [], { context, filename })` → fn · error — A function whose body is `code`.
- `timeout` — A duration; loops and calls stop once it passes (`script execution timed out after 50ms`). It belongs to the task that runs the code: other `fg` tasks (and their JIT) are not affected.
- `vm.spawn(code, sandbox?, { timeout, filename, semaphore, context }) · script.spawn(sandbox | ctx, opts) · ctx.spawn(code, opts)` → VmTask — Runs in its own `fg` task. `semaphore`: a [sync.Semaphore](#fogusync) limiting how many run at once.
- `task.wait() · task.cancel() · task.done · ok · result · error` → value · error — `cancel()` → true if it was running (`script execution cancelled`).
- `vm.all(tasks)` → list · error — Their values in order; the first error if one fails.
- `ctx.mutex · ctx.busy` → Mutex · bool — A context runs one script at a time (concurrent runs wait); its lock is a real `sync.Mutex` the program can hold. A run may nest in its own context.

```fogu
import vm from "fogu:vm"
print(vm.runInNewContext("x * 2 + y", { x: 20, y: 2 })?)
const ctx = vm.createContext({ count: 0 })
vm.runInContext("count += 1; let msg = `count is ${count}`", ctx)?
print(ctx.sandbox.count, ctx.msg)
const square = vm.Script("n * n")?
print([1, 2, 3].map((n) => square.runInNewContext({ n: n })?))
const add = vm.compileFunction("return a + b", ["a", "b"])?
print(add(2, 3))
const [v, err] = vm.runInNewContext("while true {}", nil, { timeout: 50ms })
print(err)
print(vm.runInNewContext("1 +", nil, "bad.fg"))
```

output:

```text
42
1 count is 1
[1, 4, 9]
5
error: <vm>:1: runtime error: script execution timed out after 50ms
error: bad.fg: line 1:4: syntax error: unexpected expression: Eof
```

Concurrency: each `spawn` is a task; timeouts and cancellations are per task.

```fogu
import vm from "fogu:vm"
import sync from "fogu:sync"
const square = vm.Script("Fogu.sleep(20ms)\nn * n")?
const sem = sync.Semaphore(2)
const tasks = []
for n in 1..6 { tasks.push(square.spawn({ n: n }, { semaphore: sem })) }
print(vm.all(tasks)?)
const t = vm.spawn("while true { Fogu.sleep(5ms) }", nil, { filename: "loop.fg" })
Fogu.sleep(20ms)
print(t.cancel(), t.wait())
const ctx = vm.createContext({ count: 0 })
const inc = vm.Script("{\nlet c = count\nFogu.sleep(2ms)\ncount = c + 1\n}")?
vm.all([0, 1, 2, 3, 4].map((i) => inc.spawn(ctx)))?
print(ctx.sandbox.count)
```

output:

```text
[1, 4, 9, 16, 25]
true error: loop.fg:1: runtime error: script execution cancelled
5
```

## fogu:cli

```fogu
import cli from "fogu:cli"
```

Terminal interaction: prompts (`dialoguer`), colors and terminal control (`console`), spinners and progress bars (`indicatif`), tables (`comfy-table`) and argument parsing (`clap`). Without a terminal on stdin (pipes, CI) the prompts read plain lines, so `echo Ana | fogu app.fg` works; spinners and bars draw on stderr, only on a terminal. Colors are off when stdout is not a terminal or `NO_COLOR` is set.

### Prompts

- `cli.input(prompt, { default, allowEmpty, type: "str" | "int" | "float" })` → str | int | float · error
- `cli.password(prompt, { confirm: "Repeat password", allowEmpty })` → str · error — Hidden input.
- `cli.confirm(prompt, { default })` → bool · error
- `cli.select(prompt, items, { default, fuzzy, index })` → value | nil · error — Items are strings or `{ label, value }`; nil when cancelled with Esc; `fuzzy` filters while typing.
- `cli.multiSelect(prompt, items, { defaults, index })` → list | nil · error — Without a terminal: names or numbers separated by commas.

```fogu
import cli from "fogu:cli"
const name = cli.input("Your name", { default: "Ana" })?
const age = cli.input("Age", { type: "int" })?
const lang = cli.select("Language", ["Fogu", "Rust", { label: "Go lang", value: "go" }])?
if cli.confirm(`Save ${name}?`, { default: true })? { print(name, age, lang) }
```

output (answers piped):

```text
$ printf 'Bia\n30\n3\ny\n' | fogu app.fg
Bia 30 go
```

### Styles, terminal, spinners and tables

- `cli.style(text, ...styles)` → str — `bold dim italic underline strike inverse`, colors `red green yellow blue magenta cyan white black gray`, `brightRed`, backgrounds `bgRed`, 256 colors `"208"` / `"bg208"`.
- `cli.red(text) · green · yellow · blue · magenta · cyan · white · black · gray · bold · dim · italic · underline · strike · inverse` → str
- `cli.colors(on?) · cli.stripAnsi(s) · cli.textWidth(s) · cli.link(text, url)` → bool · str · int · str — `colors(true)` forces colors.
- `cli.isTTY("stdin" | "stdout" | "stderr") · cli.size()` → bool · { columns, rows } | nil
- `cli.clear() · clearLine() · hideCursor() · showCursor() · readKey()` — `readKey` → a character or `Enter Escape Backspace Tab ArrowUp ArrowDown ArrowLeft ArrowRight Home End PageUp PageDown Delete`.
- `cli.spinner(text, { frames })` → Spinner — `text(t) succeed(msg?) fail(msg?) warn(msg?) info(msg?) stop()`.
- `cli.progress(total, { message, prefix, template, chars })` → ProgressBar — `inc(n = 1) set(n) length(n) message(m) println(line) finish(msg?)`, fields `position length elapsed finished`. Templates of `indicatif`.
- `cli.table(rows, { headers, style: "rounded" | "utf8" | "ascii" | "markdown" | "none", align, width })` → str — Rows are lists or maps; numbers are right aligned.

```fogu
import cli from "fogu:cli"
print(cli.red("error"), cli.style("ok", "bold", "green"))
print(cli.table([["ana", 3], ["rui", 15]], { headers: ["name", "jobs"] }))
print(cli.table([{ name: "fogu", stars: 9 }], { style: "markdown" }))
const bar = cli.progress(3)
for i in 0..3 { bar.inc() }
bar.finish()
print(bar.position, cli.stripAnsi(cli.red("x")))
```

output:

```text
error ok
╭──────┬──────╮
│ name ┆ jobs │
╞══════╪══════╡
│ ana  ┆    3 │
│ rui  ┆   15 │
╰──────┴──────╯
| name | stars |
|------|-------|
| fogu |     9 |
3 x
```

### Arguments

- `cli.parse(spec, argv = Fogu.args, { exit = true })` → map · error — `spec`: `{ name, version, about, args: [...], commands: [{ name, about, args }], commandRequired }`. `--help` and `--version` print and exit; a usage error prints and exits with 2, or is an `error` value with `{ exit: false }`.
- `arg: { name, type, short, long, help, positional, required, multiple, default, choices, env }` — Types `str int float bool count`; `multiple` gives a list. A subcommand sets `command` and its values in `args`.

```fogu
import cli from "fogu:cli"
const opts = cli.parse({
    name: "deploy", version: "1.0.0", about: "Deploys the app",
    args: [
        { name: "env", positional: true, required: true, choices: ["dev", "prod"] },
        { name: "force", short: "f", type: "bool", help: "skip the checks" },
        { name: "tag", type: "str", multiple: true },
        { name: "verbose", short: "v", type: "count" },
    ],
    commands: [{ name: "logs", args: [{ name: "lines", short: "n", type: "int", default: 10 }] }],
}, ["prod", "-f", "--tag", "a", "--tag", "b", "-vv", "logs", "-n", "5"])
print(opts)
```

output:

```text
{args: {lines: 5}, command: logs, env: prod, force: true, tag: [a, b], verbose: 2}
```

## fogu:html

```fogu
import html from "fogu:html"
```

Parse HTML and query it with CSS selectors (scraping), render Jinja templates (minijinja), sanitize untrusted HTML (ammonia) and convert between HTML, plain text and Markdown. An invalid selector stops the program; template errors are error values.

### Parsing and selectors

- `html.parse(text) · html.fragment(text)` → HtmlDocument — A full document or a fragment.
- `doc.select(css) · doc.find(css)` → list · HtmlElement | nil — All matches · the first one.
- `doc.title · doc.text · doc.html · doc.root`
- `el.tag · el.text · el.html · el.outerHtml · el.id · el.classes · el.attrs · el.children · el.parent` — `text` has whitespace collapsed.
- `el.attr(name) · el.hasClass(c) · el.select(css) · el.find(css) · el.closest(css) · el.matches(css)`
- `html.select(text, css)` → list — Parse + select in one call.

```fogu
import html from "fogu:html"
import http from "fogu:http"
const doc = html.parse("<ul id='news'><li><a href='/a'>Fogu 1.0</a> <b>new</b></li><li><a href='/b'>JIT notes</a></li></ul>")
for a in doc.select("#news li > a") {
    print(a.text, "→", a.attr("href"))
}
print(doc.find("b").closest("li").find("a").text, doc.find(".missing"))
// scraping a real page: html.parse(http.get(url)?.text)
```

output:

```text
Fogu 1.0 → /a
JIT notes → /b
Fogu 1.0 nil
```

### Templates (Jinja)

- `html.render(source, data)` → str · error — Autoescaped; `{{ x | markdown }}` and `| striptags` are added to the Jinja filters.
- `html.template(source, { autoescape = true })` → Template — `t.render(data)`.
- `html.templates(folder, { autoescape })` → Templates — `render(name, data)`, `renderString(source, data)`, `has(name)`; `extends`/`include`/`import` load from the folder; autoescape by extension.

```fogu
import html from "fogu:html"
const page = html.render("<h1>{{ title }}</h1>{% for p in items %}<li>{{ loop.index }}. {{ p.name | upper }}</li>{% endfor %}", {
    title: "Tom & Jerry",
    items: [{ name: "café" }, { name: "pão" }],
})?
print(page)
const [_, err] = html.render("{% if %}", {})
print(err)
```

output:

```text
<h1>Tom &amp; Jerry</h1><li>1. CAFÉ</li><li>2. PÃO</li>
error: html.render: syntax error: unexpected end of block (in <string>:1)
```

### Sanitizing and conversions

- `html.escape(text) · html.unescape(text)` → str — Entities: named, decimal and hex.
- `html.sanitize(text, { tags, addTags, removeTags, attributes, stripComments })` → str — Removes scripts, event handlers and dangerous URLs.
- `html.text(text) · el.toText()` → str — Plain text with paragraphs, bullets and tab-separated cells.
- `html.toMarkdown(text) · el.toMarkdown()` → str — Headings, emphasis, links, images, lists, code blocks, quotes, tables.
- `html.fromMarkdown(md)` → str — CommonMark + tables, strikethrough, task lists, footnotes.

```fogu
import html from "fogu:html"
print(html.sanitize("<p onclick='steal()'>hi<script>alert(1)</script></p>"))
print(html.toMarkdown("<h2>Notes</h2><p>Some <b>bold</b> and <a href='https://fogu.dev'>a link</a></p><ul><li>one</li><li>two</li></ul>"))
print(html.fromMarkdown("**fogu** is *fun*"))
```

output:

```text
<p>hi</p>
## Notes

Some **bold** and [a link](https://fogu.dev)

- one
- two
<p><strong>fogu</strong> is <em>fun</em></p>
```

## fogu:webview

```fogu
import webview from "fogu:webview"
```

Drives a real browser (Chrome, Edge, Chromium or Brave) through the DevTools protocol: render JavaScript-heavy pages, click, type, run scripts, take screenshots and PDFs. The browser is found in `FOGU_CHROME`, the `path` option, the PATH or the usual install folders. Every command runs on a worker thread; the browser processes are closed when the program ends.

- `webview.launch({ headless = true, path, args, width = 1280, height = 800, userAgent, timeout = 30s })` → Browser · error
- `webview.render(url, { wait, timeout })` → str · error — The HTML after the page's JavaScript ran (shared headless browser).
- `webview.find()` → str | nil — The browser that would be used.
- `browser.open(url?, { wait = "load" | "domcontentloaded" | "networkidle" | "none", timeout })` → Page · error — A new tab.
- `browser.version() · browser.close()`
- `page.goto(url, opts) · reload() · back() · forward()` → nil · error
- `page.title() · url() · content() · text(css?)` → str · error
- `page.eval(js, ...args)` → value · error — An expression (promises are awaited) or a function called with JSON arguments.
- `page.click(css) · type(css, text) · fill(css, value) · press(key)` → nil · error — Real mouse/keyboard events; `press("Enter")`.
- `page.waitFor(css, timeout) · waitForNavigation(opts)` → nil · error
- `page.select(css)` → list · error — `[{ tag, text, html, attrs }]` from the live DOM.
- `page.setContent(html) · setViewport(w, h) · setHeaders(map)` → nil · error
- `page.screenshot({ fullPage, element })` → Image · error — A [fogu:image](#foguimage) image.
- `page.pdf(path?, { format = "A4", landscape, background = true, scale })` → nil | bytes · error
- `page.cookies() · setCookie({ name, value, url | domain })`
- `page.close()` → nil

```fogu
import webview from "fogu:webview"
const browser = webview.launch()?
const page = browser.open("https://example.com")?
print(page.title()?)
print(page.eval("document.querySelectorAll('a').length")?)
page.screenshot({ fullPage: true })?.save("example.png")?
page.pdf("example.pdf")?
browser.close()
```

output (with Chrome installed):

```text
Example Domain
1
```

## fogu:image

```fogu
import image from "fogu:image"
```

Create, transform, draw on and encode images (PNG, JPEG, WebP, GIF, BMP, ICO, TIFF), render SVG (resvg) and HTML, and build animated GIFs. Transformations return a **new** image; drawing methods change the image in place. Colors: `"#rgb"`, `"#rrggbb(aa)"`, CSS names, `"rgb(r,g,b)"`, `"rgba(r,g,b,a)"` or `[r, g, b, a?]`. Encoded data comes back as a [buffer](#values); inputs also take a list of byte ints, base64 or a `data:` URL.

- `image.new(w, h, background?) · image.open(path) · image.decode(buffer | bytes | base64 | dataUrl)` → Image · error
- `image.fromSvg(svg, { width, height, scale, background })` → Image · error — Text uses the system fonts.
- `image.fromHtml(html, { width = 800, height = 600, fullPage = true })` → Image · error — Rendered by a headless Chrome/Edge ([fogu:webview](#foguwebview)).
- `image.info(path)` → { width, height, format } · error — Without decoding the pixels.
- `image.gif(frames, { delay = 100ms, loop = 0, speed }) · image.writeGif(path, frames, opts)` → buffer · nil — Frames: images or `{ image, delay }`.
- `image.frames(path | buffer)` → list · error — `[{ image, delay }]` of an animated GIF.
- `image.color(c)` → list — `[r, g, b, a]`.
- `img.width · img.height` → int
- `img.resize(w, h?, { fit = "fill" | "contain" | "cover", filter }) · thumbnail(w, h?) · crop(x, y, w, h)` → Image — One side keeps the aspect ratio.
- `img.rotate(degrees, { background }) · flipH() · flipV()` → Image — Any angle; 90/180/270 are exact.
- `img.grayscale() · invert() · blur(sigma) · sharpen(sigma, threshold)` → Image
- `img.brightness(d) · contrast(c) · hue(deg) · saturate(f) · opacity(f)` → Image
- `img.fill(c) · setPixel(x, y, c) · getPixel(x, y)` → nil · str
- `img.rect(x, y, w, h, { fill, stroke, width, radius }) · circle(cx, cy, r, opts) · ellipse(cx, cy, rx, ry, opts)` → nil — Anti-aliased (tiny-skia).
- `img.line(x1, y1, x2, y2, { color, width }) · polygon([[x, y], …], { fill, stroke, closed })` → nil
- `img.text(str, x, y, { size = 16, color, font, bold, align }) · measureText(str, opts)` → nil · { width, height } — `font`: a .ttf/.otf path or a family (`"sans-serif"`, `"monospace"`…).
- `img.paste(other, x, y, { opacity }) · drawSvg(svg, x, y, opts)` → nil — Alpha-blended.
- `img.save(path, { quality, background }) · encode(format) · toBase64(format) · toDataUrl(format)` — Format from the extension; JPEG flattens alpha over white. `encode` → buffer.

```fogu
import image from "fogu:image"
const card = image.new(320, 160, "#1e293b")
card.rect(16, 16, 96, 96, { fill: "#f97316", radius: 16 })
card.circle(250, 64, 40, { stroke: "white", width: 4 })
card.text("Fogu", 130, 110, { size: 28, color: "white", bold: true })
card.save("card.png")?
print(card.width, card.height, card.getPixel(30, 30))
const thumb = card.resize(160).grayscale()
print(thumb.width, thumb.height, image.info("card.png")?)
const frames = []
for i in 0..4 {
    const f = image.new(32, 32, "white")
    f.circle(16, 16, 4 + i * 3, { fill: "#f97316" })
    frames.push(f)
}
image.writeGif("pulse.gif", frames, { delay: 80ms })?
print(len(image.frames("pulse.gif")?))
```

output:

```text
320 160 #f97316
160 80 {format: png, height: 160, width: 320}
4
```

## fogu:plot

```fogu
import plot from "fogu:plot"
```

Charts in the style of matplotlib — line, scatter, area, bar (grouped), histogram and pie/donut — drawn as SVG and saved as SVG, PNG, JPEG or WebP. Works with lists and ranges (arrays and data frames from the `datascience` lib: pass `a.toList()` / `df.col(name)`).

- `plot.figure({ title, xlabel, ylabel, width = 800, height = 450, theme = "light" | "dark", grid = true, legend = true, xmin, xmax, ymin, ymax, background })` → Figure
- `fig.line(x?, y, { label, color, width, dash, marker, size, opacity }) · scatter(…) · area(…)` → nil — `line(y)` uses 0, 1, 2… as x.
- `fig.bar(categories, values, { label, color })` → nil — Several `bar` calls are grouped.
- `fig.hist(values, { bins = 10 }) · fig.pie(labels, values, { donut })` → nil
- `fig.hline(y, { color }) · vline(x) · text(x, y, str) · title(t) · xlabel(t) · ylabel(t)` → nil
- `fig.svg() · fig.image({ scale }) · fig.save(path, { scale = 2, quality })` → str · Image · nil
- `plot.line(x, y, options) · plot.bar · plot.scatter · plot.area · plot.hist · plot.pie` → Figure — One-series shortcuts (options also take the figure options).

```fogu
import plot from "fogu:plot"
const fig = plot.figure({ title: "Sales", xlabel: "month", ylabel: "€" })
fig.area([1, 2, 3, 4, 5, 6], [10, 14, 9, 17, 21, 19], { label: "2025" })
fig.line([1, 2, 3, 4, 5, 6], [8, 11, 13, 15, 14, 22], { label: "2026", dash: true, marker: true })
fig.save("sales.png")?
const langs = plot.figure({ title: "Languages", theme: "dark" })
langs.bar(["Fogu", "Rust", "Go"], [42, 30, 25], { label: "2025" })
langs.bar(["Fogu", "Rust", "Go"], [55, 33, 22], { label: "2026" })
langs.save("langs.svg")?
print(plot.pie(["A", "B"], [70, 30], { donut: true }).svg().starts_with("<svg"))
```

output:

```text
true
```

## fogu:docker

```fogu
import docker from "fogu:docker"
```

Control Docker or Podman (containers, images, networks, volumes, compose projects, Swarm services) and write or edit Dockerfiles and compose files. Commands use the `docker`/`podman` CLI (`FOGU_CONTAINER_ENGINE` or `docker.use("podman")`); `docker.remote(ssh)` runs everything on a server through [fogu:ssh](#fogussh). A failing command is an error value with its stderr.

### Containers, images, networks

- `docker.engine · docker.available() · docker.version() · docker.raw(args, { cwd })` — `raw` never fails on exit codes.
- `docker.ps({ all, filter }) · docker.status(container)` → list · map — `status`: running, state, health, exitCode, startedAt, restarts, image.
- `docker.run(image, { name, detach = true, rm, ports, env, volumes, network, restart, labels, workdir, user, entrypoint, command, args })` → str · error — The container id (or the output when `detach: false`).
- `docker.start(name) · stop · restart · kill · pause · unpause · rm(name, { force, volumes })` → nil · error
- `docker.logs(name, { tail, since, timestamps }) · exec(name, command, { user, workdir, env }) · inspect(name) · stats() · cp(from, to)`
- `docker.images() · pull · push · tag · rmi · build(context, { tag, file, args, target, noCache, platform })`
- `docker.networks() · volumes() · createNetwork · removeNetwork · createVolume · removeVolume · prune(what)`

### Compose and Swarm services

- `docker.compose(file?, { project, cwd, envFile })` → ComposeProject — On a remote engine a local compose file is copied first.
- `p.up(services?, { detach = true, build, pull, wait, removeOrphans }) · down({ volumes, images }) · ps() · logs(svc?, { tail })`
- `p.services() · p.status(service)` → list · map — `{ name, state, running, status, health, containers, ports }`, including services not created yet.
- `p.restart(svc?) · stop · start · pull · build · rm · exec(service, command) · config()`
- `docker.services({ filter }) · docker.service(name)` → list · SwarmService — Swarm: `status()`, `tasks()`, `restart()` (rolling), `stop()`, `start(n)`, `scale(n)`, `update({ image, replicas, env })`, `logs()`, `remove()`.
- `docker.readCompose(path) · writeCompose(path, value) · composeString(value)` — Compose files as maps, written with the usual key order.

### Dockerfiles

- `docker.dockerfile(from?, { as, platform }) · parseDockerfile(text) · readDockerfile(path)` → Dockerfile
- `df.from · run(cmd | [cmds]) · copy(src, dest, { from, chown, chmod }) · add · workdir · env(k, v | map) · arg · label · expose(…) · volume(…) · user · cmd · entrypoint · healthcheck(cmd, { interval, timeout, retries }) · comment · raw` → nil — A list of commands becomes one `RUN` joined with `&&`.
- `df.instructions · find(instr) · replace(instr, value) · insert(i, instr, value) · remove(i)` — Edit an existing Dockerfile.
- `df.toString() · df.save(path = "Dockerfile")`

```fogu
import docker from "fogu:docker"
const df = docker.dockerfile("node:20-alpine", { as: "app" })
df.workdir("/app")
df.copy("package*.json", ".")
df.run(["npm ci --omit=dev", "npm cache clean --force"])
df.env({ NODE_ENV: "production" })
df.expose(3000)
df.cmd(["node", "server.js"])
print(df.toString())
docker.writeCompose("compose.yaml", { services: { web: { build: ".", ports: ["3000:3000"], restart: "always" } } })?
print(docker.composeString(docker.readCompose("compose.yaml")?))
```

output:

```text
FROM node:20-alpine AS app
WORKDIR /app
COPY package*.json .
RUN npm ci --omit=dev \
    && npm cache clean --force
ENV NODE_ENV=production
EXPOSE 3000
CMD ["node","server.js"]

services:
  web:
    build: .
    restart: always
    ports:
    - 3000:3000
```

```fogu
import docker from "fogu:docker"
import ssh from "fogu:ssh"
const id = docker.run("nginx:alpine", { name: "web", ports: { "8080": "80" } })?
print(docker.status("web")?.running, docker.exec("web", "nginx -v")?.stderr)
docker.rm("web", { force: true })?
const app = docker.compose("compose.yaml", { project: "shop" })
app.up()?
for s in app.services()? { print(s.name, s.state) }
app.restart("web")?
const prod = docker.remote(ssh.connect("deploy@prod.example.com")?)
print(prod.ps()?)
```

output (with Docker):

```text
true nginx version: nginx/1.27.2
web running
worker running
[{Names: shop-web-1, State: running, …}]
```

## fogu:service

```fogu
import service from "fogu:service"
```

Linux services with systemd: generate unit files and `service.sh` management scripts, install, deploy, control and inspect them — locally or on a server with `{ ssh }`. Commands run `systemctl`/`journalctl` (with `sudo -n` when `sudo: true`).

- `service.define({ name, exec, args, description, user = "root", group, workdir, env, envFile, after = "network.target", type = "simple", restart = "always", restartSec = 5, limitNofile = 65535, wantedBy, extra, sudo, dir, ssh })` → Service — `extra`: `{ Service: { MemoryMax: "1G" } }`; `ssh`: a connection or `"user@host"`.
- `svc.unit() · svc.script()` → str — The `.service` file · a bash script: `create start stop restart status remove logs`.
- `svc.writeUnit(path?) · svc.writeScript(path = "service.sh") · svc.installScript(path)` → nil · error — `installScript` writes it on the (remote) host.
- `svc.install() · svc.uninstall()` → nil · error — Unit file, daemon-reload, enable · stop, disable, remove.
- `svc.deploy(localBinary)` → nil · error — Copies the program to its `exec` path (scp on a server), installs and restarts.
- `svc.start() · stop · restart · reload · enable · disable · isActive() · isEnabled() · logs({ lines, since })`
- `svc.status() · service.status(name, { ssh })` → map · error — `{ active, sub, enabled, running, pid, memory, since, exitStatus, description }`.
- `service.list({ all, ssh }) · service.start(name, { sudo, ssh }) · stop · restart · reload · enable · disable · logs(name, opts) · daemonReload()`
- `service.parseUnit(text) · service.readUnit(name | path, { ssh }) · service.unitString(map)` — Unit files as `{ Unit: {…}, Service: {…}, Install: {…} }`.

```fogu
import service from "fogu:service"
const kv = service.define({
    name: "microaitec-kv",
    exec: "/apps/microaitec/kv/one_kv",
    args: ["--port", "9000"],
    workdir: "/apps/microaitec/kv",
    env: { RUST_LOG: "info" },
})
print(kv.unit())
kv.writeScript("service.sh")?           // ./service.sh create | start | stop | restart | status | remove | logs
print(service.parseUnit(kv.unit()).Service.ExecStart)
```

output:

```text
[Unit]
Description=microaitec-kv
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/apps/microaitec/kv
Environment="RUST_LOG=info"
ExecStart=/apps/microaitec/kv/one_kv --port 9000
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target

/apps/microaitec/kv/one_kv --port 9000
```

```fogu
import service from "fogu:service"
import ssh from "fogu:ssh"
const server = ssh.connect("deploy@prod.example.com")?
const api = service.define({ name: "api", exec: "/opt/api/api", workdir: "/opt/api", sudo: true, ssh: server })
api.deploy("target/release/api")?       // upload, install the unit, restart
print(api.status()?.running)
print(api.logs({ lines: 20 })?)
```

output (with a server):

```text
true
2026-10-15T20:12:01+00:00 prod api[4121]: listening on :8080
```

## fogu:zip

```fogu
import zip from "fogu:zip"
```

ZIP archives (create, extract, list, read, append; AES-256 passwords), tar and tar.gz archives and gzip data. Extraction refuses entries that would escape the target folder. Binary data comes back as a [buffer](#values) (inputs also take a str or a list of byte ints).

- `zip.zip(dest, source | [sources], { level = 6, store, password })` → int · error — Folders are added recursively, named like `zip -r`; returns the number of files.
- `zip.write(dest, { "name": str | bytes, "folder/": nil }, opts)` → nil · error — An archive from memory.
- `zip.add(zipPath, name, content, opts)` → nil · error — Appends to an existing archive.
- `zip.unzip(src, dest = ".", { password, only: [names] })` → list · error — The extracted paths.
- `zip.list(src) · zip.read(src, name, { bytes, password })` → list · str | buffer — `[{ name, size, compressedSize, isDir, encrypted, modified }]`.
- `zip.tar(dest, sources) · zip.untar(src, dest) · zip.listTar(src)` — `.tar.gz`/`.tgz` are gzip-compressed.
- `zip.gzip(str | bytes, { level }) · zip.gunzip(bytes, { bytes })` → buffer · str | buffer
- `zip.gzipFile(src, dest?) · zip.gunzipFile(src, dest?)` → str · error — The written path.

```fogu
import zip from "fogu:zip"
import fs from "fogu:fs"
fs.mkdir("site/css")?
fs.writeText("site/index.html", "<h1>hi</h1>")?
fs.writeText("site/css/app.css", "h1 { color: red }")?
print(zip.zip("site.zip", "site")?)
for e in zip.list("site.zip")? { print(e.name, e.size) }
print(zip.read("site.zip", "site/index.html")?)
print(zip.unzip("site.zip", "copy")?)
zip.zip("secret.zip", "site/index.html", { password: "s3cret" })?
print(zip.read("secret.zip", "index.html", { password: "s3cret" })?)
zip.tar("site.tar.gz", "site")?
print(zip.untar("site.tar.gz", "restored")?)
print(zip.gunzip(zip.gzip("hello hello hello"))?)
```

output:

```text
2
site/ 0
site/css/ 0
site/css/app.css 17
site/index.html 11
<h1>hi</h1>
[copy/site/css/app.css, copy/site/index.html]
<h1>hi</h1>
[restored/site/css/app.css, restored/site/index.html]
hello hello hello
```

## fogu:protobuf

```fogu
import protobuf from "fogu:protobuf"
```

Protocol Buffers without code generation: `.proto` files are compiled at run time in pure Rust (protox — no `protoc`), messages are encoded from and decoded to Fogu maps (prost-reflect). Field names follow the proto3 JSON mapping (camelCase; `protoNames` for the original ones); 64-bit integers are ints; `bytes` fields are base64 text; well-known types (`Timestamp`, `Duration`…) use their JSON forms.

- `protobuf.load(path | [paths], { includes: [folders] })` → ProtoSchema · error — Imports are searched in `includes` (default: the files' folders).
- `protobuf.parse(source, { name, imports: { "file.proto": source } })` → ProtoSchema · error — Google's well-known types are built in.
- `protobuf.loadDescriptor(path | bytes)` → ProtoSchema · error — A FileDescriptorSet (`protoc --descriptor_set_out`, `buf build`).
- `schema.messages · schema.enums · schema.services` → list — Services: `{ name, methods: [{ name, input, output, clientStreaming, serverStreaming }] }`.
- `schema.encode(type, map) · schema.decode(type, bytes, { defaults, protoNames, enumNumbers })` → buffer · map — Unknown fields and wrong types are error values.
- `schema.toJson(type, bytes) · schema.fromJson(type, json) · schema.validate(type, map)` — proto3 JSON.
- `schema.fields(type) · schema.enumValues(enum)` → list · map — `[{ name, jsonName, number, type, repeated, map }]`.
- `schema.descriptor() · schema.saveDescriptor(path)` → buffer · nil

```fogu
import protobuf from "fogu:protobuf"
const schema = protobuf.parse("
syntax = \"proto3\";
package shop;
enum Status { UNKNOWN = 0; PAID = 1; }
message Item { string sku = 1; int32 qty = 2; double price = 3; }
message Order { int64 id = 1; string customer_name = 2; repeated Item items = 3; Status status = 4; }
")?
print(schema.messages)
const bytes = schema.encode("shop.Order", { id: 42, customerName: "Ana", items: [{ sku: "cafe", qty: 2, price: 4.5 }], status: "PAID" })?
print(len(bytes), "bytes")
print(schema.decode("shop.Order", bytes)?)
print(schema.toJson("shop.Order", bytes)?)
const [_, err] = schema.encode("shop.Item", { nope: 1 })
print(err)
```

output:

```text
[shop.Item, shop.Order]
28 bytes
{customerName: Ana, id: 42, items: [{price: 4.5, qty: 2, sku: cafe}], status: PAID}
{"id":"42","customerName":"Ana","items":[{"sku":"cafe","qty":2,"price":4.5}],"status":"PAID"}
error: protobuf: unrecognized field name 'nope' at line 1 column 7
```
