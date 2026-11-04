"""Standard library reference: overview, globals, values and core modules."""

G = "Standard library"
C = "Core modules"

MODULE_TABLE = [
    ("Core", "fogu:math fogu:json fogu:time fogu:types fogu:system fogu:fs fogu:path fogu:process fogu:sync fogu:test fogu:log fogu:regex fogu:crypto"),
    ("Data formats", "fogu:csv fogu:excel fogu:yaml fogu:toml fogu:xml"),
    ("Storage", "fogu:kv fogu:surrealkv fogu:surrealdb fogu:sql fogu:orm fogu:redis fogu:s3 fogu:scylla fogu:etcd"),
    ("Web &amp; network", "fogu:http fogu:web fogu:jwt fogu:topcoat fogu:websocket fogu:socketio fogu:grpc fogu:net fogu:tls fogu:security"),
    ("Messaging &amp; events", "fogu:nats fogu:kafka fogu:rabbitmq fogu:mqtt fogu:broker fogu:fogumq fogu:foguMQ-client fogu:events fogu:cron"),
    ("Integrations", "fogu:mail fogu:ai fogu:ssh fogu:shell fogu:ffi fogu:playbook"),
]


def overview_table():
    rows = "".join(
        '<tr><td class="py-2 pr-4 font-medium whitespace-nowrap">%s</td><td class="py-2">%s</td></tr>'
        % (group, " ".join('<a class="text-ember-600 dark:text-ember-400 underline" href="#mod-%s"><code>%s</code></a>' % (m.split(":")[1], m) for m in mods.split()))
        for group, mods in MODULE_TABLE
    )
    return '<div class="overflow-x-auto mt-4"><table class="w-full text-sm"><tbody class="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">%s</tbody></table></div>' % rows


