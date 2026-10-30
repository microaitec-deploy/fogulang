# Fogu — Language Specification

**Version:** 0.4 — Draft
**Extension:** `.fg`
**Pronunciation:** "Fôgu"
**Kind:** interpreted, dynamically typed programming language with optional annotations
**Reference implementation:** Rust
**Status:** in development

> The status of each section in the reference implementation is tracked in
> [CHECKLIST.md](../CHECKLIST.md).

---

## 1. Introduction

### 1.1 Overview

Fogu is an independent programming language aimed mainly at automation, infrastructure, systems integration, data processing, tool development and building small services.

The language was designed around three fundamental principles:

1. **Simplicity**
2. **Predictability**
3. **Composition**

Fogu uses a compact syntax, optional semicolons and a standard library geared towards practical tasks.

The language is not compatible with JavaScript, TypeScript, Go or any other existing language. The syntactic similarity with some of them is purely an ergonomic decision.

Fogu has its own runtime, standard library, package manager and toolchain.

---

### 1.2 Goals

Fogu must:

* be easy to learn;
* have a relatively small grammar;
* avoid implicit coercions;
* have value-based error handling;
* allow concurrent programming;
* make automation and scripting easy;
* allow building small services;
* have native access to files, processes and the network;
* allow integration with external systems;
* have a permission system;
* have an integrated toolchain;
* allow distributing applications as standalone executables;
* allow native extensions through Rust and FFI.

---

### 1.3 Non-goals

Fogu does not intend to:

* replace JavaScript for web development;
* be compatible with npm;
* be compatible with Go;
* have class inheritance;
* have `try/catch` exceptions;
* have operator overloading;
* compete directly with Rust or C++ on performance;
* provide every operating system feature through the core language.

---

## 2. Normative terminology

In this document:

* **MUST** indicates a mandatory requirement.
* **MUST NOT** indicates a prohibition.
* **MAY** indicates an optional feature.
* **IMPLEMENTATION** refers to a concrete implementation of the language.
* **RUNTIME** refers to the Fogu execution environment.
* **PROGRAM** refers to executable Fogu code.

---

## 3. Fogu files

Fogu source code uses the extension:

```text
.fg
```

Example:

```text
main.fg
server.fg
device.fg
automation.fg
```

A program can be run with:

```bash
fogu main.fg
```

or:

```bash
fogu run main.fg
```

---

## 4. Lexical structure

### 4.1 Encoding

Source code MUST use UTF-8.

---

### 4.2 Whitespace

Spaces, tabs and line breaks may separate tokens.

Whitespace has no semantic meaning, except for determining the end of a statement.

---

### 4.3 Comments

Line comment:

```fogu
// comment
```

Block comment:

```fogu
/*
   a comment
   over several lines
*/
```

---

## 5. Identifiers

An identifier:

* MUST start with a letter or `_`;
* MAY contain letters, digits and `_`;
* is case sensitive.

Valid examples:

```fogu
name
_name
counter
device_id
HTTPServer
```

Invalid examples:

```fogu
1name
device-id
```

---

## 6. Keywords

The implementation MUST reserve:

```text
let
const
fn
return

if
else
for
in
while
break
continue

match
case
default

struct
interface
embed
new
this

import
export
from

true
false
nil

fg
chan
select

panic
```

Keywords CANNOT be used as identifiers.

---

## 7. Literals

### 7.1 Integers

```fogu
42
1_000_000
0xff
0b1010
```

The default type is:

```text
int
```

`int` is a signed 64-bit integer.

---

### 7.2 Floating point

```fogu
3.14
0.5
1e-9
```

The type is:

```text
float
```

represented as a 64-bit IEEE 754 number.

---

### 7.3 Strings

Strings may use single or double quotes:

```fogu
"Hello"
'Hello'
```

Strings are UTF-8 and immutable.

---

### 7.4 Template strings

```fogu
const name = "Fogu"

const message = `Hello, ${name}`
```

Expressions inside `${...}` are evaluated when the string is created.

---

### 7.5 Booleans

```fogu
true
false
```

---

### 7.6 Nil

```fogu
nil
```

