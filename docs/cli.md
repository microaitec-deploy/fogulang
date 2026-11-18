# CLI

```text
fogu                         REPL (history, multi-line input, value echo)
fogu <file.fg> [args]        run (the args are in system.info.args())
fogu run [file|script]       run; without a target uses "entry" from fogu.json
fogu <script>                script from fogu.json (e.g. fogu dev)
fogu .                       the project in this folder: its "start" script, or its "entry" (also fogu <folder>)
fogu -e '<code>'             run code;  fogu -p '1+2*3' prints the result
fogu --watch <file>          restart when *.fg / fogu.json change
fogu check [paths] [opts]    syntax, names and types (files or folders; no args: every .fg here)
fogu build --compile [file]  standalone executable in dist/ (-o <folder>, --target <platform>|all)
fogu build --lib [file]      the program as a shared library (.so / .dll / .dylib) plus its C header
fogu test [paths] [-t text]  run the tests of *.test.fg files
fogu disasm <file.fg>        the bytecode of the file's functions and methods (see bytecode.md)
fogu init | add | remove | install | update | list | pm   (see packages.md)
fogu --version | --help
```

Options: `--no-jit`, `--jit-stats`, `--jit-debug`, `--jit-dump`, `--engine <tree|vm>`,
`--strict`, `--cores <n|cpus>`.
Environment: `FOGU_HOME`, `NO_COLOR`, `FOGU_JIT=0`, `FOGU_JIT_CALLS`, `FOGU_JIT_LOOPS`,
`FOGU_ENGINE=vm`, `FOGU_VM_DEBUG=1`, `FOGU_DOTENV=0`, `FOGU_CORES`.

`--engine=vm` (or `FOGU_ENGINE=vm`) runs function bodies on the bytecode VM
instead of the tree-walking interpreter. It is opt-in in 0.8.1: the output is
the same (the spec suite runs on both), most programs with functions, structs
and text are 2–3× faster, and a function the VM does not cover yet runs on the
tree-walker (`FOGU_VM_DEBUG=1` lists them). `fogu disasm file.fg` prints the
bytecode. See [bytecode.md](bytecode.md).

`--cores 4` (or `FOGU_CORES=4`, or `Fogu.cores(4)` at the top of a program) sets how many
threads the runtime uses for networking and blocking work (default: the machine's CPUs, at
most 8 for the I/O runtime). `--cores 0,2-3` also pins the process to those CPUs (Linux and
Windows). Fogu code itself runs on one thread (cooperative `fg` tasks); the setting must be
made before the first network call, timer or blocking job.

At startup every command (and standalone executables) loads `./.env` into the
process environment, so `Fogu.env` and `process.env` see it. Variables already
set win; a malformed line is reported as a warning and skipped.

## `fogu init`

Creates `fogu.json` (with `start` and `dev` scripts), `main.fg` and a
`.gitignore` (an existing `.gitignore` is never overwritten). `-y` accepts the
defaults without asking.

## Building `fogu` from source

`fogu` is a Cargo workspace: `crates/fogu-core` (the language), `crates/fogu-std` (the
standard library), `crates/fogu-pkg` (packages) and the root package (the CLI).

```bash
cargo build --release                       # everything (all features on)
cargo test --release                        # CLI tests, spec suite, the crates' unit tests
cargo build --release --no-default-features # language + base and io modules only
cargo build --release --no-default-features --features network,database
```

The standard library areas are features of the root package: `network`, `database`,
`messaging`, `ops`, `ai`, `media`, `rpc` (all on by default; `database`
enables `rpc`, `ai` enables `media` and `network`). `base` (time, sync, cron, log,
regex, crypto, vm, cli, ffi, the `Fogu` object) and `io` (fs, path, process, system,
shell, zip, data formats, net) are always built. A module whose feature is off is not
importable: `import web from "fogu:web"` fails as an unknown module. The optional
dependencies of disabled features are not compiled.

## Embedding (C API)

```text
cargo ffi                                  # = cargo build --release -p fogu-ffi
cargo ffi --target x86_64-pc-windows-gnu   # fogu_rt.dll from Linux (mingw-w64)
```

Builds `target/[<target>/]release/libfogu_rt.so` (`fogu_rt.dll`, `libfogu_rt.dylib`)
from the `ffi/` crate; the header is `ffi/fogu.h`. The normal `cargo build` does not
build it.

## `fogu build --lib`

