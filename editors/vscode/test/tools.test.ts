import { describe, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, writeFileSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expandPath, formatWithFogu, parseBoards, resolveFogu, run } from "../src/core/proc";
import { completionContext, importedModules, insideStringOrComment, membersOf, moduleByName, stdlib } from "../src/core/stdlib";
import { FOGU } from "./fogubin";

describe("fogu fmt - (stdin → stdout)", () => {
  test("formats code", async () => {
    const r = await formatWithFogu(FOGU, "fn   f(a,b){return a+b}\nconst  x=1+2\n");
    expect(r.error).toBeUndefined();
    expect(r.text).toBe("fn f(a, b) { return a + b }\nconst x = 1 + 2\n");
  });

  test("formatted text is stable", async () => {
    const once = (await formatWithFogu(FOGU, "struct P {\nx=0\nfn get(){return this.x}\n}\n")).text!;
    const twice = (await formatWithFogu(FOGU, once)).text!;
    expect(twice).toBe(once);
  });

  test("a lexical error is reported, not applied", async () => {
    const r = await formatWithFogu(FOGU, 'const s = "abc\n');
    expect(r.text).toBeUndefined();
    expect(r.error).toContain("unterminated string");
  });

  test("missing binary", async () => {
    const r = await formatWithFogu("/nonexistent/fogu", "x");
    expect(r.error).toContain("fogu not found");
  });
});

describe("fogu binary resolution", () => {
  const home = mkdtempSync(join(tmpdir(), "fogu-home-"));
  test("configured path wins and expands ~ and ${workspaceFolder}", () => {
    expect(resolveFogu("~/bin/fogu", { home })).toBe(join(home, "bin/fogu"));
    expect(resolveFogu("${workspaceFolder}/fogu", { workspaceFolder: "/ws" })).toBe("/ws/fogu");
    expect(expandPath("${userHome}/x", { home: "/h" })).toBe("/h/x");
  });
  test("PATH, then ~/.fogu/bin, then plain fogu", () => {
    expect(resolveFogu("", { home, path: "/nope", platform: "linux", foguHome: "" })).toBe("fogu");
    mkdirSync(join(home, ".fogu/bin"), { recursive: true });
    writeFileSync(join(home, ".fogu/bin/fogu"), "");
    expect(resolveFogu("", { home, path: "/nope", platform: "linux", foguHome: "" })).toBe(join(home, ".fogu/bin/fogu"));
    const pdir = join(home, "pathdir");
    mkdirSync(pdir);
    writeFileSync(join(pdir, "fogu"), "");
    chmodSync(join(pdir, "fogu"), 0o755);
    expect(resolveFogu("", { home, path: `/nope:${pdir}`, platform: "linux", foguHome: "" })).toBe(join(pdir, "fogu"));
  });
  test("windows looks for fogu.exe", () => {
    expect(resolveFogu("", { home, path: "", platform: "win32", foguHome: "" })).toBe("fogu");
  });
});

describe("fogu mcu boards parsing", () => {
  const sample = [
    "Boards:",
    "  nucleo-f401re    STM32F401RE LED PA5         button PC13   console USART2 PA2/PA3      flash: ST-Link drive",
    "                   ST-Link/V2-1: mass-storage drive NOD_F401RE + virtual COM port (USART2)",
    "  pico             RP2040      LED GP25        button GP15   console UART GP0/GP1        flash: UF2 drive RPI-RP2",
    "                   hold BOOTSEL while plugging in",
    "Chips:",
    "  f401re     F4         512 KB flash,  96 KB RAM, thumbv7em-none-eabihf        16 MHz (HSI)",
  ].join("\n");
  test("names, descriptions, details; chips ignored", () => {
    const b = parseBoards(sample);
    expect(b.map((x) => x.name)).toEqual(["nucleo-f401re", "pico"]);
    expect(b[0].description).toStartWith("STM32F401RE LED PA5");
    expect(b[1].detail).toBe("hold BOOTSEL while plugging in");
  });
});

describe("process runner", () => {
  test("passes stdin and collects stdout", async () => {
    const r = await run(FOGU, ["-p", "1 + 2 * 3"]);
    expect(r.code).toBe(0);
    expect(r.stdout.trim()).toBe("7");
  });
});

describe("stdlib data and completion contexts", () => {
  test("data has the main modules and members", () => {
    expect(stdlib.modules.length).toBeGreaterThan(50);
    expect(moduleByName("fs")?.members.some((m) => m.name === "readText" && m.kind === "function")).toBe(true);
    expect(moduleByName("http")?.alias).toBe("http");
    expect(stdlib.globals.map((g) => g.name)).toContain("print");
    expect(stdlib.fogu.map((g) => g.name)).toContain("signal");
    expect(moduleByName("log")?.members.map((m) => m.name)).toContain("info");
  });

  test("module names after fogu:", () => {
    expect(completionContext('import http from "fogu:ht', "")).toEqual({ kind: "module", prefix: "ht" });
    expect(completionContext("import x from 'fogu:", "")).toEqual({ kind: "module", prefix: "" });
  });

  test("named imports", () => {
    const line = 'import { readT } from "fogu:fs"';
    expect(completionContext("import { readT", line)).toEqual({ kind: "named-import", module: "fs", prefix: "readT" });
  });

  test("members after alias. and alias?.", () => {
    expect(completionContext("const t = fs.rea", "")).toEqual({ kind: "member", object: "fs", prefix: "rea" });
    expect(completionContext("x?.", "")).toEqual({ kind: "member", object: "x", prefix: "" });
    const imports = importedModules('import fs from "fogu:fs"\nimport * as j from "fogu:json"');
    expect(membersOf("fs", imports)?.length).toBeGreaterThan(5);
    expect(membersOf("j", imports)?.map((m) => m.name)).toContain("parse");
    expect(membersOf("Fogu", imports)?.map((m) => m.name)).toContain("env");
    expect(membersOf("other", imports)).toBeUndefined();
  });

  test("no completion inside strings and comments, but yes inside ${…}", () => {
    expect(insideStringOrComment('print("fs.')).toBe(true);
    expect(insideStringOrComment("x // fs.")).toBe(true);
    expect(insideStringOrComment("`a ${fs.")).toBe(false);
    expect(insideStringOrComment("`a ${b} c")).toBe(true);
    expect(completionContext('print("a.b', "").kind).toBe("none");
    expect(completionContext("const v = `${fs.re", "")).toEqual({ kind: "member", object: "fs", prefix: "re" });
  });
});
