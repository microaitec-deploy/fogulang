import React from 'react';
import { DocCodeBlock } from '../DocCodeBlock';

interface Props {
  onRun: (code: string) => void;
}

export const ToolsAndProjectSection: React.FC<Props> = ({ onRun }) => {
  return (
    <>
      {/* ================= COMMAND LINE ================= */}
      <section id="cli" data-group="Tools" data-title="CLI" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Command line</h2>
        <DocCodeBlock
          lang="bash"
          code={`fogu                         REPL (history, multi-line, value echo)
fogu <file.fg> [args]        run (the args are in system.info.args())
fogu run [file|script]       run; without a target uses "entry" from fogu.json
fogu <script>                script from fogu.json (e.g. fogu dev)
fogu -e '<code>'             run code
fogu -p '<expression>'       evaluate and print the result
fogu --watch <file>          restart when *.fg / fogu.json change
fogu check [files]           syntax, names and types (+ unused-variable warnings)
fogu build --compile [file]  standalone executable in dist/
fogu test [paths] [-t text]  run the tests of *.test.fg files
fogu fmt [paths] [--check]   format .fg files in the single Fogu style (- for stdin)
fogu play <playbook.yml>     run a playbook (-i inventory, --check, -l hosts, -t tags, -e k=v)
fogu init | add | remove | install | update | list | pm`}
        />
        <p>
          <code>fogu fmt</code> indents with 4 spaces, normalizes the spacing around commas, colons, operators and braces,
          keeps one blank line at most and never touches strings or comments. It checks that the formatted program has
          exactly the same tokens before writing a file; <code>--check</code> lists the files that need formatting (exit 1).
        </p>
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Option / variable</th>
                <th className="py-2">Effect</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr><td className="py-2 pr-4"><code>--no-jit</code> · <code>FOGU_JIT=0</code></td><td>interpreter only</td></tr>
              <tr><td className="py-2 pr-4"><code>--jit-stats</code> · <code>--jit-debug</code> · <code>--jit-dump</code></td><td>statistics, what compiles/is rejected, Cranelift IR</td></tr>
              <tr><td className="py-2 pr-4"><code>--strict</code></td><td>an error in an <code>fg</code> task terminates the process</td></tr>
              <tr><td className="py-2 pr-4"><code>FOGU_HOME</code></td><td>base of the package store (default: <code>~/.fogu</code>)</td></tr>
              <tr><td className="py-2 pr-4"><code>NO_COLOR</code></td><td>disables colors</td></tr>
              <tr><td className="py-2 pr-4"><code>FOGU_DOTENV=0</code></td><td>do not load <code>./.env</code> into <code>Fogu.env</code>/<code>process.env</code> (loaded by default; variables already set win)</td></tr>
            </tbody>
          </table>
        </div>
        <p>
          <code>fogu init</code> creates <code>fogu.json</code> (with <code>start</code> and <code>dev</code> scripts),
          <code>main.fg</code> and a <code>.gitignore</code>.
        </p>
      </section>

      {/* ================= DIAGNOSTICS ================= */}
      <section id="diagnostics" data-group="Tools" data-title="Diagnostics" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Diagnostics</h2>
        <p>
          Undefined names, uses before declaration, unknown types, misplaced <code>?</code> and non-exhaustive
          <code>match</code> are detected <strong>before</strong> the program runs. Errors show a code, the position, an excerpt
          and, when possible, a suggestion. <code>fogu check</code> also warns about unused local variables (prefix them with
          <code>_</code> to silence the warning).
        </p>
        <DocCodeBlock
          lang="text"
          code={`error[E0100]: variable 'usr' is not defined
  --> main.fg:3
   |
 3 |     print(usr.name)
   |           ^^^
   = help: did you mean 'user'?`}
        />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Code</th>
                <th className="py-2">Meaning</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr><td className="py-2 pr-4"><code>E0001</code> · <code>E0002</code></td><td>lexical error · syntax error</td></tr>
              <tr><td className="py-2 pr-4"><code>E0100</code> · <code>E0101</code></td><td>undefined variable · used before its declaration</td></tr>
              <tr><td className="py-2 pr-4"><code>E0102</code> · <code>E0103</code> · <code>E0104</code></td><td>redeclaration in the same scope · assignment to a <code>const</code> · private name</td></tr>
              <tr><td className="py-2 pr-4"><code>E0200</code> · <code>E0201</code> · <code>E0202</code></td><td>integer overflow · division by zero · index out of bounds</td></tr>
              <tr><td className="py-2 pr-4"><code>E0203</code> · <code>E0204</code> · <code>E0205</code></td><td>non-exhaustive <code>match</code> · value is not callable · stack overflow</td></tr>
              <tr><td className="py-2 pr-4"><code>E0206</code></td><td><code>?</code> outside a function, or in a function whose return type has no <code>error</code></td></tr>
              <tr><td className="py-2 pr-4"><code>E0207</code> · <code>E0208</code></td><td>type mismatch · unknown type</td></tr>
              <tr><td className="py-2 pr-4"><code>E0300</code></td><td>deadlock between tasks</td></tr>
            </tbody>
          </table>
        </div>
      </section>

      {/* ================= TESTING ================= */}
      <section id="testing" data-group="Tools" data-title="Testing" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Testing</h2>
        <p>
          Tests live in <code>*.test.fg</code> files and use <code>fogu:test</code>. <code>fogu test</code> finds them
          (under the current folder, or the given files and folders), runs each file in a fresh interpreter and exits
          with 1 if a test fails. A test fails when an expectation fails, when it raises an error, or when it returns an
          <code>error</code> value — so <code>?</code> can be used inside tests.
        </p>
        <DocCodeBlock
          title="math.test.fg"
          code={`import { test, skip, expect } from "fogu:test"
import { sum, parse } from "./math.fg"

test("adds", () => {
    expect(sum(1, 2)).toBe(3)
    expect([1, 2]).not.toContain(3)
    expect(0.1 + 0.2).toBeCloseTo(0.3)
})

test("errors are values", (): error => {
    const n = parse("42")?            // a returned error fails the test
    expect(n).toBeGreaterThan(40)
    expect(parse("-1")[1]).toBeError()
    return nil
})

skip("not written yet", () => {})`}
        />
        <DocCodeBlock
          lang="text"
          code={`$ fogu test
math.test.fg
  ✓ adds (0ms)
  ✓ errors are values (0ms)
  ○ not written yet (skipped)

2 passed, 1 skipped — 1 file(s) [1ms]

$ fogu test -t adds     # only tests whose name contains "adds"`}
        />
        <p>
          Matchers: <code>toBe</code>, <code>toEqual</code>, <code>toBeTruthy</code>, <code>toBeFalsy</code>,
          <code>toBeNil</code>, <code>toBeError</code>, <code>toBeType("int")</code>, <code>toContain</code>,
          <code>toHaveLength</code>, <code>toBeGreaterThan</code>, <code>toBeLessThan</code>, <code>toBeCloseTo(x, eps?)</code>
          — each can be negated with <code>.not</code>.
        </p>
      </section>

      {/* ================= PACKAGES ================= */}
      <section id="packages" data-group="Tools" data-title="Packages" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Packages</h2>
        <p>
          Packages come from <strong>git</strong> repositories, <code>.tar.gz</code> archives or local folders and live in a
          global store (<code>~/.fogu/fogu_modules/&lt;name&gt;@&lt;commit&gt;</code>). <code>fogu.lock</code> pins the commit and the
          <code>sha256</code> checksum; install scripts are never run.
        </p>
        <DocCodeBlock
          lang="bash"
          code={`fogu add user/greet@^1.2.0           # GitHub, highest v1.x.y tag
fogu add gitlab:group/repo#main         # branch, tag or commit
fogu add alias@github:user/repo         # import it as "alias"
fogu add ./local-folder
fogu install --frozen                   # CI: fail if the lock is out of date
fogu update && fogu list`}
        />
        <DocCodeBlock
          lang="json"
          title="fogu.json"
          code={`{
  "name": "my-app",
  "version": "0.1.0",
  "entry": "main.fg",
  "scripts": { "start": "fogu run main.fg", "dev": "fogu --watch main.fg" },
  "dependencies": { "greet": "github:user/greet#semver:^1.2.0" }
}`}
        />
        <h3 className="mt-8 text-xl font-semibold">Creating and publishing a library</h3>
        <p>
          A library is an ordinary project: a <code>fogu.json</code> with a <code>name</code>, a <code>version</code> and
          an <code>entry</code> file whose <code>export</code>s are its API. Push it to any git host; others install it
          with <code>fogu add user/repo</code> (tag releases <code>v0.1.0</code>… so <code>@^0.1.0</code> resolves). Names
          without <code>export</code> stay private; return errors as values; no global state at the top level. A complete
          example is{" "}
          <a className="underline" href="https://github.com/microaitec-deploy/fogulang_module">
            microaitec-deploy/fogulang_module
          </a>
          .
        </p>
        <DocCodeBlock
          title="main.fg (the library)"
          code={`export const version = "0.1.0"

export fn greet(name: str, lang = "en") {
    return match lang {
        "pt" => \`Olá, \${name}!\`
        _ => \`Hello, \${name}!\`
    }
}

export struct Counter {
    value: int
    step: int
    new(step = 1) { this.value = 0; this.step = step }
    fn next() { this.value = this.value + this.step; return this.value }
}`}
        />
        <DocCodeBlock lang="bash" code={`fogu add microaitec-deploy/fogulang_module      # writes fogu.json and fogu.lock (commit + sha256)`} />
        <DocCodeBlock
          title="main.fg (an app using it)"
          code={`import mod from "fogulang_module"
import { Counter } from "fogulang_module"

print(mod.greet("Ana", "pt"))        // Olá, Ana!
const c = new Counter(5)
print(c.next(), c.next())            // 5 10`}
        />
        <p>
          Libraries with native parts (a <code>cdylib</code> through <code>fogu:ffi</code>) or tools are published on the
          Fogu server instead, where the prebuilt files travel inside the package.
        </p>
      </section>

      {/* ================= STANDALONE EXECUTABLES ================= */}
      <section id="build" data-group="Tools" data-title="Standalone executables" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Standalone executables</h2>
        <p>
          <code>fogu build --compile</code> produces <code>dist/&lt;name&gt;</code> (<code>.exe</code> on Windows): a copy of the
          Fogu runtime with the program appended — the entry file, every module it imports and the packages it uses. The
          executable runs on machines without Fogu and passes every command-line argument to the program.
        </p>
        <DocCodeBlock
          lang="bash"
          code={`fogu build --compile                 # uses "entry" from fogu.json
fogu build --compile tool.fg -o out  # another entry file / output folder
fogu build --compile --embed "public/**"   # also embed files (any content)
./dist/my-app one two                # Fogu.args is ["one", "two"]`}
        />
        <p>
          Files the program reads at runtime (a React build, templates, images…) are embedded with
          <code>"embed": ["dist", "templates/*.html"]</code> in <code>fogu.json</code> or <code>--embed &lt;glob&gt;</code>. The program
          finds them with <code>Fogu.asset(path)</code>, which works the same when running from the sources:
        </p>
        <DocCodeBlock
          title="main.fg"
          code={`import web from "fogu:web"
const app = web.app()
app.static("/", Fogu.asset("dist"), { spa: true })   // the embedded React build
app.listen(3000)?`}
        />
        <p className="text-sm text-slate-500">
          On first run the bundle is extracted to <code>&lt;temp&gt;/fogu-bundles/&lt;hash&gt;</code>;
          <code>--target wasm</code> is not supported yet.
        </p>
      </section>

      {/* ================= JIT COMPILER ================= */}
      <section id="jit" data-group="Tools" data-title="JIT" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">JIT compiler</h2>
        <p>
          The interpreter is the semantic reference. The JIT (Cranelift) compiles hot numeric functions and loops
          (<code>int</code>/<code>float</code>/<code>bool</code>, <code>if</code>, <code>while</code>, <code>for a..b</code>, <code>match</code>,
          <code>math.*</code>, numeric type annotations); when something is not supported, the code keeps running in the
          interpreter, with the same results and error messages.
        </p>
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Benchmark (release)</th>
                <th className="py-2 pr-4">Interpreter</th>
                <th className="py-2">JIT</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr><td className="py-2 pr-4">fib(32)</td><td className="pr-4">3.02 s</td><td>0.025 s</td></tr>
              <tr><td className="py-2 pr-4">50M loop</td><td className="pr-4">16.7 s</td><td>0.069 s</td></tr>
              <tr><td className="py-2 pr-4">mandelbrot</td><td className="pr-4">3.5 s</td><td>0.013 s</td></tr>
            </tbody>
          </table>
        </div>
      </section>

      {/* ================= STATUS AND ROADMAP ================= */}
      <section id="status" data-group="Project" data-title="Status and roadmap" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Status and roadmap</h2>
        <p>
          The language is in development (before 1.0 there may be incompatible changes). It does <strong>not</strong> have
          yet: permissions <code>--allow-*</code>, <code>fogu audit</code>, <code>fogu publish</code>,
          static type inference, or the bytecode VM. The detailed status of every feature is in
          <code>CHECKLIST.md</code> and the full specification in <code>docs/spec.md</code>.
        </p>
        <p>
          The standard library reference on this page is generated by <code>docs/tools/gen_reference.py</code>, which runs
          the examples to show their real output.
        </p>
        <p>
          Fogu is distributed under the <strong>MIT license</strong> © 2026 Edmilson Soares.
        </p>
      </section>
    </>
  );
};
