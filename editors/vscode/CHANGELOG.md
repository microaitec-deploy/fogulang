# Changelog

## 0.8.2

For Fogu 0.8.2.

- **Your own code, with its types.** Completion, hover, parameter hints, go to
  definition and the outline now know the declarations of the file and of the
  files it imports: `import { … } from "./file.fg"` lists that file's exports with
  their typed signatures (`export fn`, `const`, `struct`, `interface`, export
  lists, `export default`, re-exports); `from "./` completes the project's files;
  members come after a module alias, after a value whose struct or interface is
  declared (`new Point(…)`, `p: Point`), and after `this.`.
- **Parameter hints** for the standard library, `Fogu.` and the published
  libraries too.
- Types: the sized names (`i8` … `u64`, `f32`, `f64`, `int8` … `float64`, `byte`,
  `rune`) and `char` are highlighted and completed.
- Snippets: exports and imports of files with types (`efn`, `econst`, `estruct`,
  `einterface`, `export`, `edefault`, `reexport`, `importf`, `importa`,
  `importd`), `lett`, `any`, `sized`, `impl`, `embed`, `fng`, `verr`, `try`,
  `worker`, `args`, `env`, `exit`.
- Standard library and library data regenerated.

## 0.7.3

For Fogu 0.7.3, desktop 0.5.0 and mcu 0.5.0.

- Warnings of `fogu check` 0.7.3 (`warning[W0001]: …` with the `--> file:line`
  of the errors) are parsed again, in the editor and by the `$fogu-warning` problem
  matcher; the one-line format of 0.7.2 and older still works (`$fogu-warning-legacy`
  for tasks).
- Completion and hover for the published libraries: the module names after
  `import x from "` (`mcu`, `desktop`, `desktop/update.fg`, `datascience/array`, …),
  their members after the alias, and the objects their READMEs document (`pin.`,
  `card.`, `w.`, …). The data comes from the API tables of `libs/*/README.md`
  (`bun scripts/gen-libs.ts`).
- Commands *Desktop: CI Workflow* (`fogu desktop ci`), *Desktop: Update Keys*
  (`fogu desktop keygen`), *MCU: Detect Board and Port* and *MCU: Serial Monitor*.
- Standard library data regenerated (crypto.hashFile / encryptFile / decryptFile, …).

## 0.7.1

First release, for Fogu 0.7.1.

- `.fg` language registration, icon, comments, brackets, auto-closing (including
  backticks), indentation and region folding.
- TextMate grammar: keywords, declarations, template strings with nested `${…}`,
  durations, numbers, operators, type annotations, builtins, `Fogu.*`,
  `"fogu:…"` modules; ` ```fogu ` blocks in Markdown.
- Diagnostics from `fogu check` on open, save and while typing.
- Formatting with `fogu fmt -`.
- Run, check, test commands, CodeLens and editor title button.
- `fogu desktop dev/build` and `fogu mcu run/build` commands with a board picker.
- Task provider (`type: "fogu"`) and problem matchers `$fogu` / `$fogu-warning`.
- Completion and hover for keywords, globals, standard library modules and members.
- Snippets.
