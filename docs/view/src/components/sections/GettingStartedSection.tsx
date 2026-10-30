import React from 'react';
import { DocCodeBlock } from '../DocCodeBlock';

interface Props {
  onRun: (code: string) => void;
}

export const GettingStartedSection: React.FC<Props> = ({ onRun }) => {
  return (
    <>
      {/* ================= INTRO ================= */}
      <section id="intro" data-group="Getting started" data-title="Introduction" className="scroll-mt-20">
        <p className="text-sm font-semibold text-amber-600 dark:text-amber-400 uppercase tracking-wider">Official documentation</p>
        <h1 className="mt-2 text-4xl sm:text-5xl font-bold tracking-tight">The Fogu language</h1>
        <p className="mt-4 text-lg text-slate-600 dark:text-slate-400 max-w-3xl">
          Fogu (<code>.fg</code> files) is a small, predictable scripting language, dynamically typed with optional type
          annotations, built for <strong>automation, infrastructure, systems integration and small services</strong>.
          The reference implementation is written in Rust and ships an interpreter, a JIT compiler (Cranelift), a REPL,
          a CLI, a package manager and standalone executables.
        </p>
        <div className="mt-6 grid gap-4 sm:grid-cols-3">
          <div className="rounded-xl border border-slate-200 dark:border-slate-800 p-4">
            <p className="font-semibold">Simple</p>
            <p className="text-sm text-slate-600 dark:text-slate-400 !my-1">A small grammar, optional semicolons, no implicit coercions.</p>
          </div>
          <div className="rounded-xl border border-slate-200 dark:border-slate-800 p-4">
            <p className="font-semibold">Predictable</p>
            <p className="text-sm text-slate-600 dark:text-slate-400 !my-1">Errors are values, lexical scope, no hoisting, names and types checked before running.</p>
          </div>
          <div className="rounded-xl border border-slate-200 dark:border-slate-800 p-4">
            <p className="font-semibold">Practical</p>
            <p className="text-sm text-slate-600 dark:text-slate-400 !my-1">HTTP, TCP/UDP, SQL, key-value, cron and events in the standard library.</p>
          </div>
        </div>
        <DocCodeBlock
          title="server.fg"
          onRun={onRun}
          code={`import http from "fogu:http"
import time from "fogu:time"

http.serve(3000, fn(req) {
    if req.path == "/hello" {
        return http.json({ message: \`Hello! You asked for \${req.method} \${req.path}\` })
    }
    return http.text("not found", { status: 404 })
})

while true { time.delay(100ms) }`}
        />
      </section>

      {/* ================= INSTALLATION ================= */}
      <section id="install" data-group="Getting started" data-title="Installation" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Installation</h2>

        <h3 className="text-lg font-semibold">Direct download</h3>
        <p>Download the pre-compiled executable for your operating system:</p>
        <div className="mt-4 flex flex-wrap gap-4">
          <a
            href="https://github.com/microaitec-deploy/fogu-lang/raw/main/bin/fogu.exe"
            download
            className="inline-flex items-center justify-center gap-2 rounded-lg bg-amber-600 px-5 py-2.5 text-sm font-semibold text-white shadow-sm hover:bg-amber-500 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-amber-600"
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
            Download for Windows (.exe)
          </a>
          <a
            href="https://github.com/microaitec-deploy/fogu-lang/raw/main/bin/fogu"
            download
            className="inline-flex items-center justify-center gap-2 rounded-lg bg-slate-800 px-5 py-2.5 text-sm font-semibold text-white shadow-sm hover:bg-slate-700 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-slate-800"
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
            Download for Linux
          </a>
        </div>
        <p className="text-sm text-slate-500 mt-2">
          After downloading, make the binary executable on Linux with <code>chmod +x fogu</code> and move it to a folder on your <code>PATH</code> (e.g. <code>/usr/local/bin</code>).
        </p>

        <h3 className="text-lg font-semibold mt-8">Install script (adds Fogu to the PATH)</h3>
        <p>The scripts in <code>scripts/</code> download the executable, copy it into a folder and add that folder to your <code>PATH</code>:</p>
        <DocCodeBlock
          lang="bash"
          title="Linux / macOS"
          code={`curl -fsSL https://github.com/microaitec-deploy/fogu-lang/raw/main/scripts/install.sh | sh
# ~/.fogu/bin, added to ~/.bashrc / ~/.zshrc / ~/.profile (and fish)
sh scripts/install.sh --system                      # /usr/local/bin, for every user (sudo)
sh scripts/install.sh --from target/release/fogu    # install a local build
sh scripts/install.sh --uninstall`}
        />
        <DocCodeBlock
          lang="powershell"
          title="Windows (PowerShell)"
          code={`irm https://github.com/microaitec-deploy/fogu-lang/raw/main/scripts/install.ps1 | iex
# %LOCALAPPDATA%\\Fogu\\bin, added to your user PATH
.\\scripts\\install.ps1 -System                     # Program Files + the system PATH (administrator)
.\\scripts\\install.ps1 -From target\\release\\fogu.exe
.\\scripts\\install.ps1 -Uninstall`}
        />

        <h3 className="text-lg font-semibold mt-8">Build from source</h3>
        <p>Requires <a className="text-amber-600 dark:text-amber-400 underline" href="https://www.rust-lang.org/tools/install">Rust</a> (2024 edition) and <code>git</code> on the <code>PATH</code> (for packages).</p>
        <DocCodeBlock
          lang="bash"
          code={`git clone <repository> fogu && cd fogu
cargo build --release
# the binary is target/release/fogu (Windows: target\\release\\fogu.exe)
cargo test --release   # CLI tests, conformance suite, the crates' unit tests

# smaller binaries: the standard library areas are cargo features, all on by default
# (network database messaging ops hardware ai media rpc; files, processes, sockets
# and data formats are always built)
cargo build --release --no-default-features --features network,database`}
        />
      </section>

      {/* ================= FIRST STEPS ================= */}
      <section id="first-steps" data-group="Getting started" data-title="First steps" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">First steps</h2>
        <DocCodeBlock
          lang="bash"
          code={`fogu                       # interactive REPL
fogu hello.fg              # run a file
fogu -p '2 ** 10'          # evaluate and print: 1024
fogu init -y               # create fogu.json, main.fg and .gitignore
fogu run                   # run the "entry" of fogu.json
fogu --watch main.fg       # restart when a file changes
fogu check                 # check syntax, names and types without running
fogu build --compile       # standalone executable in dist/
fogu test                  # run the *.test.fg tests`}
        />
        <DocCodeBlock
          title="hello.fg"
          onRun={onRun}
          code={`const name = "Fogu"
print(\`Hello, \${name}!\`)          // Hello, Fogu!

for i in 1..4 {
    print(i, i ** 2)
}`}
        />
      </section>
    </>
  );
};
