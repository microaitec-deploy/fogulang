// Generated from docs/tools/reference/*.py by docs/tools/gen_reference.py — do not edit.
// Outputs of examples marked `run` were produced by running them.
import type { RefData } from '../components/RefSection';

export const REFERENCE: Record<string, RefData> = {
 "library": {
  "id": "library",
  "group": "Standard library",
  "title": "Overview",
  "import": null,
  "intro": "Modules are imported with <code>import name from \"fogu:name\"</code> (or by name: <code>import { parse } from \"fogu:json\"</code>). Each section below lists every function with its parameters and return value, and shows an example with its <strong>real output</strong> (the examples are run when this page is generated). <div class=\"overflow-x-auto mt-4\"><table class=\"w-full text-sm\"><tbody class=\"[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60\"><tr><td class=\"py-2 pr-4 font-medium whitespace-nowrap\">Core</td><td class=\"py-2\"><a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-math\"><code>fogu:math</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-json\"><code>fogu:json</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-time\"><code>fogu:time</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-system\"><code>fogu:system</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-fs\"><code>fogu:fs</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-path\"><code>fogu:path</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-process\"><code>fogu:process</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-sync\"><code>fogu:sync</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-test\"><code>fogu:test</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-log\"><code>fogu:log</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-regex\"><code>fogu:regex</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-crypto\"><code>fogu:crypto</code></a></td></tr><tr><td class=\"py-2 pr-4 font-medium whitespace-nowrap\">Data formats</td><td class=\"py-2\"><a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-csv\"><code>fogu:csv</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-excel\"><code>fogu:excel</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-yaml\"><code>fogu:yaml</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-toml\"><code>fogu:toml</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-xml\"><code>fogu:xml</code></a></td></tr><tr><td class=\"py-2 pr-4 font-medium whitespace-nowrap\">Storage</td><td class=\"py-2\"><a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-kv\"><code>fogu:kv</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-surrealkv\"><code>fogu:surrealkv</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-surrealdb\"><code>fogu:surrealdb</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-sql\"><code>fogu:sql</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-orm\"><code>fogu:orm</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-redis\"><code>fogu:redis</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-s3\"><code>fogu:s3</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-scylla\"><code>fogu:scylla</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-etcd\"><code>fogu:etcd</code></a></td></tr><tr><td class=\"py-2 pr-4 font-medium whitespace-nowrap\">Web &amp; network</td><td class=\"py-2\"><a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-http\"><code>fogu:http</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-web\"><code>fogu:web</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-jwt\"><code>fogu:jwt</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-topcoat\"><code>fogu:topcoat</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-websocket\"><code>fogu:websocket</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-socketio\"><code>fogu:socketio</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-grpc\"><code>fogu:grpc</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-net\"><code>fogu:net</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-security\"><code>fogu:security</code></a></td></tr><tr><td class=\"py-2 pr-4 font-medium whitespace-nowrap\">Messaging &amp; events</td><td class=\"py-2\"><a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-nats\"><code>fogu:nats</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-kafka\"><code>fogu:kafka</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-rabbitmq\"><code>fogu:rabbitmq</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-mqtt\"><code>fogu:mqtt</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-broker\"><code>fogu:broker</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-fogumq\"><code>fogu:fogumq</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-foguMQ-client\"><code>fogu:foguMQ-client</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-events\"><code>fogu:events</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-cron\"><code>fogu:cron</code></a></td></tr><tr><td class=\"py-2 pr-4 font-medium whitespace-nowrap\">Integrations</td><td class=\"py-2\"><a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-mail\"><code>fogu:mail</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-ai\"><code>fogu:ai</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-ssh\"><code>fogu:ssh</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-shell\"><code>fogu:shell</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-ffi\"><code>fogu:ffi</code></a> <a class=\"text-ember-600 dark:text-ember-400 underline\" href=\"#mod-playbook\"><code>fogu:playbook</code></a></td></tr></tbody></table></div>",
  "blocks": [
   {
    "h3": "Conventions",
    "text": "<strong>Errors are values.</strong> Operational failures — a missing file, a refused connection, a server error, malformed input — come back as an <code>error</code> value: handle them with <code>?</code> (propagate) or <code>const [value, err] = f()</code>. Programming mistakes — a wrong argument type, an unknown option — stop the program with a message. <strong>Options</strong> are passed as a last map argument: <code>f(x, { timeout: 5s })</code>. <strong>Durations</strong> accept a duration literal (<code>500ms</code>, <code>2s</code>, <code>1h</code>) or an int number of milliseconds. Network operations wait cooperatively: other <code>fg</code> tasks keep running while one waits.",
    "example": "import json from \"fogu:json\"\n\nconst [cfg, err] = json.parse(\"{ broken\")      // an error value, not a crash\nif err != nil {\n    print(\"invalid config:\", err.message)\n}\n\nfn load(text: str): error {\n    const data = json.parse(text)?              // ? returns the error to the caller\n    print(\"name:\", data.name)\n    return nil\n}\nprint(load(\"{\\\"name\\\": \\\"fogu\\\"}\"), load(\"nope\") != nil)",
    "output": "invalid config: json.parse: invalid JSON at position 2: Some('b')\nname: fogu\nnil true",
    "outputTitle": "output"
   }
  ]
 },
 "globals": {
  "id": "globals",
  "group": "Standard library",
  "title": "Global functions",
  "import": null,
  "intro": "Available everywhere, without an import.",
  "blocks": [
   {
    "api": [
     [
      "print(...values)",
      "nil",
      "Writes the values separated by spaces and a newline (<code>Fogu.write</code> writes without them)."
     ],
     [
      "len(x)",
      "int",
      "Length of a <code>str</code> (characters), <code>list</code>, <code>map</code>, <code>buffer</code> (bytes) or <code>chan</code> (queued items)."
     ],
     [
      "typeof(x)",
      "str",
      "<code>int float str bool nil list map buffer fn duration chan error struct …</code>"
     ],
     [
      "str(x)",
      "str",
      "Text form of any value (the same text <code>print</code> shows)."
     ],
     [
      "int(x)",
      "int",
      "From a float (truncates), a numeric str or a bool. Invalid text stops the program."
     ],
     [
      "float(x)",
      "float",
      "From an int or a numeric str."
     ],
     [
      "bytes(x, encoding = \"utf8\")",
      "buffer",
      "A new <a class=\"underline\" href=\"#values\">buffer</a>: from a str (encodings <code>utf8 hex base64 base64url latin1</code>), an int <code>n</code> (n zero bytes), a list of byte ints, or a buffer (a copy)."
     ],
     [
      "clock()",
      "float",
      "Seconds since the UNIX epoch, with fractions — use differences to measure time."
     ],
     [
      "error(message, cause?)",
      "error",
      "A new error value; <code>err.message</code>, <code>err.cause</code>."
     ],
     [
      "panic(...values)",
      "—",
      "Stops the program with a message (for impossible situations)."
     ],
     [
      "timeout(d)",
      "chan",
      "A channel that receives once after <code>d</code> (use with <code>select</code>)."
     ],
     [
      "close(ch)",
      "nil",
      "Closes a channel: receivers get <code>nil</code> once it is empty."
     ]
    ],
    "example": "print(len(\"olá\"), len([1, 2, 3]), len({ a: 1 }))\nprint(typeof(1), typeof(1.5), typeof(\"x\"), typeof(nil), typeof([]), typeof({}), typeof(2s), typeof(bytes(2)))\nprint(str(42) + \"!\", int(\"17\") + 1, int(3.9), float(\"2.5\") * 2)\nconst e = error(\"disk full\")\nprint(typeof(e), e.message)\nconst t0 = clock()\nprint(clock() - t0 < 1)",
    "output": "3 3 1\nint float str nil list map duration buffer\n42! 18 3 5.0\nerror disk full\ntrue",
    "outputTitle": "output"
   }
  ]
 },
 "values": {
  "id": "values",
  "group": "Standard library",
  "title": "Strings, lists, maps and buffers",
  "import": null,
  "intro": "Methods of the built-in values. Indexes start at 0; negative indexes count from the end in <code>slice</code>.",
  "blocks": [
   {
    "h3": "str",
    "api": [
     [
      "s.len()",
      "int",
      "Number of characters."
     ],
     [
      "s.upper() · s.lower()",
      "str",
      "Case conversion."
     ],
     [
      "s.trim()",
      "str",
      "Without leading and trailing whitespace."
     ],
     [
      "s.split(sep)",
      "list",
      "Parts between <code>sep</code>."
     ],
     [
      "s.lines() · s.chars()",
      "list",
      "Lines (without line endings) · single characters."
     ],
     [
      "s.contains(t) · s.starts_with(t) · s.ends_with(t)",
      "bool",
      "Substring tests."
     ],
     [
      "s.index_of(t)",
      "int",
      "Character index of the first <code>t</code>, or <code>-1</code>."
     ],
     [
      "s.replace(old, new)",
      "str",
      "Every <code>old</code> replaced."
     ],
     [
      "s.repeat(n)",
      "str",
      "<code>s</code> repeated <code>n</code> times."
     ],
     [
      "s.slice(start, end?)",
      "str",
      "Characters from <code>start</code> up to (not including) <code>end</code>."
     ]
    ],
    "example": "const s = \"  Fogu, fast & small  \"\nprint(s.trim().upper(), s.trim().split(\", \"))\nprint(\"a-b-c\".replace(\"-\", \"+\"), \"ab\".repeat(3), \"olá mundo\".slice(4))\nprint(\"main.fg\".ends_with(\".fg\"), \"fogu\".index_of(\"g\"), \"x\\ny\".lines(), \"abc\".chars())\nprint(`template: ${1 + 1} items`)",
    "output": "FOGU, FAST & SMALL [Fogu, fast & small]\na+b+c ababab mundo\ntrue 2 [x, y] [a, b, c]\ntemplate: 2 items",
    "outputTitle": "output"
   },
   {
    "h3": "list",
    "api": [
     [
      "l.push(x) · l.pop()",
      "nil · value",
      "Add to / remove from the end."
     ],
     [
      "l.unshift(x) · l.shift()",
      "nil · value",
      "Add to / remove from the start."
     ],
     [
      "l.insert(i, x) · l.remove(i)",
      "nil · value",
      "At an index."
     ],
     [
      "l.first() · l.last()",
      "value",
      "<code>nil</code> when empty."
     ],
     [
      "l.contains(x) · l.index_of(x)",
      "bool · int",
      "Search by value."
     ],
     [
      "l.join(sep)",
      "str",
      "Items as text joined with <code>sep</code>."
     ],
     [
      "l.reverse() · l.sort(fn?)",
      "list",
      "New list; <code>sort</code> takes an optional comparator <code>(a, b) =&gt; int</code>."
     ],
     [
      "l.slice(start, end?) · l.concat(other)",
      "list",
      "Copies."
     ],
     [
      "l.map(fn) · l.filter(fn) · l.find(fn)",
      "list · list · value",
      "Functional helpers."
     ],
     [
      "l.any(fn) · l.all(fn) · l.each(fn)",
      "bool · bool · nil",
      ""
     ],
     [
      "l.reduce(fn, initial)",
      "value",
      "<code>fn(acc, item)</code> over the items."
     ],
     [
      "l.clear() · l.len()",
      "nil · int",
      ""
     ]
    ],
    "example": "const nums = [5, 3, 8, 1]\nnums.push(10)\nprint(nums, nums.sort(), nums.sort((a, b) => b - a))\nprint(nums.map((n) => n * 2), nums.filter((n) => n > 4), nums.find((n) => n > 6))\nprint(nums.reduce((acc, n) => acc + n, 0), nums.any((n) => n > 9), nums.all((n) => n > 0))\nprint(nums.slice(1, 3), nums.first(), nums.last(), nums.join(\" | \"))",
    "output": "[5, 3, 8, 1, 10] [1, 3, 5, 8, 10] [10, 8, 5, 3, 1]\n[10, 6, 16, 2, 20] [5, 8, 10] 8\n27 true true\n[3, 8] 5 10 5 | 3 | 8 | 1 | 10",
    "outputTitle": "output"
   },
   {
    "h3": "map",
    "api": [
     [
      "m.keys() · m.values() · m.entries()",
      "list",
      "Entries are <code>[key, value]</code> pairs."
     ],
     [
      "m.has(key)",
      "bool",
      ""
     ],
     [
      "m.get(key, default?)",
      "value",
      "The value, or <code>default</code> (nil)."
     ],
     [
      "m.set(key, value) · m[key] = value",
      "nil",
      ""
     ],
     [
      "m.remove(key)",
      "value",
      "Removes and returns the value."
     ],
     [
      "m.merge(other)",
      "map",
      "A new map; <code>other</code> wins on equal keys."
     ],
     [
      "m.len() · m.clear()",
      "int · nil",
      ""
     ]
    ],
    "example": "const user = { name: \"Ana\", age: 30 }\nuser.city = \"Lisboa\"\nprint(user.keys(), user.has(\"age\"), user.get(\"email\", \"none\"))\nprint(user.merge({ age: 31, admin: true }))\nfor entry in user.entries() { print(entry[0], \"=\", entry[1]) }",
    "output": "[age, city, name] true none\n{admin: true, age: 31, city: Lisboa, name: Ana}\nage = 30\ncity = Lisboa\nname = Ana",
    "outputTitle": "output"
   },
   {
    "h3": "buffer",
    "text": "A mutable sequence of bytes, for binary data: files, network packets, protocols. Built with <code>bytes(...)</code>; <code>typeof</code> gives <code>\"buffer\"</code> and <code>buffer</code> works as a type annotation. <code>b[i]</code> reads an int 0..255 and <code>b[i] = n</code> writes one (any other value stops the program); <code>for x in b</code> and <code>for i, x in b</code> walk the bytes; <code>==</code> compares the bytes; <code>print</code> shows the first 32 bytes in hex and <code>json.stringify</code> writes an array of ints. The network modules (<code>fogu:net</code>, <code>fogu:websocket</code>, <code>res.bytes()</code> of <code>fogu:http</code>) and <code>zip image protobuf serial usb kafka</code> return binary data as buffers; their inputs also accept a str or a list of byte ints.",
    "api": [
     [
      "b.len() · b.isEmpty()",
      "int · bool",
      ""
     ],
     [
      "b.text(enc = \"utf8\") · b.toString(enc)",
      "str",
      "Decoded (<code>utf8 hex base64 base64url latin1</code>; invalid UTF-8 becomes U+FFFD)."
     ],
     [
      "b.hex() · b.base64() · b.toList()",
      "str · str · list",
      ""
     ],
     [
      "b.get(i)",
      "int · nil",
      "The byte, or <code>nil</code> out of range."
     ],
     [
      "b.push(byte...) · b.append(x...) · b.write(x...)",
      "buffer",
      "Add to the end, in place, and return the buffer (chainable); <code>append</code>/<code>write</code> take a str, a buffer, a list or byte ints."
     ],
     [
      "b.concat(x...)",
      "buffer",
      "A new buffer: <code>b</code> followed by the arguments."
     ],
     [
      "b.slice(start, end?) · b.copy() · b.clone()",
      "buffer",
      "Copies; negative indexes count from the end."
     ],
     [
      "b.indexOf(needle, from = 0) · b.includes(needle)",
      "int · bool",
      "<code>needle</code>: a byte, a str or a buffer; <code>-1</code> when missing."
     ],
     [
      "b.startsWith(x) · b.endsWith(x) · b.equals(x)",
      "bool",
      ""
     ],
     [
      "b.split(sep)",
      "list",
      "Buffers between <code>sep</code>."
     ],
     [
      "b.fill(byte, start?, end?) · b.reverse() · b.clear()",
      "buffer · buffer · nil",
      "In place."
     ],
     [
      "b.readU8 · readI8 · readU16 · readI16 · readU32 · readI32 · readI64 · readF32 · readF64 (offset, bigEndian = false)",
      "int · float",
      "A number at <code>offset</code>, little-endian unless <code>bigEndian</code>."
     ],
     [
      "b.writeU8 … b.writeF64(offset, value, bigEndian = false)",
      "",
      "Writes a number at <code>offset</code>."
     ]
    ],
    "example": "const b = bytes(\"héllo\")\nprint(typeof(b), len(b), b[0], b.text(), b.hex(), b.base64())\nprint(b)\nprint(bytes(\"cafe\", \"hex\").toList(), bytes(3), bytes([104, 105]).text())\n\nconst msg = bytes(0)\nmsg.push(1).append(\"ok\").append(bytes([0, 255]))\nprint(msg.hex(), msg.indexOf(\"ok\"), msg.slice(1, 3).text(), msg.startsWith(bytes([1])))\nprint(bytes(\"a,b,c\").split(\",\").map((p) => p.text()))\n\nconst header = bytes(8)\nheader.writeU16(0, 513)                 // little-endian by default\nheader.writeU32(2, 3000000000, true)    // big-endian\nprint(header.readU16(0), header.readU16(0, true), header.readU32(2, true), header.hex())\n\nfn checksum(data: buffer): int {\n    let sum = 0\n    for x in data { sum = (sum + x) % 256 }\n    return sum\n}\nprint(checksum(bytes(\"abc\")), bytes(\"x\") == bytes(\"x\"))",
    "output": "buffer 6 104 héllo 68c3a96c6c6f aMOpbGxv\n<buffer 6 bytes: 68 c3 a9 6c 6c 6f>\n[202, 254] <buffer 3 bytes: 00 00 00> hi\n016f6b00ff 1 ok true\n[a, b, c]\n513 258 3000000000 0102b2d05e000000\n38 true",
    "outputTitle": "output"
   }
  ]
 },
 "fogu-object": {
  "id": "fogu-object",
  "group": "Standard library",
  "title": "The Fogu object",
  "import": null,
  "intro": "Runtime helpers available everywhere, without an import.",
  "blocks": [
   {
    "api": [
     [
      "Fogu.version · Fogu.os",
      "str",
      "Fogu's version · <code>windows</code>, <code>linux</code>, <code>macos</code>…"
     ],
     [
      "Fogu.cores() · Fogu.cores(n) · Fogu.cores([cpus])",
      "int · list · error",
      "How many threads the runtime uses for networking and blocking work (default: the machine's CPUs) · use <code>n</code> · run only on those CPUs (Linux, Windows). Set it at the top of the program — once the runtime has started it is an error value. Also <code>--cores 4</code>, <code>--cores 0,2-3</code>, <code>$FOGU_CORES</code>."
     ],
     [
      "Fogu.env",
      "map",
      "Environment variables."
     ],
     [
      "Fogu.args",
      "list",
      "Arguments after the script name (<code>fogu run app.fg a b</code>)."
     ],
     [
      "Fogu.file(path)",
      "File",
      "<code>text() lines() json() write(s) append(s) exists() size() remove()</code>, fields <code>path name ext dir</code>."
     ],
     [
      "Fogu.asset(path)",
      "str",
      "A project file: relative to the project root — and inside <code>fogu build --compile</code> executables, the extracted embedded files."
     ],
     [
      "Fogu.run(cmd, args?, opts?)",
      "map",
      "Runs a program → <code>{ ok, code, stdout, stderr }</code> (see <a class=\"underline\" href=\"#mod-process\">fogu:process</a>)."
     ],
     [
      "Fogu.glob(pattern)",
      "list",
      "Files matching <code>*</code>, <code>?</code> and <code>**</code>."
     ],
     [
      "Fogu.hash(data, algo?)",
      "str",
      "Hex digest, <code>sha256</code> (default) or <code>sha512</code>."
     ],
     [
      "Fogu.sql(url)",
      "Database",
      "Same as <code>sql.open(url)</code>."
     ],
     [
      "Fogu.sleep(d)",
      "nil",
      "Waits (other tasks keep running)."
     ],
     [
      "Fogu.signal.wait(names = [\"SIGINT\", \"SIGTERM\"], { timeout })",
      "str · nil",
      "Keeps the program alive (servers — fogu:web/http requests are answered while it waits — and <code>fg</code> tasks keep running) until one of the signals or events happens; returns its name, <code>nil</code> after <code>timeout</code>."
     ],
     [
      "Fogu.signal.on(name, fn(data)) · off(name) · emit(name, data?) · last · names",
      "",
      "Handlers for OS signals (<code>SIGINT SIGTERM SIGHUP SIGQUIT SIGUSR1 SIGUSR2</code>; Windows: <code>SIGINT SIGBREAK SIGTERM</code>) or the program's own events; they run as <code>fg</code> tasks. With no handler and nobody waiting, SIGINT/SIGTERM still end the program (exit code 130/143) — after releasing sshfs mounts, browsers and SurrealKV/broker stores. <code>emit</code> also simulates a signal."
     ],
     [
      "Fogu.host.embedded · Fogu.host.emit(name, data)",
      "bool",
      "When the runtime is embedded through the C API (<a class=\"underline\" href=\"#embed\">libfogu_rt</a>): calls the host's callback; <code>false</code> without one."
     ],
     [
      "Fogu.write(...values)",
      "nil",
      "Like <code>print</code>, without spaces or newline (streaming output)."
     ],
     [
      "Fogu.watch(path | [paths], { recursive = true, debounce = 50ms })",
      "Watcher · error",
      "File system events: <code>w.next(timeout?)</code> → <code>{ kind, path, paths }</code> (<code>create modify remove rename</code>), <code>tryNext()</code>, <code>close()</code>."
     ],
     [
      "Fogu.exit(code?)",
      "—",
      "Ends the program."
     ]
    ],
    "example": "Fogu.file(\"notes.txt\").write(\"one\\ntwo\")?\nconst f = Fogu.file(\"notes.txt\")\nprint(f.name, f.ext, f.exists(), f.size(), f.lines()?)\nprint(Fogu.hash(\"abc\"))\nprint(typeof(Fogu.env), typeof(Fogu.args), Fogu.asset(\"data/logo.png\").ends_with(\"data/logo.png\"))\nFogu.write(\"no newline: \", 1, 2)\nprint()",
    "output": "notes.txt .txt true 7 [one, two]\nba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\nmap list true\nno newline: 12",
    "outputTitle": "output"
   },
   {
    "text": "Signals and the program's events (<code>emit</code> stands in for a real Ctrl+C here):",
    "example": "const seen = []\nFogu.signal.on(\"job.done\", fn(d) { seen.push(d.id) })\nFogu.signal.emit(\"job.done\", { id: 7 })\nfg fn() {\n    Fogu.sleep(10ms)\n    Fogu.signal.emit(\"SIGINT\")          // a real one comes from Ctrl+C / kill -INT\n}()\nprint(\"stopping on\", Fogu.signal.wait())\nprint(seen, Fogu.signal.wait(\"never\", { timeout: 20ms }))",
    "output": "stopping on SIGINT\n[7] nil",
    "outputTitle": "output"
   },
   {
    "text": "Watching files — rebuild on every change:",
    "title": "watch.fg",
    "example": "const w = Fogu.watch(\"src\")?\nwhile true {\n    const ev = w.next()?\n    print(ev.kind, ev.path)\n    Fogu.run(\"fogu\", [\"test\"])\n}",
    "output": "modify src/app.fg\ncreate src/new.fg\nremove src/old.fg",
    "outputTitle": "output"
   }
  ]
 },
 "mod-math": {
  "id": "mod-math",
  "group": "Core modules",
  "title": "fogu:math",
  "import": "import math from \"fogu:math\"",
  "intro": null,
  "blocks": [
   {
    "api": [
     [
      "math.PI · math.E",
      "float",
      "Constants."
     ],
     [
      "math.abs(x)",
      "number",
      "Absolute value (int stays int)."
     ],
     [
      "math.sqrt(x)",
      "float",
      "Square root."
     ],
     [
      "math.pow(base, exp)",
      "number",
      "Power."
     ],
     [
      "math.floor(x) · math.ceil(x)",
      "int",
      "Rounding down / up."
     ]
    ],
    "example": "import math from \"fogu:math\"\nprint(math.abs(-4), math.sqrt(16), math.pow(2, 10), math.floor(2.7), math.ceil(2.1))\nprint(math.PI)",
    "output": "4 4.0 1024.0 2 3\n3.141592653589793",
    "outputTitle": "output"
   }
  ]
 },
 "mod-json": {
  "id": "mod-json",
  "group": "Core modules",
  "title": "fogu:json",
  "import": "import json from \"fogu:json\"",
  "intro": null,
  "blocks": [
   {
    "api": [
     [
      "json.parse(text)",
      "value · error",
      "JSON text → maps, lists, str, int, float, bool, nil. Invalid JSON is an error value."
     ],
     [
      "json.stringify(value)",
      "str",
      "Compact JSON (map keys in order)."
     ]
    ],
    "example": "import json from \"fogu:json\"\nconst data = json.parse(\"{\\\"name\\\": \\\"Ana\\\", \\\"tags\\\": [\\\"a\\\", \\\"b\\\"], \\\"age\\\": 30}\")?\nprint(data.name, data.tags, typeof(data.age))\nprint(json.stringify({ ok: true, list: [1, 2.5, nil] }))",
    "output": "Ana [a, b] int\n{\"list\":[1,2.5,null],\"ok\":true}",
    "outputTitle": "output"
   }
  ]
 },
 "mod-time": {
  "id": "mod-time",
  "group": "Core modules",
  "title": "fogu:time",
  "import": "import time from \"fogu:time\"",
  "intro": null,
  "blocks": [
   {
    "api": [
     [
      "time.now()",
      "int",
      "Milliseconds since the UNIX epoch."
     ],
     [
      "time.monotonic()",
      "float",
      "Seconds from a fixed point; never goes backwards (the wall clock can jump): measure durations with it."
     ],
     [
      "time.Date()",
      "Date",
      "The current instant (UTC)."
     ],
     [
      "time.delay(d) · time.sleep(seconds)",
      "nil",
      "Waits; other <code>fg</code> tasks keep running."
     ]
    ]
   },
   {
    "h3": "Date",
    "api": [
     [
      "d.year · d.month · d.day",
      "int",
      "Month 1–12."
     ],
     [
      "d.hour · d.minute · d.second · d.millis · d.weekday",
      "int",
      "Weekday 0 = Sunday."
     ],
     [
      "d.getTime()",
      "int",
      "Milliseconds since the epoch (also <code>d.ms</code>)."
     ],
     [
      "d.toISOString()",
      "str",
      "<code>2026-10-10T12:00:00.000Z</code>"
     ],
     [
      "d.toString()",
      "str",
      "Human readable."
     ],
     [
      "d.addMs(ms)",
      "nil",
      "Moves the date by <code>ms</code> (in place)."
     ],
     [
      "d.diff(other)",
      "int",
      "Milliseconds between the dates."
     ],
     [
      "getFullYear() getMonth() getDate() getDay() getHours() getMinutes() getSeconds() getMilliseconds()",
      "int",
      "JavaScript-style getters (month 0–11)."
     ]
    ],
    "example": "import time from \"fogu:time\"\nconst start = time.now()\ntime.delay(20ms)\nprint(time.now() - start >= 20)\nconst d = time.Date()\nprint(d.year > 2000, d.month >= 1, d.toISOString().ends_with(\"Z\"))\nconst later = time.Date()\nlater.addMs(24 * 3600 * 1000)\nprint(later.diff(d) >= 86400000)",
    "output": "true\ntrue true true\ntrue",
    "outputTitle": "output"
   }
  ]
 },
 "mod-system": {
  "id": "mod-system",
  "group": "Core modules",
  "title": "fogu:system",
  "import": "import system from \"fogu:system\"",
  "intro": null,
  "blocks": [
   {
    "api": [
     [
      "system.args",
      "list",
      "Script arguments (<code>fogu run app.fg a b</code> → <code>[a, b]</code>)."
     ],
     [
      "system.time()",
      "float",
      "Seconds since the epoch."
     ],
     [
      "system.readLine()",
      "str · nil",
      "The next line of stdin without its ending; <code>nil</code> at the end. Other <code>fg</code> tasks keep running while it waits."
     ],
     [
      "system.exit(code)",
      "—",
      "Ends the program."
     ],
     [
      "system.info.os · .arch · .family",
      "str",
      "<code>windows</code>/<code>linux</code>/<code>macos</code> · <code>x86_64</code>/<code>aarch64</code> · <code>windows</code>/<code>unix</code>."
     ],
     [
      "system.info.cores · .pid",
      "int",
      "Logical CPUs · process id."
     ],
     [
      "system.info.env(name)",
      "str · nil",
      "An environment variable."
     ],
     [
      "system.info.sleep_ms(ms)",
      "nil",
      "Blocks the whole program (prefer <code>time.delay</code>)."
     ]
    ],
    "example": "import system from \"fogu:system\"\nconst info = system.info\nprint(typeof(info.os), info.cores > 0, typeof(info.pid), system.args)\nprint(info.env(\"FOGU_DOES_NOT_EXIST\"), system.time() > 0)",
    "output": "str true int []\nnil true",
    "outputTitle": "output"
   }
  ]
 },
 "mod-fs": {
  "id": "mod-fs",
  "group": "Core modules",
  "title": "fogu:fs",
  "import": "import fs from \"fogu:fs\"",
  "intro": "Files and folders. Failures (missing file, permission) are error values.",
  "blocks": [
   {
    "api": [
     [
      "fs.readText(path)",
      "str · error",
      "Whole file as UTF-8 text."
     ],
     [
      "fs.readLines(path)",
      "list · error",
      "Lines without line endings."
     ],
     [
      "fs.writeText(path, text)",
      "nil · error",
      "Creates or replaces."
     ],
     [
      "fs.append(path, text)",
      "nil · error",
      "Adds to the end (creates the file)."
     ],
     [
      "fs.exists(path) · fs.isFile(path) · fs.isDir(path)",
      "bool",
      ""
     ],
     [
      "fs.mkdir(path)",
      "nil · error",
      "Creates the folder and its parents."
     ],
     [
      "fs.list(path)",
      "list · error",
      "Names inside a folder, sorted."
     ],
     [
      "fs.stat(path)",
      "map · error",
      "<code>{ size, isFile, isDir, modified }</code> (modified: ms since the epoch)."
     ],
     [
      "fs.copy(from, to) · fs.move(from, to)",
      "nil · error",
      ""
     ],
     [
      "fs.remove(path)",
      "nil · error",
      "A file or an empty folder."
     ],
     [
      "fs.removeAll(path)",
      "nil · error",
      "A folder and everything inside."
     ],
     [
      "fs.glob(pattern)",
      "list",
      "Paths matching <code>*</code>, <code>?</code>, <code>**</code>, sorted."
     ],
     [
      "fs.cwd() · fs.tempDir()",
      "str",
      "Working folder · system temp folder."
     ],
     [
      "fs.remote(ssh | \"user@host\", { sudo })",
      "RemoteFs",
      "The same functions on a server through <a href=\"#mod-ssh\">fogu:ssh</a> (plus <code>upload(local, remote)</code> and <code>download(remote, local)</code>); every call is an SSH command."
     ]
    ],
    "example": "import fs from \"fogu:fs\"\nfs.mkdir(\"data/logs\")?\nfs.writeText(\"data/a.txt\", \"hello\")?\nfs.append(\"data/a.txt\", \"\\nworld\")?\nprint(fs.readLines(\"data/a.txt\")?, fs.stat(\"data/a.txt\")?.size)\nfs.copy(\"data/a.txt\", \"data/logs/b.txt\")?\nprint(fs.list(\"data\")?, fs.glob(\"data/**/*.txt\"))\nconst [_, err] = fs.readText(\"missing.txt\")\nprint(err != nil)\nfs.removeAll(\"data\")?\nprint(fs.exists(\"data\"))",
    "output": "[hello, world] 11\n[a.txt, logs] [data/a.txt, data/logs/b.txt]\ntrue\nfalse",
    "outputTitle": "output"
   }
  ]
 },
 "mod-path": {
  "id": "mod-path",
  "group": "Core modules",
  "title": "fogu:path",
  "import": "import path from \"fogu:path\"",
  "intro": null,
  "blocks": [
   {
    "api": [
     [
      "path.join(...parts)",
      "str",
      "Joined with <code>/</code> and normalized."
     ],
     [
      "path.dirname(p) · path.basename(p)",
      "str",
      "Folder · last component."
     ],
     [
      "path.stem(p) · path.extname(p)",
      "str",
      "Name without extension · extension with the dot."
     ],
     [
      "path.normalize(p)",
      "str",
      "Resolves <code>.</code> and <code>..</code>."
     ],
     [
      "path.isAbsolute(p)",
      "bool",
      ""
     ],
     [
      "path.absolute(p)",
      "str",
      "Absolute path from the working folder."
     ],
     [
      "path.sep",
      "str",
      "The system separator."
     ]
    ],
    "example": "import path from \"fogu:path\"\nprint(path.join(\"src\", \"lib\", \"../main.fg\"))\nprint(path.dirname(\"a/b/c.txt\"), path.basename(\"a/b/c.txt\"), path.stem(\"a/b/c.tar.gz\"), path.extname(\"a/b/c.txt\"))\nprint(path.normalize(\"a/./b/../c\"), path.isAbsolute(\"/etc\"), path.isAbsolute(\"etc\"))",
    "output": "src/main.fg\na/b c.txt c.tar .txt\na/c true false",
    "outputTitle": "output"
   }
  ]
 },
 "mod-process": {
  "id": "mod-process",
  "group": "Core modules",
  "title": "fogu:process",
  "import": "import process from \"fogu:process\"",
  "intro": "A program that exits with a non-zero code is <em>not</em> an error: check <code>ok</code> and <code>code</code>. Not being able to start it is an error value. For pipes and Linux commands see <a class=\"underline\" href=\"#mod-shell\">fogu:shell</a>.",
  "blocks": [
   {
    "api": [
     [
      "process.run(cmd, args?, { cwd, env, input })",
      "map · error",
      "<code>{ ok, code, stdout, stderr }</code>."
     ],
     [
      "process.shell(command)",
      "map",
      "Through <code>sh -c</code> / <code>cmd /C</code>."
     ],
     [
      "process.env(name) · process.setEnv(name, value)",
      "str · nil",
      ""
     ],
     [
      "process.cwd() · process.chdir(path)",
      "str · nil",
      ""
     ],
     [
      "process.pid · process.os · process.arch · process.args",
      "",
      ""
     ],
     [
      "process.exit(code)",
      "—",
      ""
     ]
    ],
    "example": "import process from \"fogu:process\"\nconst r = process.run(\"git\", [\"--version\"])?\nprint(r.ok, r.code, r.stdout.starts_with(\"git version\"))\nconst [_, err] = process.run(\"no-such-program-xyz\")\nprint(err != nil)",
    "output": "true 0 true\ntrue",
    "outputTitle": "output"
   }
  ]
 },
 "mod-sync": {
  "id": "mod-sync",
  "group": "Core modules",
  "title": "fogu:sync",
  "import": "import sync from \"fogu:sync\"",
  "intro": "Coordination of <code>fg</code> tasks. Waiting operations yield to the other tasks; waiting forever is reported as a deadlock.",
  "blocks": [
   {
    "api": [
     [
      "sync.Mutex()",
      "Mutex",
      "<code>lock()</code> <code>unlock()</code> <code>tryLock()</code> → bool, field <code>locked</code>."
     ],
     [
      "sync.WaitGroup()",
      "WaitGroup",
      "<code>add(n = 1)</code> <code>done()</code> <code>wait()</code>, field <code>count</code>."
     ],
     [
      "sync.Once()",
      "Once",
      "<code>do(fn)</code> runs <code>fn</code> only the first time."
     ],
     [
      "sync.Atomic(value)",
      "Atomic",
      "<code>get()</code> <code>set(v)</code> <code>add(n = 1)</code> → new value, <code>compareAndSwap(old, new)</code> → bool."
     ],
     [
      "sync.Semaphore(n)",
      "Semaphore",
      "<code>acquire()</code> (waits for a permit) <code>tryAcquire()</code> → bool <code>release()</code>, fields <code>available size</code>. Also limits <a class=\"underline\" href=\"#mod-vm\">fogu:vm</a> tasks (<code>{ semaphore }</code>)."
     ]
    ],
    "example": "import sync from \"fogu:sync\"\nconst wg = sync.WaitGroup()\nconst total = sync.Atomic(0)\nconst once = sync.Once()\nfor i in 1..5 {\n    wg.add(1)\n    fg fn() {\n        once.do(() => print(\"first task\"))\n        total.add(i)\n        wg.done()\n    }()\n}\nwg.wait()\nprint(total.get())\n\n// at most two tasks inside at a time\nconst sem = sync.Semaphore(2)\nlet inside = 0\nlet peak = 0\nconst done = sync.WaitGroup()\nfor i in 0..5 {\n    done.add(1)\n    fg fn() {\n        sem.acquire()\n        inside += 1\n        if inside > peak { peak = inside }\n        Fogu.sleep(5ms)\n        inside -= 1\n        sem.release()\n        done.done()\n    }()\n}\ndone.wait()\nprint(\"peak\", peak, \"free\", sem.available)",
    "output": "first task\n10\npeak 2 free 2",
    "outputTitle": "output"
   }
  ]
 },
 "mod-test": {
  "id": "mod-test",
  "group": "Core modules",
  "title": "fogu:test",
  "import": "import { test, skip, expect } from \"fogu:test\"",
  "intro": "Used in <code>*.test.fg</code> files, run with <code>fogu test</code>. A test fails when an expectation fails, when it raises an error or returns an error value.",
  "blocks": [
   {
    "title": "math.test.fg",
    "api": [
     [
      "test(name, fn)",
      "nil",
      "Declares a test."
     ],
     [
      "skip(name, fn)",
      "nil",
      "A test that is reported but not run."
     ],
     [
      "expect(value)",
      "Expectation",
      "Then a matcher; <code>.not</code> negates it."
     ],
     [
      "toBe(x) · toEqual(x)",
      "",
      "Equality (<code>toEqual</code> compares lists and maps deeply)."
     ],
     [
      "toBeTruthy() · toBeFalsy() · toBeNil() · toBeError()",
      "",
      ""
     ],
     [
      "toBeType(\"int\") · toContain(x) · toHaveLength(n)",
      "",
      ""
     ],
     [
      "toBeGreaterThan(n) · toBeLessThan(n) · toBeCloseTo(x, eps?)",
      "",
      ""
     ]
    ],
    "example": "import { test, expect } from \"fogu:test\"\n\ntest(\"math\", () => {\n    expect(1 + 2).toBe(3)\n    expect([1, 2]).toContain(2)\n    expect(\"abc\").not.toHaveLength(2)\n    expect(0.1 + 0.2).toBeCloseTo(0.3)\n})",
    "output": "$ fogu test\nmath.test.fg\n  ✓ math (0ms)\n\n1 passed — 1 file(s) [1ms]",
    "outputTitle": "fogu test"
   }
  ]
 },
 "mod-log": {
  "id": "mod-log",
  "group": "Core modules",
  "title": "fogu:log",
  "import": "import log from \"fogu:log\"",
  "intro": "Structured logs to stderr (default), stdout or a file. The level comes from <code>FOGU_LOG</code> (default <code>info</code>).",
  "blocks": [
   {
    "api": [
     [
      "log.trace/debug/info/warn/error(message, fields?)",
      "nil",
      "<code>fields</code>: a map added as <code>key=value</code> (or JSON keys)."
     ],
     [
      "log.setLevel(level)",
      "nil",
      "<code>trace debug info warn error off</code>."
     ],
     [
      "log.level()",
      "str",
      "Current level."
     ],
     [
      "log.setFormat(\"text\" | \"json\")",
      "nil",
      "One line per entry."
     ],
     [
      "log.setOutput(\"stderr\" | \"stdout\" | path)",
      "nil · error",
      "A file is appended to."
     ],
     [
      "log.setTime(bool)",
      "nil",
      "Show the time stamp (default true)."
     ],
     [
      "log.child(fields)",
      "Logger",
      "A logger that adds <code>fields</code> to every line; same methods + <code>child</code>."
     ]
    ],
    "example": "import log from \"fogu:log\"\nlog.setOutput(\"stdout\")\nlog.setTime(false)\nlog.info(\"server started\", { port: 3000 })\nlog.debug(\"not shown at the info level\")\nconst db = log.child({ module: \"db\" })\ndb.warn(\"slow query\", { ms: 812, sql: \"select *\" })\nlog.setFormat(\"json\")\nlog.error(\"failed\", { code: 23 })",
    "output": "INFO  server started port=3000\nWARN  slow query module=db ms=812 sql=\"select *\"\n{\"level\":\"error\",\"msg\":\"failed\",\"code\":23}",
    "outputTitle": "output"
   }
  ]
 },
 "mod-regex": {
  "id": "mod-regex",
  "group": "Core modules",
  "title": "fogu:regex",
  "import": "import regex from \"fogu:regex\"",
  "intro": "Rust <code>regex</code> syntax (linear time, no backtracking). Positions are character indexes. In Fogu strings write <code>\\\\d</code> for <code>\\d</code>. An invalid pattern is an error value.",
  "blocks": [
   {
    "api": [
     [
      "regex.compile(pattern, { ignoreCase, multiline, dotAll, verbose })",
      "Regex · error",
      ""
     ],
     [
      "re.test(text)",
      "bool",
      "Matches somewhere."
     ],
     [
      "re.matches(text)",
      "bool",
      "The whole text matches."
     ],
     [
      "re.find(text)",
      "map · nil",
      "<code>{ text, start, end, groups, named }</code>."
     ],
     [
      "re.findAll(text)",
      "list",
      "Every match."
     ],
     [
      "re.replace(text, repl) · re.replaceAll(text, repl)",
      "str",
      "<code>$1</code>, <code>${name}</code>, <code>$$</code> in <code>repl</code>."
     ],
     [
      "re.split(text, limit?)",
      "list",
      ""
     ],
     [
      "re.source · re.groups · re.names",
      "",
      "Pattern · group count · group names."
     ],
     [
      "regex.test/find/findAll/replace/replaceAll/split/matches(text, pattern, …)",
      "",
      "One-off shortcuts."
     ],
     [
      "regex.escape(text)",
      "str",
      "A literal pattern."
     ]
    ],
    "example": "import regex from \"fogu:regex\"\nconst email = regex.compile(\"(?P<user>[\\\\w.]+)@(?P<host>[\\\\w.]+)\")?\nprint(email.test(\"write to ana@fogu.dev\"))\nconst m = email.find(\"contact: ana@fogu.dev\")\nprint(m.text, m.start, m.named.host, m.groups)\nprint(email.replaceAll(\"a@x.io, b@y.io\", \"${user} (at ${host})\"))\nprint(regex.split(\"one, two;three\", \"\\\\s*[,;]\\\\s*\"))\nprint(regex.findAll(\"v1.2 and v3.10\", \"\\\\d+\\\\.\\\\d+\").map((x) => x.text))",
    "output": "true\nana@fogu.dev 9 fogu.dev [ana, fogu.dev]\na (at x.io), b (at y.io)\n[one, two, three]\n[1.2, 3.10]",
    "outputTitle": "output"
   }
  ]
 },
 "mod-crypto": {
  "id": "mod-crypto",
  "group": "Core modules",
  "title": "fogu:crypto",
  "import": "import crypto from \"fogu:crypto\"",
  "intro": null,
  "blocks": [
   {
    "api": [
     [
      "crypto.hash(algo, data, { base64 })",
      "str",
      "<code>md5 sha1 sha256 sha384 sha512</code>; hex by default."
     ],
     [
      "crypto.hmac(algo, key, data, { base64 })",
      "str",
      ""
     ],
     [
      "crypto.encrypt(data, key)",
      "str",
      "AES-256-GCM; <code>key</code> is a password (argon2id) or 64 hex chars. Base64 output with salt and nonce."
     ],
     [
      "crypto.decrypt(box, key, { bytes })",
      "str · error",
      "Wrong key or altered data → error value."
     ],
     [
      "crypto.hashPassword(password)",
      "str",
      "argon2id PHC string, for storing passwords."
     ],
     [
      "crypto.verifyPassword(password, hash)",
      "bool",
      ""
     ],
     [
      "crypto.randomBytes(n) · crypto.randomHex(n)",
      "list · str",
      "Secure random."
     ],
     [
      "crypto.randomInt(min, max)",
      "int",
      "Inclusive, unbiased."
     ],
     [
      "crypto.uuid()",
      "str",
      "Random UUID v4."
     ],
     [
      "crypto.base64(data, { url }) · crypto.fromBase64(text, { bytes })",
      "str",
      "<code>data</code>: str or list of bytes."
     ],
     [
      "crypto.hex(data) · crypto.fromHex(text, { bytes })",
      "str",
      ""
     ],
     [
      "crypto.equal(a, b)",
      "bool",
      "Constant-time comparison."
     ]
    ],
    "example": "import crypto from \"fogu:crypto\"\nprint(crypto.hash(\"sha256\", \"abc\"))\nprint(crypto.hmac(\"sha256\", \"key\", \"The quick brown fox jumps over the lazy dog\"))\nconst box = crypto.encrypt(\"card 4242\", \"my secret\")\nprint(crypto.decrypt(box, \"my secret\")?)\nconst [_, err] = crypto.decrypt(box, \"wrong\")\nprint(err)\nconst h = crypto.hashPassword(\"hunter2\")\nprint(crypto.verifyPassword(\"hunter2\", h), crypto.verifyPassword(\"hunter3\", h))\nprint(crypto.base64(\"olá\"), crypto.fromBase64(\"b2zDoQ==\"), crypto.hex(\"hi\"), len(crypto.uuid()))",
    "output": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\nf7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8\ncard 4242\nerror: crypto.decrypt: wrong key or corrupted data\ntrue false\nb2zDoQ== olá 6869 36",
    "outputTitle": "output"
   }
  ]
 },
 "mod-csv": {
  "id": "mod-csv",
  "group": "Data formats",
  "title": "fogu:csv",
  "import": "import csv from \"fogu:csv\"",
  "intro": "Rows with a header become maps; cells are typed (<code>42</code> → int, <code>3.5</code> → float, <code>true</code> → bool, empty → nil; codes like <code>007</code> stay text). The delimiter is guessed (<code>, ; tab |</code>).",
  "blocks": [
   {
    "api": [
     [
      "csv.parse(text, { header = true, delimiter, types = true, trim })",
      "list · error",
      "Maps (or lists without header)."
     ],
     [
      "csv.stringify(rows, { header = true, delimiter = \",\", columns })",
      "str",
      "Rows: maps (a header line is written) or lists."
     ],
     [
      "csv.read(path, options?) · csv.write(path, rows, options?)",
      "list · nil",
      "The same, from/to a file."
     ]
    ],
    "example": "import csv from \"fogu:csv\"\nconst people = csv.parse(\"name;age;code;city\\nAna;30;007;Lisboa\\nBob;25.5;12;\\n\")?\nprint(people[0])\nprint(people[1].age, people[1].city)\nprint(csv.stringify([{ name: \"Ana\", note: \"says \\\"hi\\\", ok\" }, { name: \"Bob\", age: 3 }]))\ncsv.write(\"out.csv\", people, { columns: [\"name\", \"age\"] })?\nprint(csv.read(\"out.csv\")?)",
    "output": "{age: 30, city: Lisboa, code: 007, name: Ana}\n25.5 nil\nname,note,age\nAna,\"says \"\"hi\"\", ok\",\nBob,,3\n\n[{age: 30, name: Ana}, {age: 25.5, name: Bob}]",
    "outputTitle": "output"
   }
  ]
 },
 "mod-excel": {
  "id": "mod-excel",
  "group": "Data formats",
  "title": "fogu:excel",
  "import": "import excel from \"fogu:excel\"",
  "intro": "Reads <code>.xlsx .xlsm .xls .xlsb .ods</code>; writes <code>.xlsx</code> (bold, frozen header row, column widths fitted). Dates become ISO text.",
  "blocks": [
   {
    "api": [
     [
      "excel.read(path, { sheet, header = true })",
      "list · map · error",
      "The first sheet (or <code>sheet</code>: name or index) as rows; <code>sheet: \"*\"</code> → <code>{ name: rows }</code>."
     ],
     [
      "excel.sheets(path)",
      "list · error",
      "Sheet names."
     ],
     [
      "excel.write(path, rows | { Sheet: rows }, { header = true, columns })",
      "nil · error",
      "One sheet per map key."
     ]
    ],
    "example": "import excel from \"fogu:excel\"\nconst products = [\n    { id: 1, name: \"Café\", price: 4.5, stock: true },\n    { id: 2, name: \"Pão\", price: 0.8, stock: false },\n]\nexcel.write(\"shop.xlsx\", { Products: products, Notes: [[\"checked\", true]] })?\nprint(excel.sheets(\"shop.xlsx\")?)\nprint(excel.read(\"shop.xlsx\")?)\nprint(excel.read(\"shop.xlsx\", { sheet: \"Notes\", header: false })?)",
    "output": "[Notes, Products]\n[]\n[[checked, true]]",
    "outputTitle": "output"
   }
  ]
 },
 "mod-yaml": {
  "id": "mod-yaml",
  "group": "Data formats",
  "title": "fogu:yaml",
  "import": "import yaml from \"fogu:yaml\"",
  "intro": null,
  "blocks": [
   {
    "api": [
     [
      "yaml.parse(text)",
      "value · error",
      ""
     ],
     [
      "yaml.parseAll(text)",
      "list · error",
      "Every document of a <code>---</code> stream."
     ],
     [
      "yaml.stringify(value)",
      "str",
      ""
     ],
     [
      "yaml.read(path) · yaml.write(path, value)",
      "value · nil",
      ""
     ]
    ],
    "example": "import yaml from \"fogu:yaml\"\nconst cfg = yaml.parse(\"server:\\n  host: 0.0.0.0\\n  ports: [80, 443]\\ndebug: false\\n\")?\nprint(cfg.server.ports, cfg.debug)\nprint(yaml.stringify({ app: \"fogu\", tags: [\"web\", \"cli\"] }))",
    "output": "[80, 443] false\napp: fogu\ntags:\n- web\n- cli",
    "outputTitle": "output"
   }
  ]
 },
 "mod-toml": {
  "id": "mod-toml",
  "group": "Data formats",
  "title": "fogu:toml",
  "import": "import toml from \"fogu:toml\"",
  "intro": "Dates become text. TOML has no null: <code>nil</code> values are left out when writing; the top level must be a map.",
  "blocks": [
   {
    "api": [
     [
      "toml.parse(text)",
      "map · error",
      ""
     ],
     [
      "toml.stringify(map)",
      "str",
      ""
     ],
     [
      "toml.read(path) · toml.write(path, map)",
      "map · nil",
      ""
     ]
    ],
    "example": "import toml from \"fogu:toml\"\nconst t = toml.parse(\"title = \\\"app\\\"\\n[db]\\nurl = \\\"sqlite://x.db\\\"\\npool = 5\\n[[users]]\\nname = \\\"ana\\\"\\n\")?\nprint(t.title, t.db.pool, t.users[0].name)\nprint(toml.stringify({ name: \"fogu\", version: 5, server: { port: 8080 } }))",
    "output": "app 5 ana\nname = \"fogu\"\nversion = 5\n\n[server]\nport = 8080",
    "outputTitle": "output"
   }
  ]
 },
 "mod-xml": {
  "id": "mod-xml",
  "group": "Data formats",
  "title": "fogu:xml",
  "import": "import xml from \"fogu:xml\"",
  "intro": "An element is <code>{ name, attrs, children, text }</code>; <code>children</code> holds elements (maps) and text (str); <code>text</code> joins the element's own text.",
  "blocks": [
   {
    "api": [
     [
      "xml.parse(text, { whitespace })",
      "map · error",
      "The root element."
     ],
     [
      "xml.stringify(element, { pretty = true, declaration = true })",
      "str",
      ""
     ],
     [
      "xml.read(path) · xml.write(path, element, options?)",
      "map · nil",
      ""
     ]
    ],
    "example": "import xml from \"fogu:xml\"\nconst feed = xml.parse(\"<feed lang=\\\"pt\\\"><item id=\\\"1\\\">Olá &amp; adeus</item><item id=\\\"2\\\"/></feed>\")?\nprint(feed.name, feed.attrs.lang, feed.children.map((c) => c.attrs.id), feed.children[0].text)\nprint(xml.stringify({ name: \"note\", attrs: { to: \"Ana\" }, children: [{ name: \"body\", text: \"1 < 2\" }] }))",
    "output": "feed pt [1, 2] Olá & adeus\n<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<note to=\"Ana\">\n  <body>1 &lt; 2</body>\n</note>",
    "outputTitle": "output"
   }
  ]
 },
 "mod-kv": {
  "id": "mod-kv",
  "group": "Storage",
  "title": "fogu:kv",
  "import": "import kv from \"fogu:kv\"",
  "intro": "An embedded key-value store in a single file (redb, ACID). Values are any Fogu value (stored as JSON).",
  "blocks": [
   {
    "api": [
     [
      "kv.open(path)",
      "Store · error",
      "Creates the file if needed."
     ],
     [
      "store.set(key, value) · store.get(key)",
      "nil · value",
      "<code>get</code> → nil when missing."
     ],
     [
      "store.has(key) · store.delete(key)",
      "bool",
      ""
     ],
     [
      "store.keys() · store.len() · store.clear()",
      "list · int · nil",
      ""
     ],
     [
      "store.path · store.close()",
      "str · nil",
      ""
     ]
    ],
    "example": "import kv from \"fogu:kv\"\nconst db = kv.open(\"app.db\")?\ndb.set(\"user:1\", { name: \"Ana\", roles: [\"admin\"] })\ndb.set(\"visits\", 41)\ndb.set(\"visits\", db.get(\"visits\") + 1)\nprint(db.get(\"user:1\").name, db.get(\"visits\"), db.has(\"nope\"), db.keys(), db.len())\ndb.close()",
    "output": "Ana 42 false [user:1, visits] 2",
    "outputTitle": "output"
   }
  ]
 },
 "mod-sql": {
  "id": "mod-sql",
  "group": "Storage",
  "title": "fogu:sql",
  "import": "import sql from \"fogu:sql\"",
  "intro": "SQLite, PostgreSQL and MySQL/MariaDB by URL: <code>sqlite://app.db</code>, <code>sqlite::memory:</code>, <code>postgres://user:pass@host/db</code>, <code>mysql://user:pass@host/db</code>. Parameters: <code>?</code> (SQLite/MySQL) or <code>$1</code> (PostgreSQL).",
  "blocks": [
   {
    "api": [
     [
      "sql.open(url)",
      "Database · error",
      ""
     ],
     [
      "db.run(sql, params?)",
      "int · error",
      "One statement with parameters; returns the affected rows (alias <code>execute</code>)."
     ],
     [
      "db.query(sql, params?)",
      "list · error",
      "Rows as maps."
     ],
     [
      "db.one(sql, params?)",
      "map · nil · error",
      "The first row."
     ],
     [
      "db.exec(sql)",
      "nil · error",
      "Raw SQL without parameters — may hold several statements (migrations)."
     ],
     [
      "db.begin() · db.commit() · db.rollback()",
      "nil · error",
      "Transactions."
     ],
     [
      "db.driver · db.url · db.close()",
      "",
      ""
     ]
    ],
    "example": "import sql from \"fogu:sql\"\nconst db = sql.open(\"sqlite::memory:\")?\ndb.exec(\"CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, age INT)\")?\nconst n = db.run(\"INSERT INTO users (name, age) VALUES (?, ?)\", [\"Ana\", 30])?\ndb.run(\"INSERT INTO users (name, age) VALUES (?, ?)\", [\"Bob\", 25])?\nprint(\"inserted:\", n)\nprint(db.query(\"SELECT * FROM users WHERE age > ? ORDER BY name\", [20])?)\nprint(db.one(\"SELECT COUNT(*) AS n FROM users\")?.n, db.driver)\nconst [_, err] = db.query(\"SELECT * FROM nope\")\nprint(err != nil)",
    "output": "inserted: 1\n[{age: 30, id: 1, name: Ana}, {age: 25, id: 2, name: Bob}]\n2 sqlite\ntrue",
    "outputTitle": "output"
   }
  ]
 },
 "mod-orm": {
  "id": "mod-orm",
  "group": "Storage",
  "title": "fogu:orm",
  "import": "import { create_client } from \"fogu:orm\"",
  "intro": "A Prisma-style ORM for SQLite, PostgreSQL and MySQL. Models are described in a map; <code>migrate: \"auto\"</code> creates tables (with foreign keys) and adds new columns. Each model is reached as <code>db.&lt;model in camelCase&gt;</code>. Relations load with one query per level.",
  "blocks": [
   {
    "h3": "Client and schema",
    "api": [
     [
      "create_client({ schema, migrate, log })",
      "Client · error",
      "<code>schema: { provider, url, models }</code>; <code>log: true</code> prints every SQL statement."
     ],
     [
      "field: { type, primary, autoincrement, required, unique, defaultValue }",
      "",
      "Types <code>Int BigInt String Bool Float DateTime Json</code>; defaults <code>\"now\"</code>, <code>\"uuid\"</code> or a value."
     ],
     [
      "relation: { model, kind, foreignKey, references }",
      "",
      "<code>one-to-many</code>, <code>many-to-one</code>, <code>one-to-one</code>."
     ],
     [
      "db.query(sql, params?) · db.execute(sql, params?)",
      "list · int",
      "Raw SQL."
     ],
     [
      "db.begin() · db.commit() · db.rollback() · db.migrate() · db.disconnect()",
      "",
      ""
     ]
    ]
   },
   {
    "h3": "Model methods",
    "text": "<code>where</code>: <code>{ field: value }</code>, <code>{ field: { equals, not, in, notIn, lt, lte, gt, gte, contains, startsWith, endsWith, mode: \"insensitive\" } }</code>, <code>AND</code>/<code>OR</code>/<code>NOT</code>, and <code>nil</code> for IS NULL. <code>orderBy</code>: <code>{ field: \"asc\" | \"desc\" }</code> or a list of them.",
    "api": [
     [
      "create({ data, include, select })",
      "map",
      "Missing required fields stop the program; a duplicate is an error value."
     ],
     [
      "createMany({ data: [...] })",
      "{ count }",
      ""
     ],
     [
      "findMany({ where, select, include, orderBy, take, skip })",
      "list",
      ""
     ],
     [
      "findFirst(opts) · findUnique(opts)",
      "map · nil",
      ""
     ],
     [
      "update({ where, data })",
      "map · error",
      "<code>data</code> values may be <code>{ increment, decrement, multiply, divide, set }</code>."
     ],
     [
      "updateMany({ where, data }) · deleteMany({ where })",
      "{ count }",
      ""
     ],
     [
      "upsert({ where, create, update })",
      "map",
      ""
     ],
     [
      "delete({ where })",
      "map · error",
      "The deleted record."
     ],
     [
      "count({ where })",
      "int",
      ""
     ]
    ],
    "example": "import { create_client } from \"fogu:orm\"\n\nconst db = create_client({\n    schema: {\n        provider: \"sqlite\",\n        url: \"sqlite::memory:\",\n        models: {\n            User: {\n                fields: {\n                    id:    { type: \"Int\", primary: true, autoincrement: true },\n                    email: { type: \"String\", required: true, unique: true },\n                    role:  { type: \"String\", defaultValue: \"user\" },\n                },\n                relations: { posts: { model: \"Post\", kind: \"one-to-many\", foreignKey: \"authorId\" } }\n            },\n            Post: {\n                fields: {\n                    id:       { type: \"Int\", primary: true, autoincrement: true },\n                    title:    { type: \"String\", required: true },\n                    authorId: { type: \"Int\", required: true },\n                },\n                relations: { author: { model: \"User\", kind: \"many-to-one\", foreignKey: \"authorId\" } }\n            }\n        }\n    },\n    migrate: \"auto\",\n})?\n\nconst ana = db.user.create({ data: { email: \"ana@x.com\" } })?\ndb.post.createMany({ data: [{ title: \"Hello\", authorId: ana.id }, { title: \"Fogu\", authorId: ana.id }] })?\nprint(ana)\nconst users = db.user.findMany({ include: { posts: { select: { title: true } } } })?\nprint(users[0].posts)\nprint(db.post.findFirst({ where: { title: { startsWith: \"Fo\" } }, include: { author: true } })?.author.email)\nprint(db.user.update({ where: { id: ana.id }, data: { role: \"admin\" } })?.role, db.post.count()?)",
    "output": "{email: ana@x.com, id: 1, role: user}\n[{title: Hello}, {title: Fogu}]\nana@x.com\nadmin 2",
    "outputTitle": "output"
   }
  ]
 },
 "mod-surrealkv": {
  "id": "mod-surrealkv",
  "group": "Storage",
  "title": "fogu:surrealkv",
  "import": "import surrealkv from \"fogu:surrealkv\"",
  "intro": "SurrealKV: an embedded, versioned LSM key-value store in a folder, with MVCC transactions (snapshot isolation, write conflicts as error values), the history of every key, checkpoints and encryption at rest (AES-256-GCM). Keys are strings, values any Fogu value (stored as JSON); times are ms since the epoch, like <code>time.now()</code>. Stores left open are flushed when the program exits.",
  "blocks": [
   {
    "api": [
     [
      "surrealkv.open(dir, { versions, retention, sync, key, keyFile, vlog })",
      "SurrealKV · error",
      "<code>versions</code> keeps every version (for <code>retention</code>, a duration; 0 = forever); <code>sync</code> waits for fsync on each commit; <code>key</code> (64 hex digits) or <code>keyFile</code> encrypt the store. A folder already open is an error."
     ],
     [
      "db.get(key) · db.has(key) · db.set(key, value, { at })",
      "value · nil · bool",
      "<code>at</code> writes a version for a past time (versioned stores)."
     ],
     [
      "db.delete(key) · db.purge(key) · db.deleteRange(start, end) · db.deletePrefix(prefix)",
      "nil · error",
      "In a versioned store <code>delete</code> leaves a tombstone in the history; <code>purge</code> erases every version."
     ],
     [
      "db.scan({ prefix, start, end, limit, reverse, keys })",
      "list",
      "<code>[{ key, value }]</code> in key order (<code>end</code> excluded); <code>keys: true</code> → only the keys."
     ],
     [
      "db.keys(prefix?) · db.count(prefix?)",
      "list · int",
      ""
     ],
     [
      "db.getAt(key, at) · db.history(key, { limit, deleted })",
      "value · list",
      "Versioned stores. <code>history</code> → <code>[{ value, at, deleted }]</code>, newest first."
     ],
     [
      "db.batch({ set: { key: value }, delete: [keys] })",
      "nil · error",
      "One atomic commit."
     ],
     [
      "db.begin({ readOnly }) → tx · tx.get/set/delete/scan/keys/count/getAt/history · tx.commit() · tx.rollback()",
      "Txn",
      "Reads see a snapshot plus the transaction's own writes; <code>commit</code> → error value on a write conflict."
     ],
     [
      "db.checkpoint(dir) · db.restore(dir) · db.flush() · db.path · db.closed · db.close()",
      "",
      ""
     ]
    ],
    "example": "import surrealkv from \"fogu:surrealkv\"\nconst db = surrealkv.open(\"shop.db\", { versions: true })?\ndb.set(\"user:1\", { name: \"Ana\" })?\ndb.set(\"user:2\", { name: \"Bia\" })?\nprint(db.scan({ prefix: \"user:\" }))\ndb.set(\"price\", 10, { at: 1000 })?\ndb.set(\"price\", 12, { at: 2000 })?\nprint(db.getAt(\"price\", 1500), db.get(\"price\"), db.history(\"price\").len())\n\nconst t1 = db.begin()\nconst t2 = db.begin()\nt1.set(\"stock\", 5)\nt2.set(\"stock\", 7)\nt1.commit()?\nconst [_, err] = t2.commit()\nprint(err != nil, db.get(\"stock\"))\ndb.close()",
    "output": "[{key: user:1, value: {name: Ana}}, {key: user:2, value: {name: Bia}}]\n10 12 2\ntrue 5",
    "outputTitle": "output"
   }
  ]
 },
 "mod-redis": {
  "id": "mod-redis",
  "group": "Storage",
  "title": "fogu:redis",
  "import": "import redis from \"fogu:redis\"",
  "intro": "Redis, Valkey and KeyDB. <strong>Every Redis command is a method</strong> (<code>r.lpush(…)</code>, <code>r.xadd(…)</code>) or goes through <code>r.command(\"CMD\", …)</code>; maps become <code>field value</code> pairs. Redis errors (<code>WRONGTYPE</code>, <code>NOAUTH</code>) are error values.",
  "blocks": [
   {
    "api": [
     [
      "redis.connect(url | { host, port, username, password, db, tls, timeout, name })",
      "Redis · error",
      "<code>redis://[user:pass@]host[:port][/db]</code>, <code>rediss://</code> for TLS, default <code>$REDIS_URL</code>."
     ],
     [
      "r.set(key, value, { ex, px, exat, nx, xx, keepTtl, get })",
      "bool · value",
      "<code>false</code> when <code>nx</code>/<code>xx</code> prevented it."
     ],
     [
      "r.get(key) · r.del(keys) · r.incr(key) · r.expire(key, s) · r.ttl(key)",
      "",
      ""
     ],
     [
      "r.hset(key, map) · r.hget(key, f) · r.hgetall(key)",
      "int · str · map",
      ""
     ],
     [
      "r.zrange(key, start, stop, { withScores, rev, byScore })",
      "list",
      "With scores: <code>[{ member, score }]</code>."
     ],
     [
      "r.setJson(key, value, opts?) · r.getJson(key)",
      "bool · value",
      "JSON values."
     ],
     [
      "r.pipeline([[cmd, …args], …])",
      "list",
      "One round trip; a failed command is an error value in its place."
     ],
     [
      "r.transaction([...])",
      "list · error",
      "<code>MULTI … EXEC</code>."
     ],
     [
      "r.subscribe(...channels) · r.psubscribe(...patterns)",
      "Subscription",
      "Its own connection: <code>next(timeout?)</code> → <code>{ channel, message, pattern? }</code>, <code>tryNext()</code>, <code>close()</code>."
     ],
     [
      "r.publish(channel, message)",
      "int",
      "Receivers."
     ],
     [
      "r.close()",
      "nil",
      ""
     ]
    ],
    "example": "import redis from \"fogu:redis\"\nconst r = redis.connect(\"redis://127.0.0.1:6379/3\")?\nprint(r.set(\"name\", \"Ana\", { ex: 60 })?, r.get(\"name\")?, r.ttl(\"name\")?)\nprint(r.set(\"name\", \"x\", { nx: true })?)              // exists: not set\nr.hset(\"user:1\", { name: \"Ana\", age: 30 })?\nprint(r.hgetall(\"user:1\")?)\nr.zadd(\"rank\", [10, \"ana\", 25, \"bob\"])?\nprint(r.zrange(\"rank\", 0, -1, { withScores: true, rev: true })?)\nprint(r.pipeline([[\"INCR\", \"n\"], [\"INCR\", \"n\"], [\"GET\", \"n\"]])?)\nconst sub = r.subscribe(\"news\")?\nr.publish(\"news\", \"hello\")?\nprint(sub.next(1s)?)",
    "output": "true Ana 60\nfalse\n{age: 30, name: Ana}\n[{member: bob, score: 25.0}, {member: ana, score: 10.0}]\n[1, 2, 2]\n{channel: news, message: hello}",
    "outputTitle": "output (with a Redis server)"
   }
  ]
 },
 "mod-s3": {
  "id": "mod-s3",
  "group": "Storage",
  "title": "fogu:s3",
  "import": "import s3 from \"fogu:s3\"",
  "intro": "AWS S3 and compatible storage (MinIO, Cloudflare R2, Backblaze B2, DigitalOcean Spaces, Wasabi) with AWS Signature V4. Credentials default to <code>$AWS_ACCESS_KEY_ID</code>, <code>$AWS_SECRET_ACCESS_KEY</code>, <code>$AWS_REGION</code>, <code>$AWS_ENDPOINT_URL</code>.",
  "blocks": [
   {
    "text": "Every method also takes <code>{ bucket }</code> in its options.",
    "api": [
     [
      "s3.connect({ endpoint, region, accessKey, secretKey, sessionToken, bucket, pathStyle, timeout })",
      "S3",
      "Path-style URLs when an <code>endpoint</code> is given."
     ],
     [
      "store.put(key, body, { contentType, metadata, cacheControl, acl })",
      "{ etag } · error",
      "<code>body</code>: str, list of bytes, or any value (JSON)."
     ],
     [
      "store.upload(path, key?, opts?)",
      "{ etag } · error",
      "A local file."
     ],
     [
      "store.get(key) · store.getBytes(key) · store.download(key, path)",
      "str · list · nil",
      ""
     ],
     [
      "store.head(key) · store.exists(key)",
      "map · nil · bool",
      "<code>{ size, etag, contentType, lastModified, metadata }</code>."
     ],
     [
      "store.list(prefix?, { delimiter, max })",
      "list",
      "<code>{ key, size, etag, lastModified }</code>; folders <code>{ key, dir: true }</code>."
     ],
     [
      "store.copy(from, to, { fromBucket }) · store.delete(key | keys)",
      "",
      ""
     ],
     [
      "store.presign(key, { method = \"GET\", expires = 1h })",
      "str",
      "A temporary URL that needs no credentials."
     ],
     [
      "store.url(key) · store.withBucket(name)",
      "str · S3",
      ""
     ],
     [
      "store.createBucket(name?) · store.deleteBucket(name?) · store.listBuckets()",
      "",
      ""
     ]
    ],
    "example": "import s3 from \"fogu:s3\"\nconst store = s3.connect({ endpoint: \"http://127.0.0.1:9000\", accessKey: \"fogu\", secretKey: \"secret\", bucket: \"docs\" })?\nstore.put(\"notes/hello.txt\", \"Olá, S3!\", { metadata: { author: \"ana\" } })?\nprint(store.get(\"notes/hello.txt\")?)\nprint(store.head(\"notes/hello.txt\")?.metadata, store.exists(\"nope\")?)\nprint(store.list(\"notes/\")?.map((o) => o.key))\nprint(store.presign(\"notes/hello.txt\", { expires: 15m }).contains(\"X-Amz-Signature=\"))",
    "output": "Olá, S3!\n{author: ana}\nfalse\n[notes/hello.txt]\ntrue",
    "outputTitle": "output (with an S3 server)"
   }
  ]
 },
 "mod-scylla": {
  "id": "mod-scylla",
  "group": "Storage",
  "title": "fogu:scylla",
  "import": "import scylla from \"fogu:scylla\"",
  "intro": "ScyllaDB and Apache Cassandra (CQL). Statements with parameters are prepared once and cached; parameters are converted to the column types (collections, UDTs, uuid, timestamp…).",
  "blocks": [
   {
    "api": [
     [
      "scylla.connect(\"host:port\" | { hosts, keyspace, user, password, singleNode, addressMap })",
      "Scylla · error",
      "<code>addressMap</code> translates addresses of nodes behind Docker/NAT."
     ],
     [
      "db.execute(cql, params?)",
      "nil · error",
      ""
     ],
     [
      "db.query(cql, params?)",
      "list · error",
      "Rows as maps."
     ],
     [
      "db.one(cql, params?)",
      "map · nil · error",
      ""
     ],
     [
      "db.use(keyspace) · db.close()",
      "",
      ""
     ]
    ],
    "example": "import scylla from \"fogu:scylla\"\nconst db = scylla.connect({ hosts: [\"127.0.0.1:9042\"] })?\ndb.execute(\"CREATE KEYSPACE IF NOT EXISTS shop WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}\")?\ndb.use(\"shop\")?\ndb.execute(\"CREATE TABLE IF NOT EXISTS users (id int PRIMARY KEY, name text, tags set<text>)\")?\ndb.execute(\"INSERT INTO users (id, name, tags) VALUES (?, ?, ?)\", [1, \"Ana\", [\"admin\"]])?\nprint(db.one(\"SELECT * FROM users WHERE id = ?\", [1])?)",
    "output": "{id: 1, name: Ana, tags: [admin]}",
    "outputTitle": "output (with a ScyllaDB server)"
   }
  ]
 },
 "mod-surrealdb": {
  "id": "mod-surrealdb",
  "group": "Storage",
  "title": "fogu:surrealdb",
  "import": "import surrealdb from \"fogu:surrealdb\"",
  "intro": "A SurrealDB client written on the runtime's networking (no SurrealDB crate): its JSON-RPC protocol over WebSocket (<code>ws://</code>, <code>wss://</code>: a session, live queries) or HTTP (<code>http://</code>, <code>https://</code>: <code>POST /rpc</code>, the session kept by the client). Tested with SurrealDB 3.3. Record ids are strings (<code>\"person:ana\"</code>); dates come back as ISO strings.",
  "blocks": [
   {
    "api": [
     [
      "surrealdb.connect(url?, { namespace, database, user, pass, auth, token, define, timeout = 10s })",
      "SurrealDB · error",
      "Default <code>$SURREAL_URL</code> or <code>ws://127.0.0.1:8000</code> (<code>/rpc</code> is added); also <code>$SURREAL_NS $SURREAL_DB $SURREAL_USER $SURREAL_PASS</code>. <code>user</code>/<code>pass</code> sign in as a root user; <code>auth</code> is passed to <code>signin</code> as it is (<code>{ ns, db, ac, … }</code> for other levels and record access). <code>define: true</code> creates the namespace and database if needed (SurrealDB 3)."
     ],
     [
      "db.query(sql, vars?) · db.queryRaw(sql, vars?)",
      "list · error",
      "One value per statement; a failed statement is an error value (<code>statement 2: …</code>). <code>queryRaw</code> keeps <code>{ status, result, time }</code>."
     ],
     [
      "db.select(thing) · create(thing, data?) · insert(table, data) · insertRelation(table, data) · update(thing, data?) · upsert(thing, data?) · merge(thing, data) · patch(thing, ops, diff?) · delete(thing)",
      "value · error",
      "<code>thing</code>: a table (<code>\"person\"</code>) or a record (<code>\"person:ana\"</code>)."
     ],
     [
      "db.relate(from, relation, to, data?) · db.run(fn, args?)",
      "value · error",
      "<code>run(\"fn::total\", [1, 2])</code> or a built-in (<code>\"string::uppercase\"</code>)."
     ],
     [
      "db.live(table, { diff }) → live · db.listen(id) · live.next(timeout?) · live.tryNext() · live.kill() · live.id · live.done",
      "Live",
      "WebSocket only. Events <code>{ action: CREATE | UPDATE | DELETE | KILLED, record, result }</code>, one at a time; <code>nil</code> after the timeout. <code>listen</code> follows a <code>LIVE SELECT</code> run with <code>query</code>."
     ],
     [
      "db.use(ns, db) · signin(params) · signup(params) · authenticate(token) · invalidate() · set(name, value) · unset(name)",
      "",
      "<code>signin</code>/<code>signup</code> return the token and use it from then on; <code>set</code> defines a <code>$name</code> session variable."
     ],
     [
      "db.info() · db.version() · db.ping() · db.close() · db.url · db.transport · db.namespace · db.database · db.token · db.closed",
      "",
      ""
     ]
    ],
    "example": "import surrealdb from \"fogu:surrealdb\"\nconst db = surrealdb.connect(\"ws://127.0.0.1:8000\", { namespace: \"shop\", database: \"main\", user: \"root\", pass: \"root\", define: true })?\ndb.create(\"person:ana\", { name: \"Ana\", age: 30 })?\ndb.insert(\"person\", [{ id: \"bia\", name: \"Bia\", age: 17 }, { id: \"caio\", name: \"Caio\", age: 41 }])?\nconst [adults, total] = db.query(\"SELECT name FROM person WHERE age >= $min ORDER BY name; RETURN count(SELECT * FROM person)\", { min: 18 })?\nprint(adults, total)\nconst live = db.live(\"person\")?\ndb.merge(\"person:bia\", { age: 18 })?\nconst ev = live.next(2s)?\nprint(ev.action, ev.record, ev.result.age)",
    "output": "[{name: Ana}, {name: Caio}] 3\nUPDATE person:bia 18",
    "outputTitle": "output (with a SurrealDB server)"
   }
  ]
 },
 "mod-etcd": {
  "id": "mod-etcd",
  "group": "Storage",
  "title": "fogu:etcd",
  "import": "import etcd from \"fogu:etcd\"",
  "intro": "etcd v3 over gRPC (the official protos, no code generation): keys, ranges by prefix, leases, transactions, watches and cluster status. Calls go to every endpoint in turn; with <code>user</code> the token is renewed when it expires.",
  "blocks": [
   {
    "api": [
     [
      "etcd.connect(url? | { endpoints, user, password, tls, timeout = 15s, connectTimeout = 5s })",
      "Etcd · error",
      "Default <code>$ETCDCTL_ENDPOINTS</code> or <code>http://127.0.0.1:2379</code>; <code>user</code> may be <code>\"name:pass\"</code> (also <code>$ETCDCTL_USER</code>/<code>$ETCDCTL_PASSWORD</code>)."
     ],
     [
      "db.get(key, { prefix, rangeEnd, limit, revision, keysOnly, countOnly, serializable, sort, sortTarget })",
      "{ kvs, count, more, revision }",
      "<code>kvs</code>: <code>[{ key, value, version, createRevision, modRevision, lease }]</code>."
     ],
     [
      "db.put(key, value, { lease, prevKv, ignoreValue, ignoreLease }) · db.delete(key, { prefix, rangeEnd, prevKv })",
      "{ revision, prevKv } · { deleted, prevKvs }",
      ""
     ],
     [
      "db.grant(ttl) · db.revoke(id) · db.ttl(id) · db.keepAlive(id)",
      "{ id, ttl } · … · Stream",
      "<code>keepAlive</code> refreshes the lease until <code>close()</code>; <code>next(timeout?)</code> gives <code>{ id, ttl }</code>."
     ],
     [
      "db.txn({ compare: [{ key, op, field, value }], success: [ops], failure: [ops] })",
      "{ succeeded, responses, revision }",
      "<code>field</code>: <code>value version createRevision modRevision lease</code>; ops <code>{ type: put | get | delete, key, … }</code>."
     ],
     [
      "db.watch(key, { prefix, rangeEnd, prevKv, revision, progressNotify })",
      "Watch",
      "<code>w.next(timeout?)</code> → <code>{ type: put | delete, kv, prevKv }</code>, one event at a time · <code>w.close()</code>."
     ],
     [
      "db.status() · db.memberList() · db.compact(revision) · db.endpoints · db.close()",
      "",
      ""
     ]
    ],
    "example": "import etcd from \"fogu:etcd\"\nconst db = etcd.connect(\"http://127.0.0.1:2379\")?\ndb.put(\"/app/a\", \"1\")?\ndb.put(\"/app/b\", \"2\")?\nprint(db.get(\"/app/\", { prefix: true })?.count)\nconst w = db.watch(\"/app/\", { prefix: true })?\ndb.put(\"/app/a\", \"3\")?\nconst ev = w.next(2s)?\nprint(ev.type, ev.kv.key, ev.kv.value)\nw.close()\nprint(db.delete(\"/app/\", { prefix: true })?.deleted)",
    "output": "2\nput /app/a 3\n2",
    "outputTitle": "output (with an etcd server)"
   }
  ]
 },
 "mod-http": {
  "id": "mod-http",
  "group": "Web & network",
  "title": "fogu:http",
  "import": "import http from \"fogu:http\"",
  "intro": "A minimal HTTP server (one handler for every request) and an HTTP client. For routing, middleware, static files, uploads, SSE and WebSocket use <a class=\"underline\" href=\"#mod-web\">fogu:web</a>.",
  "blocks": [
   {
    "h3": "Client",
    "api": [
     [
      "http.get(url, opts?) · http.delete(url, opts?) · http.head(url, opts?)",
      "Response · error",
      "Network failures are error values; HTTP 4xx/5xx are normal responses (<code>res.ok</code> is false)."
     ],
     [
      "http.post(url, body?, opts?) · http.put(…) · http.patch(…)",
      "Response · error",
      "<code>body</code>: a str (text/plain), a map or list (JSON), bytes (octet-stream)."
     ],
     [
      "http.request({ method, url, ...opts })",
      "Response · error",
      ""
     ],
     [
      "opts: { headers, query, json, form, multipart, body, timeout, redirects = true, basicAuth: [user, pass], bearer }",
      "",
      "<code>form</code>: urlencoded fields. <code>multipart</code>: a map of fields — text values, <code>http.file(path)</code> (streamed from disk), <code>{ data, filename, type }</code> bytes, or a list (a repeated field). Map keys are sent in alphabetical order."
     ],
     [
      "http.file(path, { filename, type })",
      "HttpFile · error",
      "A file to upload; the type comes from the extension."
     ],
     [
      "http.download(url, path, opts?)",
      "int · error",
      "Streams the body to a file; returns the bytes written (non-2xx is an error)."
     ],
     [
      "res.status · res.ok · res.url · res.headers · res.body",
      "",
      "<code>headers</code>: the first value of each (lower case)."
     ],
     [
      "res.json() · res.bytes() · res.header(name) · res.cookies()",
      "",
      "<code>bytes()</code>: the body as a <a class=\"underline\" href=\"#values\">buffer</a>; <code>header</code> joins repeated values."
     ]
    ]
   },
   {
    "h3": "Server",
    "title": "server.fg",
    "api": [
     [
      "http.serve(port | \"ip:port\", fn(req))",
      "nil",
      "Requests are answered as soon as they arrive while the program is alive — waiting in <code>Fogu.signal.wait()</code>, <code>time.delay</code>, on a channel… — and each handler runs in its own task, so one that waits does not hold the others. The address is an int port (on 127.0.0.1) or <code>\"0.0.0.0:8080\"</code>, <code>\"[::1]:8080\"</code>, <code>\":8080\"</code>."
     ],
     [
      "req.method · req.path · req.query · req.headers · req.body",
      "",
      "<code>query</code> is the raw query string."
     ],
     [
      "req.header(name) · req.json() · req.form() · req.bytes()",
      "",
      "<code>form()</code>: urlencoded and multipart bodies."
     ],
     [
      "http.text(body, opts?) · http.json(value, opts?) · http.html(body, opts?)",
      "Response",
      "<code>opts: { status, headers }</code>."
     ]
    ],
    "example": "import http from \"fogu:http\"\nimport time from \"fogu:time\"\n\nhttp.serve(8080, (req) => {\n    if req.path == \"/hello\" {\n        return http.json({ hello: \"world\", method: req.method })\n    }\n    return http.text(\"not found\", { status: 404 })\n})\nwhile true { time.delay(50ms) }"
   },
   {
    "title": "client.fg",
    "example": "import http from \"fogu:http\"\nconst r = http.get(\"http://127.0.0.1:8080/hello\")?\nprint(r.status, r.headers[\"content-type\"], r.json())\nprint(http.get(\"http://127.0.0.1:8080/nope\")?.status)\nconst [_, err] = http.get(\"http://127.0.0.1:1/\")\nprint(err != nil)",
    "output": "200 application/json {hello: world, method: GET}\n404\ntrue",
    "outputTitle": "output"
   },
   {
    "text": "Forms, file uploads, headers and downloads (here against a <a class=\"underline\" href=\"#mod-web\">fogu:web</a> app in the same program):",
    "example": "import http from \"fogu:http\"\nimport web from \"fogu:web\"\nimport fs from \"fogu:fs\"\n\nfs.writeText(\"report.txt\", \"hello file\\n\")?\nconst app = web.app()\napp.post(\"/echo\", (ctx) => ({ type: ctx.headers[\"content-type\"].split(\";\")[0], body: ctx.body }))\napp.post(\"/upload\", (ctx) => {\n    const f = ctx.req.form()?\n    return { title: f.title, tags: f.tags, file: f.file.filename, text: f.file.text(), avatar: f.avatar.bytes().len() }\n})\napp.listen(18471, { quiet: true })\n\nlet done = false\nfg fn() {\n    const base = \"http://127.0.0.1:18471\"\n    print(http.post(base + \"/echo\", { name: \"Ana\" }, { query: { page: 2 } })?.json())\n    print(http.post(base + \"/echo\", nil, { form: { user: \"ana\", pass: \"a b&c\" } })?.json())\n    const up = http.post(base + \"/upload\", nil, { multipart: {\n        title: \"report\", tags: [\"a\", \"b\"],\n        file: http.file(\"report.txt\"),\n        avatar: { data: [1, 2, 3, 4], filename: \"a.png\" },\n    } })?\n    print(up.ok, up.json())\n    done = true\n}()\nwhile !done { Fogu.sleep(10ms) }",
    "output": "{body: {\"name\":\"Ana\"}, type: application/json}\n{body: pass=a+b%26c&user=ana, type: application/x-www-form-urlencoded}\ntrue {avatar: 4, file: report.txt, tags: [a, b], text: hello file\n, title: report}",
    "outputTitle": "output"
   }
  ]
 },
 "mod-web": {
  "id": "mod-web",
  "group": "Web & network",
  "title": "fogu:web",
  "import": "import web from \"fogu:web\"",
  "intro": "A web framework: routes with parameters, middleware, static files and SPA builds, uploads, downloads, Server-Sent Events and WebSocket — on one port. Requests are served as soon as they arrive while the program waits (<code>Fogu.signal.wait()</code>, <code>time.delay</code>, a channel), each in its own task: a handler that waits on a query or an HTTP call does not hold the others.",
  "blocks": [
   {
    "h3": "App",
    "api": [
     [
      "web.app()",
      "App",
      "Methods return the app, so calls can be chained."
     ],
     [
      "app.get/post/put/patch/delete/head/options/all(path, ...middlewares, handler)",
      "App",
      "<code>/users/:id</code> parameters, <code>*</code> wildcard."
     ],
     [
      "app.use(fn) · app.use(prefix, fn)",
      "App",
      "Middleware for every request, or under a path prefix."
     ],
     [
      "app.after((ctx, res) => …)",
      "App",
      "After the handler; returning a value replaces the response."
     ],
     [
      "app.onError((ctx, err) => response)",
      "App",
      "For runtime errors and returned <code>error</code> values (default: 500)."
     ],
     [
      "app.static(dir) · app.static(prefix, dir, { spa, index, maxAge, dotfiles })",
      "App",
      "Files served by the server thread: ETag/304, <code>Cache-Control</code>; <code>spa: true</code> serves <code>index.html</code> for unknown HTML routes (React, Vue, Svelte builds)."
     ],
     [
      "app.ws(path, ...middlewares, (ws, ctx) => …, { protocols })",
      "App",
      "WebSocket routes; the handler runs in its own task with a <a class=\"underline\" href=\"#mod-websocket\">WebSocket</a> on the upgraded HTTP connection."
     ],
     [
      "app.set(key, value) · app.store",
      "App · map",
      "Shared state (<code>ctx.store</code>)."
     ],
     [
      "app.listen(port | \"ip:port\", { host = \"127.0.0.1\", maxBody = 10 MB, quiet })",
      "nil · error",
      "<code>app.listen(\"0.0.0.0:3000\")</code> (or <code>host: \"0.0.0.0\"</code>) accepts other machines; IPv6 as <code>\"[::1]:3000\"</code>. A host in the string wins over <code>host</code>."
     ],
     [
      "app.handler()",
      "fn",
      "Calls the app without a network (tests): <code>h({ method, path, headers, body })</code> → Response."
     ],
     [
      "app.routes()",
      "list",
      ""
     ]
    ]
   },
   {
    "h3": "Context and responses",
    "title": "app.fg",
    "api": [
     [
      "ctx.method · ctx.path · ctx.params · ctx.query · ctx.headers · ctx.body",
      "",
      "<code>query</code> is a map; header names are lower case."
     ],
     [
      "ctx.req.json() · ctx.req.form() · ctx.req.bytes()",
      "",
      "<code>form()</code>: fields as str, files as <code>UploadedFile</code>."
     ],
     [
      "ctx.set.status · ctx.set.headers",
      "",
      "Adjust the final response from middleware or handlers."
     ],
     [
      "ctx.store",
      "map",
      "The app's shared map."
     ],
     [
      "handler result",
      "",
      "A str (text or HTML), any value (JSON), a response, or nil (204)."
     ],
     [
      "web.json/text/html(body, { status, headers })",
      "Response",
      ""
     ],
     [
      "web.redirect(url, status = 302)",
      "Response",
      ""
     ],
     [
      "web.file(path, { download, contentType, status })",
      "Response · error",
      "Sent by the server thread (binary safe)."
     ],
     [
      "web.sse({ retry, headers })",
      "SseStream",
      "Return it from the handler; <code>send(data, { event, id })</code>, <code>comment(text)</code>, <code>close()</code>, field <code>open</code>."
     ],
     [
      "web.cors({ origin, methods, headers, credentials, maxAge })",
      "middleware",
      "Answers preflight requests."
     ],
     [
      "web.logger()",
      "middleware",
      "<code>GET /path 200 0.4ms</code>."
     ],
     [
      "UploadedFile: filename, contentType, size, text(), bytes(), json(), save(path)",
      "",
      "<code>save(\"uploads/\")</code> keeps a sanitized file name."
     ]
    ],
    "example": "import web from \"fogu:web\"\nimport time from \"fogu:time\"\n\nconst app = web.app()\napp.use(web.logger())\napp.use(\"/api\", (ctx) => {\n    if ctx.headers[\"x-key\"] != \"secret\" { return web.json({ error: \"forbidden\" }, { status: 403 }) }\n})\napp.get(\"/api/users/:id\", (ctx) => ({ id: ctx.params.id, page: ctx.query.page ?? \"1\" }))\napp.post(\"/api/upload\", (ctx) => {\n    const form = ctx.req.form()?\n    return { saved: form.file.save(\"uploads/\")?, size: form.file.size }\n})\napp.get(\"/events\", (ctx) => {\n    const stream = web.sse()\n    fg fn() {\n        for i in 1..4 { stream.send({ n: i }, { event: \"tick\" }); time.delay(1s) }\n        stream.close()\n    }()\n    return stream\n})\napp.ws(\"/chat\", (ws, ctx) => {\n    while true {\n        const m = ws.next()?\n        if m == nil { break }\n        ws.send(`echo: ${m.data}`)\n    }\n})\napp.static(\"/\", \"dist\", { spa: true, maxAge: 1d })\napp.listen(3000)?\nwhile true { time.delay(50ms) }"
   },
   {
    "text": "Without a network, through <code>app.handler()</code>:",
    "example": "import web from \"fogu:web\"\nconst app = web.app()\napp.use((ctx) => { ctx.set.headers[\"x-served-by\"] = \"fogu\" })\napp.get(\"/users/:id\", (ctx) => ({ id: ctx.params.id, sort: ctx.query.sort }))\napp.get(\"/old\", (ctx) => web.redirect(\"/new\"))\napp.get(\"/fail\", (ctx) => error(\"boom\"))\napp.onError((ctx, err) => web.json({ error: err.message }, { status: 500 }))\nconst h = app.handler()\nconst show = (path) => {\n    const r = h({ method: \"GET\", path: path })\n    print(r.status, r.body, r.headers[\"x-served-by\"], r.headers[\"location\"])\n}\nshow(\"/users/7?sort=name\")\nshow(\"/old\")\nshow(\"/fail\")\nshow(\"/missing\")",
    "output": "200 {\"id\":\"7\",\"sort\":\"name\"} fogu nil\n302  fogu /new\n500 {\"error\":\"boom\"} fogu nil\n404 not found fogu nil",
    "outputTitle": "output"
   }
  ]
 },
 "mod-topcoat": {
  "id": "mod-topcoat",
  "group": "Web & network",
  "title": "fogu:topcoat",
  "import": "import tc from \"fogu:topcoat\"",
  "intro": "Server-rendered reactive pages for <a class=\"underline\" href=\"#mod-web\">fogu:web</a>, in the model of <a class=\"underline\" href=\"https://github.com/tokio-rs/topcoat\">Topcoat</a>: HTML is built on the server; <b>signals</b> hold browser state; <code>@event</code> handlers, <code>:attr</code> bindings and live text update in the browser without a round-trip; <b>shards</b> re-render on the server when their inputs change and are swapped in place (keeping focus and input state); <b>procedures</b> are server functions called from the browser. The browser runtime (~18 KB, no build step) and the stylesheets are served by every fogu:web app under <code>/_tc/</code>. Two UI kits: <code>tc.ui</code> (the look of Topcoat UI) and <code>tc.shadcn</code> (the components of <a class=\"underline\" href=\"https://github.com/hughdbrown/shadcn-rs\">shadcn-rs</a>), plus Tailwind CSS.",
  "blocks": [
   {
    "h3": "Pages and markup",
    "api": [
     [
      "tc.h(tag, attrs?, ...children)",
      "Html",
      "Children: strings (escaped), <code>Html</code>, lists, numbers, signals and expressions (live text); <code>nil</code>/<code>false</code> render nothing. Attributes: <code>true</code> → bare, <code>false</code>/<code>nil</code> → left out, <code>class</code> may be a list or a <code>{ name: on }</code> map, <code>style</code> a map."
     ],
     [
      "@event: \"js\" · :attr: \"js\" | signal",
      "",
      "<code>@click</code> runs JavaScript statements (<code>e</code> is the event, signals are in scope by name, <code>await</code> works); modifiers <code>.prevent .stop .once .self .enter .escape .debounce[.ms]</code>. <code>:value</code>, <code>:hidden</code>, <code>:class</code>, <code>:disabled</code>… follow an expression. Written as <code>tc-on:click</code> / <code>tc-bind:hidden</code> in raw HTML."
     ],
     [
      "tc.page({ title, head, body, lang = \"en\", theme = \"light\" | \"dark\" | \"system\", ui = true | \"shadcn\" | false, tailwind, icon, bodyClass })",
      "Html",
      "A whole document with the runtime, the stylesheet and the Fogu favicon (<code>icon</code>: another URL, or <code>false</code>). The theme the user picked with <code>ui.themeToggle()</code> is applied before the first paint. Return an <code>Html</code> from a handler: it is sent as HTML with the page's signals."
     ],
     [
      "tc.raw(html) · tc.fragment(...children) · tc.escape(str) · html.html",
      "Html · str",
      ""
     ],
     [
      "tc.script() · tc.styles() · tc.tailwind(true | href)",
      "Html",
      "For documents written by hand."
     ]
    ]
   },
   {
    "h3": "Signals, shards and procedures",
    "api": [
     [
      "tc.signal(ctx, name, initial)",
      "Signal",
      "Browser state named <code>name</code> (a JS identifier). <code>s.value</code> / <code>s.get()</code> read it on the server and make it <b>tracked</b>: a change in the browser renders the page (or the shard that read it) again with the new value. <code>s.peek()</code> reads without tracking. Values sent by the browser are user input: validate them."
     ],
     [
      "in the browser: s.get() · set(v) · update(f) · increment(n) · decrement(n) · toggle() · push(x)",
      "",
      "Plus <code>call(name, ...args)</code>, <code>navigate(url)</code>, <code>toast(title, { description, type, duration })</code>, <code>await ask(message, { title, confirm, cancel, danger })</code> (a confirmation dialog → bool), <code>copy(text, label)</code>, <code>toggleTheme()</code> and <code>tc.signals</code>."
     ],
     [
      "tc.expr(js, initial?)",
      "Expr",
      "A browser expression: live text, an <code>:attr</code> value or a shard prop."
     ],
     [
      "tc.shard(name, fn(ctx, props)) · shard.render(ctx, props) · app.shard(shard)",
      "Shard · Html",
      "Props are fixed values or signals/expressions; when one changes, <code>POST /_tc/s/&lt;name&gt;</code> renders it again and the browser morphs it in. The first page that renders a shard registers it; <code>app.shard</code> does it up front. Check access inside the shard: page middleware scoped to a path does not run for it."
     ],
     [
      "app.procedure(name, fn(ctx, ...args))",
      "App",
      "<code>await call(name, ...args)</code> in the browser; an <code>error</code> value (or <code>return nil, error(…)</code>) rejects the call with its message."
     ],
     [
      "ui.link(children, href) · <a tc-link>",
      "Html",
      "Navigation without a full reload (prefetched on hover, history kept)."
     ],
     [
      "protocol",
      "",
      "Topcoat's frames, one JSON object per line (<code>application/x-ndjson</code>): <code>snapshot</code> (a rerun: <code>POST</code> of the signals to the page URL with <code>x-topcoat: rerun</code>), <code>swap</code>, <code>redirect</code>, <code>error</code>. <code>ctx.tc</code>: <code>{ signals, input, rerun, page }</code>."
     ]
    ]
   },
   {
    "h3": "UI kits and Tailwind",
    "api": [
     [
      "tc.ui.button(children, { variant, size, href }) · badge · card(content, { title, description, footer, action }) · stat(label, value, { hint })",
      "Html",
      "Topcoat UI's look (its neutral theme tokens, plain CSS: <code>/_tc/ui.css</code>). Variants <code>secondary outline ghost destructive link</code>; badges also <code>success warning</code>; sizes <code>sm lg icon</code>. Extra options become attributes (<code>@click</code>, <code>class</code>…)."
     ],
     [
      "tc.ui.icon(name, { size = 16 }) · logo({ size, text }) · sparkline(values, { width, height, area }) · empty(title, { description, icon, action }) · live(text) · themeToggle()",
      "Html",
      "35 built-in stroke icons (<code>activity alert arrow-right check chevron-right clock copy database edit eye flame hash home key layers lock logout message moon network plus radio refresh search send server settings shield sun terminal trash users x zap</code>); <code>stat(label, value, { icon, tone, trend: [numbers], hint })</code> draws a trend line; tones <code>primary success warning destructive info</code>. <code>shell</code> nav items take an <code>icon</code>, and <code>footer</code> goes under the nav; <code>grid(children, { min })</code>, <code>table(…, { header: false })</code>."
     ],
     [
      "tc.ui.input · textarea · select(options, { value }) · checkbox · switch · label · field(label, control, { description, error })",
      "Html",
      ""
     ],
     [
      "tc.ui.table(columns, rows, { empty, caption }) · alert(title, text, { variant }) · progress(value) · skeleton · spinner · separator · kbd · tabs · link · shell({ brand, nav, title, actions }, content) · row · stack · grid · container",
      "Html",
      "<code>columns</code>: names or <code>{ key, label, align }</code>; rows: maps or lists; cells may be <code>Html</code>."
     ],
     [
      "tc.shadcn.button · badge · card · stat · alert · input · textarea · select · checkbox · switch(signal) · label · field · table",
      "Html",
      "The markup and class names of shadcn-rs, styled by its stylesheet (<code>page({ ui: \"shadcn\" })</code>). Button variants <code>default primary secondary destructive outline ghost link</code>, sizes <code>xs sm md lg xl</code>."
     ],
     [
      "tc.shadcn.tabs(signal, [{ value, label, content }]) · dialog(signal, { title, description, footer, trigger }, content) · accordion([{ title, content, open }], { multiple }) · dropdown(trigger, items) · tooltip(trigger, text) · pagination(page, pages, { href }) · progress · skeleton · separator · avatar · kbd · breadcrumb · shell · toaster",
      "Html",
      "Interactive parts run in the browser: tabs and dialogs follow a signal; <code>pagination</code> takes a signal or an <code>href</code> with <code>{page}</code>."
     ],
     [
      "tc.page({ tailwind: true }) · tc.page({ tailwind: \"/app.css\" }) · tc.tailwindTheme",
      "",
      "<code>true</code> loads Tailwind v4's browser build (development) with the theme tokens as utilities (<code>bg-primary</code>, <code>text-muted-foreground</code>, <code>border-border</code>, <code>dark:</code>); for production build a stylesheet with the Tailwind CLI (<code>@import \"tailwindcss\"; @source \"./**/*.fg\";</code> plus <code>tc.tailwindTheme</code>) and pass its URL."
     ]
    ],
    "example": "import web from \"fogu:web\"\nimport tc from \"fogu:topcoat\"\nimport json from \"fogu:json\"\nconst h = tc.h\nconst ui = tc.ui\n\nconst results = tc.shard(\"results\", fn(ctx, props) {\n    const found = [\"apple\", \"banana\", \"mango\"].filter(fn(f) { return f.contains(props.query ?? \"\") })\n    return ui.table([\"fruit\"], found.map(fn(f) { return { fruit: f } }), { empty: \"Nothing found\" })\n})\nconst app = web.app()\napp.procedure(\"double\", fn(ctx, n) { return n * 2 })\napp.get(\"/\", fn(ctx) {\n    const count = tc.signal(ctx, \"count\", 0)\n    const query = tc.signal(ctx, \"query\", \"\")\n    return tc.page({ title: \"Demo\", body: ui.container([\n        h(\"p\", nil, \"Count: \", count),\n        ui.button(\"+1\", { \"@click\": \"count.increment()\" }),\n        ui.button(\"double on the server\", { variant: \"outline\", \"@click\": \"count.set(await call('double', count.get()))\" }),\n        ui.input({ placeholder: \"Search\", \":value\": query, \"@input\": \"query.set(e.target.value)\" }),\n        results.render(ctx, { query: tc.expr(\"query.get()\", query.peek()) }),\n    ])})\n})\n\n// what the browser does, without a network:\nconst call = app.handler()\nprint(call({ method: \"GET\", path: \"/\" }).body.contains(\"data-tc-text=\\\"count.get()\\\"\"))\nconst swap = json.parse(call({ method: \"POST\", path: \"/_tc/s/results\", body: \"{\\\"props\\\":{\\\"query\\\":\\\"an\\\"}}\" }).body)\nprint(swap.t, swap.html.contains(\"banana\"), swap.html.contains(\"apple\"))\nprint(call({ method: \"POST\", path: \"/_tc/p/double\", body: \"[21]\" }).body)\n// app.listen(3000) and Fogu.signal.wait() to serve it",
    "output": "true\nswap true false\n{\"ok\":42}",
    "outputTitle": "output"
   }
  ]
 },
 "mod-jwt": {
  "id": "mod-jwt",
  "group": "Web & network",
  "title": "fogu:jwt",
  "import": "import jwt from \"fogu:jwt\"",
  "intro": "JSON Web Tokens with HMAC (HS256/HS384/HS512). A token that fails verification is an error value.",
  "blocks": [
   {
    "api": [
     [
      "jwt.sign(payload, secret, { algorithm, expiresIn, notBefore, issuer, audience, subject, jwtid, header, noTimestamp })",
      "str",
      "<code>iat</code> is added; durations or seconds for times."
     ],
     [
      "jwt.verify(token, secret, { algorithms, issuer, audience, subject, leeway, maxAge, ignoreExpiration })",
      "map · error",
      "The claims."
     ],
     [
      "jwt.decode(token)",
      "map · error",
      "<code>{ header, payload }</code> without verifying."
     ],
     [
      "jwt.middleware(secret, { property = \"user\", optional, …verify options })",
      "middleware",
      "For <code>fogu:web</code>: <code>Authorization: Bearer</code> → <code>ctx.user</code>, otherwise 401."
     ]
    ],
    "example": "import jwt from \"fogu:jwt\"\nconst token = jwt.sign({ sub: \"42\", role: \"admin\" }, \"s3cret\", { expiresIn: 1h, issuer: \"fogu\" })\nconst claims = jwt.verify(token, \"s3cret\", { issuer: \"fogu\" })?\nprint(claims.sub, claims.role, claims.exp - claims.iat)\nprint(jwt.decode(token)?.header)\nconst [_, e1] = jwt.verify(token, \"wrong\")\nconst [_, e2] = jwt.verify(jwt.sign({ a: 1 }, \"s3cret\", { expiresIn: -10 }), \"s3cret\")\nprint(e1, e2)",
    "output": "42 admin 3600\n{alg: HS256, typ: JWT}\nerror: jwt: invalid signature error: jwt: token expired",
    "outputTitle": "output"
   }
  ]
 },
 "mod-socketio": {
  "id": "mod-socketio",
  "group": "Web & network",
  "title": "fogu:socketio",
  "import": "import socketio from \"fogu:socketio\"",
  "intro": "Socket.IO (protocol v5) servers and clients, compatible with the official JavaScript, Python and Go libraries. The server is socketioxide (WebSocket and long polling); the client speaks Engine.IO v4 over WebSocket; it runs standalone or on the port of a <a class=\"underline\" href=\"#mod-web\">fogu:web</a> app. Handlers run as <code>fg</code> tasks in the order the messages arrived; keep the program alive meanwhile (<code>Fogu.signal.wait()</code>, a loop…).",
  "blocks": [
   {
    "h3": "Server",
    "api": [
     [
      "socketio.server({ path = \"/socket.io\", pingInterval, pingTimeout, ackTimeout, maxPayload })",
      "SocketIoServer",
      ""
     ],
     [
      "io.on(\"connection\", fn(socket)) · io.of(\"/ns\").on(\"connection\", fn)",
      "nil",
      "Namespaces."
     ],
     [
      "io.listen(port | \"host:port\") · app.listen(port, { socketio: io })",
      "int · nil",
      "Standalone, or on a fogu:web app (same port)."
     ],
     [
      "io.emit(event, ...args) · io.to(room).emit(…) · io.except(room) · io.of(ns).emit(…) · io.close()",
      "",
      "Several arguments travel as several arguments."
     ],
     [
      "socket.on(event, fn(...args, ack?)) · socket.on(\"disconnect\", fn(reason)) · socket.off(event)",
      "nil",
      "<code>ack</code> is passed when the client asked for one: <code>ack(value)</code>."
     ],
     [
      "socket.emit(event, ...args) · socket.emitWithAck(event, data, { timeout })",
      "nil · value",
      ""
     ],
     [
      "socket.join(room) · leave(room) · to(room).emit(…) · broadcast.emit(…) · disconnect()",
      "",
      "<code>to</code>/<code>broadcast</code> exclude the socket itself."
     ],
     [
      "socket.id · ns · rooms · auth · handshake · connected",
      "",
      "<code>auth</code>: what the client sent; <code>handshake</code>: headers and query."
     ]
    ]
   },
   {
    "h3": "Client",
    "api": [
     [
      "socketio.connect(url, { namespace, auth, headers, path = \"/socket.io\", reconnect = true, timeout = 10s })",
      "SocketIoClient · error",
      "Returns once connected. Like the JavaScript client, the url's path is the namespace (<code>http://host:3000/admin</code>). A refused connection (auth middleware) is an error value."
     ],
     [
      "c.on(event, fn(...args)) · c.on(\"connect\" | \"disconnect\" | \"error\", fn) · c.off(event)",
      "nil",
      "Handlers registered right after <code>connect</code> see the first messages."
     ],
     [
      "c.emit(event, ...args) · c.emitWithAck(event, data, { timeout = 5s }) · c.disconnect()",
      "",
      ""
     ]
    ],
    "example": "import socketio from \"fogu:socketio\"\nconst log = []\nconst io = socketio.server()\nio.on(\"connection\", fn(socket) {\n    socket.join(\"lobby\")\n    socket.on(\"chat\", fn(msg, ack) {\n        io.to(\"lobby\").emit(\"chat\", \"echo:\" + msg)\n        if ack != nil { ack({ ok: true }) }\n    })\n    socket.on(\"disconnect\", fn(reason) { log.push(\"bye\") })\n})\nconst port = io.listen(\"127.0.0.1:0\")\n\nconst got = []\nconst c = socketio.connect(\"http://127.0.0.1:\" + str(port), { auth: { token: \"t1\" } })?\nc.on(\"chat\", fn(m) { got.push(m) })\nc.emit(\"chat\", \"hi\")\nprint(c.emitWithAck(\"chat\", \"ack me\", { timeout: 2s })?)\nFogu.sleep(200ms)\nc.disconnect()\nFogu.sleep(200ms)\nprint(got, log)\nio.close()",
    "output": "{ok: true}\n[echo:hi, echo:ack me] [bye]",
    "outputTitle": "output"
   }
  ]
 },
 "mod-grpc": {
  "id": "mod-grpc",
  "group": "Web & network",
  "title": "fogu:grpc",
  "import": "import grpc from \"fogu:grpc\"",
  "intro": "gRPC servers and clients without code generation: the messages are described by a <a class=\"underline\" href=\"#mod-protobuf\">fogu:protobuf</a> schema (<code>.proto</code> compiled at run time) and travel as maps. Unary, server streaming, client streaming and bidirectional methods (the requests of a bidirectional call are all received before the handler runs). Each request runs as its own <code>fg</code> task.",
  "blocks": [
   {
    "api": [
     [
      "grpc.server(schema)",
      "GrpcServer",
      ""
     ],
     [
      "srv.handle(method, fn(req, call)) · srv.handle({ method: fn }) · srv.service(\"pkg.Service\", { Method: fn })",
      "nil",
      "Methods: <code>\"pkg.Service/Method\"</code>, <code>\"pkg.Service.Method\"</code> or a unique <code>\"Method\"</code>. Client streaming: <code>req</code> is a list."
     ],
     [
      "call.send(msg) · call.metadata · call.peer · call.method · call.cancelled",
      "",
      "<code>send</code> for server streaming (or return a list)."
     ],
     [
      "srv.listen(port | \"host:port\", { cert, key })",
      "int · error",
      "The port. <code>cert</code>/<code>key</code>: PEM files for TLS."
     ],
     [
      "srv.serve() · srv.close() · srv.port · srv.address · srv.methods",
      "",
      "<code>serve</code> runs until <code>close</code> (start it with <code>fg</code>)."
     ],
     [
      "return nil, grpc.status(\"NOT_FOUND\", \"msg\")",
      "",
      "Fails the call with that code (any other error is <code>UNKNOWN</code>); <code>grpc.codes</code> lists them."
     ],
     [
      "grpc.connect(url, schema, { timeout, metadata, ca, domain })",
      "GrpcClient",
      "<code>https://</code> uses TLS (system roots or <code>ca</code>). Connects on the first call."
     ],
     [
      "c.call(method, req, { timeout, metadata })",
      "value · error",
      "A list for server streaming; a list of requests for client streaming. Failures: <code>grpc: NOT_FOUND: msg</code>, <code>grpc: DEADLINE_EXCEEDED: …</code>."
     ],
     [
      "c.stream(method, req, opts) → s.next() · s.close() · s.done · s.received",
      "GrpcStream",
      "One message at a time (<code>nil</code> at the end)."
     ]
    ],
    "example": "import grpc from \"fogu:grpc\"\nimport protobuf from \"fogu:protobuf\"\nconst schema = protobuf.parse(\"syntax = \\\"proto3\\\"; package demo;\nmessage HelloRequest { string name = 1; }\nmessage HelloReply { string message = 1; }\nmessage Num { int32 n = 1; }\nservice Greeter {\n  rpc SayHello (HelloRequest) returns (HelloReply);\n  rpc Count (Num) returns (stream Num);\n}\")?\nconst srv = grpc.server(schema)\nsrv.handle(\"demo.Greeter/SayHello\", fn(req, call) {\n    if req.name == \"\" { return nil, grpc.status(\"INVALID_ARGUMENT\", \"name is required\") }\n    return { message: \"Hello, \" + req.name }\n})\nsrv.handle(\"Count\", fn(req, call) { for i in 0..req.n { call.send({ n: i }) } })\nconst port = srv.listen(\"127.0.0.1:0\")?\nfg srv.serve()\n\nconst c = grpc.connect(\"127.0.0.1:\" + str(port), schema)?\nprint(c.call(\"SayHello\", { name: \"Ana\" })?)\nprint(c.call(\"SayHello\", { name: \"\" }))\nprint(c.call(\"Count\", { n: 3 })?)\nsrv.close()",
    "output": "{message: Hello, Ana}\nerror: grpc: INVALID_ARGUMENT: name is required\n[{n: 0}, {n: 1}, {n: 2}]",
    "outputTitle": "output"
   }
  ]
 },
 "mod-security": {
  "id": "mod-security",
  "group": "Web & network",
  "title": "fogu:security",
  "import": "import security from \"fogu:security\"",
  "intro": "Security and network auditing: TLS and certificates (rustls, x509-parser, rcgen), port scanning, DNS (own resolver: UDP with EDNS0, TCP fallback), HTTP security headers, password strength (zxcvbn), TOTP two-factor codes, secret scanning, Ed25519 signatures, IP ranges (ipnet) and rate limiting. Scan only hosts you are allowed to test.",
  "blocks": [
   {
    "api": [
     [
      "security.tls(host, { port = 443, sni, ca, alpn, timeout = 5s })",
      "map · error",
      "<code>{ valid, error, protocol, cipher, alpn, daysLeft, handshakeMs, chain }</code> — the chain is inspected even when it is not trusted."
     ],
     [
      "security.parseCert(pem | der) · security.selfSigned(names, { days = 365 })",
      "map · { cert, key }",
      "<code>subject commonName issuer names serial notBefore notAfter daysLeft expired selfSigned ca signature keyType keyBits sha256</code>."
     ],
     [
      "security.scan(host, ports = \"common\", { timeout = 500ms, concurrency = 200, banner })",
      "list · error",
      "Ports: an int, a list, a range or <code>\"22,80,8000-8100\"</code>. → <code>[{ port, service, banner }]</code>."
     ],
     [
      "security.dns(name, type = \"A\", { server, timeout = 3s }) · security.reverseDns(ip, opts?)",
      "list · error",
      "<code>A AAAA MX TXT NS CNAME SOA CAA SRV PTR…</code> → <code>[{ type, value, ttl }]</code>. Servers: <code>server</code>, <code>$FOGU_DNS</code>, the system's, then 1.1.1.1/8.8.8.8."
     ],
     [
      "security.headers(url, { timeout })",
      "map · error",
      "HSTS, CSP, X-Content-Type-Options, X-Frame-Options, Referrer-Policy, Permissions-Policy → <code>{ grade, score, present, missing, warnings }</code> (leaky <code>server</code>, unsafe CSP, cookies without Secure/HttpOnly/SameSite…)."
     ],
     [
      "security.passwordStrength(pw, [userInputs])",
      "map",
      "<code>{ score 0..4, guesses, crackTime, crackTimeOnline, warning, suggestions }</code>."
     ],
     [
      "security.totpSecret() · totp(secret, opts) · verifyTotp(secret, code, { window = 1 }) · otpauthUrl(secret, account, { issuer })",
      "",
      "RFC 6238 (SHA-1, 6 digits, 30 s); the URL goes into a QR code for authenticator apps."
     ],
     [
      "security.scanSecrets(text) · security.scanFiles(path)",
      "list",
      "AWS keys, GitHub/GitLab/Slack/Stripe/OpenAI/Anthropic tokens, private keys, JWTs, passwords in URLs, <code>password = \"…\"</code> → <code>[{ file?, rule, line, column, match }]</code> (redacted)."
     ],
     [
      "security.keypair() · sign(privateKey, msg) · verify(publicKey, msg, sig)",
      "",
      "Ed25519, base64 keys and signatures."
     ],
     [
      "security.constantTimeEq(a, b)",
      "bool",
      ""
     ],
     [
      "security.cidr(c) · inCidr(ip, c | [c]) · isPrivate(ip) · hosts(c, { limit })",
      "",
      ""
     ],
     [
      "security.rateLimiter(ratePerSecond, burst) → allow(key?, cost?) · remaining(key) · reset(key?)",
      "RateLimiter",
      "A token bucket per key (e.g. the client IP)."
     ]
    ],
    "example": "import security from \"fogu:security\"\nconst pair = security.selfSigned([\"localhost\", \"127.0.0.1\"], { days: 30 })\nconst cert = security.parseCert(pair.cert)?\nprint(cert.commonName, cert.names, cert.daysLeft, cert.selfSigned)\nprint(security.passwordStrength(\"password123\").score, security.passwordStrength(\"correct horse battery staple 9!\").score)\nconst secret = \"GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ\"\nprint(security.totp(secret, { time: 59, digits: 8 }), security.verifyTotp(secret, \"287082\", { time: 59 }))\nfor s in security.scanSecrets(\"aws_key = \\\"AKIAIOSFODNN7EXAMPLE\\\"\") { print(s) }\nconst k = security.keypair()\nprint(security.verify(k.publicKey, \"hello\", security.sign(k.privateKey, \"hello\")))\nprint(security.cidr(\"192.168.1.77/24\").network, security.inCidr(\"10.1.2.3\", \"10.0.0.0/8\"), security.isPrivate(\"8.8.8.8\"))\nconst rl = security.rateLimiter(1, 2)\nprint(rl.allow(\"ip\"), rl.allow(\"ip\"), rl.allow(\"ip\"))",
    "output": "localhost [localhost, 127.0.0.1] 29 true\n0 4\n94287082 true\n{column: 12, line: 1, match: AKIA…LE, rule: aws-access-key}\ntrue\n192.168.1.0 true false\ntrue true false",
    "outputTitle": "output"
   },
   {
    "example": "import security from \"fogu:security\"\nconst t = security.tls(\"example.com\")?\nprint(t.valid, t.protocol, t.daysLeft > 0, t.chain[0].issuer)\nprint(security.scan(\"127.0.0.1\", \"22,80,443,5432\")?)\nprint(security.dns(\"gmail.com\", \"MX\")?[0].type)\nconst h = security.headers(\"https://github.com\")?\nprint(h.grade, h.missing.map(fn(m) { return m.header }))",
    "output": "true TLSv1.3 true C=US, O=SSL Corporation, CN=Cloudflare TLS Issuing ECC CA 3\\n[{banner: nil, port: 22, service: ssh}]\\nMX\\nB [permissions-policy]",
    "outputTitle": "output (with network access)"
   }
  ]
 },
 "mod-websocket": {
  "id": "mod-websocket",
  "group": "Web & network",
  "title": "fogu:websocket",
  "import": "import websocket from \"fogu:websocket\"",
  "intro": "WebSocket client (<code>ws://</code>, <code>wss://</code>) and a standalone server. The same connection object is used by <code>app.ws</code> in <code>fogu:web</code>. Each connection runs on the shared runtime (no thread per connection): <code>next</code> and <code>accept</code> wait cooperatively and wake up when a message or a client arrives.",
  "blocks": [
   {
    "api": [
     [
      "websocket.connect(url, { headers, protocols, timeout })",
      "WebSocket · error",
      "A rejected handshake (401, 404…) is an error value."
     ],
     [
      "websocket.listen(port, { host })",
      "Server · error",
      "<code>port 0</code> picks a free port; <code>server.accept(timeout?)</code> → WebSocket, fields <code>port url</code>."
     ],
     [
      "ws.send(data)",
      "nil · error",
      "str → text frame, <a class=\"underline\" href=\"#values\">buffer</a> or list of bytes → binary frame, other values → JSON text."
     ],
     [
      "ws.next(timeout?)",
      "Message · nil",
      "<code>{ type: \"text\" | \"binary\", data, text, bytes, size, json() }</code>: <code>data</code> is a str for text frames and a buffer for binary ones, <code>text</code> always a str, <code>bytes</code> always a buffer. nil on timeout or when closed."
     ],
     [
      "ws.tryNext() · ws.ping() · ws.close(code = 1000, reason)",
      "",
      "<code>tryNext</code>: a queued message or nil, without waiting."
     ],
     [
      "ws.open · ws.closeCode · ws.closeReason · ws.path · ws.url · ws.headers · ws.protocol · ws.pending · ws.error",
      "",
      "<code>pending</code>: messages queued; <code>error</code>: why the connection failed, or nil."
     ]
    ],
    "example": "import websocket from \"fogu:websocket\"\nconst server = websocket.listen(0)?\nfg fn() {\n    const conn = server.accept()?\n    while true {\n        const m = conn.next()?\n        if m == nil { break }\n        if m.type == \"binary\" {\n            conn.send(m.data)                   // a buffer: echoed as a binary frame\n        } else {\n            conn.send(`echo ${conn.path}: ${m.data}`)\n        }\n    }\n}()\nconst ws = websocket.connect(server.url + \"/room?x=1\")?\nws.send(\"hello\")\nprint(ws.next(2s)?.data)\nws.send({ kind: \"json\" })\nprint(ws.next(2s)?.data)\nws.send(bytes([1, 2, 255]))\nconst m = ws.next(2s)?\nprint(m.type, m.data, m.size, typeof(m.text))\nws.close()",
    "output": "echo /room?x=1: hello\necho /room?x=1: {\"kind\":\"json\"}\nbinary <buffer 3 bytes: 01 02 ff> 3 str",
    "outputTitle": "output"
   }
  ]
 },
 "mod-net": {
  "id": "mod-net",
  "group": "Web & network",
  "title": "fogu:net",
  "import": "import net from \"fogu:net\"",
  "intro": "TCP and UDP sockets on the shared runtime: an operation that waits (<code>accept</code>, reads, writes, <code>recv</code>, <code>tcpConnect</code>) lets the other <code>fg</code> tasks run. Data is read as a <a class=\"underline\" href=\"#values\">buffer</a> (<code>read readExact readAll recv</code>) or as text (<code>readLine readText</code>); writes take a str, a buffer or a list of byte ints. Network failures are error values. Timeouts are durations or int milliseconds.",
  "blocks": [
   {
    "api": [
     [
      "net.tcpListen(\"host:port\")",
      "TcpListener · error",
      "Port 0 picks a free one. <code>accept(timeout?)</code> → TcpStream, <code>for conn in listener</code>, <code>setTimeout(d)</code>, <code>close()</code>; fields <code>addr localAddr port</code>."
     ],
     [
      "net.tcpConnect(\"host:port\", timeout?)",
      "TcpStream · error",
      ""
     ],
     [
      "stream.read(n = 64 KiB)",
      "buffer · nil · error",
      "What arrived (up to <code>n</code> bytes); <code>nil</code> at the end of the stream."
     ],
     [
      "stream.readExact(n) · stream.readAll()",
      "buffer · error",
      "Exactly <code>n</code> bytes · everything up to the end of the stream."
     ],
     [
      "stream.readLine() · stream.readText(n)",
      "str · nil · error",
      "A line without <code>\\n</code>/<code>\\r\\n</code> · up to <code>n</code> bytes as text; <code>nil</code> at the end of the stream."
     ],
     [
      "stream.write(data) · stream.writeLine(data)",
      "int · error",
      "Bytes written."
     ],
     [
      "stream.setTimeout(d) · stream.close()",
      "",
      "<code>setTimeout</code> bounds every later operation (a timeout is an error value)."
     ],
     [
      "stream.peer · stream.localAddr · stream.open",
      "",
      ""
     ],
     [
      "net.udpBind(\"host:port\")",
      "UdpSocket · error",
      "<code>sendTo(addr, data)</code>, <code>recvFrom(n?)</code> → <code>{ data, text, from, len }</code> (<code>data</code> is a buffer), <code>connect(addr)</code>, <code>send(data)</code>, <code>recv(n?)</code> → buffer, <code>setTimeout(d)</code>, <code>close()</code>; field <code>addr</code>."
     ],
     [
      "net.resolve(host)",
      "list · error",
      "IP addresses."
     ]
    ],
    "example": "import net from \"fogu:net\"\nconst server = net.tcpListen(\"127.0.0.1:0\")?\nfg fn() {\n    while true {\n        const conn = server.accept()?\n        fg fn() {                               // one task per connection\n            while true {\n                const data = conn.read()?       // a buffer · nil at end of stream\n                if data == nil { break }\n                conn.write(data)?\n            }\n            conn.close()\n        }()\n    }\n}()\n\nconst c = net.tcpConnect(server.addr, 2s)?\nc.setTimeout(2s)                                // bounds every later operation\nc.write(\"one\\ntwo\\n\")?\nprint(c.readLine()?, \"|\", c.readLine()?)\nc.write(bytes([0, 255, 10]))?\nconst raw = c.readExact(3)?\nprint(typeof(raw), raw.hex(), raw[1])\nc.close()\n\nconst a = net.udpBind(\"127.0.0.1:0\")?\nconst b = net.udpBind(\"127.0.0.1:0\")?\na.sendTo(b.addr, \"hello udp\")?\nconst p = b.recvFrom()?\nprint(p.text, p.len, typeof(p.data), p.from == a.addr)",
    "output": "one | two\nbuffer 00ff0a 255\nhello udp 9 buffer true",
    "outputTitle": "output"
   }
  ]
 },
 "mod-events": {
  "id": "mod-events",
  "group": "Messaging & events",
  "title": "fogu:events",
  "import": "import events from \"fogu:events\"",
  "intro": null,
  "blocks": [
   {
    "api": [
     [
      "events.EventEmitter()",
      "Emitter",
      ""
     ],
     [
      "e.on(event, fn) · e.once(event, fn)",
      "nil",
      "Listeners receive every emitted argument (declare them all)."
     ],
     [
      "e.off(event, fn?) · e.removeAll(event?)",
      "nil",
      ""
     ],
     [
      "e.emit(event, ...args)",
      "bool",
      "<code>true</code> when someone listened."
     ],
     [
      "e.listeners(event) · e.eventNames() · e.hasListeners(event)",
      "",
      ""
     ]
    ],
    "example": "import events from \"fogu:events\"\nconst bus = events.EventEmitter()\nbus.on(\"order\", (id, total) => print(\"order\", id, \"total\", total))\nbus.once(\"order\", (id, total) => print(\"first order only:\", id))\nprint(bus.emit(\"order\", 1, 9.9))\nprint(bus.emit(\"order\", 2, 15))\nprint(bus.emit(\"nobody-listens\"), bus.eventNames())",
    "output": "order 1 total 9.9\nfirst order only: 1\ntrue\nfalse\nfalse []",
    "outputTitle": "output"
   }
  ]
 },
 "mod-cron": {
  "id": "mod-cron",
  "group": "Messaging & events",
  "title": "fogu:cron",
  "import": "import cron from \"fogu:cron\"",
  "intro": "Jobs run when the program calls <code>cron.tick()</code> (typically in its main loop).",
  "blocks": [
   {
    "api": [
     [
      "cron.every(interval, fn)",
      "nil",
      "A job every <code>interval</code> (duration or ms)."
     ],
     [
      "cron.schedule(expression, fn)",
      "nil",
      "5-field cron expression (<code>\"*/5 * * * *\"</code>); an invalid one stops the program."
     ],
     [
      "cron.tick()",
      "int",
      "Runs the jobs that are due; returns how many ran."
     ],
     [
      "cron.list()",
      "list",
      "Job names (<code>every(10ms)</code>, the cron expression…)."
     ],
     [
      "cron.clear()",
      "nil",
      "Removes every job."
     ]
    ],
    "example": "import cron from \"fogu:cron\"\nimport time from \"fogu:time\"\nlet beats = 0\ncron.every(10ms, () => { beats = beats + 1 })\ncron.schedule(\"0 3 * * *\", () => print(\"nightly backup\"))\nfor i in 0..6 {\n    cron.tick()\n    time.delay(10ms)\n}\nprint(beats >= 3, cron.list())",
    "output": "true [every(10ms), schedule(0 3 * * *)]",
    "outputTitle": "output"
   }
  ]
 },
 "mod-nats": {
  "id": "mod-nats",
  "group": "Messaging & events",
  "title": "fogu:nats",
  "import": "import nats from \"fogu:nats\"",
  "intro": "NATS core protocol plus JetStream (streams, acknowledged publishing, pull consumers).",
  "blocks": [
   {
    "api": [
     [
      "nats.connect(url | { url, user, pass, token, name, timeout })",
      "Nats · error",
      "<code>nats://[user:pass@|token@]host:4222</code>, default <code>$NATS_URL</code>."
     ],
     [
      "nc.publish(subject, data?, { headers, reply })",
      "nil · error",
      "<code>data</code>: str, or any value as JSON."
     ],
     [
      "nc.subscribe(subject, { queue, max })",
      "Subscription",
      "Wildcards <code>*</code> and <code>&gt;</code>; queue groups split the messages."
     ],
     [
      "sub.next(timeout?) · sub.tryNext() · sub.unsubscribe()",
      "Message · nil",
      ""
     ],
     [
      "nc.request(subject, data?, { timeout, headers })",
      "Message · error",
      "No responders → error value."
     ],
     [
      "nc.flush() · nc.close() · nc.connected · nc.server",
      "",
      ""
     ],
     [
      "msg.subject · .data · .headers · .reply · .json() · .respond(data)",
      "",
      ""
     ]
    ]
   },
   {
    "h3": "JetStream",
    "api": [
     [
      "nc.jetstream({ prefix, timeout })",
      "JetStream",
      ""
     ],
     [
      "js.addStream({ name, subjects, storage, max_msgs, max_age, … })",
      "map · error",
      "NATS stream config (snake_case keys)."
     ],
     [
      "js.updateStream(cfg) · js.streamInfo(name) · js.deleteStream(name) · js.purgeStream(name)",
      "",
      ""
     ],
     [
      "js.publish(subject, data, { msgId, headers, timeout })",
      "{ stream, seq, duplicate }",
      "<code>msgId</code> deduplicates."
     ],
     [
      "js.consumer(stream, { durable, filter, ackWait, deliver, maxDeliver })",
      "Consumer · error",
      "A durable pull consumer."
     ],
     [
      "consumer.fetch(batch = 1, expires = 5s)",
      "list",
      "Up to <code>batch</code> messages."
     ],
     [
      "msg.ack() · msg.nak(delay?) · msg.term() · msg.inProgress()",
      "nil",
      "<code>msg.seq</code>, <code>msg.delivered</code>, <code>msg.stream</code>."
     ]
    ]
   },
   {
    "example": "import nats from \"fogu:nats\"\nconst nc = nats.connect(\"nats://127.0.0.1:4222\")?\nconst sub = nc.subscribe(\"orders.*\")\nnc.publish(\"orders.created\", { id: 1 })\nprint(sub.next(1s)?.json())\n\nconst svc = nc.subscribe(\"math.double\")\nfg fn() { const req = svc.next()?; req.respond(str(int(req.data) * 2)) }()\nprint(nc.request(\"math.double\", \"21\")?.data)\n\nconst js = nc.jetstream()\njs.addStream({ name: \"TASKS\", subjects: [\"tasks.>\"] })?\nprint(js.publish(\"tasks.email\", { to: \"ana@x.com\" }, { msgId: \"t1\" })?)\nconst c = js.consumer(\"TASKS\", { durable: \"workers\" })?\nfor msg in c.fetch(10, 1s)? { print(msg.subject, msg.seq); msg.ack() }",
    "output": "{id: 1}\n42\n{duplicate: false, seq: 1, stream: TASKS}\ntasks.email 1",
    "outputTitle": "output (with a NATS server, -js)"
   },
   {
    "h3": "fogu() — the high-level layer",
    "text": "The same API as the usual <code>nats.js</code> wrapper (KV stores, <code>sub</code>/<code>pub</code> with handlers, services with auth, commands); it interoperates with it — same KV buckets, same <code>service:&lt;name&gt;</code> subjects and error payloads. Data: nil is sent empty, a string as is, the rest as JSON; received data is parsed as JSON when it can be. Handlers run as <code>fg</code> tasks.",
    "api": [
     [
      "nats.fogu(url | config) · nc.fogu()",
      "NatsFogu · error",
      "Connects (or wraps an open connection)."
     ],
     [
      "f.store.createKV(name, fn?(entry), { history = 5, ttl })",
      "NatsKv · error",
      "A JetStream KV bucket (<code>KV_name</code>). With <code>fn</code>: a watch — the last value of each key, then every change: <code>{ bucket, key, value, operation: PUT|DEL|PURGE, revision }</code>; returning <code>false</code> stops it."
     ],
     [
      "f.store.kv(name) → create(key, data) · set · get · del · keys",
      "",
      "<code>create</code> → false when the key exists. Using a store before <code>createKV</code> is an error value."
     ],
     [
      "f.sub({ topic }, fn(data), { queue }) · f.pub({ topic }, data)",
      "nil",
      ""
     ],
     [
      "f.service(name, fn(input), auth?(headers), { queue })",
      "nil",
      "On <code>service:name</code>. <code>headers.get(n) · has(n)</code>. A falsy auth answers <code>{ error: \"unauthorized\", data: nil }</code>; a failing handler <code>{ error: \"internal_error\", data: nil }</code>; <code>nil</code> answers <code>{}</code>."
     ],
     [
      "f.command(name, data = {}, headers = {}, { timeout = 5000 ms })",
      "value · error",
      "No service → error value."
     ],
     [
      "f.connect(newConfig?) · f.close() · f.connection",
      "",
      "<code>connection</code> is the underlying <code>Nats</code>."
     ]
    ],
    "example": "import nats from \"fogu:nats\"\nconst f = nats.fogu(\"nats://127.0.0.1:4222\")?\nf.store.createKV(\"config\", fn(e) { print(\"watch\", e.operation, e.key, e.value) })?\nconst kv = f.store.kv(\"config\")\nkv.set(\"theme\", { dark: true })?\nprint(kv.get(\"theme\")?, kv.create(\"theme\", 1)?, kv.keys()?)\nf.sub({ topic: \"orders.*\" }, fn(data) { print(\"order\", data) })\nf.pub({ topic: \"orders.new\" }, { id: 1 })\nf.service(\"math.double\", fn(x) { return x * 2 }, fn(h) { return h.get(\"token\") == \"s3cret\" })\nprint(f.command(\"math.double\", 21, { token: \"s3cret\" })?)\nprint(f.command(\"math.double\", 21)?)",
    "output": "watch PUT theme {dark: true}\n{dark: true} false [theme]\norder {id: 1}\n42\n{data: nil, error: unauthorized}",
    "outputTitle": "output (with nats-server -js)"
   }
  ]
 },
 "mod-rabbitmq": {
  "id": "mod-rabbitmq",
  "group": "Messaging & events",
  "title": "fogu:rabbitmq",
  "import": "import rabbitmq from \"fogu:rabbitmq\"",
  "intro": "RabbitMQ (AMQP 0-9-1, own client on the runtime's networking; <code>amqps://</code> uses TLS). Publishing waits for the broker's confirmation. A channel closed by an error (for example declaring a queue with other options) is reopened on the next call.",
  "blocks": [
   {
    "api": [
     [
      "rabbitmq.connect(url?)",
      "RabbitMQ · error",
      "<code>amqp://user:pass@host:5672/vhost</code>, default <code>$AMQP_URL</code>."
     ],
     [
      "mq.exchange(name, kind = \"direct\", { durable, autoDelete, internal, arguments })",
      "nil · error",
      "<code>direct fanout topic headers</code>."
     ],
     [
      "mq.queue(name = \"\", { durable, exclusive, autoDelete, ttl, maxLength, deadLetter, passive })",
      "{ name, messages, consumers }",
      "\"\" gives a server-named queue."
     ],
     [
      "mq.bind(queue, exchange, key = \"\") · mq.unbind(…)",
      "nil · error",
      ""
     ],
     [
      "mq.publish(exchange, key, data, { persistent, headers, contentType, correlationId, replyTo, messageId, expiration, priority, mandatory })",
      "nil · error",
      "Maps are sent as JSON."
     ],
     [
      "mq.send(queue, data, opts?)",
      "nil · error",
      "Through the default exchange."
     ],
     [
      "mq.get(queue, { noAck })",
      "Message · nil",
      ""
     ],
     [
      "mq.consume(queue, { prefetch = 10, noAck, exclusive, tag })",
      "Consumer",
      "<code>next(timeout?)</code>, <code>tryNext()</code>, <code>cancel()</code>."
     ],
     [
      "msg.data · .json() · .routingKey · .exchange · .headers · .redelivered · …",
      "",
      "Properties: <code>contentType correlationId replyTo messageId type persistent</code>."
     ],
     [
      "msg.ack() · msg.nack({ requeue = true }) · msg.reject({ requeue })",
      "nil",
      ""
     ],
     [
      "mq.purge(queue) · mq.deleteQueue(name) · mq.deleteExchange(name) · mq.close()",
      "",
      ""
     ]
    ],
    "example": "import rabbitmq from \"fogu:rabbitmq\"\nconst mq = rabbitmq.connect(\"amqp://guest:guest@127.0.0.1:5672/%2f\")?\nmq.exchange(\"shop\", \"topic\", { durable: true })?\nmq.queue(\"emails\", { durable: true })?\nmq.bind(\"emails\", \"shop\", \"order.*\")?\nmq.publish(\"shop\", \"order.created\", { id: 7 }, { persistent: true })?\nconst worker = mq.consume(\"emails\", { prefetch: 5 })?\nconst msg = worker.next(2s)?\nprint(msg.routingKey, msg.json(), msg.persistent)\nmsg.ack()",
    "output": "order.created {id: 7} true",
    "outputTitle": "output (with RabbitMQ)"
   }
  ]
 },
 "mod-mqtt": {
  "id": "mod-mqtt",
  "group": "Messaging & events",
  "title": "fogu:mqtt",
  "import": "import mqtt from \"fogu:mqtt\"",
  "intro": "MQTT 3.1.1 and 5 (own client on the runtime's networking) with automatic reconnection (subscriptions are restored). QoS 0/1/2, retained messages, wills; with <code>version: 5</code> also message properties, subscription options and reason codes.",
  "blocks": [
   {
    "api": [
     [
      "mqtt.connect(url?, { clientId, username, password, keepAlive, clean = true, will, timeout, version = 3, sessionExpiry })",
      "Mqtt · error",
      "<code>mqtt://</code> or <code>mqtts://</code>; <code>will: { topic, payload, qos, retain }</code>; <code>version: 5</code> speaks MQTT 5. A refused connection is an error with the broker's reason."
     ],
     [
      "m.publish(topic, data, { qos = 0, retain })",
      "nil · error",
      "QoS 1/2 wait for the broker's acknowledgement."
     ],
     [
      "MQTT 5 publish options: { contentType, responseTopic, correlationData, messageExpiry, userProperties }",
      "",
      "Ignored on 3.1.1."
     ],
     [
      "m.subscribe(filter | [filters], { qos, noLocal, retainAsPublished, retainHandling }) · m.unsubscribe(…)",
      "nil · error",
      "<code>+</code> one level, <code>#</code> the rest; the last three are MQTT 5."
     ],
     [
      "m.next(timeout?) · m.tryNext()",
      "Message · nil",
      "<code>{ topic, data, qos, retain, json(), matches(filter) }</code>; MQTT 5 adds <code>contentType responseTopic correlationData messageExpiry userProperties</code>."
     ],
     [
      "m.connected · m.clientId · m.version · m.subscriptions · m.close()",
      "",
      ""
     ],
     [
      "mqtt.matches(filter, topic)",
      "bool",
      ""
     ]
    ],
    "example": "import mqtt from \"fogu:mqtt\"\nconst m = mqtt.connect(\"mqtt://127.0.0.1:1883\", { clientId: \"monitor\" })?\nm.subscribe(\"home/+/temperature\", { qos: 1 })?\nm.publish(\"home/kitchen/temperature\", \"21.5\", { qos: 1, retain: true })?\nconst msg = m.next(2s)?\nprint(msg.topic, msg.data, msg.qos, msg.matches(\"home/#\"))\nprint(mqtt.matches(\"a/+/c\", \"a/b/c\"))",
    "output": "home/kitchen/temperature 21.5 1 true\ntrue",
    "outputTitle": "output (with an MQTT broker)"
   }
  ]
 },
 "mod-broker": {
  "id": "mod-broker",
  "group": "Messaging & events",
  "title": "fogu:broker",
  "import": "import broker from \"fogu:broker\"   // also \"fogu:mq\"",
  "intro": "The Fogu broker inside the runtime: key/value store with TTLs, channels, pub/sub with wildcard topics (<code>chat.*</code>, <code>chat.**</code>), RPC (<code>act</code>/<code>call</code>), API keys with scoped permissions, persistence (SurrealKV), clusters and a web management UI (<code>b.admin</code>). It speaks the FoguMQ wire protocol, so the Bun, Go, Rust and Python clients (<code>fogu/client/</code>) and <a class=\"underline\" href=\"#mod-fogumq\">fogu:fogumq</a> talk to it. The inline client has the same API as a fogumq client, over an in-memory pipe.",
  "blocks": [
   {
    "api": [
     [
      "broker.start(addr = \"127.0.0.1:7222\", { token, db, node, peers, secret, log, purgeEvery = 60s })",
      "Broker · error",
      "<code>token</code>: the root token (default <code>$FOGU_ROOT_TOKEN</code> or a random one). <code>db</code>: a SurrealKV folder (KV and API keys survive restarts, written in order by a background writer); in memory without it. A redb file of an earlier version is converted (kept as <code>&lt;db&gt;.redb.bak</code>). Also <code>broker.broker(…)</code> / <code>mq.broker(…)</code>."
     ],
     [
      "b.client({ token, requestTimeout })",
      "FoguMQ",
      "The inline client (root by default): <code>kv</code>, <code>channel(n).join/send/onMessage/sub/pub/act/call</code>, <code>apikeys</code>."
     ],
     [
      "b.admin(port | \"host:port\", { user = \"admin\", password = b.token, host, title, theme })",
      "{ url, port } · error",
      "The management UI (fogu:web + <a class=\"underline\" href=\"#mod-topcoat\">fogu:topcoat</a>): a sign-in page (session cookie; HTTP basic auth also works for scripts), a live overview with traffic per second and trend lines, channels with their members, a key-value browser/editor and API keys (create, copy the token once, revoke), light/dark themes. <code>theme</code>: <code>system</code> (default), <code>light</code>, <code>dark</code>. Keep the program alive with <code>Fogu.signal.wait()</code>."
     ],
     [
      "b.inspect() · b.keys({ prefix, limit = 200 })",
      "map · list",
      "<code>inspect</code>: <code>{ node, address, started, clients, channels: [{ name, members }], topics: [{ pattern, subscribers }], actions, remoteActions: [{ method, node }], peers, keys, apiKeys, messages, publications, calls, pendingCalls, persistent }</code>. <code>keys</code> → <code>[{ key, value, ttl }]</code> (ttl in seconds)."
     ],
     [
      "b.stats() · b.close() · b.url · b.address · b.port · b.node · b.token",
      "",
      "<code>stats()</code>: <code>{ node, clients, channels, topics, actions, remoteActions, peers, keys }</code>. <code>close()</code> ends every connection and releases the db."
     ],
     [
      "broker.connect(url, opts)",
      "FoguMQ · error",
      "A remote broker (same as <code>fogumq.connect</code>)."
     ],
     [
      "cluster: { node, peers: [addr…], secret }",
      "",
      "Nodes link to the <code>peers</code> they are given (one side is enough) and authenticate with <code>secret</code> (default: the token). Channel messages and publications reach every node; <code>call</code> goes to the node that registered the method; KV writes made while the nodes are linked are copied to every node (last write wins; <code>incr</code> is atomic per node; there is no catch-up of older keys)."
     ]
    ],
    "example": "import broker from \"fogu:broker\"\nconst b = broker.start(\"127.0.0.1:0\", { token: \"root1\", node: \"n1\" })?\nconst c = b.client()\nc.kv.set(\"visits\", 10)?\nprint(c.kv.incr(\"visits\")?)\nconst room = c.channel(\"chat\")\nconst svc = room.act(\"add\")?\nfg fn() {\n    while true {\n        const [r, err] = svc.next()\n        if err != nil { break }          // the broker closed\n        r.reply(r.params.a + r.params.b)\n    }\n}()\nprint(room.call(\"add\", { a: 1, b: 2 })?)\nconst sub = room.sub(\"news\")?\nroom.pub(\"news\", { id: 1 })?\nprint(sub.next(1s))\n\n// a second node: RPC, publications and KV writes cross the link\nconst n2 = broker.start(\"127.0.0.1:0\", { token: \"root1\", node: \"n2\", peers: [b.address] })?\nFogu.sleep(300ms)\nconst c2 = n2.client()\nc.kv.set(\"shared\", \"from n1\")?          // written while the nodes are linked\nFogu.sleep(100ms)\nprint(c2.channel(\"chat\").call(\"add\", { a: 20, b: 22 })?, c2.kv.get(\"shared\")?)\nprint(n2.stats().remoteActions)\nn2.close()\nb.close()",
    "output": "11\n3\n{data: {id: 1}, topic: chat/news}\n42 from n1\n[chat/add]",
    "outputTitle": "output"
   }
  ]
 },
 "mod-kafka": {
  "id": "mod-kafka",
  "group": "Messaging & events",
  "title": "fogu:kafka",
  "import": "import kafka from \"fogu:kafka\"",
  "intro": "Apache Kafka (and Redpanda) in pure Rust (rskafka): topics, producing and consuming, compression, TLS and SASL. Values: a string is sent as UTF-8, other values as JSON; received records have <code>value</code> as text. Without a <code>partition</code>, records with a key go to <code>hash(key) % partitions</code> (same key, same order), the others round-robin. Consumers keep their own offsets — Kafka consumer groups (coordinated rebalancing, offsets stored in Kafka) are not supported.",
  "blocks": [
   {
    "api": [
     [
      "kafka.connect(brokers, { clientId, tls: true | { ca }, sasl: { mechanism, username, password }, timeout = 10s })",
      "Kafka · error",
      "<code>brokers</code>: a list or <code>\"h1:9092,h2:9092\"</code>. SASL: <code>PLAIN SCRAM-SHA-256 SCRAM-SHA-512</code>."
     ],
     [
      "k.topics() · k.createTopic(name, { partitions = 1, replication = 1 }) · k.deleteTopic(name) · k.offsets(topic)",
      "",
      "<code>createTopic</code> → false if it existed; <code>offsets</code> → <code>{ partition: { earliest, latest } }</code>."
     ],
     [
      "k.send(topic, value, { key, headers, partition, compression })",
      "{ partition, offset } · error",
      "<code>compression</code>: <code>none gzip lz4 snappy zstd</code>."
     ],
     [
      "k.sendBatch(topic, [value | { value, key, headers }], { partition, compression })",
      "list · error",
      ""
     ],
     [
      "k.consumer(topic, { from = \"latest\" | \"earliest\" | offset, partitions, maxWait = 500ms, maxBytes })",
      "KafkaConsumer · error",
      ""
     ],
     [
      "c.next(timeout = 1s) · c.poll(timeout = 1s) · c.seek(partition, offset) · c.offsets · c.close()",
      "",
      "<code>next</code> → a record or nil; <code>poll</code> → the records that arrived."
     ],
     [
      "record.topic · partition · offset · key · value · headers · timestamp · json() · bytes()",
      "",
      ""
     ]
    ],
    "example": "import kafka from \"fogu:kafka\"\nconst k = kafka.connect([\"localhost:9092\"])?\nk.createTopic(\"orders\", { partitions: 3 })?\nconst c = k.consumer(\"orders\", { from: \"earliest\" })?\nprint(k.send(\"orders\", { id: 1, total: 9.5 }, { key: \"client-7\", headers: { source: \"fogu\" } })?)\nconst r = c.next(3s)?\nprint(r.key, r.json(), r.headers)",
    "output": "{offset: 0, partition: 2}\\nclient-7 {id: 1, total: 9.5} {source: fogu}",
    "outputTitle": "output (with Kafka 3.9)"
   }
  ]
 },
 "mod-fogumq": {
  "id": "mod-fogumq",
  "group": "Messaging & events",
  "title": "fogu:fogumq",
  "import": "import fogumq from \"fogu:fogumq\"",
  "intro": "Client of the Fogu broker (binary protocol, JSON payloads): key/value store, channels, pub/sub, RPC and API keys. Methods that wait for the server let the other <code>fg</code> tasks run.",
  "blocks": [
   {
    "api": [
     [
      "fogumq.connect(\"fogu://token@host:port\", { timeout, requestTimeout })",
      "FoguMQ · error",
      "Fields <code>connected authenticated isRoot url</code>; <code>ping() close()</code>."
     ],
     [
      "mq.kv.get(key) · set(key, value, ttl?) · del(key) · exists(key) · incr(key, n?) · decr(key, n?)",
      "value · error",
      "<code>get</code> gives nil for a missing key."
     ],
     [
      "mq.channel(name)",
      "Channel",
      "<code>join(alias?) leave() send(data) members() list()</code>."
     ],
     [
      "channel.onMessage()",
      "Listener",
      "<code>next(timeout?)</code> → <code>{ channel, data, ... }</code>; <code>tryNext() close()</code>."
     ],
     [
      "channel.subscribe(topic) · channel.publish(topic, data)",
      "Subscription · error",
      "Topics live under the channel: <code>name/topic</code>. Short forms: <code>sub</code>, <code>pub</code>."
     ],
     [
      "channel.act(method)",
      "Service · error",
      "<code>next()</code> → Request with <code>method params reply(result) fail(msg)</code>."
     ],
     [
      "channel.call(method, params?, timeout?)",
      "value · error",
      ""
     ],
     [
      "mq.apikeys.create(name, { channels, kv, topics, rpc }) · delete(id) · get(id) · list()",
      "value · error",
      "Root token only."
     ]
    ],
    "example": "import fogumq from \"fogu:fogumq\"\nconst mq = fogumq.connect(Fogu.env.FOGUMQ_URL)?\nmq.kv.set(\"visits\", 1)?\nprint(mq.kv.incr(\"visits\")?)\nconst room = mq.channel(\"calc\")\nconst svc = room.act(\"sum\")?\nfg fn() { const r = svc.next(); r.reply(r.params.reduce((a, n) => a + n, 0)) }()\nprint(room.call(\"sum\", [1, 2, 3])?)",
    "output": "2\n6",
    "outputTitle": "output (with a Fogu broker)"
   }
  ]
 },
 "mod-foguMQ-client": {
  "id": "mod-foguMQ-client",
  "group": "Messaging & events",
  "title": "fogu:foguMQ-client",
  "import": "import mqc from \"fogu:foguMQ-client\"",
  "intro": "Client of the FlowMQ v6 wire protocol (length-prefixed binary frames): numbered channels, pub/sub with wildcard topics (<code>*</code> one token, <code>&gt;</code> the rest), RPC, job queues and API keys. Each method sends exactly one protocol frame. The socket runs on the shared I/O runtime; methods that wait for the broker let the other <code>fg</code> tasks run, so one task can answer the requests another makes. Messages, requests and jobs are queued in <code>Subscription</code> / <code>Service</code> / <code>Worker</code> objects. Network and broker failures are <code>error</code> values. Payloads: a str is sent as is, any other value as JSON; received payloads are strs (use <code>json.parse</code>). <code>FLOWMQ_DEBUG=1</code> prints protocol errors to stderr.",
  "blocks": [
   {
    "h3": "Connection and channels",
    "api": [
     [
      "mqc.connect(\"flowmq://token@host:7222?namespace=…\", { timeout, requestTimeout, clientId, autoReconnect })",
      "FoguMQClient · error",
      "URL query: <code>clientId namespace requestTimeout connectTimeout reconnectBackoff</code> (ms) <code>maxFrame maxChannels autoReconnect</code>. Defaults: port 7222, timeouts 5s. With <code>autoReconnect</code> the client reconnects and re-opens its channels, subscriptions, services and workers."
     ],
     [
      "mq.connected · namespace · authMode · keyId · sessionId · url",
      "",
      "From the broker's HELLO_OK. <code>authMode</code>: <code>master</code>, an API key mode, or <code>unknown</code>."
     ],
     [
      "mq.channelOpen(ch) · mq.channelClose(ch)",
      "nil · error",
      "Channels are ints 1..65535 and must be open before use; opening one twice is an error."
     ],
     [
      "mq.close()",
      "nil",
      "Sends CLOSE and ends the connection."
     ]
    ]
   },
   {
    "h3": "Pub/sub",
    "api": [
     [
      "mq.subscribe(ch, pattern, group?)",
      "Subscription · error",
      "<code>next(timeout?)</code> → <code>{ topic, group, data }</code> (nil after the timeout); <code>tryNext()</code>; fields <code>pattern group channel pending</code>; <code>close()</code> unsubscribes."
     ],
     [
      "mq.unsubscribe(ch, pattern, group?)",
      "nil · error",
      ""
     ],
     [
      "mq.publish(ch, topic, data)",
      "nil · error",
      "Fire and forget."
     ],
     [
      "mq.publishSync(ch, topic, data)",
      "nil · error",
      "Waits for the broker's ACK."
     ]
    ]
   },
   {
    "h3": "RPC",
    "api": [
     [
      "mq.serve(ch, service, topic)",
      "Service · error",
      "<code>next(timeout?)</code> → Request with <code>service topic data answered</code> and <code>reply(data) fail(msg)</code>. A request dropped without an answer fails on the caller's side. <code>close()</code> unserves."
     ],
     [
      "mq.unserve(ch, service, topic)",
      "nil · error",
      ""
     ],
     [
      "mq.request(ch, service, topic, data)",
      "str · error",
      "The reply payload; a <code>fail</code> or a missing handler is a broker error."
     ]
    ]
   },
   {
    "h3": "Job queues",
    "api": [
     [
      "mq.registerWorker(ch, queue, { filter = \">\", prefetch = 1, visibility = 30s })",
      "Worker · error",
      "<code>next(timeout?)</code> → Job with <code>id attempt deadline queue type data channel done</code>. <code>close()</code> sends WORKER_OFF."
     ],
     [
      "job.ack() · job.nack({ requeue = true, delay, reason }) · job.extend(duration)",
      "nil",
      "A job dropped without an answer is nacked and requeued."
     ],
     [
      "mq.workerOff(ch, queue)",
      "nil · error",
      ""
     ],
     [
      "mq.enqueue(ch, queue, type, data, { jobId, priority, delay, retries, ttl })",
      "{ jobId, seq } · error",
      "<code>delay</code> and <code>ttl</code>: durations or ms."
     ]
    ]
   },
   {
    "h3": "API keys",
    "api": [
     [
      "mq.apikeyCreate(ch, name, permissions, labels?)",
      "{ id, token, permissions } · error",
      "<code>permissions</code> is sent as JSON, e.g. <code>{ channels, services, queues, publish, subscribe, worker, enqueue }</code>; the key is created in the client's namespace."
     ],
     [
      "mq.apikeyRevoke(ch, id) · mq.apikeyGet(ch, id) · mq.apikeyList(ch)",
      "nil · str · str · error",
      "<code>get</code>/<code>list</code> return the broker's JSON text. All API key operations need a master token."
     ]
    ],
    "example": "import mqc from \"fogu:foguMQ-client\"\nimport json from \"fogu:json\"\nconst mq = mqc.connect(Fogu.env.FLOWMQ_URL)?\nmq.channelOpen(1)?\nmq.channelOpen(2)?\nconst sub = mq.subscribe(2, \"orders.>\")?\nmq.publishSync(1, \"orders.created\", { id: 42 })?\nprint(json.parse(sub.next(2s).data))\nconst svc = mq.serve(2, \"users\", \"get\")?\nfg fn() { const r = svc.next(); r.reply(\"user \" + r.data) }()\nprint(mq.request(1, \"users\", \"get\", \"42\")?)\nconst jobs = mq.registerWorker(2, \"emails\")?\nmq.enqueue(1, \"emails\", \"welcome\", \"ana@example.com\")?\nconst job = jobs.next(5s)\nprint(job.type, job.data)\njob.ack()\nmq.close()",
    "output": "{id: 42}\nuser 42\nwelcome ana@example.com",
    "outputTitle": "output (with a FlowMQ v6 broker)"
   }
  ]
 },
 "mod-bullmq": {
  "id": "mod-bullmq",
  "group": "Messaging & events",
  "title": "fogu:bullmq",
  "import": "import bullmq from \"fogu:bullmq\"",
  "intro": "Job queues on Redis, wire-compatible with <a class=\"underline\" href=\"https://docs.bullmq.io\">BullMQ</a> for Node.js v5 (on the <code>bullmq-rs</code> crate): Fogu and Node producers and workers can share the same queues, and dashboards such as Bull Board see them. The worker owns locks, stalled-job recovery, retries with backoff, concurrency and rate limits; each job is handed to the program, which finishes it with <code>complete</code> or <code>fail</code>. Redis failures are <code>error</code> values.",
  "blocks": [
   {
    "h3": "Queue",
    "api": [
     [
      "bullmq.queue(name, { connection, prefix = \"bull\" })",
      "BullQueue · error",
      "<code>connection</code> is a <code>redis://</code> / <code>rediss://</code> URL (default <code>$REDIS_URL</code> or <code>redis://127.0.0.1:6379</code>)."
     ],
     [
      "q.add(name, data, opts?)",
      "job · error",
      "Options: <code>delay attempts backoff: { type: \"fixed\" | \"exponential\", delay, max } priority jobId ttl deduplication: { id, ttl }</code>."
     ],
     [
      "q.addBulk([{ name, data, opts }])",
      "list · error",
      ""
     ],
     [
      "q.getJob(id) · q.getJobs(states, start = 0, end = -1, asc = false)",
      "job | nil · list",
      "A job: <code>{ id name data state timestamp attemptsMade progress processedOn finishedOn failedReason returnvalue opts }</code>."
     ],
     [
      "q.counts() · q.count()",
      "map · int",
      "Per state: <code>wait active delayed prioritized completed failed paused waiting-children</code>."
     ],
     [
      "q.upsertJobScheduler(id, { every | pattern, tz, limit, immediately }, { name, data, opts })",
      "job · error",
      "Repeatable jobs (cron <code>pattern</code> or <code>every</code> duration)."
     ],
     [
      "q.removeJobScheduler(id)",
      "bool · error",
      ""
     ],
     [
      "q.pause() · resume() · isPaused() · drain() · remove(id) · clean(grace, limit, state) · obliterate({ force }) · getJobLogs(id)",
      "",
      ""
     ]
    ]
   },
   {
    "h3": "Worker and events",
    "text": "Limitation: the worker of <code>bullmq-rs</code> publishes <code>completed</code> events with an empty return value; <code>q.getJob(id).returnvalue</code> has the value given to <code>complete</code>.",
    "api": [
     [
      "bullmq.worker(name, { connection, prefix, concurrency = 1, lockDuration, maxStalledCount, limiter: { max, duration } })",
      "BullWorker · error",
      ""
     ],
     [
      "w.next(timeout?) · w.tryNext()",
      "BullJob | nil · error",
      "Waits cooperatively (other <code>fg</code> tasks keep running); nil on timeout or after <code>close()</code>."
     ],
     [
      "job.complete(value?) · job.fail(reason)",
      "nil",
      "<code>fail</code> retries the job while <code>attempts</code> remain. A job dropped without an answer fails."
     ],
     [
      "job.progress(value) · job.log(text)",
      "nil",
      "Fields are those of the job map, plus <code>done</code>."
     ],
     [
      "w.pause() · w.resume() · w.close() · w.running · w.pending",
      "",
      ""
     ],
     [
      "bullmq.events(name, { connection, prefix, lastEventId })",
      "BullEvents · error",
      "<code>next(timeout?)</code> → <code>{ event, jobId, id, … }</code> (<code>added waiting active progress completed failed delayed stalled removed</code>)."
     ]
    ],
    "example": "import bullmq from \"fogu:bullmq\"\nconst conn = { connection: \"redis://127.0.0.1:6379\" }\nconst q = bullmq.queue(\"emails\", conn)?\nq.add(\"welcome\", { to: \"ana@x.com\" }, { attempts: 3, backoff: { type: \"exponential\", delay: 1s } })?\nconst w = bullmq.worker(\"emails\", { connection: conn.connection, concurrency: 4 })?\nwhile true {\n    const job = w.next()?\n    print(job.name, job.data.to, job.attemptsMade)\n    job.progress(50)\n    job.complete({ sent: true })          // or job.fail(\"smtp down\") → retried\n}",
    "output": "welcome ana@x.com 0",
    "outputTitle": "output (with Redis)"
   }
  ]
 },
 "mod-mail": {
  "id": "mod-mail",
  "group": "Integrations",
  "title": "fogu:mail",
  "import": "import mail from \"fogu:mail\"",
  "intro": "E-mail over SMTP (presets for Gmail, Outlook, Office 365, Yahoo, iCloud, Zoho, Fastmail, AOL, GMX, Amazon SES, Mailtrap, Mailpit) or the HTTP APIs of Resend, SendGrid, Mailgun, Postmark, Brevo, MailerSend and Mailjet. The SMTP client and the MIME builder are Fogu's own (STARTTLS, implicit TLS, AUTH PLAIN/LOGIN, attachments, inline images, UTF-8 headers). The <code>console</code> provider only prints (development and tests).",
  "blocks": [
   {
    "api": [
     [
      "mail.connect({ provider, … } | url)",
      "Mailer",
      "No connection is made yet."
     ],
     [
      "SMTP: { provider: \"gmail\" | \"smtp\" | …, host, port, secure, user, password, region }",
      "",
      "<code>secure</code>: <code>tls</code> (465), <code>starttls</code> (587), <code>none</code>; URLs <code>smtp://user:pass@host:587</code>, <code>smtps://…</code>."
     ],
     [
      "APIs: { provider: \"resend\" | \"sendgrid\" | …, apiKey, domain, region, baseUrl }",
      "",
      "<code>apiKey</code> defaults to <code>$RESEND_API_KEY</code>, <code>$SENDGRID_API_KEY</code>…; Mailgun needs <code>domain</code>, Mailjet <code>apiSecret</code>."
     ],
     [
      "common: { from, timeout }",
      "",
      "Default sender."
     ],
     [
      "mailer.send(message)",
      "{ id, provider } · error",
      "A rejected message, a bad address or a network failure is an error value."
     ],
     [
      "mailer.sendMany([messages])",
      "list · error",
      ""
     ],
     [
      "mailer.verify()",
      "bool · error",
      "SMTP: connects and authenticates."
     ],
     [
      "mailer.render(message)",
      "str",
      "The RFC 5322 text that SMTP would send."
     ],
     [
      "message: { from, to, cc, bcc, replyTo, subject, text, html, attachments, headers }",
      "",
      "Addresses: <code>\"a@x.com\"</code>, <code>\"Ana &lt;a@x.com&gt;\"</code>, <code>{ name, email }</code> or a list."
     ],
     [
      "attachment: { filename, content | path, contentType, cid, encoding }",
      "",
      "<code>content</code>: str or list of bytes; <code>cid</code> makes it inline (<code>&lt;img src=\"cid:logo\"&gt;</code>)."
     ]
    ],
    "example": "import mail from \"fogu:mail\"\nconst mailer = mail.connect({ provider: \"console\", from: \"Shop <shop@example.com>\" })?\nconst r = mailer.send({\n    to: [\"ana@example.com\", { name: \"Bob\", email: \"bob@example.com\" }],\n    subject: \"Your order\",\n    text: \"Thanks for buying!\",\n    attachments: [{ filename: \"invoice.csv\", content: \"item,price\\nbook,10\\n\" }],\n})?\nprint(r)\nconst gmail = mail.connect({ provider: \"gmail\", user: \"me@gmail.com\", password: \"app-password\" })?\nprint(gmail.host, gmail.port, gmail.secure)\nconst [_, err] = mailer.send({ to: \"not-an-address\", subject: \"x\", text: \"x\" })\nprint(err)",
    "output": "--- mail ---\nFrom: Shop <shop@example.com>\nTo: ana@example.com, Bob <bob@example.com>\nSubject: Your order\nAttachment: invoice.csv (text/csv, 19 bytes)\n\nThanks for buying!\n---\n{id: console-1, provider: console}\nsmtp.gmail.com 587 starttls\nerror: mail: invalid address 'not-an-address' in 'to'",
    "outputTitle": "output"
   }
  ]
 },
 "mod-ai": {
  "id": "mod-ai",
  "group": "Integrations",
  "title": "fogu:ai",
  "import": "import ai from \"fogu:ai\"",
  "intro": "LLMs over HTTP: OpenAI-compatible APIs (OpenAI, Groq, OpenRouter, DeepSeek, Mistral, xAI, Together, Fireworks, Perplexity, Ollama, LM Studio), Anthropic and Google Gemini — chat, streaming, tools, agents, embeddings, and MCP: serve tools, resources and prompts to Claude and other clients, or use any MCP server's tools in an agent. Keys default to <code>$OPENAI_API_KEY</code>, <code>$ANTHROPIC_API_KEY</code>, <code>$GEMINI_API_KEY</code>…",
  "blocks": [
   {
    "h3": "Client",
    "api": [
     [
      "ai.client({ provider, model, apiKey, baseUrl, timeout, headers, maxTokens } | \"provider/model\")",
      "AiClient",
      "Provider <code>compatible</code> + <code>baseUrl</code> for any OpenAI-style server."
     ],
     [
      "client.chat(prompt | { messages, prompt, system, model, temperature, maxTokens, topP, stop, tools, toolChoice, json })",
      "map · error",
      "<code>{ text, toolCalls, finishReason, usage: { inputTokens, outputTokens }, model, message }</code>."
     ],
     [
      "client.stream(same)",
      "AiStream · error",
      "<code>next(timeout?)</code> → the next piece of text or nil at the end; <code>collect()</code> → like <code>chat</code>; fields <code>text done result</code>."
     ],
     [
      "client.models()",
      "list · error",
      "<code>[{ id, name }]</code>."
     ],
     [
      "client.embed(text | [texts], { model })",
      "list · error",
      "Vectors (OpenAI-style and Gemini)."
     ],
     [
      "message: { role: \"system\" | \"user\" | \"assistant\" | \"tool\", content, toolCalls, toolCallId, name }",
      "",
      "<code>result.message</code> can be appended to continue the conversation."
     ]
    ]
   },
   {
    "h3": "Tools and agents",
    "text": "A tool that returns an <code>error</code> value sends <code>error: …</code> back to the model, which can recover.",
    "api": [
     [
      "ai.tool(name, description, parameters, run?)",
      "map",
      "<code>parameters</code> is a JSON schema; also a plain map <code>{ name, description, parameters, run }</code>."
     ],
     [
      "ai.agent({ client, model, instructions, tools, maxSteps = 10, temperature, maxTokens, memory, onStep })",
      "Agent",
      "Tools need <code>run(args)</code>."
     ],
     [
      "agent.run(input)",
      "map · error",
      "Calls the model and the tools until it answers: <code>{ text, steps, messages, usage, finishReason }</code>."
     ],
     [
      "agent.reset() · agent.history",
      "",
      "With <code>memory: true</code> the conversation continues across runs."
     ]
    ],
    "example": "import ai from \"fogu:ai\"\nconst llm = ai.client({ provider: \"anthropic\" })?          // $ANTHROPIC_API_KEY\nprint(llm.chat(\"Say hi in Portuguese, one word\")?.text)\n\nconst s = llm.stream(\"Count to 3\")?\nwhile true { const t = s.next()?; if t == nil { break }; Fogu.write(t) }\nprint()\n\nconst weather = ai.tool(\"weather\", \"Current temperature of a city\",\n    { type: \"object\", properties: { city: { type: \"string\" } }, required: [\"city\"] },\n    (args) => ({ city: args.city, celsius: 21 }))\nconst agent = ai.agent({ client: llm, instructions: \"Use the tools\", tools: [weather] })\nconst answer = agent.run(\"Is it warm in Lisbon?\")?\nprint(answer.text)\nprint(answer.steps[0].toolCalls[0].name, answer.steps[0].toolCalls[0].arguments)",
    "output": "Olá\n1, 2, 3\nYes — it is 21 °C in Lisbon, a pleasant temperature.\nweather {city: Lisbon}",
    "outputTitle": "output (the model's words vary)"
   },
   {
    "h3": "MCP servers and clients",
    "api": [
     [
      "ai.mcpServer({ name, version, title, instructions })",
      "McpServer",
      "A <a class=\"underline\" href=\"https://modelcontextprotocol.io\">Model Context Protocol</a> server (protocol 2025-06-18, also 2025-03-26 and 2024-11-05) that Claude Desktop, Claude Code, IDEs and agents can use."
     ],
     [
      "server.tool(ai.tool(…)) · server.tool(name, description, schema, run)",
      "McpServer",
      "A tool; <code>run(args)</code> returns a value (sent as text, maps also as <code>structuredContent</code>) or an <code>error</code> (sent as <code>isError</code>). A runtime error in a tool is reported to the client, the server keeps running."
     ],
     [
      "server.resource(uri, { name, description, mimeType }, fn() → text) · server.prompt(name, { description, arguments: [{ name, required }] }, fn(args) → text | [{ role, text }])",
      "McpServer",
      ""
     ],
     [
      "server.serveStdio()",
      "nil",
      "Serves on stdin/stdout until stdin closes — the transport of local servers. Do not <code>print</code> anything else in this mode (use <code>log</code> on stderr)."
     ],
     [
      "server.listen(port, { path = \"/mcp\", host = \"127.0.0.1\", token, origins })",
      "int · error",
      "Streamable HTTP (JSON answers). <code>token</code> requires <code>Authorization: Bearer</code>; requests from browsers are only accepted from <code>localhost</code> or <code>origins</code>."
     ],
     [
      "server.handle(message)",
      "map · list · nil",
      "One JSON-RPC message (str or map) → the answer: tests, or another transport."
     ],
     [
      "ai.mcpClient({ command, args, env, cwd, stderr } | { url, headers }, { timeout = 30s })",
      "McpClient · error",
      "Starts a local server (stdio) or connects to a remote one (Streamable HTTP, JSON or SSE answers); fields <code>serverInfo protocolVersion capabilities instructions closed</code>."
     ],
     [
      "mcp.listTools() · callTool(name, args) · listResources() · readResource(uri) · listPrompts() · getPrompt(name, args) · ping() · close()",
      "",
      "<code>callTool</code> → the structured result, else the text; a tool's <code>isError</code> is an error value."
     ],
     [
      "mcp.tools()",
      "list",
      "The server's tools as <code>ai.tool</code> values: <code>ai.agent({ client, tools: mcp.tools()? })</code> lets a model use any MCP server."
     ]
    ],
    "example": "import ai from \"fogu:ai\"\nimport json from \"fogu:json\"\n\nconst mcp = ai.mcpServer({ name: \"notes\", version: \"1.0.0\" })\nmcp.tool(\"add\", \"Adds two numbers\", { type: \"object\", properties: { a: { type: \"number\" }, b: { type: \"number\" } } },\n    (args) => args.a + args.b)\nmcp.resource(\"notes://today\", { name: \"Today\" }, () => \"- ship MCP\")\n\n// what a client sends (mcp.serveStdio() or mcp.listen(8000) serve it for real)\nconst call = (method, params) => mcp.handle({ jsonrpc: \"2.0\", id: 1, method: method, params: params })\nprint(call(\"initialize\", { protocolVersion: \"2025-06-18\" }).result.serverInfo)\nprint(json.stringify(call(\"tools/call\", { name: \"add\", arguments: { a: 2, b: 3 } }).result.content))\nprint(call(\"resources/read\", { uri: \"notes://today\" }).result.contents[0].text)",
    "output": "{name: notes, version: 1.0.0}\n[{\"text\":\"5\",\"type\":\"text\"}]\n- ship MCP",
    "outputTitle": "output"
   }
  ]
 },
 "mod-ssh": {
  "id": "mod-ssh",
  "group": "Integrations",
  "title": "fogu:ssh",
  "import": "import ssh from \"fogu:ssh\"",
  "intro": "Commands, file copies and tunnels over the system's OpenSSH client (key or password).",
  "blocks": [
   {
    "api": [
     [
      "ssh.connect(\"user@host:port\" | { host, user, port, key, password, strictHostKey, knownHosts, timeout, options, jump })",
      "Ssh · error",
      "Checks the connection; <code>strictHostKey</code> defaults to <code>accept-new</code>; <code>jump</code> (another connection or <code>\"user@host\"</code>) goes through a bastion with that connection's key and settings."
     ],
     [
      "s.run(command, { input })",
      "{ ok, code, stdout, stderr } · error",
      "A non-zero exit code is not an error."
     ],
     [
      "s.upload(local, remote) · s.download(remote, local)",
      "nil · error",
      "scp, folders included."
     ],
     [
      "s.forward(localPort | 0, host, port)",
      "Tunnel · error",
      "<code>-L</code>: a local port that reaches <code>host:port</code> through the server."
     ],
     [
      "s.reverse(remotePort, host, localPort)",
      "Tunnel · error",
      "<code>-R</code>: a port on the server that reaches a local service."
     ],
     [
      "tunnel.localPort · .remotePort · .alive() · .close()",
      "",
      ""
     ],
     [
      "s.mount(remote, local, { readonly, keep, options, tunnel, jump })",
      "Mount · error",
      "Like sshfs: the remote folder appears as a local folder. Uses the connection's user, port, key (or password, sent on stdin), <code>knownHosts</code> and <code>strict</code>; <code>options</code> are extra <code>-o</code> sshfs options. Needs <code>sshfs</code> (Linux: <code>apt install sshfs</code>; macOS: macFUSE + sshfs; or <code>$FOGU_SSHFS</code>). <code>tunnel</code>: through an open <code>s.forward(…)</code>; <code>jump</code>: through a bastion. Unmounted when the program ends — also on Ctrl+C/SIGTERM — unless <code>keep</code>. Windows: SSHFS-Win (+ WinFsp), the local side is a drive letter (<code>\"X:\"</code>), only <code>%USERPROFILE%\\.ssh\\id_rsa</code> or a password, no <code>readonly</code>."
     ],
     [
      "mount.path · .remote · .host · .mounted · .unmount()",
      "",
      "A missing remote folder, a refused key or a busy mount point is an error value."
     ],
     [
      "s.shell({ pty, shell = \"sh\", cwd, env })",
      "SshShell · error",
      "One persistent remote shell: <code>sh.run(cmd, { timeout })</code> → <code>{ ok, code, stdout, stderr }</code> keeps the working directory and variables between commands (one command at a time per shell)."
     ],
     [
      "sh.send(text) · sh.expect(text | regex, { timeout = 30s }) · sh.read({ timeout = 200ms }) · sh.alive() · sh.close()",
      "",
      "Expect-style control of programs that ask questions; with <code>pty: true</code> the remote side gets a terminal (sudo prompts…)."
     ],
     [
      "s.interactive(command?)",
      "int · error",
      "Hands the user's terminal to the remote shell (<code>ssh -t</code>); returns its exit code."
     ],
     [
      "FOGU_SSH",
      "",
      "The ssh client to use (another build, a wrapper)."
     ],
     [
      "s.close()",
      "nil",
      ""
     ]
    ],
    "example": "import ssh from \"fogu:ssh\"\nimport fs from \"fogu:fs\"\nconst s = ssh.connect(\"deploy@server.example.com:22\", { key: \"~/.ssh/id_ed25519\" })?\nprint(s.run(\"uname -s\")?.stdout.trim())\ns.upload(\"dist/app\", \"/srv/app\")?\nconst db = s.forward(0, \"127.0.0.1\", 5432)?          // the server's PostgreSQL, locally\nprint(\"postgres://app@127.0.0.1:\" + str(db.localPort) + \"/app\")\ndb.close()\nconst www = s.mount(\"/var/www\", \"./www\", { readonly: true })?   // a local folder backed by the server\nprint(fs.list(\"./www\")?)\nwww.unmount()?\ns.close()",
    "output": "Linux\npostgres://app@127.0.0.1:53110/app\n[index.html, static]",
    "outputTitle": "output (with an SSH server)"
   },
   {
    "example": "import ssh from \"fogu:ssh\"\nconst s = ssh.connect(\"deploy@server.example.com\")?\nconst sh = s.shell({ cwd: \"/srv/app\", env: { APP_ENV: \"prod\" } })?\nsh.run(\"git pull -q && export VERSION=$(git rev-parse --short HEAD)\")?\nprint(sh.run(\"echo $APP_ENV $VERSION; pwd\")?.stdout)\nconst t = s.shell({ pty: true })?\nt.send(\"sudo -k; sudo systemctl restart app\\n\")\nt.expect(\"password\")?\nt.send(Fogu.env.SUDO_PASS + \"\\n\")\nprint(t.expect(\"\\\\$ \")?.contains(\"restart\"))",
    "output": "prod 3f9c2a1\n/srv/app\n\ntrue",
    "outputTitle": "output (with an SSH server)"
   }
  ]
 },
 "mod-shell": {
  "id": "mod-shell",
  "group": "Integrations",
  "title": "fogu:shell",
  "import": "import shell from \"fogu:shell\"",
  "intro": "A small POSIX-style shell: pipes, <code>&amp;&amp;</code> <code>||</code> <code>;</code>, redirections (<code>&gt; &gt;&gt; &lt; 2&gt; 2&gt;&amp;1 &amp;&gt;</code>), <code>$VAR</code>, <code>$(…)</code>, quotes, globs and <code>~</code>. The usual Linux commands are built in, so scripts behave the same on Windows: <code>echo cat ls pwd cd mkdir rm cp mv touch grep head tail wc sort uniq cut tr tee basename dirname which env export true false exit sleep seq</code>. Other commands run as programs.",
  "blocks": [
   {
    "api": [
     [
      "shell.run(command, { cwd, env, input, native = true })",
      "{ ok, code, stdout, stderr }",
      "A non-zero exit is not an error; <code>native: false</code> uses system programs only."
     ],
     [
      "shell.exec(command, opts?)",
      "same",
      "Output goes to the terminal as it happens."
     ],
     [
      "shell.text(command, opts?)",
      "str · error",
      "stdout without the final newline; a failure is an error value."
     ],
     [
      "shell.lines(command, opts?) · shell.json(command, opts?)",
      "list · value",
      ""
     ],
     [
      "shell.quote(text)",
      "str",
      "Safe to interpolate into a command."
     ],
     [
      "shell.cd(dir) · shell.cwd()",
      "nil · str",
      "The shell's own working folder."
     ],
     [
      "shell.which(program) · shell.builtins",
      "str · list",
      ""
     ]
    ],
    "example": "import shell from \"fogu:shell\"\nshell.run(\"mkdir -p data && echo banana > data/fruits.txt && echo apple >> data/fruits.txt\")?\nprint(shell.text(\"cat data/fruits.txt | sort | head -n 1\")?)\nprint(shell.lines(\"ls data\")?, shell.text(\"cat data/*.txt | wc -l\")?)\nconst name = \"file with spaces.txt\"\nshell.run(`echo ok > data/${shell.quote(name)}`)?\nprint(shell.text(\"echo \\\"$WHO has $(cat data/fruits.txt | wc -l) fruits\\\"\", { env: { WHO: \"Ana\" } })?)\nconst r = shell.run(\"grep kiwi data/fruits.txt || echo 'no kiwi'\")?\nprint(r.ok, r.stdout.trim())\nconst [_, err] = shell.text(\"cat missing.txt\")\nprint(err)",
    "output": "apple\n[fruits.txt] 2\nAna has 2 fruits\ntrue no kiwi\nerror: shell.text: exit code 1: cat: missing.txt: No such file or directory",
    "outputTitle": "output"
   }
  ]
 },
 "mod-ffi": {
  "id": "mod-ffi",
  "group": "Integrations",
  "title": "fogu:ffi",
  "import": "import ffi from \"fogu:ffi\"",
  "intro": "Calls functions of native libraries (C ABI): <code>.dll</code>, <code>.so</code>, <code>.dylib</code>. <strong>Unsafe by nature</strong>: a wrong signature can crash the program. Supported signatures: up to 4 arguments mixing integers and floats, or up to 8 integer/pointer arguments.",
  "blocks": [
   {
    "api": [
     [
      "ffi.open(name)",
      "Library · error",
      "A path or a name; <code>\"c\"</code> and <code>\"m\"</code> are the C runtime and the math library. The same name works on every platform: without an extension it gets the platform's prefix and suffix (<code>\"native/mylib\"</code> → <code>native/libmylib.so</code> on Linux, <code>native/libmylib.dylib</code> on macOS, <code>native/mylib.dll</code> on Windows), and another platform's extension is mapped to the native one (<code>\"mylib.dll\"</code> on Linux also tries <code>libmylib.so</code>). The error lists every file tried."
     ],
     [
      "lib.func(symbol, [argTypes], returnType = \"void\")",
      "fn · error",
      "A Fogu function that calls the symbol."
     ],
     [
      "lib.has(symbol) · lib.name",
      "bool · str",
      ""
     ],
     [
      "types",
      "",
      "<code>i8 i16 i32 i64 u8 u16 u32 u64 isize usize bool f64 ptr str buffer</code>; results also <code>f32 void</code>. <code>str</code> passes a NUL-terminated copy; a <code>str</code> result reads a C string."
     ],
     [
      "ffi.alloc(size)",
      "Buffer",
      "Memory for out-parameters: <code>text() bytes(start?, len?) write(offset, str | bytes) i32/u32/i64/f64(offset)</code>, fields <code>size address</code>."
     ],
     [
      "ffi.types · ffi.suffix",
      "list · str",
      ""
     ]
    ],
    "example": "import ffi from \"fogu:ffi\"\nconst libc = ffi.open(\"c\")?\nconst strlen = libc.func(\"strlen\", [\"str\"], \"usize\")?\nconst atoi = libc.func(\"atoi\", [\"str\"], \"i32\")?\nprint(strlen(\"hello\"), strlen(\"olá\"), atoi(\"42abc\"))\nconst pow = ffi.open(\"m\")?.func(\"pow\", [\"f64\", \"f64\"], \"f64\")?\nprint(pow(2.0, 10))\nconst strcpy = libc.func(\"strcpy\", [\"buffer\", \"str\"], \"ptr\")?\nconst buf = ffi.alloc(16)\nstrcpy(buf, \"copied\")\nprint(buf.text(), buf.bytes(0, 3))",
    "output": "5 4 42\n1024.0\ncopied [99, 111, 112]",
    "outputTitle": "output"
   }
  ]
 },
 "mod-playbook": {
  "id": "mod-playbook",
  "group": "Integrations",
  "title": "fogu:playbook",
  "import": "import playbook from \"fogu:playbook\"",
  "intro": "Configuration management in the style of Ansible: an <strong>inventory</strong> of hosts and <strong>playbooks</strong> of idempotent tasks, run over SSH (the system's OpenSSH client) or on the local machine (through <a class=\"underline\" href=\"#mod-shell\">fogu:shell</a>, so it also works on Windows). Hosts run in parallel; the output and the <code>PLAY RECAP</code> follow Ansible's. From the command line: <code>fogu play site.yml -i inventory.yml [--check] [-l hosts] [-t tags] [--skip-tags t] [-e key=value] [-f forks] [-v]</code> (exit code 2 when a host failed or was unreachable).",
  "blocks": [
   {
    "h3": "Inventory",
    "text": "<code>vars</code> for every host; <code>hosts</code>: name → <code>{ host, port, user, key, password, become_password, connection: \"local\", …vars }</code>; <code>groups</code>: name → list of hosts or <code>{ hosts, vars }</code>. Patterns in <code>hosts:</code> — <code>all</code>, a host, a group, <code>web:db</code>, <code>web:!web2</code>.",
    "lang": "yaml",
    "title": "inventory.yml",
    "example": "hosts:\n  web1: { host: 192.0.2.10, user: deploy, key: ~/.ssh/id_ed25519 }\n  laptop: { connection: local }\ngroups:\n  web: { hosts: [web1], vars: { port: 8080 } }"
   },
   {
    "h3": "Playbooks and tasks",
    "text": "Templates: <code>{{ expr }}</code> with variables (<code>item</code>, <code>facts</code>, <code>inventory_hostname</code>, <code>groups</code>, registered results), <code>. []</code>, <code>+ - * / % ~</code>, comparisons, <code>and or not in</code>, <code>is defined</code>, and the filters <code>default upper lower trim length join first last replace split int float string bool to_json basename dirname</code>.",
    "lang": "yaml",
    "title": "site.yml",
    "api": [
     [
      "play: { name, hosts, vars, become, gather_facts = true, tags, tasks, handlers }",
      "",
      "Facts: <code>facts.os arch hostname distribution distribution_version pkg_mgr service_mgr user</code>."
     ],
     [
      "task keys: name, when, loop, register, notify, ignore_errors, changed_when, check_mode, become, tags",
      "",
      "<code>when</code> is an expression; <code>loop</code> sets <code>item</code>; <code>register</code> keeps the result."
     ],
     [
      "shell / command: \"cmd\" | { cmd, chdir, creates, removes, stdin }",
      "",
      "Skipped in check mode (unless <code>check_mode: false</code>); <code>{ rc, stdout, stderr, stdout_lines }</code>."
     ],
     [
      "copy: { content | src, dest, mode } · template: { src, dest, mode }",
      "",
      "Written only when the content or mode differs; <code>template</code> renders <code>{{ }}</code>."
     ],
     [
      "file: { path, state: directory | absent | touch | file, mode }",
      "",
      ""
     ],
     [
      "lineinfile: { path, line, regexp, state, create, mode }",
      "",
      "Replaces the last line matching <code>regexp</code>, or appends."
     ],
     [
      "package: { name, state: present | absent, update_cache, use }",
      "",
      "apt, dnf, yum, zypper, apk, pacman (from the facts)."
     ],
     [
      "service: { name, state: started | stopped | restarted | reloaded, enabled }",
      "",
      "systemd."
     ],
     [
      "debug: { msg } | { var } · set_fact: { … } · fail: { msg } · wait_for: { port | path, host, timeout }",
      "",
      ""
     ]
    ],
    "example": "- name: Web servers\n  hosts: web\n  become: true\n  vars: { app: shop }\n  tasks:\n    - name: App folder\n      file: { path: \"/srv/{{ app }}\", state: directory, mode: \"0755\" }\n    - name: Config\n      template: { src: templates/app.conf, dest: \"/srv/{{ app }}/app.conf\" }\n      notify: restart nginx\n    - name: Packages\n      package: { name: [curl, nginx], state: present }\n      when: facts.os == 'linux'\n  handlers:\n    - name: restart nginx\n      service: { name: nginx, state: restarted }"
   },
   {
    "h3": "From Fogu",
    "api": [
     [
      "playbook.run(plays | path, { inventory, vars, check, limit, tags, skipTags, forks, verbose, quiet })",
      "map · error",
      "<code>{ ok, failed: [hosts], recap: { host: { ok, changed, unreachable, failed, skipped, ignored } } }</code>."
     ],
     [
      "playbook.inventory(path | map)",
      "map",
      "The hosts and groups it describes."
     ],
     [
      "playbook.render(text, vars)",
      "str · error",
      "The template engine on its own."
     ]
    ],
    "example": "import playbook from \"fogu:playbook\"\nconst plays = [{\n    hosts: \"all\",\n    gather_facts: false,\n    vars: { dir: \"site\", pages: [\"index\", \"about\"] },\n    tasks: [\n        { name: \"pages\", copy: { content: \"<h1>{{ item | upper }}</h1>\", dest: \"{{ dir }}/{{ item }}.html\" }, loop: \"{{ pages }}\" },\n        { debug: { msg: \"{{ pages | length }} pages in {{ dir }}\" } },\n    ],\n}]\nconst first = playbook.run(plays)?\nconst again = playbook.run(plays, { quiet: true })?\nprint(first.ok, again.recap.localhost.changed)\nprint(playbook.render(\"{{ name | default('anon') | upper }}: {{ n * 3 }}\", { n: 7 }))",
    "output": "PLAY [all] *************************************************************\n\nTASK [pages] ***********************************************************\nchanged: [localhost] => (item=index)\nchanged: [localhost] => (item=about)\n\nTASK [debug] ***********************************************************\nok: [localhost] => 2 pages in site\n\nPLAY RECAP *************************************************************\nlocalhost : ok=2    changed=1    unreachable=0    failed=0    skipped=0    ignored=0\ntrue 0\nANON: 21",
    "outputTitle": "output"
   }
  ]
 },
 "embed": {
  "id": "embed",
  "group": "Integrations",
  "title": "Embedding Fogu (C API, .so / .dll)",
  "import": null,
  "intro": "The runtime builds as a dynamic library that other programs load: <code>cargo ffi</code> (= <code>cargo build --release -p fogu-ffi</code>) writes <code>target/release/libfogu_rt.so</code> (<code>fogu_rt.dll</code> on Windows — <code>cargo ffi --target x86_64-pc-windows-gnu</code> from Linux — and <code>libfogu_rt.dylib</code> on macOS); the header is <code>ffi/fogu.h</code>. A runtime runs on its own thread: code evaluated by the host shares its globals, and servers, <code>fg</code> tasks and <code>Fogu.signal</code> handlers keep running between calls. Values cross as JSON; every call returns <code>{\"ok\":true,\"value\":…}</code> or <code>{\"ok\":false,\"error\":\"…\"}</code>, freed with <code>fogu_free_string</code>. Rust programs can use <code>fogu::embed::Runtime</code> directly.",
  "blocks": [
   {
    "title": "host.ts (bun:ffi)",
    "api": [
     [
      "fogu_new() · fogu_free(rt) · fogu_version() · fogu_free_string(s)",
      "",
      ""
     ],
     [
      "fogu_eval(rt, code)",
      "char*",
      "Runs code in the global scope → the value of its last expression."
     ],
     [
      "fogu_spawn(rt, code) · fogu_run_file(rt, path)",
      "char*",
      "<code>spawn</code>: as a background task (servers, loops), returns at once."
     ],
     [
      "fogu_call(rt, name, argsJson) · fogu_get(rt, name) · fogu_set(rt, name, json)",
      "char*",
      "Global functions and variables."
     ],
     [
      "fogu_emit(rt, name, dataJson)",
      "char*",
      "Fires the program's <code>Fogu.signal.on(name, fn)</code> handlers."
     ],
     [
      "fogu_on_host(rt, cb, user)",
      "char*",
      "<code>void cb(char *name, char *json, void *user)</code> — called by <code>Fogu.host.emit(name, data)</code> on the runtime's thread; it owns both strings (<code>fogu_free_string</code>), so asynchronous hosts can read them later."
     ]
    ],
    "example": "import { dlopen, FFIType, CString, JSCallback } from \"bun:ffi\";\nconst { symbols: F } = dlopen(\"target/release/libfogu_rt.so\", {\n  fogu_new: { args: [], returns: FFIType.ptr },\n  fogu_eval: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.ptr },\n  fogu_call: { args: [FFIType.ptr, FFIType.cstring, FFIType.cstring], returns: FFIType.ptr },\n  fogu_emit: { args: [FFIType.ptr, FFIType.cstring, FFIType.cstring], returns: FFIType.ptr },\n  fogu_on_host: { args: [FFIType.ptr, FFIType.function, FFIType.ptr], returns: FFIType.ptr },\n  fogu_free_string: { args: [FFIType.ptr], returns: FFIType.void },\n  fogu_free: { args: [FFIType.ptr], returns: FFIType.void },\n});\nconst c = (t: string) => Buffer.from(t + \"\\\\0\");\nconst take = (p: any) => { const r = JSON.parse(new CString(p).toString()); F.fogu_free_string(p); if (!r.ok) throw new Error(r.error); return r.value; };\n\nconst rt = F.fogu_new();\ntake(F.fogu_eval(rt, c(\"fn greet(name, n) { return { msg: \\\"hi \\\" + name, twice: n * 2 } }\")));\nconsole.log(take(F.fogu_call(rt, c(\"greet\"), c('[\"bun\", 21]'))));      // { msg: \"hi bun\", twice: 42 }\n\nconst cb = new JSCallback((name, json) => {\n  console.log(new CString(name).toString(), new CString(json).toString());   // done {\"id\":7}\n  F.fogu_free_string(name); F.fogu_free_string(json);\n}, { args: [FFIType.ptr, FFIType.ptr, FFIType.ptr], returns: FFIType.void, threadsafe: true });\ntake(F.fogu_on_host(rt, cb.ptr, null));\ntake(F.fogu_eval(rt, c('Fogu.signal.on(\"job\", fn(d) { Fogu.host.emit(\"done\", d) })')));\ntake(F.fogu_emit(rt, c(\"job\"), c('{\"id\": 7}')));\nawait Bun.sleep(100);\nF.fogu_free(rt);"
   }
  ]
 },
 "mod-vm": {
  "id": "mod-vm",
  "group": "Integrations",
  "title": "fogu:vm",
  "import": "import vm from \"fogu:vm\"",
  "intro": "Runs Fogu source code at run time, in the style of Node's <code>node:vm</code>. The code runs on the same interpreter, so its functions can be called from the program and vice versa. A new context only sees the global natives plus the keys of its sandbox map (it may still <code>import</code> modules); like <code>node:vm</code>, it is <strong>not a security boundary</strong>. The value of the last expression is returned; syntax, name, runtime and timeout errors come back as <code>error</code> values.",
  "blocks": [
   {
    "api": [
     [
      "vm.runInNewContext(code, sandbox?, options?)",
      "value · error",
      "Options: <code>{ filename = \"&lt;vm&gt;\", timeout }</code> or a filename. Variables the code declares are copied into the sandbox map."
     ],
     [
      "vm.createContext(sandbox?) · vm.isContext(x)",
      "VmContext · bool",
      "A context keeps its variables between runs: <code>ctx.sandbox</code>, <code>ctx.name</code> reads a variable, <code>ctx.run(code, options?)</code>."
     ],
     [
      "vm.runInContext(code, ctx, options?)",
      "value · error",
      "Changes to the sandbox map are seen by the next run."
     ],
     [
      "vm.runInThisContext(code, options?)",
      "value · error",
      "Sees the program's globals; its declarations persist between calls."
     ],
     [
      "vm.Script(code, options?)",
      "VmScript · error",
      "Parsed once: <code>runInContext(ctx) runInNewContext(sandbox?) runInThisContext()</code>, fields <code>source filename</code>."
     ],
     [
      "vm.compileFunction(code, params = [], { context, filename })",
      "fn · error",
      "A function whose body is <code>code</code>."
     ],
     [
      "timeout",
      "",
      "A duration; loops and calls stop once it passes (<code>script execution timed out after 50ms</code>). It belongs to the task that runs the code: other <code>fg</code> tasks (and their JIT) are not affected."
     ],
     [
      "vm.spawn(code, sandbox?, { timeout, filename, semaphore, context }) · script.spawn(sandbox | ctx, opts) · ctx.spawn(code, opts)",
      "VmTask",
      "Runs in its own <code>fg</code> task. <code>semaphore</code>: a <a class=\"underline\" href=\"#mod-sync\">sync.Semaphore</a> limiting how many run at once."
     ],
     [
      "task.wait() · task.cancel() · task.done · ok · result · error",
      "value · error",
      "<code>cancel()</code> → true if it was running (<code>script execution cancelled</code>)."
     ],
     [
      "vm.all(tasks)",
      "list · error",
      "Their values in order; the first error if one fails."
     ],
     [
      "ctx.mutex · ctx.busy",
      "Mutex · bool",
      "A context runs one script at a time (concurrent runs wait); its lock is a real <code>sync.Mutex</code> the program can hold. A run may nest in its own context."
     ]
    ],
    "example": "import vm from \"fogu:vm\"\nprint(vm.runInNewContext(\"x * 2 + y\", { x: 20, y: 2 })?)\nconst ctx = vm.createContext({ count: 0 })\nvm.runInContext(\"count += 1; let msg = `count is ${count}`\", ctx)?\nprint(ctx.sandbox.count, ctx.msg)\nconst square = vm.Script(\"n * n\")?\nprint([1, 2, 3].map((n) => square.runInNewContext({ n: n })?))\nconst add = vm.compileFunction(\"return a + b\", [\"a\", \"b\"])?\nprint(add(2, 3))\nconst [v, err] = vm.runInNewContext(\"while true {}\", nil, { timeout: 50ms })\nprint(err)\nprint(vm.runInNewContext(\"1 +\", nil, \"bad.fg\"))",
    "output": "42\n1 count is 1\n[1, 4, 9]\n5\nerror: <vm>:1: runtime error: script execution timed out after 50ms\nerror: bad.fg: line 1:4: syntax error: unexpected expression: Eof",
    "outputTitle": "output"
   },
   {
    "text": "Concurrency: each <code>spawn</code> is a task; timeouts and cancellations are per task.",
    "example": "import vm from \"fogu:vm\"\nimport sync from \"fogu:sync\"\nconst square = vm.Script(\"Fogu.sleep(20ms)\\nn * n\")?\nconst sem = sync.Semaphore(2)\nconst tasks = []\nfor n in 1..6 { tasks.push(square.spawn({ n: n }, { semaphore: sem })) }\nprint(vm.all(tasks)?)\nconst t = vm.spawn(\"while true { Fogu.sleep(5ms) }\", nil, { filename: \"loop.fg\" })\nFogu.sleep(20ms)\nprint(t.cancel(), t.wait())\nconst ctx = vm.createContext({ count: 0 })\nconst inc = vm.Script(\"{\\nlet c = count\\nFogu.sleep(2ms)\\ncount = c + 1\\n}\")?\nvm.all([0, 1, 2, 3, 4].map((i) => inc.spawn(ctx)))?\nprint(ctx.sandbox.count)",
    "output": "[1, 4, 9, 16, 25]\ntrue error: loop.fg:1: runtime error: script execution cancelled\n5",
    "outputTitle": "output"
   }
  ]
 },
 "mod-cli": {
  "id": "mod-cli",
  "group": "Integrations",
  "title": "fogu:cli",
  "import": "import cli from \"fogu:cli\"",
  "intro": "Terminal interaction: prompts (<code>dialoguer</code>), colors and terminal control (<code>console</code>), spinners and progress bars (<code>indicatif</code>), tables (<code>comfy-table</code>) and argument parsing (<code>clap</code>). Without a terminal on stdin (pipes, CI) the prompts read plain lines, so <code>echo Ana | fogu app.fg</code> works; spinners and bars draw on stderr, only on a terminal. Colors are off when stdout is not a terminal or <code>NO_COLOR</code> is set.",
  "blocks": [
   {
    "h3": "Prompts",
    "api": [
     [
      "cli.input(prompt, { default, allowEmpty, type: \"str\" | \"int\" | \"float\" })",
      "str | int | float · error",
      ""
     ],
     [
      "cli.password(prompt, { confirm: \"Repeat password\", allowEmpty })",
      "str · error",
      "Hidden input."
     ],
     [
      "cli.confirm(prompt, { default })",
      "bool · error",
      ""
     ],
     [
      "cli.select(prompt, items, { default, fuzzy, index })",
      "value | nil · error",
      "Items are strings or <code>{ label, value }</code>; nil when cancelled with Esc; <code>fuzzy</code> filters while typing."
     ],
     [
      "cli.multiSelect(prompt, items, { defaults, index })",
      "list | nil · error",
      "Without a terminal: names or numbers separated by commas."
     ]
    ],
    "example": "import cli from \"fogu:cli\"\nconst name = cli.input(\"Your name\", { default: \"Ana\" })?\nconst age = cli.input(\"Age\", { type: \"int\" })?\nconst lang = cli.select(\"Language\", [\"Fogu\", \"Rust\", { label: \"Go lang\", value: \"go\" }])?\nif cli.confirm(`Save ${name}?`, { default: true })? { print(name, age, lang) }",
    "output": "$ printf 'Bia\\n30\\n3\\ny\\n' | fogu app.fg\nBia 30 go",
    "outputTitle": "output (answers piped)"
   },
   {
    "h3": "Styles, terminal, spinners and tables",
    "api": [
     [
      "cli.style(text, ...styles)",
      "str",
      "<code>bold dim italic underline strike inverse</code>, colors <code>red green yellow blue magenta cyan white black gray</code>, <code>brightRed</code>, backgrounds <code>bgRed</code>, 256 colors <code>\"208\"</code> / <code>\"bg208\"</code>."
     ],
     [
      "cli.red(text) · green · yellow · blue · magenta · cyan · white · black · gray · bold · dim · italic · underline · strike · inverse",
      "str",
      ""
     ],
     [
      "cli.colors(on?) · cli.stripAnsi(s) · cli.textWidth(s) · cli.link(text, url)",
      "bool · str · int · str",
      "<code>colors(true)</code> forces colors."
     ],
     [
      "cli.isTTY(\"stdin\" | \"stdout\" | \"stderr\") · cli.size()",
      "bool · { columns, rows } | nil",
      ""
     ],
     [
      "cli.clear() · clearLine() · hideCursor() · showCursor() · readKey()",
      "",
      "<code>readKey</code> → a character or <code>Enter Escape Backspace Tab ArrowUp ArrowDown ArrowLeft ArrowRight Home End PageUp PageDown Delete</code>."
     ],
     [
      "cli.spinner(text, { frames })",
      "Spinner",
      "<code>text(t) succeed(msg?) fail(msg?) warn(msg?) info(msg?) stop()</code>."
     ],
     [
      "cli.progress(total, { message, prefix, template, chars })",
      "ProgressBar",
      "<code>inc(n = 1) set(n) length(n) message(m) println(line) finish(msg?)</code>, fields <code>position length elapsed finished</code>. Templates of <code>indicatif</code>."
     ],
     [
      "cli.table(rows, { headers, style: \"rounded\" | \"utf8\" | \"ascii\" | \"markdown\" | \"none\", align, width })",
      "str",
      "Rows are lists or maps; numbers are right aligned."
     ]
    ],
    "example": "import cli from \"fogu:cli\"\nprint(cli.red(\"error\"), cli.style(\"ok\", \"bold\", \"green\"))\nprint(cli.table([[\"ana\", 3], [\"rui\", 15]], { headers: [\"name\", \"jobs\"] }))\nprint(cli.table([{ name: \"fogu\", stars: 9 }], { style: \"markdown\" }))\nconst bar = cli.progress(3)\nfor i in 0..3 { bar.inc() }\nbar.finish()\nprint(bar.position, cli.stripAnsi(cli.red(\"x\")))",
    "output": "error ok\n╭──────┬──────╮\n│ name ┆ jobs │\n╞══════╪══════╡\n│ ana  ┆    3 │\n│ rui  ┆   15 │\n╰──────┴──────╯\n| name | stars |\n|------|-------|\n| fogu |     9 |\n3 x",
    "outputTitle": "output"
   },
   {
    "h3": "Arguments",
    "api": [
     [
      "cli.parse(spec, argv = Fogu.args, { exit = true })",
      "map · error",
      "<code>spec</code>: <code>{ name, version, about, args: [...], commands: [{ name, about, args }], commandRequired }</code>. <code>--help</code> and <code>--version</code> print and exit; a usage error prints and exits with 2, or is an <code>error</code> value with <code>{ exit: false }</code>."
     ],
     [
      "arg: { name, type, short, long, help, positional, required, multiple, default, choices, env }",
      "",
      "Types <code>str int float bool count</code>; <code>multiple</code> gives a list. A subcommand sets <code>command</code> and its values in <code>args</code>."
     ]
    ],
    "example": "import cli from \"fogu:cli\"\nconst opts = cli.parse({\n    name: \"deploy\", version: \"1.0.0\", about: \"Deploys the app\",\n    args: [\n        { name: \"env\", positional: true, required: true, choices: [\"dev\", \"prod\"] },\n        { name: \"force\", short: \"f\", type: \"bool\", help: \"skip the checks\" },\n        { name: \"tag\", type: \"str\", multiple: true },\n        { name: \"verbose\", short: \"v\", type: \"count\" },\n    ],\n    commands: [{ name: \"logs\", args: [{ name: \"lines\", short: \"n\", type: \"int\", default: 10 }] }],\n}, [\"prod\", \"-f\", \"--tag\", \"a\", \"--tag\", \"b\", \"-vv\", \"logs\", \"-n\", \"5\"])\nprint(opts)",
    "output": "{args: {lines: 5}, command: logs, env: prod, force: true, tag: [a, b], verbose: 2}",
    "outputTitle": "output"
   }
  ]
 },
 "mod-html": {
  "id": "mod-html",
  "group": "HTML & browser",
  "title": "fogu:html",
  "import": "import html from \"fogu:html\"",
  "intro": "Parse HTML and query it with CSS selectors (scraping), render Jinja templates (minijinja), sanitize untrusted HTML (ammonia) and convert between HTML, plain text and Markdown. An invalid selector stops the program; template errors are error values.",
  "blocks": [
   {
    "h3": "Parsing and selectors",
    "api": [
     [
      "html.parse(text) · html.fragment(text)",
      "HtmlDocument",
      "A full document or a fragment."
     ],
     [
      "doc.select(css) · doc.find(css)",
      "list · HtmlElement | nil",
      "All matches · the first one."
     ],
     [
      "doc.title · doc.text · doc.html · doc.root",
      "",
      ""
     ],
     [
      "el.tag · el.text · el.html · el.outerHtml · el.id · el.classes · el.attrs · el.children · el.parent",
      "",
      "<code>text</code> has whitespace collapsed."
     ],
     [
      "el.attr(name) · el.hasClass(c) · el.select(css) · el.find(css) · el.closest(css) · el.matches(css)",
      "",
      ""
     ],
     [
      "html.select(text, css)",
      "list",
      "Parse + select in one call."
     ]
    ],
    "example": "import html from \"fogu:html\"\nimport http from \"fogu:http\"\nconst doc = html.parse(\"<ul id='news'><li><a href='/a'>Fogu 1.0</a> <b>new</b></li><li><a href='/b'>JIT notes</a></li></ul>\")\nfor a in doc.select(\"#news li > a\") {\n    print(a.text, \"→\", a.attr(\"href\"))\n}\nprint(doc.find(\"b\").closest(\"li\").find(\"a\").text, doc.find(\".missing\"))\n// scraping a real page: html.parse(http.get(url)?.text)",
    "output": "Fogu 1.0 → /a\nJIT notes → /b\nFogu 1.0 nil",
    "outputTitle": "output"
   },
   {
    "h3": "Templates (Jinja)",
    "api": [
     [
      "html.render(source, data)",
      "str · error",
      "Autoescaped; <code>{{ x | markdown }}</code> and <code>| striptags</code> are added to the Jinja filters."
     ],
     [
      "html.template(source, { autoescape = true })",
      "Template",
      "<code>t.render(data)</code>."
     ],
     [
      "html.templates(folder, { autoescape })",
      "Templates",
      "<code>render(name, data)</code>, <code>renderString(source, data)</code>, <code>has(name)</code>; <code>extends</code>/<code>include</code>/<code>import</code> load from the folder; autoescape by extension."
     ]
    ],
    "example": "import html from \"fogu:html\"\nconst page = html.render(\"<h1>{{ title }}</h1>{% for p in items %}<li>{{ loop.index }}. {{ p.name | upper }}</li>{% endfor %}\", {\n    title: \"Tom & Jerry\",\n    items: [{ name: \"café\" }, { name: \"pão\" }],\n})?\nprint(page)\nconst [_, err] = html.render(\"{% if %}\", {})\nprint(err)",
    "output": "<h1>Tom &amp; Jerry</h1><li>1. CAFÉ</li><li>2. PÃO</li>\nerror: html.render: syntax error: unexpected end of block (in <string>:1)",
    "outputTitle": "output"
   },
   {
    "h3": "Sanitizing and conversions",
    "api": [
     [
      "html.escape(text) · html.unescape(text)",
      "str",
      "Entities: named, decimal and hex."
     ],
     [
      "html.sanitize(text, { tags, addTags, removeTags, attributes, stripComments })",
      "str",
      "Removes scripts, event handlers and dangerous URLs."
     ],
     [
      "html.text(text) · el.toText()",
      "str",
      "Plain text with paragraphs, bullets and tab-separated cells."
     ],
     [
      "html.toMarkdown(text) · el.toMarkdown()",
      "str",
      "Headings, emphasis, links, images, lists, code blocks, quotes, tables."
     ],
     [
      "html.fromMarkdown(md)",
      "str",
      "CommonMark + tables, strikethrough, task lists, footnotes."
     ]
    ],
    "example": "import html from \"fogu:html\"\nprint(html.sanitize(\"<p onclick='steal()'>hi<script>alert(1)</script></p>\"))\nprint(html.toMarkdown(\"<h2>Notes</h2><p>Some <b>bold</b> and <a href='https://fogu.dev'>a link</a></p><ul><li>one</li><li>two</li></ul>\"))\nprint(html.fromMarkdown(\"**fogu** is *fun*\"))",
    "output": "<p>hi</p>\n## Notes\n\nSome **bold** and [a link](https://fogu.dev)\n\n- one\n- two\n<p><strong>fogu</strong> is <em>fun</em></p>",
    "outputTitle": "output"
   }
  ]
 },
 "mod-webview": {
  "id": "mod-webview",
  "group": "HTML & browser",
  "title": "fogu:webview",
  "import": "import webview from \"fogu:webview\"",
  "intro": "Drives a real browser (Chrome, Edge, Chromium or Brave) through the DevTools protocol: render JavaScript-heavy pages, click, type, run scripts, take screenshots and PDFs. The browser is found in <code>FOGU_CHROME</code>, the <code>path</code> option, the PATH or the usual install folders. Every command runs on a worker thread; the browser processes are closed when the program ends.",
  "blocks": [
   {
    "api": [
     [
      "webview.launch({ headless = true, path, args, width = 1280, height = 800, userAgent, timeout = 30s })",
      "Browser · error",
      ""
     ],
     [
      "webview.render(url, { wait, timeout })",
      "str · error",
      "The HTML after the page's JavaScript ran (shared headless browser)."
     ],
     [
      "webview.find()",
      "str | nil",
      "The browser that would be used."
     ],
     [
      "browser.open(url?, { wait = \"load\" | \"domcontentloaded\" | \"networkidle\" | \"none\", timeout })",
      "Page · error",
      "A new tab."
     ],
     [
      "browser.version() · browser.close()",
      "",
      ""
     ],
     [
      "page.goto(url, opts) · reload() · back() · forward()",
      "nil · error",
      ""
     ],
     [
      "page.title() · url() · content() · text(css?)",
      "str · error",
      ""
     ],
     [
      "page.eval(js, ...args)",
      "value · error",
      "An expression (promises are awaited) or a function called with JSON arguments."
     ],
     [
      "page.click(css) · type(css, text) · fill(css, value) · press(key)",
      "nil · error",
      "Real mouse/keyboard events; <code>press(\"Enter\")</code>."
     ],
     [
      "page.waitFor(css, timeout) · waitForNavigation(opts)",
      "nil · error",
      ""
     ],
     [
      "page.select(css)",
      "list · error",
      "<code>[{ tag, text, html, attrs }]</code> from the live DOM."
     ],
     [
      "page.setContent(html) · setViewport(w, h) · setHeaders(map)",
      "nil · error",
      ""
     ],
     [
      "page.screenshot({ fullPage, element })",
      "Image · error",
      "A <a href=\"#mod-image\">fogu:image</a> image."
     ],
     [
      "page.pdf(path?, { format = \"A4\", landscape, background = true, scale })",
      "nil | bytes · error",
      ""
     ],
     [
      "page.cookies() · setCookie({ name, value, url | domain })",
      "",
      ""
     ],
     [
      "page.close()",
      "nil",
      ""
     ]
    ],
    "example": "import webview from \"fogu:webview\"\nconst browser = webview.launch()?\nconst page = browser.open(\"https://example.com\")?\nprint(page.title()?)\nprint(page.eval(\"document.querySelectorAll('a').length\")?)\npage.screenshot({ fullPage: true })?.save(\"example.png\")?\npage.pdf(\"example.pdf\")?\nbrowser.close()",
    "output": "Example Domain\n1",
    "outputTitle": "output (with Chrome installed)"
   }
  ]
 },
 "mod-image": {
  "id": "mod-image",
  "group": "Images & charts",
  "title": "fogu:image",
  "import": "import image from \"fogu:image\"",
  "intro": "Create, transform, draw on and encode images (PNG, JPEG, WebP, GIF, BMP, ICO, TIFF), render SVG (resvg) and HTML, and build animated GIFs. Transformations return a <strong>new</strong> image; drawing methods change the image in place. Colors: <code>\"#rgb\"</code>, <code>\"#rrggbb(aa)\"</code>, CSS names, <code>\"rgb(r,g,b)\"</code>, <code>\"rgba(r,g,b,a)\"</code> or <code>[r, g, b, a?]</code>. Encoded data comes back as a <a class=\"underline\" href=\"#values\">buffer</a>; inputs also take a list of byte ints, base64 or a <code>data:</code> URL.",
  "blocks": [
   {
    "api": [
     [
      "image.new(w, h, background?) · image.open(path) · image.decode(buffer | bytes | base64 | dataUrl)",
      "Image · error",
      ""
     ],
     [
      "image.fromSvg(svg, { width, height, scale, background })",
      "Image · error",
      "Text uses the system fonts."
     ],
     [
      "image.fromHtml(html, { width = 800, height = 600, fullPage = true })",
      "Image · error",
      "Rendered by a headless Chrome/Edge (<a href=\"#mod-webview\">fogu:webview</a>)."
     ],
     [
      "image.info(path)",
      "{ width, height, format } · error",
      "Without decoding the pixels."
     ],
     [
      "image.gif(frames, { delay = 100ms, loop = 0, speed }) · image.writeGif(path, frames, opts)",
      "buffer · nil",
      "Frames: images or <code>{ image, delay }</code>."
     ],
     [
      "image.frames(path | buffer)",
      "list · error",
      "<code>[{ image, delay }]</code> of an animated GIF."
     ],
     [
      "image.color(c)",
      "list",
      "<code>[r, g, b, a]</code>."
     ],
     [
      "img.width · img.height",
      "int",
      ""
     ],
     [
      "img.resize(w, h?, { fit = \"fill\" | \"contain\" | \"cover\", filter }) · thumbnail(w, h?) · crop(x, y, w, h)",
      "Image",
      "One side keeps the aspect ratio."
     ],
     [
      "img.rotate(degrees, { background }) · flipH() · flipV()",
      "Image",
      "Any angle; 90/180/270 are exact."
     ],
     [
      "img.grayscale() · invert() · blur(sigma) · sharpen(sigma, threshold)",
      "Image",
      ""
     ],
     [
      "img.brightness(d) · contrast(c) · hue(deg) · saturate(f) · opacity(f)",
      "Image",
      ""
     ],
     [
      "img.fill(c) · setPixel(x, y, c) · getPixel(x, y)",
      "nil · str",
      ""
     ],
     [
      "img.rect(x, y, w, h, { fill, stroke, width, radius }) · circle(cx, cy, r, opts) · ellipse(cx, cy, rx, ry, opts)",
      "nil",
      "Anti-aliased (tiny-skia)."
     ],
     [
      "img.line(x1, y1, x2, y2, { color, width }) · polygon([[x, y], …], { fill, stroke, closed })",
      "nil",
      ""
     ],
     [
      "img.text(str, x, y, { size = 16, color, font, bold, align }) · measureText(str, opts)",
      "nil · { width, height }",
      "<code>font</code>: a .ttf/.otf path or a family (<code>\"sans-serif\"</code>, <code>\"monospace\"</code>…)."
     ],
     [
      "img.paste(other, x, y, { opacity }) · drawSvg(svg, x, y, opts)",
      "nil",
      "Alpha-blended."
     ],
     [
      "img.save(path, { quality, background }) · encode(format) · toBase64(format) · toDataUrl(format)",
      "",
      "Format from the extension; JPEG flattens alpha over white. <code>encode</code> → buffer."
     ]
    ],
    "example": "import image from \"fogu:image\"\nconst card = image.new(320, 160, \"#1e293b\")\ncard.rect(16, 16, 96, 96, { fill: \"#f97316\", radius: 16 })\ncard.circle(250, 64, 40, { stroke: \"white\", width: 4 })\ncard.text(\"Fogu\", 130, 110, { size: 28, color: \"white\", bold: true })\ncard.save(\"card.png\")?\nprint(card.width, card.height, card.getPixel(30, 30))\nconst thumb = card.resize(160).grayscale()\nprint(thumb.width, thumb.height, image.info(\"card.png\")?)\nconst frames = []\nfor i in 0..4 {\n    const f = image.new(32, 32, \"white\")\n    f.circle(16, 16, 4 + i * 3, { fill: \"#f97316\" })\n    frames.push(f)\n}\nimage.writeGif(\"pulse.gif\", frames, { delay: 80ms })?\nprint(len(image.frames(\"pulse.gif\")?))",
    "output": "320 160 #f97316\n160 80 {format: png, height: 160, width: 320}\n4",
    "outputTitle": "output"
   }
  ]
 },
 "mod-plot": {
  "id": "mod-plot",
  "group": "Images & charts",
  "title": "fogu:plot",
  "import": "import plot from \"fogu:plot\"",
  "intro": "Charts in the style of matplotlib — line, scatter, area, bar (grouped), histogram and pie/donut — drawn as SVG and saved as SVG, PNG, JPEG or WebP. Works with lists and ranges (arrays and data frames from the <code>datascience</code> lib: pass <code>a.toList()</code> / <code>df.col(name)</code>).",
  "blocks": [
   {
    "api": [
     [
      "plot.figure({ title, xlabel, ylabel, width = 800, height = 450, theme = \"light\" | \"dark\", grid = true, legend = true, xmin, xmax, ymin, ymax, background })",
      "Figure",
      ""
     ],
     [
      "fig.line(x?, y, { label, color, width, dash, marker, size, opacity }) · scatter(…) · area(…)",
      "nil",
      "<code>line(y)</code> uses 0, 1, 2… as x."
     ],
     [
      "fig.bar(categories, values, { label, color })",
      "nil",
      "Several <code>bar</code> calls are grouped."
     ],
     [
      "fig.hist(values, { bins = 10 }) · fig.pie(labels, values, { donut })",
      "nil",
      ""
     ],
     [
      "fig.hline(y, { color }) · vline(x) · text(x, y, str) · title(t) · xlabel(t) · ylabel(t)",
      "nil",
      ""
     ],
     [
      "fig.svg() · fig.image({ scale }) · fig.save(path, { scale = 2, quality })",
      "str · Image · nil",
      ""
     ],
     [
      "plot.line(x, y, options) · plot.bar · plot.scatter · plot.area · plot.hist · plot.pie",
      "Figure",
      "One-series shortcuts (options also take the figure options)."
     ]
    ],
    "example": "import plot from \"fogu:plot\"\nconst fig = plot.figure({ title: \"Sales\", xlabel: \"month\", ylabel: \"€\" })\nfig.area([1, 2, 3, 4, 5, 6], [10, 14, 9, 17, 21, 19], { label: \"2025\" })\nfig.line([1, 2, 3, 4, 5, 6], [8, 11, 13, 15, 14, 22], { label: \"2026\", dash: true, marker: true })\nfig.save(\"sales.png\")?\nconst langs = plot.figure({ title: \"Languages\", theme: \"dark\" })\nlangs.bar([\"Fogu\", \"Rust\", \"Go\"], [42, 30, 25], { label: \"2025\" })\nlangs.bar([\"Fogu\", \"Rust\", \"Go\"], [55, 33, 22], { label: \"2026\" })\nlangs.save(\"langs.svg\")?\nprint(plot.pie([\"A\", \"B\"], [70, 30], { donut: true }).svg().starts_with(\"<svg\"))",
    "output": "true",
    "outputTitle": "output"
   }
  ]
 },
 "mod-docker": {
  "id": "mod-docker",
  "group": "Containers & services",
  "title": "fogu:docker",
  "import": "import docker from \"fogu:docker\"",
  "intro": "Control Docker or Podman (containers, images, networks, volumes, compose projects, Swarm services) and write or edit Dockerfiles and compose files. Commands use the <code>docker</code>/<code>podman</code> CLI (<code>FOGU_CONTAINER_ENGINE</code> or <code>docker.use(\"podman\")</code>); <code>docker.remote(ssh)</code> runs everything on a server through <a href=\"#mod-ssh\">fogu:ssh</a>. A failing command is an error value with its stderr.",
  "blocks": [
   {
    "h3": "Containers, images, networks",
    "api": [
     [
      "docker.engine · docker.available() · docker.version() · docker.raw(args, { cwd })",
      "",
      "<code>raw</code> never fails on exit codes."
     ],
     [
      "docker.ps({ all, filter }) · docker.status(container)",
      "list · map",
      "<code>status</code>: running, state, health, exitCode, startedAt, restarts, image."
     ],
     [
      "docker.run(image, { name, detach = true, rm, ports, env, volumes, network, restart, labels, workdir, user, entrypoint, command, args })",
      "str · error",
      "The container id (or the output when <code>detach: false</code>)."
     ],
     [
      "docker.start(name) · stop · restart · kill · pause · unpause · rm(name, { force, volumes })",
      "nil · error",
      ""
     ],
     [
      "docker.logs(name, { tail, since, timestamps }) · exec(name, command, { user, workdir, env }) · inspect(name) · stats() · cp(from, to)",
      "",
      ""
     ],
     [
      "docker.images() · pull · push · tag · rmi · build(context, { tag, file, args, target, noCache, platform })",
      "",
      ""
     ],
     [
      "docker.networks() · volumes() · createNetwork · removeNetwork · createVolume · removeVolume · prune(what)",
      "",
      ""
     ]
    ]
   },
   {
    "h3": "Compose and Swarm services",
    "api": [
     [
      "docker.compose(file?, { project, cwd, envFile })",
      "ComposeProject",
      "On a remote engine a local compose file is copied first."
     ],
     [
      "p.up(services?, { detach = true, build, pull, wait, removeOrphans }) · down({ volumes, images }) · ps() · logs(svc?, { tail })",
      "",
      ""
     ],
     [
      "p.services() · p.status(service)",
      "list · map",
      "<code>{ name, state, running, status, health, containers, ports }</code>, including services not created yet."
     ],
     [
      "p.restart(svc?) · stop · start · pull · build · rm · exec(service, command) · config()",
      "",
      ""
     ],
     [
      "docker.services({ filter }) · docker.service(name)",
      "list · SwarmService",
      "Swarm: <code>status()</code>, <code>tasks()</code>, <code>restart()</code> (rolling), <code>stop()</code>, <code>start(n)</code>, <code>scale(n)</code>, <code>update({ image, replicas, env })</code>, <code>logs()</code>, <code>remove()</code>."
     ],
     [
      "docker.readCompose(path) · writeCompose(path, value) · composeString(value)",
      "",
      "Compose files as maps, written with the usual key order."
     ]
    ]
   },
   {
    "h3": "Dockerfiles",
    "api": [
     [
      "docker.dockerfile(from?, { as, platform }) · parseDockerfile(text) · readDockerfile(path)",
      "Dockerfile",
      ""
     ],
     [
      "df.from · run(cmd | [cmds]) · copy(src, dest, { from, chown, chmod }) · add · workdir · env(k, v | map) · arg · label · expose(…) · volume(…) · user · cmd · entrypoint · healthcheck(cmd, { interval, timeout, retries }) · comment · raw",
      "nil",
      "A list of commands becomes one <code>RUN</code> joined with <code>&amp;&amp;</code>."
     ],
     [
      "df.instructions · find(instr) · replace(instr, value) · insert(i, instr, value) · remove(i)",
      "",
      "Edit an existing Dockerfile."
     ],
     [
      "df.toString() · df.save(path = \"Dockerfile\")",
      "",
      ""
     ]
    ],
    "example": "import docker from \"fogu:docker\"\nconst df = docker.dockerfile(\"node:20-alpine\", { as: \"app\" })\ndf.workdir(\"/app\")\ndf.copy(\"package*.json\", \".\")\ndf.run([\"npm ci --omit=dev\", \"npm cache clean --force\"])\ndf.env({ NODE_ENV: \"production\" })\ndf.expose(3000)\ndf.cmd([\"node\", \"server.js\"])\nprint(df.toString())\ndocker.writeCompose(\"compose.yaml\", { services: { web: { build: \".\", ports: [\"3000:3000\"], restart: \"always\" } } })?\nprint(docker.composeString(docker.readCompose(\"compose.yaml\")?))",
    "output": "FROM node:20-alpine AS app\nWORKDIR /app\nCOPY package*.json .\nRUN npm ci --omit=dev \\\n    && npm cache clean --force\nENV NODE_ENV=production\nEXPOSE 3000\nCMD [\"node\",\"server.js\"]\n\nservices:\n  web:\n    build: .\n    restart: always\n    ports:\n    - 3000:3000",
    "outputTitle": "output"
   },
   {
    "example": "import docker from \"fogu:docker\"\nimport ssh from \"fogu:ssh\"\nconst id = docker.run(\"nginx:alpine\", { name: \"web\", ports: { \"8080\": \"80\" } })?\nprint(docker.status(\"web\")?.running, docker.exec(\"web\", \"nginx -v\")?.stderr)\ndocker.rm(\"web\", { force: true })?\nconst app = docker.compose(\"compose.yaml\", { project: \"shop\" })\napp.up()?\nfor s in app.services()? { print(s.name, s.state) }\napp.restart(\"web\")?\nconst prod = docker.remote(ssh.connect(\"deploy@prod.example.com\")?)\nprint(prod.ps()?)",
    "output": "true nginx version: nginx/1.27.2\nweb running\nworker running\n[{Names: shop-web-1, State: running, …}]",
    "outputTitle": "output (with Docker)"
   }
  ]
 },
 "mod-service": {
  "id": "mod-service",
  "group": "Containers & services",
  "title": "fogu:service",
  "import": "import service from \"fogu:service\"",
  "intro": "Linux services with systemd: generate unit files and <code>service.sh</code> management scripts, install, deploy, control and inspect them — locally or on a server with <code>{ ssh }</code>. Commands run <code>systemctl</code>/<code>journalctl</code> (with <code>sudo -n</code> when <code>sudo: true</code>).",
  "blocks": [
   {
    "api": [
     [
      "service.define({ name, exec, args, description, user = \"root\", group, workdir, env, envFile, after = \"network.target\", type = \"simple\", restart = \"always\", restartSec = 5, limitNofile = 65535, wantedBy, extra, sudo, dir, ssh })",
      "Service",
      "<code>extra</code>: <code>{ Service: { MemoryMax: \"1G\" } }</code>; <code>ssh</code>: a connection or <code>\"user@host\"</code>."
     ],
     [
      "svc.unit() · svc.script()",
      "str",
      "The <code>.service</code> file · a bash script: <code>create start stop restart status remove logs</code>."
     ],
     [
      "svc.writeUnit(path?) · svc.writeScript(path = \"service.sh\") · svc.installScript(path)",
      "nil · error",
      "<code>installScript</code> writes it on the (remote) host."
     ],
     [
      "svc.install() · svc.uninstall()",
      "nil · error",
      "Unit file, daemon-reload, enable · stop, disable, remove."
     ],
     [
      "svc.deploy(localBinary)",
      "nil · error",
      "Copies the program to its <code>exec</code> path (scp on a server), installs and restarts."
     ],
     [
      "svc.start() · stop · restart · reload · enable · disable · isActive() · isEnabled() · logs({ lines, since })",
      "",
      ""
     ],
     [
      "svc.status() · service.status(name, { ssh })",
      "map · error",
      "<code>{ active, sub, enabled, running, pid, memory, since, exitStatus, description }</code>."
     ],
     [
      "service.list({ all, ssh }) · service.start(name, { sudo, ssh }) · stop · restart · reload · enable · disable · logs(name, opts) · daemonReload()",
      "",
      ""
     ],
     [
      "service.parseUnit(text) · service.readUnit(name | path, { ssh }) · service.unitString(map)",
      "",
      "Unit files as <code>{ Unit: {…}, Service: {…}, Install: {…} }</code>."
     ]
    ],
    "example": "import service from \"fogu:service\"\nconst kv = service.define({\n    name: \"microaitec-kv\",\n    exec: \"/apps/microaitec/kv/one_kv\",\n    args: [\"--port\", \"9000\"],\n    workdir: \"/apps/microaitec/kv\",\n    env: { RUST_LOG: \"info\" },\n})\nprint(kv.unit())\nkv.writeScript(\"service.sh\")?           // ./service.sh create | start | stop | restart | status | remove | logs\nprint(service.parseUnit(kv.unit()).Service.ExecStart)",
    "output": "[Unit]\nDescription=microaitec-kv\nAfter=network.target\n\n[Service]\nType=simple\nUser=root\nWorkingDirectory=/apps/microaitec/kv\nEnvironment=\"RUST_LOG=info\"\nExecStart=/apps/microaitec/kv/one_kv --port 9000\nRestart=always\nRestartSec=5\nLimitNOFILE=65535\n\n[Install]\nWantedBy=multi-user.target\n\n/apps/microaitec/kv/one_kv --port 9000",
    "outputTitle": "output"
   },
   {
    "example": "import service from \"fogu:service\"\nimport ssh from \"fogu:ssh\"\nconst server = ssh.connect(\"deploy@prod.example.com\")?\nconst api = service.define({ name: \"api\", exec: \"/opt/api/api\", workdir: \"/opt/api\", sudo: true, ssh: server })\napi.deploy(\"target/release/api\")?       // upload, install the unit, restart\nprint(api.status()?.running)\nprint(api.logs({ lines: 20 })?)",
    "output": "true\n2026-10-15T20:12:01+00:00 prod api[4121]: listening on :8080",
    "outputTitle": "output (with a server)"
   }
  ]
 },
 "mod-zip": {
  "id": "mod-zip",
  "group": "Archives & serialization",
  "title": "fogu:zip",
  "import": "import zip from \"fogu:zip\"",
  "intro": "ZIP archives (create, extract, list, read, append; AES-256 passwords), tar and tar.gz archives and gzip data. Extraction refuses entries that would escape the target folder. Binary data comes back as a <a class=\"underline\" href=\"#values\">buffer</a> (inputs also take a str or a list of byte ints).",
  "blocks": [
   {
    "api": [
     [
      "zip.zip(dest, source | [sources], { level = 6, store, password })",
      "int · error",
      "Folders are added recursively, named like <code>zip -r</code>; returns the number of files."
     ],
     [
      "zip.write(dest, { \"name\": str | bytes, \"folder/\": nil }, opts)",
      "nil · error",
      "An archive from memory."
     ],
     [
      "zip.add(zipPath, name, content, opts)",
      "nil · error",
      "Appends to an existing archive."
     ],
     [
      "zip.unzip(src, dest = \".\", { password, only: [names] })",
      "list · error",
      "The extracted paths."
     ],
     [
      "zip.list(src) · zip.read(src, name, { bytes, password })",
      "list · str | buffer",
      "<code>[{ name, size, compressedSize, isDir, encrypted, modified }]</code>."
     ],
     [
      "zip.tar(dest, sources) · zip.untar(src, dest) · zip.listTar(src)",
      "",
      "<code>.tar.gz</code>/<code>.tgz</code> are gzip-compressed."
     ],
     [
      "zip.gzip(str | bytes, { level }) · zip.gunzip(bytes, { bytes })",
      "buffer · str | buffer",
      ""
     ],
     [
      "zip.gzipFile(src, dest?) · zip.gunzipFile(src, dest?)",
      "str · error",
      "The written path."
     ]
    ],
    "example": "import zip from \"fogu:zip\"\nimport fs from \"fogu:fs\"\nfs.mkdir(\"site/css\")?\nfs.writeText(\"site/index.html\", \"<h1>hi</h1>\")?\nfs.writeText(\"site/css/app.css\", \"h1 { color: red }\")?\nprint(zip.zip(\"site.zip\", \"site\")?)\nfor e in zip.list(\"site.zip\")? { print(e.name, e.size) }\nprint(zip.read(\"site.zip\", \"site/index.html\")?)\nprint(zip.unzip(\"site.zip\", \"copy\")?)\nzip.zip(\"secret.zip\", \"site/index.html\", { password: \"s3cret\" })?\nprint(zip.read(\"secret.zip\", \"index.html\", { password: \"s3cret\" })?)\nzip.tar(\"site.tar.gz\", \"site\")?\nprint(zip.untar(\"site.tar.gz\", \"restored\")?)\nprint(zip.gunzip(zip.gzip(\"hello hello hello\"))?)",
    "output": "2\nsite/ 0\nsite/css/ 0\nsite/css/app.css 17\nsite/index.html 11\n<h1>hi</h1>\n[copy/site/css/app.css, copy/site/index.html]\n<h1>hi</h1>\n[restored/site/css/app.css, restored/site/index.html]\nhello hello hello",
    "outputTitle": "output"
   }
  ]
 },
 "mod-protobuf": {
  "id": "mod-protobuf",
  "group": "Archives & serialization",
  "title": "fogu:protobuf",
  "import": "import protobuf from \"fogu:protobuf\"",
  "intro": "Protocol Buffers without code generation: <code>.proto</code> files are compiled at run time in pure Rust (protox — no <code>protoc</code>), messages are encoded from and decoded to Fogu maps (prost-reflect). Field names follow the proto3 JSON mapping (camelCase; <code>protoNames</code> for the original ones); 64-bit integers are ints; <code>bytes</code> fields are base64 text; well-known types (<code>Timestamp</code>, <code>Duration</code>…) use their JSON forms.",
  "blocks": [
   {
    "api": [
     [
      "protobuf.load(path | [paths], { includes: [folders] })",
      "ProtoSchema · error",
      "Imports are searched in <code>includes</code> (default: the files' folders)."
     ],
     [
      "protobuf.parse(source, { name, imports: { \"file.proto\": source } })",
      "ProtoSchema · error",
      "Google's well-known types are built in."
     ],
     [
      "protobuf.loadDescriptor(path | bytes)",
      "ProtoSchema · error",
      "A FileDescriptorSet (<code>protoc --descriptor_set_out</code>, <code>buf build</code>)."
     ],
     [
      "schema.messages · schema.enums · schema.services",
      "list",
      "Services: <code>{ name, methods: [{ name, input, output, clientStreaming, serverStreaming }] }</code>."
     ],
     [
      "schema.encode(type, map) · schema.decode(type, bytes, { defaults, protoNames, enumNumbers })",
      "buffer · map",
      "Unknown fields and wrong types are error values."
     ],
     [
      "schema.toJson(type, bytes) · schema.fromJson(type, json) · schema.validate(type, map)",
      "",
      "proto3 JSON."
     ],
     [
      "schema.fields(type) · schema.enumValues(enum)",
      "list · map",
      "<code>[{ name, jsonName, number, type, repeated, map }]</code>."
     ],
     [
      "schema.descriptor() · schema.saveDescriptor(path)",
      "buffer · nil",
      ""
     ]
    ],
    "example": "import protobuf from \"fogu:protobuf\"\nconst schema = protobuf.parse(\"\nsyntax = \\\"proto3\\\";\npackage shop;\nenum Status { UNKNOWN = 0; PAID = 1; }\nmessage Item { string sku = 1; int32 qty = 2; double price = 3; }\nmessage Order { int64 id = 1; string customer_name = 2; repeated Item items = 3; Status status = 4; }\n\")?\nprint(schema.messages)\nconst bytes = schema.encode(\"shop.Order\", { id: 42, customerName: \"Ana\", items: [{ sku: \"cafe\", qty: 2, price: 4.5 }], status: \"PAID\" })?\nprint(len(bytes), \"bytes\")\nprint(schema.decode(\"shop.Order\", bytes)?)\nprint(schema.toJson(\"shop.Order\", bytes)?)\nconst [_, err] = schema.encode(\"shop.Item\", { nope: 1 })\nprint(err)",
    "output": "[shop.Item, shop.Order]\n28 bytes\n{customerName: Ana, id: 42, items: [{price: 4.5, qty: 2, sku: cafe}], status: PAID}\n{\"id\":\"42\",\"customerName\":\"Ana\",\"items\":[{\"sku\":\"cafe\",\"qty\":2,\"price\":4.5}],\"status\":\"PAID\"}\nerror: protobuf: unrecognized field name 'nope' at line 1 column 7",
    "outputTitle": "output"
   }
  ]
 }
};