Represents the absence of a value.

---

### 7.7 Durations

```fogu
500ms
2s
5m
1h
```

The type is:

```text
duration
```

Internally, a duration is represented in nanoseconds.

---

## 8. Types

Fogu is dynamically typed.

The fundamental types are:

```text
nil
bool
int
float
str
duration
list
map
tuple
fn
struct
chan
error
```

---

### 8.1 Optional types

A variable may hold `nil`:

```fogu
const name: str | nil = nil
```

---

### 8.2 Lists

```fogu
const values = [1, 2, 3]
```

Lists are ordered and mutable.

Operations:

```fogu
values.push(4)
values.pop()
values.len()
```

---

### 8.3 Maps

Maps have keys of type `str`.

```fogu
const user = {
    name: "John",
    age: 30
}
```

Access:

```fogu
user.name
user["name"]
```

A missing key returns:

```fogu
nil
```

---

### 8.4 Tuples

Multiple values returned by a function are represented internally as tuples.

Example:

```fogu
fn divide(a: int, b: int): (int, error) {
    if b == 0 {
        return 0, error("division by zero")
    }

    return a / b, nil
}
```

Use:

```fogu
const [result, err] = divide(10, 2)
```

Tuples are immutable and may hold different types.

---

## 9. Dynamic typing and annotations

Type annotations are optional.

Without an annotation:

```fogu
fn sum(a, b) {
    return a + b
}
```

With an annotation:

```fogu
fn sum(a: int, b: int): int {
    return a + b
}
```

Annotations:

* are checked at runtime when needed;
* may be checked statically by `fogu check`;
* do not change the dynamic model of the language.

---

## 10. Conversions

There are no implicit coercions between types.

This is invalid:

```fogu
"10" + 2
```

Conversions are explicit:

```fogu
int("10")
str(10)
float(10)
```

The only automatic promotion allowed is:

```text
int → float
```

when needed in an arithmetic operation.

---

## 11. Truthiness

Only the following values are falsy:

```fogu
false
nil
```

The following are truthy:

```fogu
0
""
[]
{}
```

Example:

```fogu
if list {
    print("the list exists")
}
```

---

## 12. Equality

`==` compares values.

For primitive types:

```fogu
1 == 1
"abc" == "abc"
```

Lists and maps are compared structurally.

Structs use identity by default.

A struct may provide:

```fogu
equals(other)
```

to define value equality.

There is no `===` operator.

---

## 13. Variables

Mutable variable:

```fogu
let counter = 0

counter = counter + 1
```

Immutable binding:

```fogu
const name = "Fogu"
```

`const` makes the binding immutable, not necessarily the referenced object.

Example:

```fogu
const list = [1, 2]

list.push(3)
```

is valid.

---

### 13.1 Scope

Fogu uses lexical scope.

Each block creates a new scope:

```fogu
{
    let x = 10
}

print(x) // error
```

Redeclaring a name in the same scope is an error.

Variables cannot be used before their declaration.

There is no `var`.

There is no hoisting.

---

## 14. Operators

Precedence, from highest to lowest:

| Level | Operators |
| ----- | --------- |
| 1     | `()` `[]` `.` `?.` |
| 2     | `!` `-` `<-` |
| 3     | `**` |
| 4     | `*` `/` `%` |
| 5     | `+` `-` |
| 6     | `<` `<=` `>` `>=` |
| 7     | `==` `!=` |
| 8     | `&&` |
| 9     | `\|\|` `??` |
| 10    | `..` |
| 11    | `=` `+=` `-=` `*=` `/=` |

`**` is right-associative.

---

## 15. Optional access

```fogu
const city = user?.address?.city
```

If any intermediate element is `nil`, the result is `nil`.

---

## 16. Coalescing

```fogu
const city = user?.city ?? "unknown"
```

`??` returns the second operand when the first is `nil`.

---

## 17. Ranges

```fogu
0..10
```

represents:

```text
[0, 10)
```

Example:

```fogu
for i in 0..10 {
    print(i)
}
```

The value `10` is not included.

---

## 18. Control structures

### 18.1 if