SECTIONS = [
    {
        "id": "library",
        "group": G,
        "title": "Overview",
        "heading": "Standard library",
        "intro": "Modules are imported with <code>import name from \"fogu:name\"</code> (or by name: "
        "<code>import { parse } from \"fogu:json\"</code>). Each section below lists every function with its "
        "parameters and return value, and shows an example with its <strong>real output</strong> "
        "(the examples are run when this page is generated). " + overview_table(),
        "blocks": [
            {
                "h3": "Conventions",
                "text": "<strong>Errors are values.</strong> Operational failures — a missing file, a refused "
                "connection, a server error, malformed input — come back as an <code>error</code> value: handle them "
                "with <code>?</code> (propagate) or <code>const [value, err] = f()</code>. Programming mistakes — a "
                "wrong argument type, an unknown option — stop the program with a message. "
                "<strong>Options</strong> are passed as a last map argument: <code>f(x, { timeout: 5s })</code>. "
                "<strong>Durations</strong> accept a duration literal (<code>500ms</code>, <code>2s</code>, "
                "<code>1h</code>) or an int number of milliseconds. Network operations wait cooperatively: other "
                "<code>fg</code> tasks keep running while one waits.",
                "example": r'''
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
''',
                "run": True,
            },
        ],
    },
    {
        "id": "globals",
        "group": G,
        "title": "Global functions",
        "intro": "Available everywhere, without an import.",
        "blocks": [
            {
                "api": [
                    ("print(...values)", "nil", "Writes the values separated by spaces and a newline (<code>Fogu.write</code> writes without them)."),
                    ("len(x)", "int", "Length of a <code>str</code> (characters), <code>list</code>, <code>map</code>, <code>buffer</code> (bytes) or <code>chan</code> (queued items)."),
                    ("typeof(x)", "str", "<code>int float str bool nil list map buffer fn duration chan error struct …</code>"),
                    ("str(x)", "str", "Text form of any value (the same text <code>print</code> shows)."),
                    ("int(x)", "int", "From a float (truncates), a numeric str or a bool. Invalid text stops the program."),
                    ("float(x)", "float", "From an int or a numeric str."),
                    ("bytes(x, encoding = \"utf8\")", "buffer", "A new <a class=\"underline\" href=\"#values\">buffer</a>: from a str (encodings <code>utf8 hex base64 base64url latin1</code>), an int <code>n</code> (n zero bytes), a list of byte ints, or a buffer (a copy)."),
                    ("clock()", "float", "Seconds since the UNIX epoch, with fractions — use differences to measure time."),
                    ("error(message, cause?)", "error", "A new error value; <code>err.message</code>, <code>err.cause</code>."),
                    ("panic(...values)", "—", "Stops the program with a message (for impossible situations)."),
                    ("timeout(d)", "chan", "A channel that receives once after <code>d</code> (use with <code>select</code>)."),
                    ("close(ch)", "nil", "Closes a channel: receivers get <code>nil</code> once it is empty."),
                ],
                "example": r'''
print(len("olá"), len([1, 2, 3]), len({ a: 1 }))
print(typeof(1), typeof(1.5), typeof("x"), typeof(nil), typeof([]), typeof({}), typeof(2s), typeof(bytes(2)))
print(str(42) + "!", int("17") + 1, int(3.9), float("2.5") * 2)
const e = error("disk full")
print(typeof(e), e.message)
const t0 = clock()
print(clock() - t0 < 1)
''',
                "run": True,
            },
        ],
    },
    {
        "id": "values",
        "group": G,
        "title": "Strings, lists, maps and buffers",
        "intro": "Methods of the built-in values. Indexes start at 0; negative indexes count from the end in "
        "<code>slice</code>.",
        "blocks": [
            {
                "h3": "str",
                "api": [
                    ("s.len()", "int", "Number of characters."),
                    ("s.upper() · s.lower()", "str", "Case conversion."),
                    ("s.trim()", "str", "Without leading and trailing whitespace."),
                    ("s.split(sep)", "list", "Parts between <code>sep</code>."),
                    ("s.lines() · s.chars()", "list", "Lines (without line endings) · single characters."),
                    ("s.contains(t) · s.starts_with(t) · s.ends_with(t)", "bool", "Substring tests."),
                    ("s.index_of(t)", "int", "Character index of the first <code>t</code>, or <code>-1</code>."),
                    ("s.replace(old, new)", "str", "Every <code>old</code> replaced."),
                    ("s.repeat(n)", "str", "<code>s</code> repeated <code>n</code> times."),
                    ("s.slice(start, end?)", "str", "Characters from <code>start</code> up to (not including) <code>end</code>."),
                ],
                "example": r'''
const s = "  Fogu, fast & small  "
print(s.trim().upper(), s.trim().split(", "))
print("a-b-c".replace("-", "+"), "ab".repeat(3), "olá mundo".slice(4))
print("main.fg".ends_with(".fg"), "fogu".index_of("g"), "x\ny".lines(), "abc".chars())
print(`template: ${1 + 1} items`)
''',
                "run": True,
            },
            {
                "h3": "list",
                "api": [
                    ("l.push(x) · l.pop()", "nil · value", "Add to / remove from the end."),
                    ("l.unshift(x) · l.shift()", "nil · value", "Add to / remove from the start."),
                    ("l.insert(i, x) · l.remove(i)", "nil · value", "At an index."),
                    ("l.first() · l.last()", "value", "<code>nil</code> when empty."),
                    ("l.contains(x) · l.index_of(x)", "bool · int", "Search by value."),
                    ("l.join(sep)", "str", "Items as text joined with <code>sep</code>."),
                    ("l.reverse() · l.sort(fn?)", "list", "New list; <code>sort</code> takes an optional comparator <code>(a, b) =&gt; int</code>."),
                    ("l.slice(start, end?) · l.concat(other)", "list", "Copies."),
                    ("l.map(fn) · l.filter(fn) · l.find(fn)", "list · list · value", "Functional helpers."),
                    ("l.any(fn) · l.all(fn) · l.each(fn)", "bool · bool · nil", ""),
                    ("l.reduce(fn, initial)", "value", "<code>fn(acc, item)</code> over the items."),
                    ("l.clear() · l.len()", "nil · int", ""),
                ],
                "example": r'''
const nums = [5, 3, 8, 1]
nums.push(10)
print(nums, nums.sort(), nums.sort((a, b) => b - a))
print(nums.map((n) => n * 2), nums.filter((n) => n > 4), nums.find((n) => n > 6))
print(nums.reduce((acc, n) => acc + n, 0), nums.any((n) => n > 9), nums.all((n) => n > 0))
print(nums.slice(1, 3), nums.first(), nums.last(), nums.join(" | "))
''',
                "run": True,
            },
            {
                "h3": "map",
                "api": [
                    ("m.keys() · m.values() · m.entries()", "list", "Entries are <code>[key, value]</code> pairs."),
                    ("m.has(key)", "bool", ""),
                    ("m.get(key, default?)", "value", "The value, or <code>default</code> (nil)."),
                    ("m.set(key, value) · m[key] = value", "nil", ""),
                    ("m.remove(key)", "value", "Removes and returns the value."),
                    ("m.merge(other)", "map", "A new map; <code>other</code> wins on equal keys."),
                    ("m.len() · m.clear()", "int · nil", ""),
                ],
                "example": r'''
const user = { name: "Ana", age: 30 }
user.city = "Lisboa"
print(user.keys(), user.has("age"), user.get("email", "none"))
print(user.merge({ age: 31, admin: true }))
for entry in user.entries() { print(entry[0], "=", entry[1]) }
''',
                "run": True,
            },
            {
                "h3": "buffer",
                "text": "A mutable sequence of bytes, for binary data: files, network packets, protocols. Built with "
                "<code>bytes(...)</code>; <code>typeof</code> gives <code>\"buffer\"</code> and <code>buffer</code> works as a "
                "type annotation. <code>b[i]</code> reads an int 0..255 and <code>b[i] = n</code> writes one (any other value "
                "stops the program); <code>for x in b</code> and <code>for i, x in b</code> walk the bytes; <code>==</code> "
                "compares the bytes; <code>print</code> shows the first 32 bytes in hex and <code>json.stringify</code> writes an "
                "array of ints. The network modules (<code>fogu:net</code>, <code>fogu:websocket</code>, <code>res.bytes()</code> "
                "of <code>fogu:http</code>) and <code>zip image protobuf serial usb kafka</code> return binary data as buffers; "
                "their inputs also accept a str or a list of byte ints.",
                "api": [
                    ("b.len() · b.isEmpty()", "int · bool", ""),
                    ("b.text(enc = \"utf8\") · b.toString(enc)", "str", "Decoded (<code>utf8 hex base64 base64url latin1</code>; invalid UTF-8 becomes U+FFFD)."),
                    ("b.hex() · b.base64() · b.toList()", "str · str · list", ""),
                    ("b.get(i)", "int · nil", "The byte, or <code>nil</code> out of range."),
                    ("b.push(byte...) · b.append(x...) · b.write(x...)", "buffer", "Add to the end, in place, and return the buffer (chainable); <code>append</code>/<code>write</code> take a str, a buffer, a list or byte ints."),
                    ("b.concat(x...)", "buffer", "A new buffer: <code>b</code> followed by the arguments."),
                    ("b.slice(start, end?) · b.copy() · b.clone()", "buffer", "Copies; negative indexes count from the end."),
                    ("b.indexOf(needle, from = 0) · b.includes(needle)", "int · bool", "<code>needle</code>: a byte, a str or a buffer; <code>-1</code> when missing."),
                    ("b.startsWith(x) · b.endsWith(x) · b.equals(x)", "bool", ""),
                    ("b.split(sep)", "list", "Buffers between <code>sep</code>."),
                    ("b.fill(byte, start?, end?) · b.reverse() · b.clear()", "buffer · buffer · nil", "In place."),
                    ("b.readU8 · readI8 · readU16 · readI16 · readU32 · readI32 · readI64 · readF32 · readF64 (offset, bigEndian = false)", "int · float", "A number at <code>offset</code>, little-endian unless <code>bigEndian</code>."),
                    ("b.writeU8 … b.writeF64(offset, value, bigEndian = false)", "", "Writes a number at <code>offset</code>."),
                ],
                "example": r'''
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
''',
                "run": True,
            },
        ],
    },
    {
        "id": "fogu-object",
        "group": G,
        "title": "The Fogu object",
        "intro": "Runtime helpers available everywhere, without an import.",
        "blocks": [
            {
                "api": [
                    ("Fogu.version · Fogu.os", "str", "Fogu's version · <code>windows</code>, <code>linux</code>, <code>macos</code>…"),
                    ("Fogu.cores() · Fogu.cores(n) · Fogu.cores([cpus])", "int · list · error", "How many threads the runtime uses for networking and blocking work (default: the machine's CPUs) · use <code>n</code> · run only on those CPUs (Linux, Windows). Set it at the top of the program — once the runtime has started it is an error value. Also <code>--cores 4</code>, <code>--cores 0,2-3</code>, <code>$FOGU_CORES</code>."),
                    ("Fogu.env", "map", "Environment variables."),
                    ("Fogu.args", "list", "Arguments after the script name (<code>fogu run app.fg a b</code>)."),
                    ("Fogu.file(path)", "File", "<code>text() lines() json() write(s) append(s) exists() size() remove()</code>, fields <code>path name ext dir</code>."),
                    ("Fogu.asset(path)", "str", "A project file: relative to the project root — and inside <code>fogu build --compile</code> executables, the extracted embedded files."),
                    ("Fogu.run(cmd, args?, opts?)", "map", "Runs a program → <code>{ ok, code, stdout, stderr }</code> (see <a class=\"underline\" href=\"#mod-process\">fogu:process</a>)."),
                    ("Fogu.glob(pattern)", "list", "Files matching <code>*</code>, <code>?</code> and <code>**</code>."),
                    ("Fogu.hash(data, algo?)", "str", "Hex digest, <code>sha256</code> (default) or <code>sha512</code>."),
                    ("Fogu.sql(url)", "Database", "Same as <code>sql.open(url)</code>."),
                    ("Fogu.sleep(d)", "nil", "Waits (other tasks keep running)."),
                    ("Fogu.signal.wait(names = [\"SIGINT\", \"SIGTERM\"], { timeout })", "str · nil", "Keeps the program alive (servers — fogu:web/http requests are answered while it waits — and <code>fg</code> tasks keep running) until one of the signals or events happens; returns its name, <code>nil</code> after <code>timeout</code>."),
                    ("Fogu.signal.on(name, fn(data)) · off(name) · emit(name, data?) · last · names", "", "Handlers for OS signals (<code>SIGINT SIGTERM SIGHUP SIGQUIT SIGUSR1 SIGUSR2</code>; Windows: <code>SIGINT SIGBREAK SIGTERM</code>) or the program's own events; they run as <code>fg</code> tasks. With no handler and nobody waiting, SIGINT/SIGTERM still end the program (exit code 130/143) — after releasing sshfs mounts, browsers and SurrealKV/broker stores. <code>emit</code> also simulates a signal."),
                    ("Fogu.host.embedded · Fogu.host.emit(name, data)", "bool", "When the runtime is embedded through the C API (<a class=\"underline\" href=\"#embed\">libfogu_rt</a>): calls the host's callback; <code>false</code> without one."),
                    ("Fogu.write(...values)", "nil", "Like <code>print</code>, without spaces or newline (streaming output)."),
                    ("Fogu.watch(path | [paths], { recursive = true, debounce = 50ms })", "Watcher · error", "File system events: <code>w.next(timeout?)</code> → <code>{ kind, path, paths }</code> (<code>create modify remove rename</code>), <code>tryNext()</code>, <code>close()</code>."),
                    ("Fogu.exit(code?)", "—", "Ends the program."),
                ],
                "example": r'''
Fogu.file("notes.txt").write("one\ntwo")?
const f = Fogu.file("notes.txt")
print(f.name, f.ext, f.exists(), f.size(), f.lines()?)
print(Fogu.hash("abc"))
print(typeof(Fogu.env), typeof(Fogu.args), Fogu.asset("data/logo.png").ends_with("data/logo.png"))
Fogu.write("no newline: ", 1, 2)
print()
''',
                "run": True,
            },
            {
                "text": "Signals and the program's events (<code>emit</code> stands in for a real Ctrl+C here):",
                "example": r'''
const seen = []
Fogu.signal.on("job.done", fn(d) { seen.push(d.id) })
Fogu.signal.emit("job.done", { id: 7 })
fg fn() {
    Fogu.sleep(10ms)
    Fogu.signal.emit("SIGINT")          // a real one comes from Ctrl+C / kill -INT
}()
print("stopping on", Fogu.signal.wait())
print(seen, Fogu.signal.wait("never", { timeout: 20ms }))
''',
                "run": True,
            },
            {
                "text": "Watching files — rebuild on every change:",
                "example": r'''
const w = Fogu.watch("src")?
while true {
    const ev = w.next()?
    print(ev.kind, ev.path)
    Fogu.run("fogu", ["test"])
}
''',
                "title": "watch.fg",
                "output": "modify src/app.fg\ncreate src/new.fg\nremove src/old.fg",
            },
        ],
    },
    # ------------------------------------------------------------------ core
    {
        "id": "mod-math",
        "group": C,
        "title": "fogu:math",
        "import": 'import math from "fogu:math"',
        "intro": "Numbers. Rounding to an int (<code>floor ceil trunc round</code>) keeps ints as they are; "
        "the other functions return floats. The JIT compiles these calls inside hot code with the same results.",
        "blocks": [
            {
                "api": [
                    ("math.PI · E · TAU · LN2 · LN10 · SQRT2 · EPSILON · INF · NAN", "float", "Constants."),
                    ("math.MAX_INT · math.MIN_INT", "int", "The int range (64-bit)."),
                    ("math.abs(x) · math.sign(x)", "number", "Ints stay ints; <code>sign</code> is -1, 0 or 1."),
                    ("math.sqrt(x) · cbrt(x) · pow(base, exp) · hypot(x, y)", "float", ""),
                    ("math.floor(x) · ceil(x) · trunc(x) · round(x)", "int", "<code>round</code> takes halves away from zero."),
                    ("math.round(x, digits) · math.fract(x)", "float", "Rounded to <code>digits</code> decimals; the fractional part."),
                    ("math.sin cos tan asin acos atan (x) · atan2(y, x)", "float", "Radians."),
                    ("math.sinh cosh tanh asinh acosh atanh (x)", "float", "Hyperbolic."),
                    ("math.exp(x) · exp2 · expm1 · log(x, base?) · log2 · log10 · log1p", "float", "<code>log(x)</code> is the natural logarithm."),
                    ("math.degrees(rad) · math.radians(deg)", "float", ""),
                    ("math.min(...) · math.max(...)", "number", "Of the arguments or of one list; keeps the winner's type."),
                    ("math.clamp(x, lo, hi) · math.lerp(a, b, t)", "number · float", "<code>clamp</code> of ints is an int."),
                    ("math.isNaN(x) · isInf(x) · isFinite(x)", "bool", ""),
                    ("math.gcd(a, b) · lcm(a, b) · factorial(n) · isPrime(n)", "int · bool", "Ints; overflow is an error."),
                    ("math.sum(list) · mean(list) · median(list)", "number · float", "<code>sum</code> of ints is an int."),
                    ("math.random() · randomInt(min, max) · uniform(lo, hi)", "float · int · float", "<code>[0, 1)</code>; both ends included; <code>[lo, hi)</code>. Not for secrets: use <code>crypto</code>."),
                    ("math.choice(list) · math.shuffle(list) · math.seed(n)", "value · list · nil", "A random element (<code>nil</code> if empty); shuffles in place and returns the list; a repeatable sequence."),
                ],
                "example": r'''
import math from "fogu:math"
print(math.abs(-4), math.sqrt(16), math.pow(2, 10), math.floor(2.7), math.ceil(2.1))
print(math.round(math.sin(math.PI / 2), 6), math.atan2(1, 1) == math.PI / 4, math.hypot(3, 4))
print(math.log(math.E), math.log(8, 2), math.round(2.5), math.round(3.14159, 2))
print(math.min(3, 1, 2), math.max([4, 9, 2]), math.clamp(15, 0, 10), math.lerp(0, 10, 0.25))
print(math.gcd(12, 18), math.factorial(10), math.mean([1, 2, 3, 4]), math.median([5, 1, 3]))
math.seed(7)
const a = math.randomInt(1, 6)
math.seed(7)
print(a == math.randomInt(1, 6), math.random() < 1)
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-json",
        "group": C,
        "title": "fogu:json",
        "import": 'import json from "fogu:json"',
        "blocks": [
            {
                "api": [
                    ("json.parse(text)", "value · error", "JSON text → maps, lists, str, int, float, bool, nil. Invalid JSON is an error value."),
                    ("json.stringify(value)", "str", "Compact JSON (map keys in order)."),
                ],
                "example": r'''
import json from "fogu:json"
const data = json.parse("{\"name\": \"Ana\", \"tags\": [\"a\", \"b\"], \"age\": 30}")?
print(data.name, data.tags, typeof(data.age))
print(json.stringify({ ok: true, list: [1, 2.5, nil] }))
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-types",
        "group": C,
        "title": "fogu:types",
        "import": 'import types from "fogu:types"',
        "intro": "Runtime type checks, lenient conversions and schemas for data that comes from outside "
        "(JSON bodies, query strings, environment variables, files).",
        "blocks": [
            {
                "api": [
                    ("types.isInt isFloat isNumber isStr isBool isNil isList isMap isBuffer isFn isError isDuration isRange isStruct isObject isChan (v)", "bool", ""),
                    ("types.isEmpty(v)", "bool", "<code>nil</code>, <code>\"\"</code>, <code>[]</code>, <code>{}</code>, an empty buffer or range."),
                    ("types.of(v)", "str", "Like <code>typeof</code>, but the name of a struct or native object (<code>\"User\"</code>, <code>\"Date\"</code>)."),
                    ("types.toInt toFloat toBool toStr toList toMap (v, default?)", "value · error", "Lenient conversions: <code>\" 0x1F \"</code> → 31, <code>\"1_000\"</code> → 1000, <code>\"yes\"/\"off\"</code> → bool, a struct → map, pairs → map. When it can't convert: <code>default</code>, or an error value."),
                    ("types.is(v, spec)", "bool", "Whether <code>v</code> matches the spec."),
                    ("types.check(v, spec)", "value · error", "<code>v</code>, or an error value naming the first problem with its path (<code>tags[1]: expected str, got int 2</code>)."),
                    ("types.validate(v, spec)", "list", "Every problem (empty when valid)."),
                    ("types.assert(v, spec)", "value", "Like <code>check</code>, but a mismatch stops the program."),
                    ("types.coerce(v, spec)", "value · error", "A copy where texts become the ints, floats and bools the spec asks for (query strings, forms, env vars), then checked."),
                    ("types.equal(a, b) · types.clone(v)", "bool · value", "Deep equality (structs by fields); a deep copy."),
                    ("spec", "", "A type name (<code>any nil bool int float number str list map buffer fn error duration range struct object chan</code>, or a struct/object name); <code>\"T?\"</code> also nil, <code>\"A|B\"</code>, <code>\"[T]\"</code> a list of T, <code>\"{T}\"</code> a map of T; <code>[spec]</code>; <code>{ field: spec, \"opt?\": spec }</code>; or <code>{ \"$type\": spec, min, max, minLen, maxLen, pattern, oneOf, nullable, fields, items, strict }</code>."),
                ],
                "example": r'''
import types from "fogu:types"
print(types.isInt(3), types.of([]), types.toInt("0x1F"), types.toBool("yes"), types.toInt("x", 0))
const User = { name: { "$type": "str", minLen: 1 }, "age?": { "$type": "int", min: 0 }, tags: "[str]" }
print(types.is({ name: "Ana", tags: [] }, User))
for p in types.validate({ name: "", age: -1, tags: [1] }, User) { print(p) }
print(types.coerce({ page: "2", debug: "true" }, { page: "int", debug: "bool" })?)
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-time",
        "group": C,
        "title": "fogu:time",
        "import": 'import time from "fogu:time"',
        "intro": "Clocks, waits and dates. A Date is an instant plus a time zone (UTC unless you choose another); "
        "its fields and <code>format</code> use that zone. The IANA time zone database is built in.",
        "blocks": [
            {
                "api": [
                    ("time.now()", "int", "Milliseconds since the UNIX epoch."),
                    ("time.monotonic()", "float", "Seconds from a fixed point; never goes backwards (the wall clock can jump): measure durations with it."),
                    ("time.Date(at?, zone?)", "Date · error", "The current instant; or <code>at</code> = milliseconds, a text (as <code>time.parse</code>) or a map <code>{ year, month, day, hour, minute, second, millis, zone }</code>."),
                    ("time.zone() · time.setZone(name)", "str · nil · error", "The default zone of new dates (\"UTC\" at start): an IANA name (<code>\"America/Sao_Paulo\"</code>), <code>\"UTC\"</code> or <code>\"local\"</code> (the system's zone). An unknown name is an error value."),
                    ("time.localZone() · time.zones()", "str · list", "The system's zone (<code>TZ</code>, then the OS); every IANA name."),
                    ("time.parse(text, format?, zone?)", "Date · error", "RFC 3339 / ISO 8601, RFC 2822, <code>YYYY-MM-DD[ HH:MM[:SS]]</code>, or a strftime <code>format</code>. Texts without an offset are read in <code>zone</code> (default: the default zone)."),
                    ("time.format(date | ms, format, zone?)", "str", "strftime: <code>%Y %m %d %H %M %S %3f %A %a %B %b %j %U %Z %z %s</code>…"),
                    ("time.isLeapYear(y) · time.daysInMonth(y, m)", "bool · int", ""),
                    ("time.delay(d) · time.sleep(seconds)", "nil", "Waits; other <code>fg</code> tasks keep running."),
                ],
            },
            {
                "h3": "Date",
                "api": [
                    ("d.year · d.month · d.day", "int", "Month 1–12."),
                    ("d.hour · d.minute · d.second · d.millis · d.weekday", "int", "Weekday 0 = Sunday."),
                    ("d.zone · d.offset", "str · int", "The zone name; minutes east of UTC (São Paulo: -180)."),
                    ("d.dayOfYear · d.week · d.isLeapYear · d.daysInMonth", "int · bool", "<code>week</code> is the ISO week."),
                    ("d.getTime()", "int", "Milliseconds since the epoch (also <code>d.ms</code>)."),
                    ("d.toISOString() · d.iso()", "str", "UTC (<code>2026-10-10T12:00:00.000Z</code>); RFC 3339 with the zone's offset."),
                    ("d.toString() · d.format(fmt)", "str", "Human readable; strftime."),
                    ("d.setZone(name) · d.in(name)", "nil · Date", "The same instant in another zone: in place, or a copy."),
                    ("d.add(units) · d.sub(units)", "Date", "A new Date. <code>units</code>: a duration (<code>90m</code>), ms, or <code>{ years, months, weeks, days, hours, minutes, seconds, millis }</code> — calendar units follow the zone's clock (a day can be 23 or 25 hours; Jan 31 + 1 month = Feb 28)."),
                    ("d.startOf(unit) · d.clone()", "Date", "<code>year month week day hour minute second</code> (weeks start on Monday)."),
                    ("d.addMs(ms)", "nil", "Moves the date by <code>ms</code> (in place)."),
                    ("d.diff(other) · before · after · equals · sameDay", "int · bool", "Milliseconds between the dates; comparisons (a Date or ms)."),
                    ("getFullYear() getMonth() getDate() getDay() getHours() getMinutes() getSeconds() getMilliseconds() getTimezoneOffset()", "int", "JavaScript-style getters (month 0–11)."),
                ],
                "example": r'''
import time from "fogu:time"
const start = time.now()
time.delay(20ms)
print(time.now() - start >= 20)
const d = time.Date(1793793600000)
print(d.toISOString(), d.in("America/Sao_Paulo").format("%d/%m/%Y %H:%M %Z"))
time.setZone("America/New_York")
const x = time.Date({ year: 2026, month: 1, day: 31, hour: 9 })
print(x.iso(), x.add({ months: 1 }).iso(), x.startOf("month").format("%A"))
print(time.parse("04/11/2026 08:05", "%d/%m/%Y %H:%M", "Europe/Lisbon").toISOString())
time.setZone("UTC")
''',
                "run": True,
            },
        ],
    },
    {
        "id": "mod-system",
        "group": C,
        "title": "fogu:system",
        "import": 'import system from "fogu:system"',
        "blocks": [
            {
                "api": [
                    ("system.args", "list", "Script arguments (<code>fogu run app.fg a b</code> → <code>[a, b]</code>)."),
                    ("system.time()", "float", "Seconds since the epoch."),
                    ("system.readLine()", "str · nil", "The next line of stdin without its ending; <code>nil</code> at the end. Other <code>fg</code> tasks keep running while it waits."),
                    ("system.exit(code)", "—", "Ends the program."),
                    ("system.info.os · .arch · .family", "str", "<code>windows</code>/<code>linux</code>/<code>macos</code> · <code>x86_64</code>/<code>aarch64</code> · <code>windows</code>/<code>unix</code>."),
                    ("system.info.cores · .pid", "int", "Logical CPUs · process id."),
                    ("system.info.env(name)", "str · nil", "An environment variable."),
                    ("system.info.sleep_ms(ms)", "nil", "Blocks the whole program (prefer <code>time.delay</code>)."),
                ],
                "example": r'''
import system from "fogu:system"
const info = system.info
print(typeof(info.os), info.cores > 0, typeof(info.pid), system.args)
print(info.env("FOGU_DOES_NOT_EXIST"), system.time() > 0)
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-fs",
        "group": C,
        "title": "fogu:fs",
        "import": 'import fs from "fogu:fs"',
        "intro": "Files and folders. Failures (missing file, permission) are error values.",
        "blocks": [
            {
                "api": [
                    ("fs.readText(path)", "str · error", "Whole file as UTF-8 text."),
                    ("fs.readLines(path)", "list · error", "Lines without line endings."),
                    ("fs.writeText(path, text)", "nil · error", "Creates or replaces."),
                    ("fs.append(path, text)", "nil · error", "Adds to the end (creates the file)."),
                    ("fs.exists(path) · fs.isFile(path) · fs.isDir(path)", "bool", ""),
                    ("fs.mkdir(path)", "nil · error", "Creates the folder and its parents."),
                    ("fs.list(path)", "list · error", "Names inside a folder, sorted."),
                    ("fs.stat(path)", "map · error", "<code>{ size, isFile, isDir, modified }</code> (modified: ms since the epoch)."),
                    ("fs.copy(from, to) · fs.move(from, to)", "nil · error", ""),
                    ("fs.remove(path)", "nil · error", "A file or an empty folder."),
                    ("fs.removeAll(path)", "nil · error", "A folder and everything inside."),
                    ("fs.glob(pattern)", "list", "Paths matching <code>*</code>, <code>?</code>, <code>**</code>, sorted."),
                    ("fs.cwd() · fs.tempDir()", "str", "Working folder · system temp folder."),
                    ("fs.remote(ssh | \"user@host\", { sudo })", "RemoteFs", "The same functions on a server through <a href=\"#mod-ssh\">fogu:ssh</a> (plus <code>upload(local, remote)</code> and <code>download(remote, local)</code>); every call is an SSH command."),
                ],
                "example": r'''
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
''',
                "run": True,
            },
        ],
    },
    {
        "id": "mod-path",
        "group": C,
        "title": "fogu:path",
        "import": 'import path from "fogu:path"',
        "blocks": [
            {
                "api": [
                    ("path.join(...parts)", "str", "Joined with <code>/</code> and normalized."),
                    ("path.dirname(p) · path.basename(p)", "str", "Folder · last component."),
                    ("path.stem(p) · path.extname(p)", "str", "Name without extension · extension with the dot."),
                    ("path.normalize(p)", "str", "Resolves <code>.</code> and <code>..</code>."),
                    ("path.isAbsolute(p)", "bool", ""),
                    ("path.absolute(p)", "str", "Absolute path from the working folder."),
                    ("path.sep", "str", "The system separator."),
                ],
                "example": r'''
import path from "fogu:path"
print(path.join("src", "lib", "../main.fg"))
print(path.dirname("a/b/c.txt"), path.basename("a/b/c.txt"), path.stem("a/b/c.tar.gz"), path.extname("a/b/c.txt"))
print(path.normalize("a/./b/../c"), path.isAbsolute("/etc"), path.isAbsolute("etc"))
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-process",
        "group": C,
        "title": "fogu:process",
        "import": 'import process from "fogu:process"',
        "intro": "A program that exits with a non-zero code is <em>not</em> an error: check <code>ok</code> and "
        "<code>code</code>. Not being able to start it is an error value. For pipes and Linux commands see "
        "<a class=\"underline\" href=\"#mod-shell\">fogu:shell</a>.",
        "blocks": [
            {
                "api": [
                    ("process.run(cmd, args?, { cwd, env, input })", "map · error", "<code>{ ok, code, stdout, stderr }</code>."),
                    ("process.shell(command)", "map", "Through <code>sh -c</code> / <code>cmd /C</code>."),
                    ("process.env(name) · process.setEnv(name, value)", "str · nil", ""),
                    ("process.cwd() · process.chdir(path)", "str · nil", ""),
                    ("process.pid · process.os · process.arch · process.args", "", ""),
                    ("process.exit(code)", "—", ""),
                ],
                "example": r'''
import process from "fogu:process"
const r = process.run("git", ["--version"])?
print(r.ok, r.code, r.stdout.starts_with("git version"))
const [_, err] = process.run("no-such-program-xyz")
print(err != nil)
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-sync",
        "group": C,
        "title": "fogu:sync",
        "import": 'import sync from "fogu:sync"',
        "intro": "Coordination of <code>fg</code> tasks. Waiting operations yield to the other tasks; waiting forever "
        "is reported as a deadlock.",
        "blocks": [
            {
                "api": [
                    ("sync.Mutex()", "Mutex", "<code>lock()</code> <code>unlock()</code> <code>tryLock()</code> → bool, field <code>locked</code>."),
                    ("sync.WaitGroup()", "WaitGroup", "<code>add(n = 1)</code> <code>done()</code> <code>wait()</code>, field <code>count</code>."),
                    ("sync.Once()", "Once", "<code>do(fn)</code> runs <code>fn</code> only the first time."),
                    ("sync.Atomic(value)", "Atomic", "<code>get()</code> <code>set(v)</code> <code>add(n = 1)</code> → new value, <code>compareAndSwap(old, new)</code> → bool."),
                    ("sync.Semaphore(n)", "Semaphore", "<code>acquire()</code> (waits for a permit) <code>tryAcquire()</code> → bool <code>release()</code>, fields <code>available size</code>. Also limits <a class=\"underline\" href=\"#mod-vm\">fogu:vm</a> tasks (<code>{ semaphore }</code>)."),
                ],
                "example": r'''
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
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-test",
        "group": C,
        "title": "fogu:test",
        "import": 'import { test, skip, expect } from "fogu:test"',
        "intro": "Used in <code>*.test.fg</code> files, run with <code>fogu test</code>. A test fails when an expectation "
        "fails, when it raises an error or returns an error value.",
        "blocks": [
            {
                "api": [
                    ("test(name, fn)", "nil", "Declares a test."),
                    ("skip(name, fn)", "nil", "A test that is reported but not run."),
                    ("expect(value)", "Expectation", "Then a matcher; <code>.not</code> negates it."),
                    ("toBe(x) · toEqual(x)", "", "Equality (<code>toEqual</code> compares lists and maps deeply)."),
                    ("toBeTruthy() · toBeFalsy() · toBeNil() · toBeError()", "", ""),
                    ("toBeType(\"int\") · toContain(x) · toHaveLength(n)", "", ""),
                    ("toBeGreaterThan(n) · toBeLessThan(n) · toBeCloseTo(x, eps?)", "", ""),
                ],
                "example": r'''
import { test, expect } from "fogu:test"

test("math", () => {
    expect(1 + 2).toBe(3)
    expect([1, 2]).toContain(2)
    expect("abc").not.toHaveLength(2)
    expect(0.1 + 0.2).toBeCloseTo(0.3)
})
''',
                "title": "math.test.fg",
                "output": "$ fogu test\nmath.test.fg\n  ✓ math (0ms)\n\n1 passed — 1 file(s) [1ms]",
                "output_title": "fogu test",
            }
        ],
    },
    {
        "id": "mod-log",
        "group": C,
        "title": "fogu:log",
        "import": 'import log from "fogu:log"',
        "intro": "Structured logs to stderr (default), stdout or a file. The level comes from <code>FOGU_LOG</code> (default <code>info</code>).",
        "blocks": [
            {
                "api": [
                    ("log.trace/debug/info/warn/error(message, fields?)", "nil", "<code>fields</code>: a map added as <code>key=value</code> (or JSON keys)."),
                    ("log.setLevel(level)", "nil", "<code>trace debug info warn error off</code>."),
                    ("log.level()", "str", "Current level."),
                    ("log.setFormat(\"text\" | \"json\")", "nil", "One line per entry."),
                    ("log.setOutput(\"stderr\" | \"stdout\" | path)", "nil · error", "A file is appended to."),
                    ("log.setTime(bool)", "nil", "Show the time stamp (default true)."),
                    ("log.child(fields)", "Logger", "A logger that adds <code>fields</code> to every line; same methods + <code>child</code>."),
                ],
                "example": r'''
import log from "fogu:log"
log.setOutput("stdout")
log.setTime(false)
log.info("server started", { port: 3000 })
log.debug("not shown at the info level")
const db = log.child({ module: "db" })
db.warn("slow query", { ms: 812, sql: "select *" })
log.setFormat("json")
log.error("failed", { code: 23 })
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-regex",
        "group": C,
        "title": "fogu:regex",
        "import": 'import regex from "fogu:regex"',
        "intro": "Rust <code>regex</code> syntax (linear time, no backtracking). Positions are character indexes. "
        "In Fogu strings write <code>\\\\d</code> for <code>\\d</code>. An invalid pattern is an error value.",
        "blocks": [
            {
                "api": [
                    ("regex.compile(pattern, { ignoreCase, multiline, dotAll, verbose })", "Regex · error", ""),
                    ("re.test(text)", "bool", "Matches somewhere."),
                    ("re.matches(text)", "bool", "The whole text matches."),
                    ("re.find(text)", "map · nil", "<code>{ text, start, end, groups, named }</code>."),
                    ("re.findAll(text)", "list", "Every match."),
                    ("re.replace(text, repl) · re.replaceAll(text, repl)", "str", "<code>$1</code>, <code>${name}</code>, <code>$$</code> in <code>repl</code>."),
                    ("re.split(text, limit?)", "list", ""),
                    ("re.source · re.groups · re.names", "", "Pattern · group count · group names."),
                    ("regex.test/find/findAll/replace/replaceAll/split/matches(text, pattern, …)", "", "One-off shortcuts."),
                    ("regex.escape(text)", "str", "A literal pattern."),
                ],
                "example": r'''
import regex from "fogu:regex"
const email = regex.compile("(?P<user>[\\w.]+)@(?P<host>[\\w.]+)")?
print(email.test("write to ana@fogu.dev"))
const m = email.find("contact: ana@fogu.dev")
print(m.text, m.start, m.named.host, m.groups)
print(email.replaceAll("a@x.io, b@y.io", "${user} (at ${host})"))
print(regex.split("one, two;three", "\\s*[,;]\\s*"))
print(regex.findAll("v1.2 and v3.10", "\\d+\\.\\d+").map((x) => x.text))
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-crypto",
        "group": C,
        "title": "fogu:crypto",
        "import": 'import crypto from "fogu:crypto"',
        "blocks": [
            {
                "api": [
                    ("crypto.hash(algo, data, { base64 })", "str", "<code>md5 sha1 sha256 sha384 sha512</code>; hex by default."),
                    ("crypto.hmac(algo, key, data, { base64 })", "str", ""),
                    ("crypto.encrypt(data, key)", "str", "AES-256-GCM; <code>key</code> is a password (argon2id) or 64 hex chars. Base64 output with salt and nonce."),
                    ("crypto.decrypt(box, key, { bytes })", "str · error", "Wrong key or altered data → error value."),
                    ("crypto.hashPassword(password)", "str", "argon2id PHC string, for storing passwords."),
                    ("crypto.verifyPassword(password, hash)", "bool", ""),
                    ("crypto.randomBytes(n) · crypto.randomHex(n)", "list · str", "Secure random."),
                    ("crypto.randomInt(min, max)", "int", "Inclusive, unbiased."),
                    ("crypto.uuid(version = 4, namespace?, name?)", "str", "<code>uuid()</code> / <code>uuid(4)</code> random, <code>uuid(7)</code> time-ordered, <code>uuid(5, namespace, name)</code> name-based."),
                    ("crypto.uuidv4() · crypto.uuidv7()", "str", "v4: random. v7: the creation time in milliseconds followed by random bits — sorts by creation time (also within the same millisecond), good for database keys."),
                    ("crypto.uuidv5(namespace, name)", "str", "SHA-1 of a namespace and a name: the same inputs always give the same UUID. <code>namespace</code>: <code>\"dns\"</code>, <code>\"url\"</code>, <code>\"oid\"</code>, <code>\"x500\"</code> or a UUID; <code>name</code>: str or bytes."),
                    ("crypto.uuidInfo(text)", "map · error", "<code>{ version, variant, time }</code>; <code>time</code> is the creation time in ms since the epoch for v1/v6/v7, else nil. Text that is not a UUID → error value."),
                    ("crypto.base64(data, { url }) · crypto.fromBase64(text, { bytes })", "str", "<code>data</code>: str or list of bytes."),
                    ("crypto.hex(data) · crypto.fromHex(text, { bytes })", "str", ""),
                    ("crypto.equal(a, b)", "bool", "Constant-time comparison."),
                ],
                "example": r'''
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
print(crypto.uuidv5("dns", "example.com"), crypto.uuidv5("url", "https://fogu.dev") == crypto.uuid(5, "url", "https://fogu.dev"))
const a = crypto.uuidv7()
const b = crypto.uuidv7()
print(a < b, crypto.uuidInfo(a)?.version, crypto.uuidInfo(crypto.uuidv4())?.time, crypto.uuidInfo(a)?.time > 0)
print(crypto.uuidInfo("not-a-uuid"))
''',
                "run": True,
            }
        ],
    },
]
