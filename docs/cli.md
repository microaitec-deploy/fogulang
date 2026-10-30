# CLI

```text
fogu                         REPL (history, multi-line input, value echo)
fogu <file.fg> [args]        run (the args are in system.info.args())
fogu run [file|script]       run; without a target uses "entry" from fogu.json
fogu <script>                script from fogu.json (e.g. fogu dev)
fogu -e '<code>'             run code;  fogu -p '1+2*3' prints the result
fogu --watch <file>          restart when *.fg / fogu.json change
fogu check [files]           syntax, names and types (no args: every .fg here)
fogu build --compile [file]  standalone executable in dist/ (-o <folder>, --target <platform>|all)
fogu test [paths] [-t text]  run the tests of *.test.fg files
fogu init | add | remove | install | update | list | pm   (see packages.md)
fogu --version | --help
```

Options: `--no-jit`, `--jit-stats`, `--jit-debug`, `--jit-dump`, `--strict`,
`--cores <n|cpus>`.
Environment: `FOGU_HOME`, `NO_COLOR`, `FOGU_JIT=0`, `FOGU_JIT_CALLS`, `FOGU_JIT_LOOPS`,
`FOGU_DOTENV=0`, `FOGU_CORES`.

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

## `fogu check`

Checks every file without running it: syntax, undefined names, use before
declaration, unknown type names, `?` in a function whose return type has no
`error`, non-exhaustive `match`. Unused local variables are reported as
warnings (prefix a name with `_` to silence it). The same errors stop
`fogu <file>` before the program starts.

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
fogu desktop dev                              # Vite dev server + window (current folder)
fogu desktop build --target windows           # dist/my-app.exe: the generic host + the app, in seconds (no Rust)
fogu desktop build --installer                # compiled in: installers and the app's icon (Rust + Tauri CLI)
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