```fogu
if age >= 18 {
    print("adult")
} else {
    print("minor")
}
```

Parentheses are not used.

---

### 18.2 for

Iterating over a range:

```fogu
for i in 0..10 {
    print(i)
}
```

Iterating over a list:

```fogu
for item in list {
    print(item)
}
```

Iterating over a map:

```fogu
for key, value in map {
    print(key, value)
}
```

---

### 18.3 while

```fogu
while connected {
    process()
}
```

---

### 18.4 break

```fogu
for item in list {
    if item == 10 {
        break
    }
}
```

---

### 18.5 continue

```fogu
for item in list {
    if item == nil {
        continue
    }

    process(item)
}
```

---

## 19. Match

`match` is an expression.

```fogu
const result = match value {
    0 => "zero"
    1 | 2 => "small"
    n if n < 0 => "negative"
    _ => "other"
}
```

A `match` MUST be exhaustive.

The `_` pattern matches any value.

---

## 20. Functions

Declaration:

```fogu
fn sum(a, b) {
    return a + b
}
```

Annotations:

```fogu
fn sum(a: int, b: int): int {
    return a + b
}
```

Default value:

```fogu
fn connect(host = "localhost", port = 80) {
}
```

---

## 21. Anonymous functions

Traditional form:

```fogu
const double = fn(x) {
    return x * 2
}
```

Arrow function:

```fogu
const double = (x) => x * 2
```

Both are equivalent.

Functions are first-class values and support closures.

---

## 22. Variadic functions

```fogu
fn log(prefix, ...args) {
    print(prefix, args)
}
```

---

## 23. Structs

Structs represent programmer-defined types.

```fogu
struct Point {
    x: float
    y: float

    new(x, y) {
        this.x = x
        this.y = y
    }

    norm() {
        return (this.x ** 2 + this.y ** 2) ** 0.5
    }
}
```

Instantiation:

```fogu
const p = new Point(3, 4)

print(p.norm())
```

Result:

```text
5
```

---

## 24. Composition

Fogu has no class inheritance.

Reuse is achieved through composition.

```fogu
struct Logger {
    log(msg) {
        print(msg)
    }
}

struct Service {
    embed Logger

    name = "api"
}
```

The methods of `Logger` become available on `Service`.

---

## 25. Interfaces

Interfaces are structural.

```fogu
interface Closable {
    close(): error
}
```

A type satisfies an interface when it has the required methods.

No explicit implementation declaration is needed.

---

## 26. Visibility

Members starting with `_` are private to the module.

Example:

```fogu
struct User {
    name
    _token
}
```

`_token` cannot be used outside the module where it was declared.

---

## 27. Generics

Fogu supports generic parameters.

```fogu
fn first<T>(items: list<T>): T | nil {
    if items.len() == 0 {
        return nil
    }

    return items[0]
}
```

Structs may also be generic:

```fogu
struct Stack<T> {
    items: list<T> = []

    push(value: T) {
        this.items.push(value)
    }
}
```

Instantiation:

```fogu
const stack = new Stack<int>()
```

Constraints use interfaces:

```fogu
fn sort<T: Comparable>(items: list<T>) {
}
```

Generics are checked statically and are not a runtime reflection mechanism.

---

## 28. Error handling

Fogu has no traditional exceptions.

There is no:

```text
try
catch
finally
throw
```

Errors are values.

Example:

```fogu
fn divide(a: int, b: int): (int, error) {
    if b == 0 {
        return 0, error("division by zero")
    }

    return a / b, nil
}
```

Use:

```fogu
const [result, err] = divide(10, 0)

if err != nil {
    print(err.message)
    return
}
```

---

## 29. The error type

An `error` has:

```text
message: str
cause: error | nil
trace: list<str>
```

Chaining:

```fogu
error("failed to open the file", cause)
```

---

## 30. The `?` propagation operator

The `?` operator propagates an error to the caller.

Example:

```fogu
fn load(path): (map | nil, error) {
    const data = Fogu.file(path).json()?

    return data, nil
}
```

The `?` operator can only be used in a function whose return type contains `error`.

The implementation must semantically transform:

