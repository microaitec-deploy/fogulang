import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  callAt,
  enclosingStruct,
  localImports,
  localMembers,
  membersOfType,
  moduleExports,
  parseSymbols,
  pathCandidates,
  resolveImport,
  scan,
  scopeOf,
  splitParams,
  symbolMarkdown,
  typeOfName,
} from "../src/core/symbols";

const dir = join(import.meta.dir, "fixtures", "project");
const shapes = join(dir, "shapes.fg");
const mainFile = join(dir, "main.fg");
const main = readFileSync(mainFile, "utf8");

describe("symbols: declarations of a file", () => {
  const mod = parseSymbols(readFileSync(shapes, "utf8"));
  const sym = (n: string) => mod.symbols.find((s) => s.name === n)!;

  test("functions keep their typed parameters and return type", () => {
    expect(sym("circle").signature).toBe("fn circle(r: float): float");
    expect(sym("circle").doc).toBe("The area of a circle.");
    expect(sym("parse").signature).toBe("fn parse(text: str, strict = false): (int, error)");
    expect(sym("helper").signature).toBe("fn helper(a: int, b: int): int");
    expect(sym("helper").kind).toBe("function");
    expect(sym("_private").exported).toBe(false);
  });

  test("variables: the declared type, or the one the value shows", () => {
    expect(sym("PI").signature).toBe("const PI: float");
    expect(sym("counter").signature).toBe("let counter: int");
  });

  test("structs: fields, constructor and methods (a brace inside a template string is not a block)", () => {
    const p = sym("Point");
    expect(p.kind).toBe("struct");
    expect(p.doc).toBe("A point on the plane.");
    expect(p.members!.map((m) => m.signature)).toEqual([
      "x: int",
      "y: int",
      "label: str",
      "new Point(x: int, y: int = 0)",
      "norm(): float",
      "move(dx: int, dy: int)",
    ]);
    expect(p.members!.find((m) => m.name === "norm")!.doc).toBe("Distance from the origin.");
    expect(sym("Pixel").embeds).toEqual(["Point"]);
    expect(sym("Pixel").members!.map((m) => m.signature)).toEqual(["color: u8"]);
  });

  test("interfaces: one requirement per line, or several on one line", () => {
    expect(sym("Shape").members!.map((m) => m.signature)).toEqual(["area(): float", "name(): str"]);
    expect(sym("Mini").members!.map((m) => m.signature)).toEqual(["a()", "b(x: int): int"]);
  });

  test("exports: declarations, export lists with renames, the default", () => {
    expect([...mod.exports.keys()].sort()).toEqual(["PI", "Pixel", "Point", "Shape", "add", "circle", "counter", "parse"]);
    expect(mod.exports.get("add")!.name).toBe("helper");
    expect(mod.default!.name).toBe("Point");
  });

  test("re-exports are followed", () => {
    const idx = moduleExports(join(dir, "index.fg"))!;
    expect(idx.exports.has("VERSION")).toBe(true);
    expect(idx.exports.get("round")!.name).toBe("circle");
    expect(idx.exports.get("Point")!.kind).toBe("struct");
  });
});

describe("symbols: what a document can name", () => {
  test("imports from files", () => {
    const imps = localImports(main);
    expect(imps.map((i) => i.spec)).toEqual(["./shapes.fg", "./shapes.fg", "./index.fg"]);
    expect(imps[0]).toEqual({ spec: "./shapes.fg", default: "Point", names: [["circle", "circle"], ["parse", "parseInt"], ["Shape", "Shape"]] });
    expect(imps[1].namespace).toBe("shapes");
    expect(resolveImport(mainFile, "./shapes")).toBe(shapes);
    expect(resolveImport(mainFile, "./nope.fg")).toBeUndefined();
  });

  test("scope: imported names keep their signatures; modules keep their exports", () => {
    const scope = scopeOf(main, mainFile);
    expect(scope.names.get("parseInt")!.signature).toBe("fn parse(text: str, strict = false): (int, error)");
    expect(scope.names.get("Point")!.kind).toBe("struct");
    expect(scope.names.get("show")!.signature).toBe("fn show(s: Shape, p: Point): str");
    expect(scope.modules.get("shapes")!.exports.has("circle")).toBe(true);
    // a file without a default export imports as the module
    expect(scope.modules.get("lib")!.exports.has("VERSION")).toBe(true);
    expect(symbolMarkdown(scope.names.get("circle")!, scope, "./shapes.fg")).toContain("fn circle(r: float): float");
  });

  test("members after a dot: module alias, typed variable, parameter, this", () => {
    const names = (object: string, at = main.length) => localMembers(main, mainFile, object, at)?.map((m) => m.name);
    expect(names("shapes")).toContain("circle");
    expect(names("origin")).toEqual(["x", "y", "label", "norm", "move"]);
    expect(names("p")).toEqual(["x", "y", "label", "norm", "move"]);
    expect(names("s")).toEqual(["area", "name"]);
    expect(names("this", main.indexOf("this.size"))).toEqual(["size", "grow"]);
    expect(names("fs")).toBeUndefined();
    expect(typeOfName(main, "origin", main.length)).toBe("Point");
    expect(enclosingStruct(main, main.indexOf("this.size"))).toBe("Local");
    expect(enclosingStruct(main, main.indexOf("return s.name"))).toBeUndefined();
  });

  test("embedded structs give their members", () => {
    const scope = scopeOf(readFileSync(shapes, "utf8"), shapes);
    expect(membersOfType("Pixel", scope)!.map((m) => m.name)).toEqual(["color", "x", "y", "label", "norm", "move"]);
  });

  test("paths after from", () => {
    const c = pathCandidates(mainFile, "./");
    expect(c.map((x) => x.label).sort()).toEqual(["index.fg", "shapes.fg"]);
  });
});

describe("symbols: the call at the cursor", () => {
  test("name, object and argument index", () => {
    expect(callAt("print(circle(1, ")).toEqual({ object: undefined, name: "circle", arg: 1, isNew: false });
    expect(callAt("const p = new Point(1, [2, 3], ")).toEqual({ object: undefined, name: "Point", arg: 2, isNew: true });
    expect(callAt('fs.readText("a,b", ')).toEqual({ object: "fs", name: "readText", arg: 1, isNew: false });
    expect(callAt("x = f(1) + ")).toBeUndefined();
    expect(callAt("if (a, ")).toBeUndefined();
  });

  test("parameters split at top-level commas", () => {
    expect(splitParams("a: int, b: map<str, int>, f: fn(int, int): int = nil, ...rest")).toEqual(["a: int", "b: map<str, int>", "f: fn(int, int): int = nil", "...rest"]);
    expect(splitParams("")).toEqual([]);
  });

  test("scan blanks strings and comments and keeps offsets", () => {
    const s = scan('a = "{" // }\nb = `x ${ {k: 1} } y {`\nc');
    expect(s.code.length).toBe('a = "{" // }\nb = `x ${ {k: 1} } y {`\nc'.length);
    expect(s.depth).toEqual([0, 0, 0]);
  });
});
