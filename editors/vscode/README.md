# Fogu for Visual Studio Code

Language support for [Fogu](https://fogulang.microaitec.com) (`.fg` files): syntax
highlighting, errors and warnings from `fogu check` while you type, formatting with
`fogu fmt`, run/test commands, snippets, and completion and hover for the standard
library.

Works on Windows, Linux and macOS, and in **Remote - WSL** windows (the extension runs
next to the files, so it uses the `fogu` installed inside WSL).

## Requirements

The `fogu` executable. The extension looks for it in this order:

1. the `fogu.path` setting;
2. `fogu` (`fogu.exe` on Windows) on the `PATH`;
3. `$FOGU_HOME/bin/fogu`, then `~/.fogu/bin/fogu`.

In a WSL window, install Fogu inside WSL (not only on Windows).

## Features

- **Syntax highlighting** — keywords, declarations (`fn`, `struct`, `interface`,
  `let`/`const`, `import … from`), template strings with `${…}` (nested expressions,
  braces and templates), duration literals (`500ms`, `1.5h`), hex/binary numbers,
  operators (`?`, `?.`, `??`, `=>`, `<-`, `..`), type annotations, builtins and the
  `Fogu` object, `"fogu:http"` module specifiers. Fenced ` ```fogu ` blocks in
  Markdown are highlighted too.
- **Diagnostics** — `fogu check` runs when a file is opened or saved and (debounced)
  while you type; syntax, name and type errors and unused-variable warnings appear as
  squiggles at the exact column, with the `help:` hint in the message.
- **Formatting** — *Format Document* (and `editor.formatOnSave`) runs `fogu fmt -`.
  A file that does not parse is left as is.
- **Run / test** — the ▶ button in the editor title and the CodeLens at the top of a
  file run it in a terminal; in `*.test.fg` files there is a *Run test* lens above each
  `test(...)` and *Run tests in file*. Errors in the output are linked to the source
  (problem matchers `$fogu` and `$fogu-warning`).
- **Tasks** — *Terminal → Run Task → fogu*: `check`, `test`, `fmt --check`, `run`
  (the project's entry) and the scripts of `fogu.json`. In `tasks.json`:

  ```json
  { "type": "fogu", "command": "check", "problemMatcher": ["$fogu", "$fogu-warning"] }
  ```
- **Desktop apps** — in a project whose `fogu.json` has a `desktop` section:
  *Fogu: Desktop: Dev* (`fogu desktop dev`: the window with hot reload — the Fogu
  code reloads in place, a Vite UI gets HMR; a Windows window when you are in WSL),
  *Desktop: Build Executable*, *Desktop: CI Workflow* (`fogu desktop ci`: a GitHub
  Actions workflow that builds Windows, macOS and Linux) and *Desktop: Update Keys*
  (`fogu desktop keygen`, for self-updating apps).
- **Microcontrollers** — in a file that imports `mcu`: *Fogu: MCU: Build, Flash and
  Monitor* (`fogu mcu run`), *MCU: Build Firmware*, *MCU: Detect Board and Port*
  (`fogu mcu detect`) and *MCU: Serial Monitor*, with a board picker filled from
  `fogu mcu boards` and a serial-port prompt (both remembered; an empty port lets
  `fogu mcu` pick the one whose USB id matches the board).
- **Completion and hover** — keywords, globals, module names after `"fogu:`, members
  of imported standard library modules (`fs.`, `http.`, …) and of `Fogu.`, names in
  `import { … } from "fogu:…"`, with signatures and descriptions from the standard
  library reference. The **published libraries** too: `import mcu from "` offers
  `mcu`, `desktop`, `gpio`, `datascience/array`, …; after `mcu.` or `desktop.` come
  their functions, and after an object their READMEs document (`pin.`, `card.`,
  `w.` …) its methods, all with the descriptions of the library's API tables.
- **Your own code** — the declarations of the file and of the files it imports,
  with their types:
  - `import { … } from "./shapes.fg"` lists what that file exports (`export fn`,
    `export const`, `export struct`, `export interface`, `export { a as b }`,
    `export default`, re-exports), each with its typed signature; after
    `from "./` come the files and folders of the project.
  - functions, constants, structs and interfaces of the file, and imported names,
    in completion and hover: `fn circle(r: float): float`, the comment above the
    declaration, and the file it comes from.
  - members after a dot: the exports of `import * as m`, the fields and methods of
    a variable whose struct is known (`const p = new Point(…)`, `p: Point`, a
    parameter `s: Shape`), embedded structs included, and `this.` inside a struct.
  - **Parameter hints** while typing a call — your functions, methods and
    constructors (`new Point(`), and the standard library's.
  - **Go to definition** (F12) on a name, a member or an import path, and the
    **outline** of the file (functions, structs with their fields and methods).
- **Snippets** — `fn`, `fne` (returns value, error), `struct`, `interface`, `match`,
  `for`, `foreach`, `while`, `loop`, `fg`, `chan`, `select`, `import`, `err`
  (`const [v, err] = …`), `server` (fogu:web), `httpget`, `test`, `testfile`,
  `expect`, … With types and modules: `fnt`, `fng` (generic), `lett`, `any`,
  `sized` (`list<u8>` … `list<char>`), `impl` (interface + struct), `embed`,
  `efn` / `econst` / `estruct` / `einterface` (exported, typed), `export`,
  `edefault`, `reexport`, `importf` / `importa` / `importd` (from a file), `verr`,
  `try`, `worker`, and `Fogu.`: `args`, `env`, `exit`, `wait`.

## Commands

| Command | |
| --- | --- |
| Fogu: Run File | `fogu <file>` in a terminal (from the project folder) |
| Fogu: Check File | check now and show the problems |
| Fogu: Check Workspace | `fogu check` as a task |
| Fogu: Run Tests / Run Tests in File | `fogu test` / `fogu test <file>` |
| Fogu: Format File | format with `fogu fmt` |
| Fogu: Desktop: Dev / Build Executable | `fogu desktop dev` / `fogu desktop build` |
| Fogu: Desktop: CI Workflow / Update Keys | `fogu desktop ci` / `fogu desktop keygen` |
| Fogu: MCU: Build, Flash and Monitor / Build Firmware | `fogu mcu run` / `fogu mcu build` |
| Fogu: MCU: Detect Board and Port / Serial Monitor | `fogu mcu detect` / `fogu mcu monitor` |
| Fogu: Show Output | the extension's log |

## Settings

| Setting | Default | |
| --- | --- | --- |
| `fogu.path` | `""` | Path of `fogu`. `~` and `${workspaceFolder}` are expanded. |
| `fogu.checkOnSave` | `true` | Check on open and save. |
| `fogu.checkOnType` | `true` | Check unsaved text while typing (via a temporary file). |
| `fogu.checkDelay` | `400` | Milliseconds after the last keystroke. |
| `fogu.codeLens` | `true` | Run/Check/Run test lenses. |
| `fogu.mcu.board` | `""` | Board for the MCU commands (empty: ask). |
| `fogu.mcu.port` | `""` | Serial port for *MCU Run* (empty: ask). |

To format on save:

```json
"[fogu]": { "editor.formatOnSave": true }
```

## Install from a .vsix

```sh
code --install-extension fogu-vscode.vsix
```

In VS Code: *Extensions* view → `…` → *Install from VSIX…*. In a WSL window the
extension is installed on the WSL side (VS Code asks, or use *Install in WSL*).

## Development

```sh
bun install
bun run gen        # syntaxes/*.json from scripts/gen-grammar.ts, src/data/stdlib.json from lang/skills/fogu/stdlib.md
bun run typecheck
bun test           # grammar (vscode-textmate), diagnostics against the real fogu, fmt, matchers
bun run package    # fogu-vscode.vsix
```

`FOGU_BIN=/path/to/fogu bun test` picks the binary the tests use (default:
`../../lang/target/release/fogu`, else `fogu` on the PATH).

## Limitations

- Not a language server: declarations are read from the text of the files, at
  their top level. Types are the ones written (annotations, `new Name(…)`,
  literals): the type of an expression such as `a.b().c` is not inferred, so
  members are offered for standard library modules, `Fogu.`, module aliases and
  values whose struct or interface is declared. No rename, no find-references.
- `fogu check` stops at the first error of a file, so one error is shown at a time.
- Highlighting of types is limited to annotations after `let`/`const`, in function
  signatures and in struct fields.

## License

MIT © Edmilson Soares
