# Packages

Fogu packages come from **git** repositories (GitHub, GitLab, any URL),
`.tar.gz` archives or local folders, and live in a **global store**:

```text
~/.fogu/fogu_modules/<name>@<commit>/        (FOGU_HOME changes the base)
```

Several versions coexist; each project picks its own through `fogu.lock`.

## Project files

`fogu.json`

```json
{ "name": "my-app", "version": "0.1.0", "entry": "main.fg",
  "scripts": { "start": "fogu run main.fg" },
  "dependencies": { "greet": "github:user/greet#semver:^1.2.0" } }
```

`fogu.lock` pins, per package, the exact commit and the `sha256` checksum of the
tree. If the content in the store does not match the lock, the installation
fails (integrity). Install scripts are never run.

## Sources

| Form | Meaning |
| --- | --- |
| `mcu` · `mcu@0.1.0` | a library of the Fogu server: `<server>/libs/<name>/<version>.tar.gz` |
| `user/repo` · `github:u/r` | GitHub |
| `gitlab:g/r` · `bitbucket:u/r` · `codeberg:u/r` | other hosts |
| `https://host/x.git` · `git@host:u/r.git` · `git+file:///path` | any git |
| `...#main` · `...#v1.0.0` · `...#<sha>` | branch / tag / commit |
| `...#semver:^1.2.0` or `u/r@^1.2.0` | highest `vX.Y.Z` tag that satisfies |
| `https://host/pkg.tar.gz` | archive (checksum of the file) |
| `./folder` · `file:../folder` | local folder (copied into the store) |

Ranges: `^1.2.0`, `~1.2.3`, `1.x`, `>=1.0.0 <2.0.0`, `*`.
To use another name in the import: `fogu add alias@github:u/r`.

A bare name is a library of the Fogu server: the server is `FOGU_SERVER` or the
one the install script saved in `~/.fogu/server`. `fogu add mcu` asks
`<server>/api/libs/mcu` for the latest version and writes the archive URL of that
version to `fogu.json`, so the project stays pinned (and `fogu.lock` pins its
sha256). A file of an installed package runs with `fogu <name>/<file>.fg`
(e.g. `fogu desktop/build.fg .`).

## Commands

```text
fogu init [-y]             create fogu.json, main.fg and .gitignore
fogu add <package...> [-g] add and install (-g = global list in ~/.fogu)
fogu remove <package...>   remove (the store is not cleaned)
fogu install [--frozen]    install what fogu.json declares (--frozen: CI)
fogu update [package...]   move to the latest commits/tags
fogu list [-g]             dependency tree
fogu pm path|ls|cache [clean]|hash <folder>
```

## Using them in code

```fogu
import { hello } from "greet"            // the package's entry file
import extra from "greet/lib/extra"      // a file inside the package
```

The entry is the package's `fogu.json` `"entry"` (or `main.fg`/`index.fg`).
Resolution order: `<project>/fogu_modules/<name>` (optional vendoring) → the
project lock → the global lock → the most recent version in the store.
`fogu build --compile` embeds the packages a program uses.

## Creating and publishing a library

A library is an ordinary Fogu project: a `fogu.json` with a `name`, a `version`
and an `entry` file whose `export`s are the library's API. Publish it on any git
host and others install it with `fogu add`. The repository
[microaitec-deploy/fogulang_module](https://github.com/microaitec-deploy/fogulang_module)
is a complete example (and the test of this feature).

```text
fogulang_module/
  fogu.json        { "name": "fogulang_module", "version": "0.1.0", "entry": "main.fg", "license": "MIT" }
  main.fg          export fn greet(name, lang = "en") { … }   export struct Counter { … }
  main.test.fg     tests with fogu:test — `fogu test`
  readme.md        shown on GitHub; what is exported and how to use it
  LICENSE
```

```fogu
// main.fg of the library
export const version = "0.1.0"

export fn greet(name: str, lang = "en") {
    return match lang {
        "pt" => `Olá, ${name}!`
        _ => `Hello, ${name}!`
    }
}

export struct Counter {
    value: int
    step: int
    new(step = 1) { this.value = 0; this.step = step }
    fn next() { this.value = this.value + this.step; return this.value }
}
```

Rules worth following: names without `export` stay private; return errors as
values (`return nil, error("…")`) instead of `panic`; no global state at the top
level (it runs once, when the library is first imported); put extra files next to
the entry (`import extra from "fogulang_module/extra"` works for any file of the
package); tag releases `v0.1.0`, `v0.2.0`… so `fogu add user/repo@^0.1.0` resolves.

Publishing is a `git push`; using it from another project:

```bash
fogu add microaitec-deploy/fogulang_module        # GitHub shorthand (also github:…, gitlab:…, a URL)
fogu add microaitec-deploy/fogulang_module@^0.1.0 # the highest matching vX.Y.Z tag
```

```fogu
import mod from "fogulang_module"
import { Counter } from "fogulang_module"

print(mod.greet("Ana", "pt"))        // Olá, Ana!
const c = new Counter(5)
print(c.next(), c.next())            // 5 10
```

`fogu add` writes the dependency to `fogu.json` and the exact commit and
checksum to `fogu.lock`; `fogu install --frozen` reproduces it in CI, `fogu update`
moves to the latest commit or tag. A library with native parts (a `cdylib` loaded
through `fogu:ffi`, like `datascience` or `mqtt-broker`) or with a tool
(`mcu`) is published on the Fogu server instead, where the prebuilt files travel
inside the package: see [Libraries on the server](../../docs.md#libraries-on-the-server).

## Limitations

No central registry: `"^1.2.0"` alone does not resolve, always use a source.
Private repositories use the user's `git`/`ssh` credentials (no prompts).
Transitive dependencies are flat (the first version found wins; a conflict warning is shown).
No signatures or `fogu audit` yet (spec §41).