```fogu
const x = operation()?
```

into behaviour equivalent to:

```fogu
const [x, err] = operation()

if err != nil {
    return nil, err
}
```

keeping the function's return type.

---

## 31. Panic

`panic` represents an execution failure that the current function cannot recover from.

```fogu
panic("impossible state")
```

It should only be used for:

* invariant violations;
* invalid indices;
* overflow;
* internal failures;
* impossible assertions.

A panic in a task should not terminate other tasks by default.

The mode:

```bash
fogu run app.fg --strict
```

may configure the runtime to terminate the process on panic.

---

## 32. Concurrency

Fogu's concurrency is based on:

```text
fg
chan
select
```

---

## 33. Tasks

A task is started with `fg`.

```fogu
fg fn() {
    print("running concurrently")
}()
```

or:

```fogu
fg () => {
    process()
}()
```

`fg` schedules the function for concurrent execution and returns immediately.

A task is not equivalent to an operating system thread.

---

## 34. Channels

Creation:

```fogu
const c = chan<int>(buffer: 1)
```

Sending:

```fogu
c <- 42
```

Receiving:

```fogu
const value = <-c
```

Blocking on a channel operation blocks only the task, not the operating system thread.

---

## 35. Transfer model

Values sent through channels are transferred to the receiving task.

After:

```fogu
c <- data
```

the sending task should not use `data` again when the value is considered non-shareable.

Immutable values may be shared safely.

Shared mutable values must use synchronization primitives.

---

## 36. Select

```fogu
select {
    case value = <-c:
        print(value)

    case c2 <- value:
        print("sent")

    case <-timeout(1s):
        print("timed out")

    default:
        print("no operation ready")
}
```

`select` waits for the first available operation.

---

## 37. Synchronization

The module:

```text
fogu:sync
```

provides:

```text
Mutex
Atomic
WaitGroup
Once
```

Example:

```fogu
const mutex = sync.Mutex()

mutex.lock()
counter = counter + 1
mutex.unlock()
```

---

## 38. Modules

Relative import:

```fogu
import { sum } from "./util.fg"
```

Standard module import:

```fogu
import json from "fogu:json"
```

Package import:

```fogu
import http from "http-x"
```

Export:

```fogu
export fn sum(a, b) {
    return a + b
}
```

---

## 39. Module evaluation

Each module is evaluated only once per process.

The result of the evaluation is stored in the module cache.

Cyclic dependencies are a load error.

---

## 40. Packages

A Fogu project uses:

```text
fogu.json
```

Example:

```json
{
    "name": "my-app",
    "version": "0.1.0",
    "entry": "main.fg",
    "dependencies": {
        "http-x": "^1.2.0"
    }
}
```

The file:

```text
fogu.lock
```

pins the exact versions and their checksums.

---

## 41. Dependency security

Published packages:

* MUST be immutable;
* MUST have a checksum;
* MUST be signed;
* MUST NOT run install scripts;
* MUST support vulnerability auditing.

The command:

```bash
fogu audit
```

checks known vulnerabilities and licenses.

---

## 42. Permissions

Fogu programs start without external permissions.

Privileged operations require explicit authorization.

Examples:

```bash
--allow-net
--allow-fs=/data
--allow-run
--allow-ffi
--allow-device
```

Permissions may be restricted per resource.

Example:

```bash
--allow-net=broker.local:1883
```

---

## 43. Standard library

The standard library is split into two categories:

#### Runtime

```text
Fogu.*
```

#### Modules

```text
fogu:*
```

`Fogu.*` contains fundamental primitives provided directly by the runtime.

---

## 44. Fundamental API

The minimal API includes:

```text
print(...)
error(message)
panic(message)

len(value)
int(value)
float(value)
str(value)
typeof(value)
```

---

## 45. Fogu runtime

The `Fogu` API provides:

```text
Fogu.file()
Fogu.serve()
Fogu.reply()
Fogu.env
Fogu.args
Fogu.sleep()
Fogu.sql()
Fogu.hash()
Fogu.exit()
Fogu.run()
Fogu.glob()
Fogu.watch()
```

---

