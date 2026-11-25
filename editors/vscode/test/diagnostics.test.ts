import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parseCheckOutput, stripAnsi } from "../src/core/diagparse";
import { checkWithFogu } from "../src/core/proc";
import { FOGU } from "./fogubin";

const dir = join(import.meta.dir, "fixtures/check");

async function diagnose(name: string) {
  const file = join(dir, name);
  const lines = readFileSync(file, "utf8").split(/\r?\n/);
  const r = await checkWithFogu(FOGU, name, dir);
  expect(r.error).toBeUndefined();
  return { r, lines, diags: parseCheckOutput(r.stdout + "\n" + r.stderr, (n) => lines[n]) };
}

describe("fogu check output (real binary)", () => {
  test("undefined variable: column from the caret, help appended", async () => {
    const { r, diags } = await diagnose("undefined.fg");
    expect(r.code).toBe(1);
    expect(diags).toHaveLength(1);
    const d = diags[0];
    expect(d.severity).toBe("error");
    expect(d.code).toBe("E0100");
    expect([d.line, d.start, d.end]).toEqual([2, 9, 10]);
    expect(d.message).toBe("variable 'y' is not defined\nhelp: did you mean 'f'?");
    expect(d.file).toBe("undefined.fg");
  });

  test("lexical error with an explicit column", async () => {
    const { diags } = await diagnose("unterminated.fg");
    expect(diags).toHaveLength(1);
    expect([diags[0].line, diags[0].start, diags[0].end]).toEqual([0, 10, 11]);
    expect(diags[0].code).toBe("E0001");
    expect(diags[0].message).toBe("unterminated string");
  });

  test("syntax error", async () => {
    const { diags } = await diagnose("syntax.fg");
    expect(diags).toHaveLength(1);
    expect([diags[0].line, diags[0].start, diags[0].end]).toEqual([2, 0, 1]);
    expect(diags[0].code).toBe("E0002");
  });

  test("unknown type", async () => {
    const { diags } = await diagnose("type.fg");
    expect([diags[0].line, diags[0].start, diags[0].end]).toEqual([0, 9, 12]);
    expect(diags[0].message).toStartWith("unknown type 'Foo'");
  });

  test("unused variables are warnings with ranges on the name", async () => {
    const { r, diags } = await diagnose("unused.fg");
    expect(r.code).toBe(0);
    expect(diags.map((d) => [d.severity, d.line, d.start, d.end, d.unnecessary])).toEqual([
      ["warning", 1, 6, 11, true],
      ["warning", 2, 6, 10, true],
    ]);
    expect(diags[0].message).toStartWith("unused variable 'alpha'");
  });

  test("tabs: the caret (printed on tab-expanded text) maps back to the real column", async () => {
    const { lines, diags } = await diagnose("tabs.fg");
    const at = lines[2].indexOf("zed");
    expect([diags[0].line, diags[0].start, diags[0].end]).toEqual([2, at, at + 3]);
  });

  test("non-ASCII text before the error: UTF-16 columns", async () => {
    let { lines, diags } = await diagnose("unicode.fg");
    let at = lines[0].indexOf("bogus");
    expect([diags[0].start, diags[0].end]).toEqual([at, at + 5]);
    ({ lines, diags } = await diagnose("unicode_col.fg"));
    at = lines[0].indexOf("*");
    expect([diags[0].start, diags[0].end]).toEqual([at, at + 1]);
  });

  test("a clean file has no diagnostics", async () => {
    const r = await checkWithFogu(FOGU, join(import.meta.dir, "fixtures/clean.fg"));
    expect(r.code).toBe(0);
    expect(parseCheckOutput(r.stdout + r.stderr)).toEqual([]);
  });

  test("missing binary reports ENOENT", async () => {
    const r = await checkWithFogu("/nonexistent/fogu", "x.fg");
    expect(r.error?.code).toBe("ENOENT");
  });
});

describe("parser on captured output", () => {
  test("colored output and several files", () => {
    const out = [
      "\x1b[31m✗\x1b[0m \x1b[31merror[E0100]:\x1b[0m \x1b[1mvariable 'y' is not defined\x1b[0m",
      "  \x1b[36m-->\x1b[0m C:\\work\\a.fg:4",
      "   \x1b[36m|\x1b[0m",
      " \x1b[36m4\x1b[0m \x1b[36m|\x1b[0m   return y",
      "   \x1b[36m|\x1b[0m          \x1b[31m^\x1b[0m",
      "✓ b.fg",
      "  warning: b.fg: line 2: unused variable 'a' (prefix it with '_' to silence this)",
      "",
      "2 file(s), 1 with errors",
    ].join("\n");
    expect(stripAnsi(out)).not.toContain("\x1b");
    const d = parseCheckOutput(out);
    expect(d).toHaveLength(2);
    expect(d[0]).toMatchObject({ severity: "error", file: "C:\\work\\a.fg", line: 3, start: 9, end: 10 });
    expect(d[1]).toMatchObject({ severity: "warning", file: "b.fg", line: 1 });
  });

  test("errors without a position (CLI errors) are ignored", () => {
    expect(parseCheckOutput("error: unknown option: --x\n")).toEqual([]);
  });

  test("runtime error rendered by fogu test", () => {
    const out = "  ✗ rt\n    error[E0201]: division by zero\n      --> m.test.fg:7\n       |\n     7 |     const x = 1 / 0\n       |     ^^^^^^^^^^^^^^^\n       = help: check the divisor before dividing\n";
    const d = parseCheckOutput(out, () => "    const x = 1 / 0");
    expect(d[0]).toMatchObject({ line: 6, start: 4, end: 19, code: "E0201" });
    expect(d[0].message).toContain("help: check the divisor");
  });
});
