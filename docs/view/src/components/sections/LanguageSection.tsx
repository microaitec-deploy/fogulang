import React from 'react';
import { DocCodeBlock } from '../DocCodeBlock';

interface Props {
  onRun: (code: string) => void;
}

export const LanguageSection: React.FC<Props> = ({ onRun }) => {
  return (
    <>
      {/* ================= VARIABLES AND TYPES ================= */}
      <section id="variables" data-group="Language" data-title="Variables and types" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Variables and types</h2>
        <p>
          <code>let</code> creates a mutable variable; <code>const</code> an immutable binding (the referenced object may
          still change). Scope is lexical, each <code>{`{ }`}</code> block creates a new scope and there is <strong>no hoisting</strong>.
          Declaring the same name twice in the same scope is an error.
        </p>
        <DocCodeBlock
          onRun={onRun}
          code={`let counter = 0
counter += 1

const list = [1, 2]
list.push(3)               // fine: const does not freeze the object

print(typeof(42))          // int     — 64-bit signed: 1_000_000, 0xff, 0b1010
print(typeof(3.14))        // float   — IEEE 754: 1e-9
print(typeof("text"))      // str     — "..." or '...', UTF-8, immutable
print(typeof(true))        // bool
print(typeof(nil))         // nil
print(typeof(500ms))       // duration
print(typeof([1, 2]))      // list
print(typeof({a: 1}))      // map
print(typeof(bytes("hi")))  // buffer  — mutable bytes: b[0], b.hex(), b.readU16(0)`}
        />
        <h3 className="text-lg font-semibold">Conversions and truthiness</h3>
        <ul>
          <li>There are no implicit coercions: <code>"10" + 2</code> is an error. Use <code>int("10")</code>, <code>float(10)</code>, <code>str(10)</code>.</li>
          <li>The only automatic promotion is <code>int → float</code> in arithmetic: <code>1 + 2.5 == 3.5</code>.</li>
          <li>Only <code>false</code> and <code>nil</code> are falsy; <code>0</code>, <code>""</code>, <code>[]</code> and <code>{`{}`}</code> are truthy.</li>
          <li><code>==</code> compares lists, maps and buffers by structure; structs by identity (or with their <code>equals</code> method).</li>
        </ul>
      </section>

      {/* ================= TYPE ANNOTATIONS ================= */}
      <section id="types" data-group="Language" data-title="Type annotations" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Type annotations</h2>
        <p>
          Annotations are optional. They are <strong>checked at runtime</strong> (parameters on entry, return values on
          exit, <code>let</code>/<code>const</code> on assignment) and unknown type names are rejected before the program runs.
          Annotated numeric functions are still compiled by the JIT.
        </p>
        <DocCodeBlock
          onRun={onRun}
          code={`fn add(a: int, b: int): int { return a + b }
fn half(x: float): float { return x / 2.0 }     // an int is accepted as a float
print(add(2, 3), half(3))                      // 5 1.5

fn find(xs: list<int>, v: int): int? {          // int? = int | nil
    for i, x in xs { if x == v { return i } }
    return nil
}

fn parse_age(s: str): (int, error) {            // multiple return values
    const n = int(s)
    if n < 0 { return 0, error("negative age") }
    return n, nil
}

let label: str | nil = nil                      // union type
label = "ready"`}
        />
        <p>
          Built-in type names: <code>int float str bool nil duration list map buffer tuple fn chan error any</code>, plus struct
          and interface names. <code>error</code> also accepts <code>nil</code>. Generic arguments such as <code>list&lt;int&gt;</code>
          document the intent; only the outer kind is checked.
        </p>
        <DocCodeBlock
          lang="text"
          code={`add(1, "2")

error[E0207]: type mismatch: parameter 'b' expects int, got str
  --> main.fg:5
   = help: the value does not match the type annotation`}
        />
      </section>

      {/* ================= STRINGS AND TEMPLATES ================= */}
      <section id="strings" data-group="Language" data-title="Strings and templates" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Strings and template strings</h2>
        <p>Template strings use backticks and interpolate any expression with <code>{`\${...}`}</code>. They may span several lines.</p>
        <DocCodeBlock
          onRun={onRun}
          code={`const name = "Ana"
const items = [3, 4]
print(\`Hello, \${name}! You have \${len(items)} items: \${items}\`)
print(\`sum = \${items.reduce((a, b) => a + b, 0)}\`)
print(\`escapes: \\\${literal} and \\\`backtick\\\`\`)

print("a,b,c".split(","))            // [a, b, c]
print("  fogu  ".trim().upper())      // FOGU
print("fogu".replace("u", "o"))       // fogo
print("olé"[0], len("olé"))           // o 3`}
        />
        <p><code>str</code> methods: <code>split upper lower trim replace starts_with ends_with repeat lines chars slice</code>.</p>
      </section>

      {/* ================= DURATIONS ================= */}
      <section id="durations" data-group="Language" data-title="Durations" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Durations</h2>
        <p>
          Literals with a unit (<code>ns us ms s m h d</code>, decimals allowed) create <code>duration</code> values, stored in
          nanoseconds. They are accepted by <code>time.delay</code>, <code>time.sleep</code>, <code>cron.every</code> and <code>timeout</code>.
        </p>
        <DocCodeBlock
          onRun={onRun}
          code={`print(1s + 500ms)          // 1500ms
print(2m - 30s, 2s * 3)    // 90s 6s
print(1m / 2s)             // 30.0   (duration / duration = float)
print(1m > 59s)            // true
print(1.5s.ms, 90s.minutes) // 1500 1.5

const d = 250ms
print(\`waiting \${d}\`)      // waiting 250ms`}
        />
        <p>Fields: <code>ns</code>, <code>us</code>, <code>ms</code> (int) and <code>seconds</code>, <code>minutes</code>, <code>hours</code> (float).</p>
      </section>

      {/* ================= OPERATORS ================= */}
      <section id="operators" data-group="Language" data-title="Operators" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Operators</h2>
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Precedence (high → low)</th>
                <th className="py-2">Meaning</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr><td className="py-2 pr-4"><code>()</code> <code>[]</code> <code>.</code> <code>?.</code></td><td>call, index, field, optional access</td></tr>
              <tr><td className="py-2 pr-4"><code>!</code> <code>-</code> <code>&lt;-</code></td><td>negation, unary minus, channel receive</td></tr>
              <tr><td className="py-2 pr-4"><code>**</code></td><td>power (right-associative)</td></tr>
              <tr><td className="py-2 pr-4"><code>*</code> <code>/</code> <code>%</code> · <code>+</code> <code>-</code></td><td>checked arithmetic (overflow and division by zero are errors)</td></tr>
              <tr><td className="py-2 pr-4"><code>&lt;</code> <code>&lt;=</code> <code>&gt;</code> <code>&gt;=</code> · <code>==</code> <code>!=</code></td><td>comparison</td></tr>
              <tr><td className="py-2 pr-4"><code>&amp;&amp;</code> · <code>||</code> <code>??</code></td><td>logical; <code>a ?? b</code> returns <code>b</code> if <code>a</code> is <code>nil</code></td></tr>
              <tr><td className="py-2 pr-4"><code>..</code></td><td>half-open range <code>[a, b)</code></td></tr>
              <tr><td className="py-2 pr-4"><code>=</code> <code>+=</code> <code>-=</code> <code>*=</code> <code>/=</code> <code>%=</code></td><td>assignment (also to fields and indices)</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          onRun={onRun}
          code={`const u = { name: "Ana", address: { city: "Porto" } }
const nobody = nil
print(u?.address?.city)                     // Porto
print(nobody?.address?.city)                // nil — the whole chain is nil
print(nobody?.name.upper() ?? "anonymous")  // anonymous
print(7 / 2, 7 % 2, 2 ** 3 ** 2)            // 3 1 512`}
        />
      </section>

      {/* ================= CONTROL FLOW ================= */}
      <section id="control" data-group="Language" data-title="Control flow" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Control flow</h2>
        <p>
          No parentheses around conditions. <code>match</code> is an expression, with literal patterns, <code>|</code>
          alternatives, bindings with guards and the <code>_</code> wildcard. A <code>match</code> must be
          <strong>exhaustive</strong>: without a <code>_</code> (or plain name) arm it is rejected before the program runs.
        </p>
        <DocCodeBlock
          onRun={onRun}
          code={`const age = 20
if age >= 18 { print("adult") } else if age > 12 { print("teen") } else { print("child") }

let i = 0
while i < 3 { i += 1 }

for n in 0..10 {
    if n == 2 { continue }
    if n == 5 { break }
    print(n)
}

for item in ["a", "b"] { print(item) }
for key, value in { b: 2, a: 1 } { print(key, value) }   // alphabetical order
for index, item in ["x", "y"] { print(index, item) }

fn classify(n) {
    return match n {
        0 => "zero"
        1 | 2 => "small"
        x if x < 0 => "negative"
        _ => "large"
    }
}
print(classify(-3), classify(2))      // negative small`}
        />
      </section>

      {/* ================= FUNCTIONS ================= */}
      <section id="functions" data-group="Language" data-title="Functions" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Functions</h2>
        <p>
          Functions are first-class values with closures. There are three forms: declaration, anonymous function and
          arrow function. Parameters may have default values (which may use earlier parameters) and the last one may be
          variadic. Functions and structs may be generic; type parameters are erased at runtime.
        </p>
        <DocCodeBlock
          onRun={onRun}
          code={`fn sum(a, b) { return a + b }

const double = fn(x) { return x * 2 }
const triple = (x) => x * 3
const area = (w, h) => {
    const r = w * h
    return r
}

fn connect(host = "localhost", port = 80) {
    return \`\${host}:\${port}\`
}
print(connect(), connect("api.local", 8080))   // localhost:80 api.local:8080

fn log(level, ...messages) {
    print(\`[\${level}]\`, messages.join(" "))
}
log("info", "server", "ready")                // [info] server ready

fn first<T>(items: list<T>): T? {
    if len(items) == 0 { return nil }
    return items[0]
}
print(first([9, 8]), first([]))               // 9 nil

fn counter() {
    let n = 0
    return () => {
        n += 1
        return n
    }
}
const next = counter()
next()
print(next())                                  // 2`}
        />
        <p>
          Missing arguments are <code>nil</code> (or their default value); passing too many is an error, except for variadic functions.
        </p>
      </section>

      {/* ================= LISTS AND MAPS ================= */}
      <section id="collections" data-group="Language" data-title="Lists and maps" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Lists and maps</h2>
        <DocCodeBlock
          onRun={onRun}
          code={`let xs = [5, 3, 8]
xs.push(1)
print(xs[0], xs[-1], len(xs))                     // 5 1 4
print(xs.sort(), xs.filter((x) => x > 3))         // [1, 3, 5, 8] [5, 8]
print(xs.map((x) => x * 2).join(","))             // 10,6,16,2
print(xs.reduce((a, b) => a + b, 0))              // 17

const m = { name: "Fogu", version: 3 }
m.year = 2026
m["kind"] = "language"
print(m.name, m["version"], m.missing)            // Fogu 3 nil
print(m.keys(), m.has("year"))                    // [kind, name, version, year] true`}
        />
        <ul>
          <li><strong>Lists</strong>: <code>push pop shift unshift insert remove first last contains index_of join reverse slice concat sort map filter each find any all reduce</code></li>
          <li><strong>Maps</strong>: <code>keys values entries has get set remove merge</code></li>
          <li>All of them work as methods (<code>xs.push(1)</code>) or as functions (<code>push(xs, 1)</code>).</li>
        </ul>
      </section>

      {/* ================= ERRORS AS VALUES ================= */}
      <section id="errors" data-group="Language" data-title="Errors" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Errors as values</h2>
        <p>
          There is no <code>try/catch</code>. A function returns <code>value, error</code>; the <code>?</code> operator returns
          the error to the caller. An <code>error</code> has <code>message</code>, <code>cause</code> and <code>trace</code>.
          <code>?</code> may only be used in functions whose declared return type includes <code>error</code> (or that are not annotated).
        </p>
        <DocCodeBlock
          onRun={onRun}
          code={`fn divide(a, b) {
    if b == 0 {
        return nil, error("division by zero")
    }
    return a / b, nil
}

fn compute(a, b): (int, error) {
    const x = divide(a, b)?        // on error, returns (nil, err)
    return x * 2, nil
}

const [r, err] = compute(1, 0)
if err != nil {
    print(err.message)              // division by zero
}

const wrapped = error("failed to load", err)
print(wrapped.cause.message)        // division by zero`}
        />
        <p>
          The <strong>operational</strong> errors of native functions (a file that does not open, a refused connection,
          invalid SQL or JSON) are <code>error</code> values too. Programming errors (for example, an argument of the wrong
          type) stop the program.
        </p>
        <DocCodeBlock
          onRun={onRun}
          code={`import json from "fogu:json"
import net from "fogu:net"

const [conn, err] = net.tcpConnect("127.0.0.1:9")
if err != nil { print("no connection:", err.message) }

fn read_config(text) {
    const cfg = json.parse(text)?   // propagates invalid JSON
    return cfg, nil
}`}
        />
        <div className="mt-4 rounded-xl border-l-4 border-amber-500 bg-amber-50 dark:bg-amber-900/20 p-4 text-sm">
          <strong>Note:</strong> <code>const [a, b] = f()</code> destructures lists. If a native function returns a list
          (for example <code>json.parse("[1, 2]")</code> or <code>db.query(...)</code>), use <code>?</code> instead of destructuring.
        </div>
        <p>
          <code>panic("msg")</code> signals an unrecoverable failure. In an <code>fg</code> task it does not stop the other
          tasks, unless the program runs with <code>--strict</code>.
        </p>
      </section>

      {/* ================= STRUCTS AND INTERFACES ================= */}
      <section id="structs" data-group="Language" data-title="Structs and interfaces" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Structs, interfaces and composition</h2>
        <p>
          There is no inheritance: reuse comes from <code>embed</code>. The constructor is called <code>new</code> and methods
          reach the instance through <code>this</code>. Structs compare by identity, unless they define <code>equals</code>.
          Interfaces are <strong>structural</strong>: any struct (or map of functions) with the required methods satisfies
          them, without declaring it.
        </p>
        <DocCodeBlock
          onRun={onRun}
          code={`struct Point {
    x = 0
    y = 0
    new(x, y = 0) {
        this.x = x
        this.y = y
    }
    norm() { return (this.x ** 2 + this.y ** 2) ** 0.5 }
    equals(other) { return this.x == other.x && this.y == other.y }
}

const p = new Point(3, 4)
print(p.norm())                       // 5.0
print(p == new Point(3, 4))           // true (uses equals)

struct Logger {
    prefix = "[log]"
    log(msg) { print(this.prefix, msg) }
}

struct Service {
    embed Logger                      // Logger's fields and methods become available
    name = "api"
    start() { this.log(\`service \${this.name} starting\`) }
}

interface Startable { start() }
fn boot(s: Startable) { s.start() }
boot(new Service())                   // [log] service api starting

struct Stack<T> {                     // generic struct
    items = []
    push(v: T) { this.items.push(v) }
}
new Stack<int>().push(1)`}
        />
      </section>

      {/* ================= MODULES AND VISIBILITY ================= */}
      <section id="modules" data-group="Language" data-title="Modules" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Modules and visibility</h2>
        <p>
          Each file is a module, evaluated only once. Use <code>export</code> to export functions, <code>let</code>/<code>const</code>,
          <code>struct</code>s and <code>interface</code>s. Names starting with <code>_</code> are private to the module: they cannot be
          imported, read through <code>module._x</code>, or used as fields or methods of a struct outside the module where it
          was declared.
        </p>
        <DocCodeBlock
          title="account.fg"
          code={`export struct Account {
    owner = ""
    _balance = 0
    new(owner, balance = 0) {
        this.owner = owner
        this._balance = balance
    }
    balance() { return this._balance }
    deposit(v) { this._balance += v }
}

export const CURRENCY = "EUR"
fn _audit() { return true }           // private`}
        />
        <DocCodeBlock
          title="main.fg"
          onRun={onRun}
          code={`import { Account, CURRENCY } from "./account.fg"
import { Account as Acc } from "./account.fg"   // under another name
import account from "./account.fg"             // namespace: account.CURRENCY
import json from "fogu:json"                    // standard library
// import greet from "greet"                    // a package installed with fogu add

const a = new Account("Ana", 10)
a.deposit(5)
print(a.balance(), CURRENCY)                    // 15 EUR
// a._balance                                   // error[E0104]: '_balance' is private`}
        />
      </section>

      {/* ================= CONCURRENCY ================= */}
      <section id="concurrency" data-group="Language" data-title="Concurrency" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Concurrency: <code>fg</code>, <code>chan</code>, <code>select</code></h2>
        <p>
          <code>fg</code> starts a cooperative task (not an OS thread). Buffered or unbuffered channels connect tasks;
          <code>select</code> waits for the first ready case (choosing randomly among ready ones) and <code>timeout(d)</code>
          returns a channel that receives a value after <code>d</code>. The runtime detects deadlocks.
        </p>
        <DocCodeBlock
          onRun={onRun}
          code={`const results = chan(3)

fg fn() {
    for i in 0..3 { results <- i * 10 }
    close(results)
}()

for v in results { print("received", v) }

const replies = chan<str>(buffer: 1)
fg () => { replies <- "ready" }()

select {
    case r = <-replies: print(r)
    case <-timeout(500ms): print("timed out")
}`}
        />
      </section>
    </>
  );
};