## 46. File system

The module:

```text
fogu:fs
```

provides operations for:

* reading;
* writing;
* creating;
* removing;
* copying;
* moving;
* directories;
* permissions;
* watching for changes.

---

## 47. Network

The module:

```text
fogu:net
```

provides:

```text
TCP
UDP
TLS
```

Example:

```fogu
import net from "fogu:net"

const server = net.tcp.listen(":9000")?

for conn in server {
    fg process(conn)
}
```

---

## 48. HTTP

The module:

```text
fogu:http
```

provides an HTTP client and server.

Example:

```fogu
import http from "fogu:http"

http.serve({
    port: 3000,

    fetch(req) {
        if req.path == "/ping" {
            return http.text("pong")
        }

        return http.text(
            "not found",
            { status: 404 }
        )
    }
})
```

HTTP/1.1 and HTTP/2 are supported by the reference implementation.

---

## 49. JSON

```fogu
import json from "fogu:json"

const data = json.parse(text)?
const text = json.stringify(data)
```

JSON is not a global feature.

---

## 50. Other standard modules

The official distribution MUST progressively provide:

```text
fogu:math
fogu:json
fogu:test
fogu:sync
fogu:context
fogu:fs
fogu:path
fogu:time
fogu:process
fogu:crypto
fogu:ffi

fogu:yaml
fogu:toml
fogu:csv
fogu:regex
fogu:log

fogu:net
fogu:http
fogu:websocket

fogu:mqtt
fogu:nats
fogu:redis
fogu:amqp
fogu:kafka

fogu:dns
fogu:ssh
fogu:smtp

fogu:cron

fogu:serial
fogu:modbus
```

Adding a module does not change the fundamental specification of the language.

---

## 51. FFI

Fogu can call native libraries through:

```text
fogu:ffi
```

The operation requires:

```bash
--allow-ffi
```

Example:

```fogu
import ffi from "fogu:ffi"

const [lib, err] = ffi.open("libm.so.6")?

const cos = lib.fn(
    "cos",
    {
        args: ["f64"],
        ret: "f64"
    }
)

print(cos(0))
```

Native extensions can be written in Rust.

The extension ABI follows semantic versioning.

---

## 52. Processes

Fogu can run external processes through:

```fogu
Fogu.run()
```

Example:

```fogu
const [result, err] = Fogu.run(
    "docker",
    ["ps"]
)?
```

Running processes requires:

```bash
--allow-run
```

---

## 53. Cron

The module:

```text
fogu:cron
```

allows scheduling.

Example:

```fogu
import cron from "fogu:cron"

cron.every(2m, () => {
    update()
})
```

It also supports cron expressions:

```fogu
cron.schedule("*/5 * * * *", () => {
    check()
})
```

---

## 54. Execution model

The reference implementation uses:

```text
Source
  ↓
Lexer
  ↓
Parser
  ↓
AST
  ↓
Resolver
  ↓
Bytecode
  ↓
VM
```

The AST does not need to stay in memory for the whole execution.

---

## 55. Virtual machine

The VM runs Fogu bytecode.

The VM MUST provide:

* function execution;
* closures;
* control flow;
* arithmetic operations;
* data structures;
* native calls;
* error handling;
* task support.

---

## 56. Memory management

The reference implementation uses garbage collection.

The language specification does not expose:

```text
Rc
Arc
Box
GC
```

to the programmer.

These mechanisms are internal details of the implementation.

The reference implementation may use:

* reference counting;
* tracing GC;
* arenas;
* specialized structures;

as long as it preserves the semantics defined by the language.

---

## 57. Scheduler

The runtime uses an M:N scheduler.

Fogu tasks may be distributed over several operating system threads.

The reference implementation may use:

```text
work stealing
thread pool
event loop
```

Blocking I/O operations should release the scheduler thread whenever technically possible.

The number of threads can be configured through:

```text
FOGU_THREADS
```

---

## 58. Diagnostics

Errors should show, whenever possible:

```text
file
line
column
source code
description
suggestion
```

Example:

```text
error[E0012]: undefined variable

  main.fg:12:17

  12 | print(user.name)
             ^^^^

help: declare 'user' before using it
```

