import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { scopesOf, tokenize, type Tok } from "./tm";

const has = (scopes: string[], s: string) => scopes.some((x) => x === s || x.startsWith(s + "."));
const toks = (src: string) => tokenize(src);
const find = (ts: Tok[], text: string, nth = 0) => scopesOf(ts, text, nth);

describe("grammar", () => {
  test("template string with nested braces and nested templates", async () => {
    const ts = await toks("const msg = `Hi ${ { a: 1 }.a + count } and ${`in ${x}`} // no`");
    // the map literal braces inside ${…} do not close the interpolation
    expect(has(find(ts, "count"), "meta.template.expression.fogu")).toBe(true);
    expect(has(find(ts, "count"), "variable.other.readwrite")).toBe(true);
    expect(has(find(ts, "a", 0), "meta.object-literal.key")).toBe(true);
    expect(has(find(ts, "1"), "constant.numeric.integer")).toBe(true);
    // nested template inside an interpolation
    expect(find(ts, "x").filter((s) => s === "string.template.fogu").length).toBe(2);
    // `//` inside the template text is not a comment
    const tail = ts.find((t) => t.text.includes("// no"))!;
    expect(has(tail.scopes, "string.template")).toBe(true);
    expect(tail.scopes.some((s) => s.startsWith("comment"))).toBe(false);
    // and the line ends outside the string
    const last = ts.filter((t) => t.text === "`").pop()!;
    expect(has(last.scopes, "punctuation.definition.string.template.end")).toBe(true);
    expect(last.scopes.filter((s) => s === "string.template.fogu").length).toBe(1);
  });

  test("template escapes", async () => {
    const ts = await toks("const s = `a \\${not} \\` b ${y}`");
    expect(has(find(ts, "\\$"), "constant.character.escape")).toBe(true);
    expect(has(find(ts, "\\`"), "constant.character.escape")).toBe(true);
    expect(has(find(ts, "y"), "meta.template.expression")).toBe(true);
  });

  test("duration literals", async () => {
    const ts = await toks("const dur = 500ms + 1s + 1.5h + 2d + 10us + 5m");
    for (const n of ["500", "1", "1.5", "2", "10", "5"]) expect(has(find(ts, n), "constant.numeric.duration")).toBe(true);
    for (const u of ["ms", "s", "h", "d", "us", "m"]) expect(has(find(ts, u), "keyword.other.unit.duration")).toBe(true);
  });

  test("numbers", async () => {
    const ts = await toks("x = 0xFF + 0b101 + 1_000 + 2.5e-3 + 0..3");
    expect(has(find(ts, "FF"), "constant.numeric.hex")).toBe(true);
    expect(has(find(ts, "101"), "constant.numeric.binary")).toBe(true);
    expect(has(find(ts, "1_000"), "constant.numeric.integer")).toBe(true);
    expect(has(find(ts, "2.5e-3"), "constant.numeric.float")).toBe(true);
    expect(has(find(ts, ".."), "keyword.operator.range")).toBe(true);
    expect(has(find(ts, "3"), "constant.numeric.integer")).toBe(true);
  });

  test("optional chaining, nullish, error propagation, arrows, channels", async () => {
    const ts = await toks("const v = user?.profile?.name ?? fs.readText(p)? ; ch <- (x) => x; a -> b");
    expect(has(find(ts, "?."), "keyword.operator.optional-chain")).toBe(true);
    expect(has(find(ts, "profile"), "variable.other.property")).toBe(true);
    expect(has(find(ts, "??"), "keyword.operator.nullish")).toBe(true);
    expect(has(find(ts, "?"), "keyword.operator.error-propagation")).toBe(true);
    expect(has(find(ts, "readText"), "entity.name.function")).toBe(true);
    expect(has(find(ts, "<-"), "keyword.operator.channel")).toBe(true);
    expect(has(find(ts, "=>"), "storage.type.function.arrow")).toBe(true);
    expect(has(find(ts, "->"), "keyword.operator.pipe")).toBe(true);
  });

  test("comments inside strings and strings inside comments", async () => {
    const ts = await toks(`const s = "a // b /* c */" // real "comment"\n/* block "x" */ const t = 'q // r'`);
    expect(has(find(ts, "a // b /* c */"), "string.quoted.double")).toBe(true);
    const c = ts.find((t) => t.text.includes('real "comment"'))!;
    expect(has(c.scopes, "comment.line.double-slash")).toBe(true);
    expect(has(find(ts, ' block "x" '), "comment.block")).toBe(true);
    expect(has(find(ts, "q // r"), "string.quoted.single")).toBe(true);
    expect(has(find(ts, "const", 1), "storage.type")).toBe(true);
  });

  test("string escapes", async () => {
    const ts = await toks(`"a\\n\\q"`);
    expect(has(find(ts, "\\n"), "constant.character.escape")).toBe(true);
    expect(has(find(ts, "\\q"), "invalid.illegal.escape")).toBe(true);
  });

  test("imports and module specifiers", async () => {
    const ts = await toks(`import http from "fogu:http"\nimport { readText as rt } from "fogu:fs"\nimport { sum } from "./util.fg"`);
    expect(has(find(ts, "import"), "keyword.control.import")).toBe(true);
    expect(has(find(ts, "from"), "keyword.control.import")).toBe(true);
    expect(has(find(ts, "http", 1), "support.module")).toBe(true);
    expect(has(find(ts, "fs"), "support.module")).toBe(true);
    expect(has(find(ts, "./util.fg"), "string.quoted.double")).toBe(true);
    expect(has(find(ts, "as"), "keyword.control.import")).toBe(true);
  });

  test("declarations and type annotations", async () => {
    const ts = await toks(
      "fn area<T>(w: float, h: list<int> = [], ...rest): (float, error) {\n  return w * h, nil\n}\nlet count: int? = 0\nconst f: fn(int): bool = nil"
    );
    expect(has(find(ts, "fn"), "storage.type.function")).toBe(true);
    expect(has(find(ts, "area"), "entity.name.function")).toBe(true);
    expect(has(find(ts, "T"), "entity.name.type.parameter")).toBe(true);
    expect(has(find(ts, "w"), "variable.parameter")).toBe(true);
    expect(has(find(ts, "float"), "support.type.primitive")).toBe(true);
    expect(has(find(ts, "list"), "support.type.primitive")).toBe(true);
    expect(has(find(ts, "int"), "support.type.primitive")).toBe(true);
    expect(has(find(ts, "rest"), "variable.parameter")).toBe(true);
    expect(has(find(ts, "error"), "support.type.primitive")).toBe(true);
    expect(has(find(ts, "return"), "keyword.control.flow")).toBe(true);
    expect(has(find(ts, "count"), "variable.other.declaration")).toBe(true);
    expect(has(find(ts, "int", 1), "support.type.primitive")).toBe(true);
    expect(has(find(ts, "bool"), "support.type.primitive")).toBe(true);
  });

  test("structs, new, this, embed", async () => {
    const ts = await toks("struct Point {\n  x: int = 0\n  embed Base\n  new(x) { this.x = x }\n  fn len(): int { return 1 }\n}\nconst p = new Point(1)");
    expect(has(find(ts, "struct"), "storage.type.struct")).toBe(true);
    expect(has(find(ts, "Point"), "entity.name.type.struct")).toBe(true);
    expect(has(find(ts, "x"), "variable.other.property")).toBe(true);
    expect(has(find(ts, "int"), "support.type.primitive")).toBe(true);
    expect(has(find(ts, "embed"), "storage.modifier.embed")).toBe(true);
    expect(has(find(ts, "new"), "entity.name.function.constructor")).toBe(true);
    expect(has(find(ts, "this"), "variable.language.this")).toBe(true);
    expect(has(find(ts, "len"), "entity.name.function")).toBe(true);
    expect(has(find(ts, "new", 1), "keyword.operator.new")).toBe(true);
    expect(has(find(ts, "Point", 1), "entity.name.type")).toBe(true);
  });

  test("control flow, concurrency, builtins and Fogu", async () => {
    const ts = await toks("loop { break }\nfg fn(n) { ch <- n }(1)\nselect { case v = <-ch: print(v)\n default: nil }\nFogu.signal.wait()\nconst n = len(xs)\nmatch x { _ => 1 }");
    expect(has(find(ts, "loop"), "keyword.control.loop")).toBe(true);
    expect(has(find(ts, "fg"), "keyword.control.concurrency")).toBe(true);
    expect(has(find(ts, "select"), "keyword.control")).toBe(true);
    expect(has(find(ts, "case"), "keyword.control")).toBe(true);
    expect(has(find(ts, "default"), "keyword.control")).toBe(true);
    expect(has(find(ts, "print"), "support.function.builtin")).toBe(true);
    expect(has(find(ts, "len"), "support.function.builtin")).toBe(true);
    expect(has(find(ts, "Fogu"), "support.class")).toBe(true);
    expect(has(find(ts, "signal"), "support.variable.property")).toBe(true);
    expect(has(find(ts, "_"), "variable.language.wildcard")).toBe(true);
    expect(has(find(ts, "match"), "keyword.control")).toBe(true);
  });

  test("loop is only a keyword before a block", async () => {
    const ts = await toks("const loop2 = loop(x)");
    expect(has(find(ts, "loop"), "keyword.control")).toBe(false);
  });

  test("the sample file tokenizes and ends outside any rule", async () => {
    const src = readFileSync(join(import.meta.dir, "fixtures/sample.fg"), "utf8");
    const ts = await toks(src);
    const lastLine = Math.max(...ts.map((t) => t.line));
    // `const f …` on the last code line is at the top level again
    const lastConst = ts.filter((t) => t.text === "const").pop()!;
    expect(lastConst.scopes).toEqual(["source.fogu", "storage.type.fogu"]);
    expect(lastLine).toBeGreaterThan(40);
  });

  test("markdown fenced code blocks are highlighted as Fogu", async () => {
    const md = "# Title\n\n```fogu\nconst d = 500ms\n```\n\ntext `x`\n";
    const ts = await tokenize(md, "text.html.markdown");
    const ms = ts.find((t) => t.text === "ms")!;
    expect(ms.scopes).toContain("meta.embedded.block.fogu");
    expect(has(ms.scopes, "keyword.other.unit.duration")).toBe(true);
    const text = ts.find((t) => t.text.includes("text"))!;
    expect(text.scopes.some((s) => s.includes("fogu"))).toBe(false);
  });
});
