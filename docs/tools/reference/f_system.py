"""Standard library reference: native code and configuration management."""

I = "Integrations"

SECTIONS = [
    {
        "id": "mod-ffi",
        "group": I,
        "title": "fogu:ffi",
        "import": 'import ffi from "fogu:ffi"',
        "intro": "Calls functions of native libraries (C ABI): <code>.dll</code>, <code>.so</code>, <code>.dylib</code>. "
        "<strong>Unsafe by nature</strong>: a wrong signature can crash the program. Supported signatures: up to 4 "
        "arguments mixing integers and floats, or up to 8 integer/pointer arguments.",
        "blocks": [
            {
                "api": [
                    ("ffi.open(name)", "Library · error", "A path or a name; <code>\"c\"</code> and <code>\"m\"</code> are the C runtime and the math library. The same name works on every platform: without an extension it gets the platform's prefix and suffix (<code>\"native/mylib\"</code> → <code>native/libmylib.so</code> on Linux, <code>native/libmylib.dylib</code> on macOS, <code>native/mylib.dll</code> on Windows), and another platform's extension is mapped to the native one (<code>\"mylib.dll\"</code> on Linux also tries <code>libmylib.so</code>). The error lists every file tried."),
                    ("lib.func(symbol, [argTypes], returnType = \"void\")", "fn · error", "A Fogu function that calls the symbol."),
                    ("lib.has(symbol) · lib.name", "bool · str", ""),
                    ("types", "", "<code>i8 i16 i32 i64 u8 u16 u32 u64 isize usize bool f64 ptr str buffer</code>; results also <code>f32 void</code>. <code>str</code> passes a NUL-terminated copy; a <code>str</code> result reads a C string."),
                    ("ffi.alloc(size)", "Buffer", "Memory for out-parameters: <code>text() bytes(start?, len?) write(offset, str | bytes) i32/u32/i64/f64(offset)</code>, fields <code>size address</code>."),
                    ("ffi.types · ffi.suffix", "list · str", ""),
                ],
                "example": r'''
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
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-playbook",
        "group": I,
        "title": "fogu:playbook",
        "import": 'import playbook from "fogu:playbook"',
        "intro": "Configuration management in the style of Ansible: an <strong>inventory</strong> of hosts and "
        "<strong>playbooks</strong> of idempotent tasks, run over SSH (the system's OpenSSH client) or on the local "
        "machine (through <a class=\"underline\" href=\"#mod-shell\">fogu:shell</a>, so it also works on Windows). "
        "Hosts run in parallel; the output and the <code>PLAY RECAP</code> follow Ansible's. From the command line: "
        "<code>fogu play site.yml -i inventory.yml [--check] [-l hosts] [-t tags] [--skip-tags t] [-e key=value] [-f forks] [-v]</code> "
        "(exit code 2 when a host failed or was unreachable).",
        "blocks": [
            {
                "h3": "Inventory",
                "text": "<code>vars</code> for every host; <code>hosts</code>: name → <code>{ host, port, user, key, password, "
                "become_password, connection: \"local\", …vars }</code>; <code>groups</code>: name → list of hosts or "
                "<code>{ hosts, vars }</code>. Patterns in <code>hosts:</code> — <code>all</code>, a host, a group, "
                "<code>web:db</code>, <code>web:!web2</code>.",
                "example": r'''
hosts:
  web1: { host: 192.0.2.10, user: deploy, key: ~/.ssh/id_ed25519 }
  laptop: { connection: local }
groups:
  web: { hosts: [web1], vars: { port: 8080 } }
''',
                "title": "inventory.yml",
                "lang": "yaml",
            },
            {
                "h3": "Playbooks and tasks",
                "api": [
                    ("play: { name, hosts, vars, become, gather_facts = true, tags, tasks, handlers }", "", "Facts: <code>facts.os arch hostname distribution distribution_version pkg_mgr service_mgr user</code>."),
                    ("task keys: name, when, loop, register, notify, ignore_errors, changed_when, check_mode, become, tags", "", "<code>when</code> is an expression; <code>loop</code> sets <code>item</code>; <code>register</code> keeps the result."),
                    ("shell / command: \"cmd\" | { cmd, chdir, creates, removes, stdin }", "", "Skipped in check mode (unless <code>check_mode: false</code>); <code>{ rc, stdout, stderr, stdout_lines }</code>."),
                    ("copy: { content | src, dest, mode } · template: { src, dest, mode }", "", "Written only when the content or mode differs; <code>template</code> renders <code>{{ }}</code>."),
                    ("file: { path, state: directory | absent | touch | file, mode }", "", ""),
                    ("lineinfile: { path, line, regexp, state, create, mode }", "", "Replaces the last line matching <code>regexp</code>, or appends."),
                    ("package: { name, state: present | absent, update_cache, use }", "", "apt, dnf, yum, zypper, apk, pacman (from the facts)."),
                    ("service: { name, state: started | stopped | restarted | reloaded, enabled }", "", "systemd."),
                    ("debug: { msg } | { var } · set_fact: { … } · fail: { msg } · wait_for: { port | path, host, timeout }", "", ""),
                ],
                "text": "Templates: <code>{{ expr }}</code> with variables (<code>item</code>, <code>facts</code>, "
                "<code>inventory_hostname</code>, <code>groups</code>, registered results), <code>. []</code>, "
                "<code>+ - * / % ~</code>, comparisons, <code>and or not in</code>, <code>is defined</code>, and the "
                "filters <code>default upper lower trim length join first last replace split int float string bool "
                "to_json basename dirname</code>.",
                "example": r'''
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
''',
                "title": "site.yml",
                "lang": "yaml",
            },
            {
                "h3": "From Fogu",
                "api": [
                    ("playbook.run(plays | path, { inventory, vars, check, limit, tags, skipTags, forks, verbose, quiet })", "map · error", "<code>{ ok, failed: [hosts], recap: { host: { ok, changed, unreachable, failed, skipped, ignored } } }</code>."),
                    ("playbook.inventory(path | map)", "map", "The hosts and groups it describes."),
                    ("playbook.render(text, vars)", "str · error", "The template engine on its own."),
                ],
                "example": r'''
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
''',
                "run": True,
            },
        ],
    },
    {
        "id": "embed",
        "group": I,
        "title": "Embedding Fogu (C API, .so / .dll)",
        "intro": "The runtime builds as a dynamic library that other programs load: <code>cargo ffi</code> "
        "(= <code>cargo build --release -p fogu-ffi</code>) writes <code>target/release/libfogu_rt.so</code> "
        "(<code>fogu_rt.dll</code> on Windows — <code>cargo ffi --target x86_64-pc-windows-gnu</code> from Linux — and "
        "<code>libfogu_rt.dylib</code> on macOS); the header is <code>ffi/fogu.h</code>. A runtime runs on its own thread: "
        "code evaluated by the host shares its globals, and servers, <code>fg</code> tasks and <code>Fogu.signal</code> "
        "handlers keep running between calls. Values cross as JSON; every call returns "
        "<code>{\"ok\":true,\"value\":…}</code> or <code>{\"ok\":false,\"error\":\"…\"}</code>, freed with <code>fogu_free_string</code>. "
        "Rust programs can use <code>fogu::embed::Runtime</code> directly.",
        "blocks": [
            {
                "api": [
                    ("fogu_new() · fogu_free(rt) · fogu_version() · fogu_free_string(s)", "", ""),
                    ("fogu_eval(rt, code)", "char*", "Runs code in the global scope → the value of its last expression."),
                    ("fogu_spawn(rt, code) · fogu_run_file(rt, path)", "char*", "<code>spawn</code>: as a background task (servers, loops), returns at once."),
                    ("fogu_call(rt, name, argsJson) · fogu_get(rt, name) · fogu_set(rt, name, json)", "char*", "Global functions and variables."),
                    ("fogu_emit(rt, name, dataJson)", "char*", "Fires the program's <code>Fogu.signal.on(name, fn)</code> handlers."),
                    ("fogu_on_host(rt, cb, user)", "char*", "<code>void cb(char *name, char *json, void *user)</code> — called by <code>Fogu.host.emit(name, data)</code> on the runtime's thread; it owns both strings (<code>fogu_free_string</code>), so asynchronous hosts can read them later."),
                ],
                "example": r'''
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
''',
                "title": "host.ts (bun:ffi)",
            }
        ],
    },
    {
        "id": "mod-vm",
        "group": I,
        "title": "fogu:vm",
        "import": 'import vm from "fogu:vm"',
        "intro": "Runs Fogu source code at run time, in the style of Node's <code>node:vm</code>. The code runs on the same "
        "interpreter, so its functions can be called from the program and vice versa. A new context only sees the "
        "global natives plus the keys of its sandbox map (it may still <code>import</code> modules); like "
        "<code>node:vm</code>, it is <strong>not a security boundary</strong>. The value of the last expression is "
        "returned; syntax, name, runtime and timeout errors come back as <code>error</code> values.",
        "blocks": [
            {
                "api": [
                    ("vm.runInNewContext(code, sandbox?, options?)", "value · error", "Options: <code>{ filename = \"&lt;vm&gt;\", timeout }</code> or a filename. Variables the code declares are copied into the sandbox map."),
                    ("vm.createContext(sandbox?) · vm.isContext(x)", "VmContext · bool", "A context keeps its variables between runs: <code>ctx.sandbox</code>, <code>ctx.name</code> reads a variable, <code>ctx.run(code, options?)</code>."),
                    ("vm.runInContext(code, ctx, options?)", "value · error", "Changes to the sandbox map are seen by the next run."),
                    ("vm.runInThisContext(code, options?)", "value · error", "Sees the program's globals; its declarations persist between calls."),
                    ("vm.Script(code, options?)", "VmScript · error", "Parsed once: <code>runInContext(ctx) runInNewContext(sandbox?) runInThisContext()</code>, fields <code>source filename</code>."),
                    ("vm.compileFunction(code, params = [], { context, filename })", "fn · error", "A function whose body is <code>code</code>."),
                    ("timeout", "", "A duration; loops and calls stop once it passes (<code>script execution timed out after 50ms</code>). It belongs to the task that runs the code: other <code>fg</code> tasks (and their JIT) are not affected."),
                    ("vm.spawn(code, sandbox?, { timeout, filename, semaphore, context }) · script.spawn(sandbox | ctx, opts) · ctx.spawn(code, opts)", "VmTask", "Runs in its own <code>fg</code> task. <code>semaphore</code>: a <a class=\"underline\" href=\"#mod-sync\">sync.Semaphore</a> limiting how many run at once."),
                    ("task.wait() · task.cancel() · task.done · ok · result · error", "value · error", "<code>cancel()</code> → true if it was running (<code>script execution cancelled</code>)."),
                    ("vm.all(tasks)", "list · error", "Their values in order; the first error if one fails."),
                    ("ctx.mutex · ctx.busy", "Mutex · bool", "A context runs one script at a time (concurrent runs wait); its lock is a real <code>sync.Mutex</code> the program can hold. A run may nest in its own context."),
                ],
                "example": r"""
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
""",
                "run": True,
            },
            {
                "text": "Concurrency: each <code>spawn</code> is a task; timeouts and cancellations are per task.",
                "example": r"""
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
""",
                "run": True,
            }
        ],
    },

    {
        "id": "mod-cli",
        "group": I,
        "title": "fogu:cli",
        "import": 'import cli from "fogu:cli"',
        "intro": "Terminal interaction: prompts (<code>dialoguer</code>), colors and terminal control (<code>console</code>), "
        "spinners and progress bars (<code>indicatif</code>), tables (<code>comfy-table</code>) and argument parsing "
        "(<code>clap</code>). Without a terminal on stdin (pipes, CI) the prompts read plain lines, so "
        "<code>echo Ana | fogu app.fg</code> works; spinners and bars draw on stderr, only on a terminal. Colors are "
        "off when stdout is not a terminal or <code>NO_COLOR</code> is set.",
        "blocks": [
            {
                "h3": "Prompts",
                "api": [
                    ("cli.input(prompt, { default, allowEmpty, type: \"str\" | \"int\" | \"float\" })", "str | int | float · error", ""),
                    ("cli.password(prompt, { confirm: \"Repeat password\", allowEmpty })", "str · error", "Hidden input."),
                    ("cli.confirm(prompt, { default })", "bool · error", ""),
                    ("cli.select(prompt, items, { default, fuzzy, index })", "value | nil · error", "Items are strings or <code>{ label, value }</code>; nil when cancelled with Esc; <code>fuzzy</code> filters while typing."),
                    ("cli.multiSelect(prompt, items, { defaults, index })", "list | nil · error", "Without a terminal: names or numbers separated by commas."),
                ],
                "example": r"""
import cli from "fogu:cli"
const name = cli.input("Your name", { default: "Ana" })?
const age = cli.input("Age", { type: "int" })?
const lang = cli.select("Language", ["Fogu", "Rust", { label: "Go lang", value: "go" }])?
if cli.confirm(`Save ${name}?`, { default: true })? { print(name, age, lang) }
""",
                "output": "$ printf 'Bia\\n30\\n3\\ny\\n' | fogu app.fg\nBia 30 go",
                "output_title": "output (answers piped)",
            },
            {
                "h3": "Styles, terminal, spinners and tables",
                "api": [
                    ("cli.style(text, ...styles)", "str", "<code>bold dim italic underline strike inverse</code>, colors <code>red green yellow blue magenta cyan white black gray</code>, <code>brightRed</code>, backgrounds <code>bgRed</code>, 256 colors <code>\"208\"</code> / <code>\"bg208\"</code>."),
                    ("cli.red(text) · green · yellow · blue · magenta · cyan · white · black · gray · bold · dim · italic · underline · strike · inverse", "str", ""),
                    ("cli.colors(on?) · cli.stripAnsi(s) · cli.textWidth(s) · cli.link(text, url)", "bool · str · int · str", "<code>colors(true)</code> forces colors."),
                    ("cli.isTTY(\"stdin\" | \"stdout\" | \"stderr\") · cli.size()", "bool · { columns, rows } | nil", ""),
                    ("cli.clear() · clearLine() · hideCursor() · showCursor() · readKey()", "", "<code>readKey</code> → a character or <code>Enter Escape Backspace Tab ArrowUp ArrowDown ArrowLeft ArrowRight Home End PageUp PageDown Delete</code>."),
                    ("cli.spinner(text, { frames })", "Spinner", "<code>text(t) succeed(msg?) fail(msg?) warn(msg?) info(msg?) stop()</code>."),
                    ("cli.progress(total, { message, prefix, template, chars })", "ProgressBar", "<code>inc(n = 1) set(n) length(n) message(m) println(line) finish(msg?)</code>, fields <code>position length elapsed finished</code>. Templates of <code>indicatif</code>."),
                    ("cli.table(rows, { headers, style: \"rounded\" | \"utf8\" | \"ascii\" | \"markdown\" | \"none\", align, width })", "str", "Rows are lists or maps; numbers are right aligned."),
                ],
                "example": r"""
import cli from "fogu:cli"
print(cli.red("error"), cli.style("ok", "bold", "green"))
print(cli.table([["ana", 3], ["rui", 15]], { headers: ["name", "jobs"] }))
print(cli.table([{ name: "fogu", stars: 9 }], { style: "markdown" }))
const bar = cli.progress(3)
for i in 0..3 { bar.inc() }
bar.finish()
print(bar.position, cli.stripAnsi(cli.red("x")))
""",
                "run": True,
            },
            {
                "h3": "Arguments",
                "api": [
                    ("cli.parse(spec, argv = Fogu.args, { exit = true })", "map · error", "<code>spec</code>: <code>{ name, version, about, args: [...], commands: [{ name, about, args }], commandRequired }</code>. <code>--help</code> and <code>--version</code> print and exit; a usage error prints and exits with 2, or is an <code>error</code> value with <code>{ exit: false }</code>."),
                    ("arg: { name, type, short, long, help, positional, required, multiple, default, choices, env }", "", "Types <code>str int float bool count</code>; <code>multiple</code> gives a list. A subcommand sets <code>command</code> and its values in <code>args</code>."),
                ],
                "example": r"""
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
""",
                "run": True,
            },
        ],
    },
]