---

## 59. CLI

The main tool is called:

```text
fogu
```

Commands:

```text
fogu <file>
fogu run <file>
fogu repl

fogu init
fogu add <package>
fogu remove <package>
fogu install

fogu test
fogu check
fogu fmt

fogu build
fogu build --compile
fogu build --target wasm

fogu publish
fogu audit
fogu doc
```

---

## 60. REPL

```bash
fogu repl
```

allows evaluating expressions interactively.

Example:

```text
> 1 + 2
3

> const name = "Fogu"
> print(name)
Fogu
```

---

## 61. Formatting

The official implementation provides:

```bash
fogu fmt
```

The formatter uses a single style.

The project configuration MUST NOT allow arbitrary changes that produce incompatible styles.

---

## 62. Static checking

```bash
fogu check
```

may detect:

* syntax errors;
* unused variables;
* undeclared variables;
* mismatches with annotated types;
* invalid calls;
* unsatisfied interfaces;
* non-exhaustive `match`;
* invalid imports;
* incorrect use of generics.

---

## 63. Tests

Tests use files named:

```text
*.test.fg
```

Example:

```fogu
import { test, expect } from "fogu:test"
import { sum } from "./util.fg"

test("adds two integers", () => {
    expect(sum(1, 2)).toBe(3)
})
```

Running:

```bash
fogu test
```

---

## 64. Build

The command:

```bash
fogu build --compile
```

generates a standalone executable.

The executable MUST include:

* the bytecode;
* the runtime;
* the required resources;
* the compatible Fogu dependencies.

External operating system dependencies may still be required when explicitly used, for example FFI libraries.

---

## 65. WebAssembly

The target:

```bash
fogu build --target wasm
```

is defined as a platform with restricted capabilities.

The specification splits the APIs into:

```text
Core
Host
```

The Core layer includes:

* types;
* strings;
* lists;
* maps;
* functions;
* math;
* logic;
* basic execution.

Access to:

* the filesystem;
* the network;
* processes;
* devices;

depends on the WASM host environment.

The implementation must not assume that operating system APIs are available in WASM.

---

## 66. Manifest

Complete example:

```json
{
    "name": "gateway",
    "version": "0.1.0",
    "entry": "main.fg",

    "dependencies": {
        "mqtt-client": "^1.2.0"
    },

    "permissions": {
        "net": [
            "broker.local:1883"
        ],
        "fs": [
            "/data"
        ]
    }
}
```

---

## 67. Security

Fogu follows the principle of least privilege.

By default, a program MUST NOT have access to:

* the network;
* the filesystem outside the allowed context;
* external processes;
* devices;
* FFI.

The programmer or operator must grant permissions explicitly.

---

## 68. Concurrency and safety

The runtime MUST prevent data races.

Mutable structures cannot be shared arbitrarily between tasks.

The programmer must use:

```text
chan
Mutex
Atomic
```

or equivalent mechanisms provided by the runtime.

The runtime may reject or interrupt operations that violate memory safety rules.

---

## 69. Device library

Hardware features are provided by specialized modules:

```text
fogu:serial
fogu:modbus
```

Access requires:

```bash
--allow-device
```

Device access MUST NOT be a mandatory feature of the language core.

---

## 70. Conformance

A Fogu implementation is considered compatible when it passes the official conformance suite.

The suite lives in:

```text
spec/tests/
```

Each test may specify:

```text
source
expected output
expected error
```

The suite MUST cover:

* the lexer;
* the parser;
* operators;
* types;
* functions;
* structs;
* interfaces;
* generics;
* modules;
* errors;
* concurrency;
* channels;
* permissions.

---

## 71. Fuzzing

The reference implementation should use fuzzing for:

```text
lexer
parser
bytecode loader
VM
```

Recommended tools:

```text
cargo-fuzz
```

---

## 72. Architecture of the Rust implementation

Workspace:

