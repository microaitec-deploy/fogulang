import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { run } from "../src/core/proc";
import { FOGU } from "./fogubin";

const root = join(import.meta.dir, "..");
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const patterns: Record<string, any> = Object.fromEntries(pkg.contributes.problemPatterns.map((p: any) => [p.name, p]));

/** Applies a VS Code problem pattern (single or multi-line) to output lines, like the task system does. */
function applyPattern(p: any, lines: string[]) {
  const steps = p.patterns ?? [p];
  const found: Record<string, string>[] = [];
  for (let i = 0; i < lines.length; i++) {
    const res: Record<string, string> = {};
    let ok = true;
    for (let k = 0; k < steps.length; k++) {
      const m = new RegExp(steps[k].regexp).exec(lines[i + k] ?? "");
      if (!m) {
        ok = false;
        break;
      }
      for (const key of ["file", "line", "column", "message", "code", "severity"]) {
        if (typeof steps[k][key] === "number" && m[steps[k][key]] !== undefined) res[key] = m[steps[k][key]];
      }
    }
    if (ok) found.push(res);
  }
  return found;
}

describe("problem matchers", () => {
  test("fogu check output (several files)", async () => {
    const dir = join(import.meta.dir, "fixtures/check");
    const r = await run(FOGU, ["check", "undefined.fg", "unterminated.fg", "unused.fg"], { cwd: dir });
    const lines = (r.stdout + "\n" + r.stderr).split("\n");
    const errors = applyPattern(patterns["fogu-error"], lines);
    expect(errors).toEqual([
      { severity: "error", code: "E0100", message: "variable 'y' is not defined", file: "undefined.fg", line: "3" },
      { severity: "error", code: "E0001", message: "unterminated string", file: "unterminated.fg", line: "1", column: "11" },
    ]);
    const warnings = applyPattern(patterns["fogu-warning"], lines);
    expect(warnings.map((w) => [w.severity, w.file, w.line])).toEqual([
      ["warning", "unused.fg", "2"],
      ["warning", "unused.fg", "3"],
    ]);
  });

  test("fogu test output", async () => {
    const dir = join(import.meta.dir, "fixtures/tests");
    const r = await run(FOGU, ["test"], { cwd: dir });
    expect(r.code).toBe(1);
    const errors = applyPattern(patterns["fogu-error"], (r.stdout + "\n" + r.stderr).split("\n"));
    expect(errors).toEqual([{ severity: "error", message: "expected 2 to be 3", file: "m.test.fg", line: "8" }]);
  });
});

describe("manifest", () => {
  test("every contributed file exists", () => {
    const files = [
      pkg.main.replace(/^\.\//, ""),
      pkg.icon,
      ...pkg.contributes.grammars.map((g: any) => g.path),
      ...pkg.contributes.snippets.map((s: any) => s.path),
      ...pkg.contributes.languages.map((l: any) => l.configuration),
    ];
    for (const f of files) expect(() => readFileSync(join(root, f))).not.toThrow();
  });

  test("every command used in menus is declared, and all declared commands are registered in the source", () => {
    const declared = new Set(pkg.contributes.commands.map((c: any) => c.command));
    for (const items of Object.values(pkg.contributes.menus) as any[]) for (const m of items) expect(declared.has(m.command)).toBe(true);
    const src = readFileSync(join(root, "src/commands.ts"), "utf8");
    for (const c of declared) expect(src.includes(`"${c}"`)).toBe(true);
  });

  test("snippets are well formed", () => {
    const snippets = JSON.parse(readFileSync(join(root, "snippets/fogu.json"), "utf8"));
    const prefixes = new Set<string>();
    for (const [name, s] of Object.entries(snippets) as [string, any][]) {
      expect(typeof s.prefix).toBe("string");
      expect(Array.isArray(s.body)).toBe(true);
      expect(prefixes.has(s.prefix)).toBe(false);
      prefixes.add(s.prefix);
      expect(s.description, name).toBeTruthy();
    }
  });

  test("language configuration is valid JSON with backtick auto-closing", () => {
    const lc = JSON.parse(readFileSync(join(root, "language-configuration.json"), "utf8"));
    expect(lc.comments.lineComment).toBe("//");
    expect(lc.autoClosingPairs.some((p: any) => p.open === "`")).toBe(true);
    new RegExp(lc.indentationRules.increaseIndentPattern);
    new RegExp(lc.indentationRules.decreaseIndentPattern);
  });
});
