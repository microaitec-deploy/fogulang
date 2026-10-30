# Contributing to Fogu

Thank you for your interest! This guide summarizes how to set up, test and
propose changes.

## Setup

```bash
cargo build --release
cargo test --release      # CLI tests, spec suite and the crates' unit tests
```

The standard library areas are cargo features (`network database messaging ops
hardware ai media rpc`, all on by default; `base` and `io` are always built).
`cargo build --release --no-default-features [--features network,…]` builds a
smaller binary; check that a change still builds with the features it does not
need turned off. A new native module goes in `crates/fogu-std/src/native/<area>/`.

Requires Rust (2024 edition) and `git` on the `PATH`.

Cross builds for ARM Linux work from x86_64 Linux with the GNU cross compilers
(OpenSSL is vendored for those targets, `.cargo/config.toml` sets the linkers):

```bash
sudo apt install gcc-aarch64-linux-gnu gcc-arm-linux-gnueabihf
rustup target add aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf
cargo build --release --target aarch64-unknown-linux-gnu
```

## Layout

```text
crates/
├── fogu-core/    the language: lexer, parser, resolver, interpreter, JIT, scheduler
│                 (src/runtime/), values, built-ins (str/list/map/buffer methods,
│                 globals, fogu:math/json/test, events), the tokio runtime, the Host trait
├── fogu-std/     standard library (fogu:*), by area: src/native/<area>
│                 (base io network database messaging ops hardware aiml media rpc)
└── fogu-pkg/     package manager
src/
├── lib.rs        re-exports the crates (fogu::runtime, fogu::native, fogu::pkg)
├── main.rs       `fogu` binary
├── embed.rs      Rust embedding API
└── toolkit/      CLI, REPL, commands, diagnostics, build
ffi/              libfogu_rt (C API, fogu.h)
tests/
├── cli.rs        CLI tests
├── spec.rs       conformance suite runner
└── spec/         .fg programs with the expected output (.out / .err)
examples/         example programs
benches/          JIT benchmarks (.fg)
docs/             specification, guides and design notes
```

## Rules

- **The interpreter is the semantic reference.** The JIT must produce exactly
  the same output and the same errors; when in doubt, the JIT gives up (`bail`).
- **Every feature comes with a test** in `tests/spec/`: `<name>.fg` +
  `<name>.out` (expected stdout, written by hand) and, for errors, `<name>.err`
  (lines that stderr must contain). Each test runs with and without the JIT.
- Documentation, error messages, comments, examples and tests are in **English**.
- Update [CHECKLIST.md](CHECKLIST.md) and [CHANGELOG.md](CHANGELOG.md).
- Significant language changes follow the FEP process
  ([docs/spec.md](docs/spec.md) §76).

## License

By contributing, you agree that your code is distributed under the
[MIT license](LICENSE).