```text
fogu build --lib                         # the project's entry → dist/lib<name>.so (+ dist/<name>.h)
fogu build --lib tools.fg -o out
fogu build --lib --target windows-x64    # dist/<name>-windows-x64.dll
fogu build --lib --target linux-x64,linux-arm64,windows-x64
fogu build --lib --runtime path/to/libfogu_rt.so
```

Turns a Fogu program into a **shared library** another program loads: C, C++,
Python (`ctypes`), C# (`DllImport`), Go (cgo), Node / Bun (FFI), Rust… The
output is the embeddable runtime (`libfogu_rt`, the C API above) with the
program appended — the entry file, every `.fg` it imports, its packages and
its embedded files, exactly the bundle of `fogu build --compile` — so the one
file is all a host needs. The names are `lib<name>.so` on Linux,
`<name>.dll` on Windows and `lib<name>.dylib` on macOS (`<name>` from
`fogu.json`, else the folder); with `--target` the platform is part of the
name, as for executables.

The host calls `fogu_load`, which starts a runtime and runs the entry file
once: what the file declares at its top level then exists, and `fogu_call`
calls its functions by name. Arguments and results cross as JSON:

```c
#include "mathlib.h"                       /* written next to the library */

FoguRuntime *rt = NULL;
char *r = fogu_load(&rt);                  /* {"ok":true,"value":null} */
fogu_free_string(r);
r = fogu_call(rt, "add", "[2, 40]");       /* {"ok":true,"value":42} */
fogu_free_string(r);
fogu_free(rt);
```

```python
import ctypes, json
lib = ctypes.CDLL("dist/libmathlib.so")
lib.fogu_load.restype = lib.fogu_call.restype = ctypes.c_void_p
lib.fogu_load.argtypes = [ctypes.POINTER(ctypes.c_void_p)]
lib.fogu_call.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
lib.fogu_free_string.argtypes = [ctypes.c_void_p]
rt = ctypes.c_void_p()
lib.fogu_free_string(lib.fogu_load(ctypes.byref(rt)))
p = lib.fogu_call(rt, b"add", json.dumps([2, 40]).encode())
print(json.loads(ctypes.string_at(p)))     # {'ok': True, 'value': 42}
lib.fogu_free_string(p)
```

- Every top-level `fn` of the entry file can be called (exported or not; names
  starting with `_` are left out of the header, not hidden). The header lists
  them with their parameters. Top-level variables are read and written with
  `fogu_get` / `fogu_set`; the rest of the C API works as on any runtime
  (`fogu_eval`, `fogu_register` for functions the host gives the program,
  `fogu_on_host` for `Fogu.host.emit`).
- An error is `{"ok":false,"error":"message"}`: a runtime error, an `error`
  value returned by the function, or the error of a `return value, error(...)`
  pair. A function that returns `value, nil` gives the host the pair
  `[value, null]`.
- The runtime lives on its own thread: globals, `fg` tasks and servers the
  program started stay alive between calls, until `fogu_free`. Calls on one
  runtime are serialized.