```text
fogu/
├── crates/
│   ├── fogu-lexer/
│   ├── fogu-parser/
│   ├── fogu-ast/
│   ├── fogu-resolver/
│   ├── fogu-types/
│   ├── fogu-compiler/
│   ├── fogu-bytecode/
│   ├── fogu-vm/
│   ├── fogu-runtime/
│   ├── fogu-std/
│   ├── fogu-ffi/
│   ├── fogu-pm/
│   ├── fogu-fmt/
│   └── fogu-diagnostics/
│
├── cli/
├── spec/
├── tests/
├── docs/
└── examples/
```

---

## 73. Dependencies of the reference implementation

The implementation may use:

| Component   | Technology          |
| ----------- | ------------------- |
| Lexer       | logos               |
| Parser      | hand-written parser |
| Diagnostics | ariadne             |
| Runtime     | tokio               |
| HTTP        | hyper               |
| TLS         | rustls              |
| WebSocket   | tokio-tungstenite   |
| MQTT        | own client (3.1.1, 5) |
| NATS        | async-nats          |
| Redis       | redis               |
| AMQP        | own client (0-9-1)  |
| Kafka       | rdkafka             |
| SSH         | russh               |
| SQLite      | rusqlite            |
| Serial      | serialport          |
| Modbus      | tokio-modbus        |
| FFI         | libloading / libffi |
| CLI         | clap                |
| Serialization | serde             |

These dependencies are implementation details and are NOT part of the Fogu language.

---

## 74. Non-functional requirements

The reference implementation should aim for:

#### Platforms

```text
Linux
macOS
Windows
```

Priority architectures:

```text
x86_64
aarch64
```

#### Performance

Initial goal:

```text
startup of an empty program < 10 ms
```

The goal may be revised before version 1.0.

The executable size should be optimized, but it is not an absolute requirement of the language.

---

## 75. Versioning

The language follows Semantic Versioning after version 1.0.

Before 1.0:

* incompatible changes are allowed;
* changes must be documented;
* significant changes must have an FEP proposal.

---

## 76. Fogu Enhancement Proposal

Relevant changes must follow the FEP process.

An FEP must contain:

```text
Title
Problem
Motivation
Proposal
Syntax
Semantics
Alternatives considered
Impact on the implementation
Impact on compatibility
Migration plan
```

---

## 77. Roadmap

### 0.1 — Basic language

* lexer;
* parser;
* AST;
* `let`;
* `const`;
* functions;
* `if`;
* `for`;
* `while`;
* lists;
* maps;
* REPL;
* interpreter.

### 0.2 — Types and structures

* structs;
* methods;
* interfaces;
* anonymous functions;
* closures;
* generics;
* tuples;
* multiple returns;
* the `?` operator;
* modules.

### 0.3 — VM

* bytecode;
* VM;
* closures in bytecode;
* diagnostics;
* `fogu fmt`;
* `fogu check`.

### 0.4 — Runtime

* `fg`;
* `chan`;
* `select`;
* scheduler;
* `fogu:sync`;
* filesystem;
* processes;
* TCP;
* UDP;
* HTTP.

### 0.5 — Toolchain

* package manager;
* `fogu test`;
* lockfile;
* permissions;
* `fogu audit`;
* automatic documentation.

### 0.6 — Extensibility

* FFI;
* Rust extensions;
* native API;
* versioned ABI.

### 0.7 — Ecosystem

* official registry;
* publishing;
* signing;
* private dependencies;
* auditing.

### 0.8 — Messaging

* MQTT;
* NATS;
* Redis;
* WebSocket;
* AMQP.

### 0.9 — Automation

* cron;
* SSH;
* DNS;
* SMTP;
* serial;
* Modbus.

### 1.0 — Stability

* stable specification;
* stable VM;
* stable runtime;
* stable package manager;
* stable permissions;
* conformance suite;
* complete documentation;
* standalone build;
* WASM.

---

## 78. Design principles

Fogu must follow these principles:

#### 1. Explicit

The language must not hide potentially dangerous operations.

#### 2. Simple

Features should only be added when they solve real problems.

#### 3. Safe

External operations must require explicit permissions.

#### 4. Composable

Composition should be preferred over inheritance.

#### 5. Predictable

A program's behaviour must be easy to understand from the code.

