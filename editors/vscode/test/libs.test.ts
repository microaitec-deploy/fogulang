import { describe, expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { parseLib, parseLibs, readmeImports, tableCells } from "../scripts/gen-libs";
import { completionContext, importedModules, libModule, libs, membersOf, moduleByName } from "../src/core/stdlib";

const SAMPLE = `# mcu

\`\`\`fogu
import mcu from "mcu"
const led = mcu.led().output()
\`\`\`

| Call | Result |
| --- | --- |
| \`mcu.pin("PA5")\` | a pin (names per chip) |
| \`mcu.led()\`, \`mcu.button()\` | the board's user LED / button pin |
| \`pin.high()\`, \`pin.low()\`, \`pin.toggle()\`, \`pin.write(b)\` | drive an output |
| \`pin.onEdge("rising" \\| "falling" \\| "both", h)\` | \`h\` runs after that edge |
| \`mcu.timer(period, h, { oneShot, immediate })\` → \`timer\` | a hardware timer: \`t.cancel()\`, \`t.restart()\` |
| \`mcu.ui.button({ x, y, w, h, text, onClick })\` | a push button |
| \`mcu.wifi.connect(ssid, password)\` → bool | joins a network |
| \`e.g.\` | not an object |
| \`let\` / \`const\` | keywords only |
| \`mcu.fs\`, the file \`fogu-mcu.d.ts\` and \`examples/01-blink.fg\` | file names are not members |
`;

describe("gen-libs: README API tables", () => {
  test("table cells keep escaped pipes inside code", () => {
    expect(tableCells('| `a \\| b` | c |')).toEqual(["`a | b`", "c"]);
    expect(tableCells("not a row")).toBeUndefined();
  });

  test("import aliases: the most used one per spec, the lib's own name as the fallback", () => {
    const md = 'import np from "datascience/array"\nimport df from "datascience/dataframe"\nimport df from "datascience/dataframe"\n';
    const m = readmeImports(md, "datascience");
    expect(m.get("datascience/array")).toBe("np");
    expect(m.get("datascience/dataframe")).toBe("df");
    expect(m.get("datascience")).toBe("datascience");
  });

  test("members of the module and of the documented objects", () => {
    const lib = parseLib("mcu", SAMPLE, { version: "0.5.0", description: "Microcontrollers" });
    const mod = lib.modules.find((m) => m.spec === "mcu")!;
    expect(mod.alias).toBe("mcu");
    expect(mod.members.map((m) => m.name)).toEqual(["pin", "led", "button", "timer", "ui", "wifi", "fs"]);
    const pin = mod.members.find((m) => m.name === "pin")!;
    expect(pin.kind).toBe("function");
    expect(pin.signature).toBe('mcu.pin("PA5")');
    expect(pin.doc).toBe("a pin (names per chip)");
    expect(mod.members.find((m) => m.name === "ui")!.kind).toBe("property");
    // the pin object: its own table, with the escaped pipe kept
    expect(lib.objects.pin.map((m) => m.name)).toEqual(["high", "low", "toggle", "write", "onEdge"]);
    expect(lib.objects.pin.find((m) => m.name === "onEdge")!.signature).toBe('pin.onEdge("rising" | "falling" | "both", h)');
    // `mcu.ui.button` and `mcu.wifi.connect`: the ui / wifi objects
    expect(lib.objects.ui.map((m) => m.name)).toEqual(["button"]);
    expect(lib.objects.wifi.map((m) => m.name)).toEqual(["connect"]);
    // one-member noise (`e.g.`) is dropped; the timer handle has two members
    expect(lib.objects.e).toBeUndefined();
    expect(lib.objects.t.map((m) => m.name)).toEqual(["cancel", "restart"]);
  });

  test("the repository's libraries parse and the generated data is current", () => {
    const root = join(import.meta.dir, "../../../libs");
    if (!existsSync(root)) return;
    const data = parseLibs(root);
    const names = data.libs.map((l) => l.name);
    expect(names).toContain("mcu");
    expect(names).toContain("desktop");
    const mcu = data.libs.find((l) => l.name === "mcu")!;
    expect(mcu.modules[0].members.length).toBeGreaterThan(20);
    expect(Object.keys(mcu.objects)).toContain("pin");
    // src/data/libs.json is what the extension ships: regenerate it with `bun run gen`
    expect(libs.libs.map((l) => [l.name, l.version])).toEqual(data.libs.map((l) => [l.name, l.version]));
  });
});

describe("library modules in completion", () => {
  const doc = 'import mcu from "mcu"\nimport update from "desktop/update.fg"\nimport fs from "fogu:fs"\nimport x from "./x.fg"\n';

  test("imports of libraries are recognised, relative ones are not", () => {
    const m = importedModules(doc);
    expect(m.get("mcu")).toBe("lib:mcu");
    expect(m.get("update")).toBe("lib:desktop/update");
    expect(m.get("fs")).toBe("fs");
    expect(m.has("x")).toBe(false);
  });

  test("members after a dot: the module, then the objects its README documents", () => {
    const imports = importedModules(doc);
    const mcu = membersOf("mcu", imports)!;
    expect(mcu.some((m) => m.name === "pin")).toBe(true);
    expect(membersOf("pin", imports)!.some((m) => m.name === "toggle")).toBe(true);
    expect(membersOf("update", imports)!.some((m) => m.name === "check")).toBe(true);
    expect(membersOf("nothing", imports)).toBeUndefined();
    // without the library imported, its objects are not offered
    expect(membersOf("pin", importedModules('import fs from "fogu:fs"'))).toBeUndefined();
  });

  test("the module specifier is completed with the libraries", () => {
    expect(completionContext('import mcu from "', 'import mcu from "')).toEqual({ kind: "library", prefix: "" });
    expect(completionContext('import np from "data', 'import np from "data')).toEqual({ kind: "library", prefix: "data" });
    expect(completionContext('import fs from "fogu:', 'import fs from "fogu:')).toEqual({ kind: "module", prefix: "" });
    expect(completionContext('import x from "./', 'import x from "./')).toEqual({ kind: "path", typed: "./" });
    expect(completionContext('import { a, ', 'import { a,  } from "./shapes.fg"')).toEqual({ kind: "named-local", spec: "./shapes.fg", prefix: "", listed: ["a"] });
    expect(moduleByName("lib:mcu")?.alias).toBe("mcu");
    expect(libModule("desktop/update.fg")?.name).toBe("desktop/update");
  });
});