- The runtime library of each platform is looked up like the runtimes of
  `--compile`: `$FOGU_RUNTIMES`, next to the `fogu` executable (also
  `runtimes/`), `dist/`, `$FOGU_HOME/runtimes`, else downloaded from the Fogu
  server and checked against its SHA-256. Its names are
  `libfogu_rt-<target>.so`, `fogu_rt-<target>.dll`, `libfogu_rt-<target>.dylib`
  (and the plain `libfogu_rt.so` … of `cargo ffi` for this machine's platform).
  The server publishes linux-x64, linux-arm64 and windows-x64; the others are
  built with `cargo ffi --target <triple>` on a machine that can link them.
- macOS: appending the program changes the file, so sign the library before
  loading it (`codesign --force -s - dist/lib<name>.dylib`). This step was not
  tested: no macOS runtime library is built yet.
- The library is the full runtime (about 60 MB on linux-x64) whatever the
  program uses; there are no per-program "flavors" for libraries yet.

## `fogu check`

```text
fogu check                       every .fg file under the current folder
fogu check main.fg lib/ examples one or more files and folders
fogu check --json src            machine-readable diagnostics
fogu check --strict              warnings also fail (same as --deny-warnings)
fogu check -q                    only print the problems
fogu check --watch src           check again whenever a .fg file changes
fogu check --help
```

Checks files without running them. Folders are searched recursively, skipping
hidden folders (`.git`, `.fogu`…), `node_modules`, `target`, `dist` and
`fogu_modules`. Every error of a file is reported (not only the first).

**Errors** — syntax, undefined names, use before declaration, a name declared
twice in a scope, an assignment to a `const`, function, struct, import or loop
variable, imports of a `fogu:` module that does not exist, of a package that is
not installed, of a missing file or of a name the module does not export,
unknown type names, `?` in a function whose return type has no `error`,
non-exhaustive `match`, and the type errors the code would hit when it runs, found by a small
conservative inference over literals, annotated variables, variables that are
never reassigned and the signatures of the functions declared with `fn`:

```fogu
const n: int = "x"          // E0207 type mismatch: variable 'n' is declared as int, got str
print(1 + "a")              // E0210 operator not applicable between 'int' and 'str'
const s = "ab"
print(s * 2)                // E0210 operator not applicable between 'str' and 'int'
print(len(1, 2, 3))         // E0209 native function 'len' expects 1 arguments, got 3
fn add(a: int, b = 0): int { return a + b }
add(1, 2, 3)                // E0209 function 'add' expects at most 2 arguments, got 3
add("1")                    // E0207 type mismatch: parameter 'a' expects int, got str
fn name(): str { return 1 } // E0207 type mismatch: function 'name' must return str, got int
const k = 5
k()                         // E0204 value of type 'int' is not callable
k = 6                       // E0103 cannot assign to 'k' because it is a const binding
let k = 7                   // E0102 'k' is already declared in this scope
```

When a type is not known (a parameter without annotation, a variable that is
reassigned somewhere, a field, a call result without a declared return type…)
nothing is reported. Missing arguments are not an error (they are `nil`, or the
default value), and a variadic `...rest` accepts any number.

**Warnings** — unused local variables (W0001) and imports (W0002), code after
`return`/`break`/`continue` (W0003), a variable that shadows another one of an
enclosing block of the same function (W0004). Prefix a name with `_` to
silence the warnings about it. Warnings never fail the check unless
`--strict`/`--deny-warnings` is given.

The same errors stop `fogu <file>` (and imported modules) before the program
starts, reported with the first one; warnings are only printed by `fogu check`.

The exit code is 0 when there are no errors (and no warnings with `--strict`), 1
otherwise, 2 for an unknown option. `--json` prints one object (here for `const x = 1` / `print(1 + "a")`):

```json
{
  "files": 1,
  "errors": 1,
  "warnings": 1,
  "ok": false,
  "diagnostics": [
    { "file": "main.fg", "line": 2, "column": 1, "severity": "error", "code": "E0210",
      "message": "operator not applicable between 'int' and 'str'",
      "help": ["convert one side first, e.g. str(n) or int(s)"] },
    { "file": "main.fg", "line": 1, "column": 7, "severity": "warning", "code": "W0001",
      "message": "unused variable 'x' (prefix it with '_' to silence this)", "help": [] }
  ]
}
```

`column` is the position of the name the message quotes (or of the statement)
when the error itself has no column.

## `fogu test`

Finds every `*.test.fg` file under the current folder (or under the given
folders; files may also be given directly), loads each one in a fresh
interpreter — `test(name, fn)` from `fogu:test` registers a test — and runs the
registered tests. A test fails when it raises an error (a failed `expect`, a
runtime error) or returns an `error` value. `skip(name, fn)` reports a test as
skipped. `-t <text>` runs only the tests whose name contains the text. The
exit code is 1 when a test fails or a file cannot be loaded.

## `fogu build --compile`

Produces `dist/<name>` (`.exe` on Windows): the `fogu` runtime with the program
appended (entry file, the modules it imports and the packages it uses). The
name comes from `fogu.json` (or the entry file). The executable passes every
command-line argument to the program and extracts its bundle once to
`<temp>/fogu-bundles/<hash>`. `--target wasm` is not supported yet.

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

Without Docker, the ARM Linux runtimes also build from an x86_64 Linux machine with
the GNU cross compilers: OpenSSL is compiled from source for those targets
(`crates/fogu-std/Cargo.toml`) and `.cargo/config.toml` sets the linkers, so no ARM sysroot or
`libssl-dev` is needed.

```bash
sudo apt install gcc-aarch64-linux-gnu gcc-arm-linux-gnueabihf
rustup target add aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf
cargo build --release --target aarch64-unknown-linux-gnu      # → fogu-linux-arm64
cargo build --release --target armv7-unknown-linux-gnueabihf  # → fogu-linux-armv7
```

The binary is `target/<triple>/release/fogu`; rename it to `fogu-<target>` to use it
as a runtime.

### Services (Windows service, systemd)

```bash
fogu build --compile app.fg --target windows --service \
    --service-name myapp --display-name "My App" --description "Serves the API"
```

`--service` (or any of `--service-name`, `--display-name`, `--description`,
`--start-type auto|manual|delayed`) records service settings in the bundle; so
does `"service"` in `fogu.json` (`true`, or an object — the flags win):

```json
"service": { "name": "myapp", "displayName": "My App", "description": "Serves the API",
             "startType": "auto", "args": ["--port", "8080"], "stopTimeout": 20, "log": "logs/myapp.log" }
```

The executable then takes these commands (anything else runs the program, so a
console run is still the program itself — handy for debugging):

| Command | |
|---|---|
| `app install [--start auto\|manual\|delayed] [--user <account> [--password <p>]] [--log <file>] [-- <args>]` | registers the service (Administrator / root); `-- <args>` replaces `"args"` |
| `app uninstall` | stops and removes it |
| `app start` · `app stop` · `app status` | through the service manager |
| `app -- <args>` | runs the program with arguments that look like commands (`app -- install`) |
| `app service-help` | the usage |

**Windows.** `install` registers `"<app.exe>" service-run [--log <file>] -- <args>` with the
service manager (SCM), start type auto (default), manual or delayed auto-start, as
LocalSystem unless `--user` (e.g. `"NT AUTHORITY\LocalService"`, or `.\name --password …`).
Started by the SCM, the program runs with:

- stdout and stderr appended to `<name>.log` next to the executable (or `"log"` /
  `install --log`), with a line from the runner when it starts and stops;
- the executable's folder as the current folder (`.env` there is loaded);
- a stop or shutdown from the SCM delivered as the `service-stop` and `SIGTERM`
  events (`Fogu.signal.on(...)` / `Fogu.signal.wait()`) — the ones the program
  listens to. It has `stopTimeout` seconds (default 20) to end, then it is ended
  (exit code 1). A program that listens to neither is ended right away (code 0);
- its exit code reported to the SCM with SERVICE_STOPPED (non-zero is a
  service-specific error, so the recovery actions of `sc failure` apply). `exit()`
  inside the program also reports it.

A service executable without a console (e.g. registered by hand with `sc create`
without `service-run`) also connects to the SCM. `service-run` from a console says it
is for the SCM and exits with 1. The 32-bit runtime (`--target windows-x86`) works the same.

**Linux.** `sudo ./app install` writes `/etc/systemd/system/<name>.service` (ExecStart
= the executable, WorkingDirectory = its folder, `Restart=on-failure`,
`TimeoutStopSec=stopTimeout`, `User=` from `--user`, output to the journal or to
`--log`), runs `systemctl daemon-reload` and enables it (not for `manual`);
`start`/`stop`/`status`/`uninstall` call `systemctl`. systemd stops it with SIGTERM.
`fogu build --service` for a Linux target also writes `dist/<name>.service` (for
`/opt/<name>/`), to copy by hand. macOS and WebAssembly ignore the settings.

The settings are read by the runtime: build the executable with a runtime of this
Fogu version or newer (an older runtime runs it as a plain program).

## `fogu desktop` and `fogu mcu`

Front-ends for the desktop and microcontroller libraries (`libs/desktop`,
`libs/mcu`). The library is looked up like an import (the project's dependency,
a `libs/` checkout, the global list) and installed globally from the Fogu server
the first time (`fogu add -g desktop`); `update` installs the latest version.

```sh
fogu desktop create my-app --template react   # vanilla | ts | react | vue | svelte
fogu desktop dev                              # the window with hot reload (current folder): Fogu reloads in place, Vite HMR
fogu desktop build --target windows           # dist/my-app.exe: the generic host + the app, in seconds (no Rust)
fogu desktop build --installer                # compiled in: installers and the app's icon (Rust + Tauri CLI)
fogu desktop ci                               # .github/workflows/fogu-desktop.yml: builds Windows, macOS and Linux
fogu desktop keygen --endpoint URL            # signing keys of self-updates; build then writes dist/latest.json
fogu mcu boards                               # board profiles
fogu mcu build blink.fg --board pico          # .uf2 / .hex / .bin + .elf
fogu mcu run blink.fg --board uno --port COM5 # build + flash + serial monitor
fogu mcu help                                 # every option of the fogu-mcu tool
```

## Errors

Errors are printed with a code, the position and an excerpt of the source:

```text
error[E0201]: division by zero
  --> main.fg:3
   |
 3 |     return a / b
   |     ^^^^^^^^^^^^
   = help: check the divisor before dividing
```