#### 6. Practical

The standard library must solve common automation problems.

#### 7. Independent

The language does not depend on the JavaScript, Python or Go ecosystems.

#### 8. Extensible

Integration with Rust, C and external systems must be possible without changing the language core.

---

## 79. Complete example

A simple Fogu program:

```fogu
import http from "fogu:http"
import cron from "fogu:cron"

const port = 3000

fn health() {
    return {
        status: "ok",
        service: "gateway"
    }
}

http.serve({
    port: port,

    fetch(req) {
        if req.path == "/health" {
            return http.json(health())
        }

        return http.text(
            "not found",
            { status: 404 }
        )
    }
})

cron.every(5m, () => {
    print("check executed")
})
```

---

## 80. Status of the specification

This specification is a **Draft 0.4**.

The areas considered provisional are:

* internal details of the VM;
* the GC implementation;
* the ABI of native extensions;
* WASM integration;
* the final format of the registry;
* the final granularity of permissions;
* specific APIs of some standard library modules.

The fundamental syntax and semantics must be stabilized before development of version 1.0 starts.

---

## 81. Definition of 1.0

Fogu 1.0 will be considered stable when:

1. the grammar is frozen;
2. the semantics of the fundamental operations are defined;
3. the type system is defined;
4. the error model is defined;
5. the concurrency model is defined;
6. the module system is stable;
7. the package manager is stable;
8. the permission model is stable;
9. there is a working reference implementation;
10. there is a conformance suite;
11. Fogu programs can be distributed reproducibly;
12. future incompatible changes require a new major version.

**Fogu 1.0 does not mean the internal implementation is frozen.**

The VM, GC, scheduler and optimizations may keep evolving as long as they preserve the language semantics.

---

## 82. Example project layout

```text
my-project/
├── fogu.json
├── fogu.lock
├── main.fg
├── src/
│   ├── server.fg
│   ├── config.fg
│   └── worker.fg
├── tests/
│   └── server.test.fg
└── README.md
```

Running:

```bash
fogu install
fogu check
fogu test
fogu run main.fg
```

Build:

```bash
fogu build --compile
```

Result:

```text
dist/
└── my-project
```

---

## 83. Automation example

Fogu can be used for an infrastructure gateway:

```fogu
import mqtt from "fogu:mqtt"
import redis from "fogu:redis"
import cron from "fogu:cron"

fn main(): error {
    const broker = mqtt.connect(
        "mqtt://broker.local:1883",
        { id: "gateway" }
    )?

    const db = redis.connect(
        "redis://localhost:6379"
    )?

    fg fn() {
        for msg in broker.subscribe(
            "sensors/+/temperature"
        ) {
            db.set(
                "latest:" + msg.topic,
                msg.payload
            )
        }
    }()

    cron.every(5m, fn() {
        db.publish("health", "ok")
    })

    return nil
}
```

This kind of application represents the main use case of the language:

```text
Fogu
  ↓
Automation
  ↓
Systems
  ├── HTTP
  ├── MQTT
  ├── NATS
  ├── Redis
  ├── TCP
  ├── processes
  ├── files
  ├── devices
  └── services
```

---

## 84. Architecture summary

Fogu's official architecture is:

```text
                 ┌──────────────────┐
                 │    .fg code      │
                 └────────┬─────────┘
                          │
                     Lexer / Parser
                          │
                          ▼
                       AST
                          │
                    Resolver / Check
                          │
                          ▼
                       Bytecode
                          │
                          ▼
                    ┌─────────────┐
                    │     VM      │
                    └──────┬──────┘
                           │
             ┌─────────────┼──────────────┐
             │             │              │
             ▼             ▼              ▼
          Runtime       Scheduler       GC
             │             │
             └──────┬──────┘
                    │
                    ▼
              Standard Library
                    │
       ┌────────────┼─────────────┐
       ▼            ▼             ▼
      HTTP        MQTT           NATS
       │            │             │
       └────────────┼─────────────┘
                    ▼
               External systems
```

The Rust implementation is only the **reference implementation**. The specification defines the behaviour of the language independently of the technology used to implement the runtime.
