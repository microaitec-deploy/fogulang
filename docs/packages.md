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

## Limitations

No central registry: `"^1.2.0"` alone does not resolve, always use a source.
Private repositories use the user's `git`/`ssh` credentials (no prompts).
Transitive dependencies are flat (the first version found wins; a conflict warning is shown).
No signatures or `fogu audit` yet (spec §41).
