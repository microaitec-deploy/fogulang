// Builds src/data/libs.json (the libraries' APIs for completion and hover) from
// the API tables of libs/<name>/README.md: every table row whose first cell is
// code, such as `| \`mcu.pin("PA5")\` | a pin … |`, gives the members of an
// object (`mcu`, `pin`, `desktop`, `w`, …). Members of the library's import
// alias (from `import mcu from "mcu"` in the README, also `import np from
// "datascience/array"`) belong to that module; the others are object tables
// (`pin.`, `ch.`, `card.`) offered after a dot when that library is imported.
// Run with: bun scripts/gen-libs.ts [path/to/libs]
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export interface Member {
  name: string;
  signature: string;
  returns?: string;
  doc?: string;
  kind: "function" | "property";
}

export interface LibModule {
  /** what the import names: `mcu`, `datascience/array` */
  spec: string;
  /** the import alias the README uses: `mcu`, `np` */
  alias: string;
  members: Member[];
}

export interface LibInfo {
  name: string;
  version: string;
  summary: string;
  modules: LibModule[];
  /** object name (`pin`, `w`, `card`) → its members */
  objects: Record<string, Member[]>;
}

export interface LibsData {
  libs: LibInfo[];
}

const IDENT = /^[A-Za-z_][A-Za-z0-9_]*$/;
/** `fogu-desktop.d.ts`, `update.fg`, `fogu.json`: a file name, not a member, when there is no call. */
const FILE_EXT = new Set(["fg", "js", "ts", "d", "mjs", "json", "md", "txt", "html", "css", "exe", "dll", "so", "dylib", "png", "ico", "svg", "toml", "lock", "yml", "yaml", "rs", "py", "sh", "ps1", "tar", "gz", "zip", "wasm", "pdf", "jpg", "bin", "hex", "uf2", "elf"]);

/** The cells of a Markdown table row (`\|` inside a cell is kept as `|`). */
export function tableCells(line: string): string[] | undefined {
  if (!/^\s*\|/.test(line)) return undefined;
  const cells: string[] = [];
  let cur = "";
  let tick = false;
  for (let i = 1; i < line.length; i++) {
    const c = line[i];
    if (c === "\\" && line[i + 1] === "|") {
      cur += "|";
      i++;
      continue;
    }
    if (c === "`") tick = !tick;
    if (c === "|" && !tick) {
      cells.push(cur.trim());
      cur = "";
      continue;
    }
    cur += c;
  }
  if (cur.trim() !== "") cells.push(cur.trim());
  return cells;
}

/** The code spans of a cell: `a`, `b` → ["a", "b"]. */
function codeSpans(cell: string): string[] {
  const out: string[] = [];
  const re = /`([^`]+)`/g;
  for (let m = re.exec(cell); m; m = re.exec(cell)) out.push(m[1]);
  return out;
}

/** `obj.member(args)` occurrences in a code span: `mcu.ui.button({…})` → mcu.ui (object `mcu`, member `ui`) and ui.button. */
function calls(span: string): { object: string; member: string; signature: string; call: boolean }[] {
  const out: { object: string; member: string; signature: string; call: boolean }[] = [];
  const re = /\b([A-Za-z_]\w*)\.([A-Za-z_]\w*)/g;
  for (let m = re.exec(span); m; m = re.exec(span)) {
    const object = m[1];
    const member = m[2];
    let end = m.index + m[0].length;
    let call = false;
    let sig = m[0];
    if (span[end] === "(") {
      // the balanced argument list
      let depth = 0;
      let j = end;
      for (; j < span.length; j++) {
        if (span[j] === "(") depth++;
        else if (span[j] === ")") {
          depth--;
          if (depth === 0) {
            j++;
            break;
          }
        }
      }
      sig = span.slice(m.index, j);
      call = true;
      end = j;
      re.lastIndex = j;
    } else if (span[end] === "." && /^[A-Za-z_]/.test(span[end + 1] ?? "")) {
      // `mcu.ui.button(...)`: `ui` is an object of `mcu`; the next match starts at `ui.button`
      sig = m[0];
      re.lastIndex = m.index + object.length + 1;
    }
    if (!call && FILE_EXT.has(member)) continue;
    out.push({ object, member, signature: sig, call });
  }
  return out;
}

/** A bare call at the start of a cell: `output(pin, opts?)` → name and signature. */
function bareCall(span: string): { member: string; signature: string } | undefined {
  const m = /^([A-Za-z_]\w*)\s*\(/.exec(span);
  if (!m) return undefined;
  let depth = 0;
  let j = m.index + m[0].length - 1;
  for (; j < span.length; j++) {
    if (span[j] === "(") depth++;
    else if (span[j] === ")") {
      depth--;
      if (depth === 0) {
        j++;
        break;
      }
    }
  }
  return { member: m[1], signature: span.slice(0, j) };
}

/** The text of a cell as a one-line doc (markdown kept, whitespace collapsed). */
function docText(cell: string): string {
  return cell.replace(/\s+/g, " ").trim();
}

function addMember(list: Member[], c: { member: string; signature: string; call: boolean }, doc: string) {
  if (!IDENT.test(c.member)) return;
  const existing = list.find((m) => m.name === c.member);
  if (existing) {
    if (c.call && existing.kind === "property") {
      existing.kind = "function";
      existing.signature = c.signature;
    }
    if (doc && !existing.doc?.includes(doc)) existing.doc = [existing.doc, doc].filter(Boolean).join("\n\n");
    return;
  }
  list.push({ name: c.member, signature: c.signature, doc: doc || undefined, kind: c.call ? "function" : "property" });
}

/** The import aliases a README uses for a library: spec → alias (`mcu` → `mcu`, `datascience/array` → `np`). */
export function readmeImports(md: string, lib: string): Map<string, string> {
  const out = new Map<string, string>();
  const re = new RegExp(String.raw`^\s*import\s+([A-Za-z_]\w*)\s+from\s+"(${lib.replace(/[-]/g, "\\-")}(?:/[\w./-]+)?)"`, "gm");
  const counts = new Map<string, Map<string, number>>();
  for (let m = re.exec(md); m; m = re.exec(md)) {
    const spec = m[2].replace(/\.fg$/, "");
    const per = counts.get(spec) ?? new Map<string, number>();
    per.set(m[1], (per.get(m[1]) ?? 0) + 1);
    counts.set(spec, per);
  }
  for (const [spec, per] of counts) {
    const best = [...per.entries()].sort((a, b) => b[1] - a[1])[0];
    out.set(spec, best[0]);
  }
  if (!out.has(lib)) out.set(lib, lib.replace(/[^A-Za-z0-9_]/g, "_"));
  return out;
}

export function parseLib(name: string, md: string, manifest: { version?: string; description?: string }): LibInfo {
  const imports = readmeImports(md, name);
  const aliasToSpec = new Map<string, string>();
  for (const [spec, alias] of imports) aliasToSpec.set(alias, spec);
  const modules = new Map<string, LibModule>();
  for (const [spec, alias] of imports) modules.set(spec, { spec, alias, members: [] });
  const objects: Record<string, Member[]> = {};
  const rows: string[][] = [];
  let inCode = false;
  for (const line of md.split("\n")) {
    if (line.startsWith("```")) {
      inCode = !inCode;
      continue;
    }
    if (inCode) continue;
    const cells = tableCells(line);
    if (!cells || cells.length < 2 || !cells[0].includes("`")) continue;
    if (/^-+$/.test(cells[0].replace(/\s/g, ""))) continue;
    rows.push(cells);
  }
  for (const cells of rows) {
    const doc = docText(cells.slice(1).join(" — "));
    for (const span of codeSpans(cells[0])) {
      for (const c of calls(span)) {
        const spec = aliasToSpec.get(c.object);
        if (spec) addMember(modules.get(spec)!.members, c, doc);
        else {
          if (!IDENT.test(c.object)) continue;
          (objects[c.object] ??= []);
          addMember(objects[c.object], c, doc);
        }
      }
    }
    // handles described in the other cells (`a hardware timer: \`t.cancel()\`, \`t.restart()\``): object tables only
    for (const cell of cells.slice(1)) {
      for (const span of codeSpans(cell)) {
        for (const c of calls(span)) {
          if (aliasToSpec.has(c.object) || !IDENT.test(c.object)) continue;
          (objects[c.object] ??= []);
          addMember(objects[c.object], c, doc);
        }
      }
    }
  }
  // a README whose tables never write `alias.member` (gpio: `output(pin)`) documents the module's own functions bare
  const main = modules.get(name);
  if (main && main.members.length === 0) {
    for (const cells of rows) {
      const first = codeSpans(cells[0])[0];
      const b = first && bareCall(first);
      if (b && IDENT.test(b.member)) addMember(main.members, { member: b.member, signature: `${main.alias}.${b.signature}`, call: true }, docText(cells.slice(1).join(" — ")));
    }
  }
  // objects with a single accidental member (`e.g.`, `fogu.json`) are noise, unless the
  // object is a property of a module (`mcu.ui`, `mcu.wifi`)
  const properties = new Set<string>();
  for (const m of modules.values()) for (const x of m.members) if (x.kind === "property") properties.add(x.name);
  for (const k of Object.keys(objects)) {
    if ((objects[k].length < 2 && !properties.has(k)) || k === "fogu" || k === "e" || k === "i") delete objects[k];
  }
  return {
    name,
    version: manifest.version ?? "",
    summary: manifest.description ?? "",
    modules: [...modules.values()].filter((m) => m.members.length > 0 || m.spec === name),
    objects,
  };
}

export function parseLibs(root: string): LibsData {
  const libs: LibInfo[] = [];
  for (const entry of readdirSync(root).sort()) {
    const dir = join(root, entry);
    const manifestPath = join(dir, "fogu.json");
    const readme = join(dir, "README.md");
    if (!existsSync(manifestPath) || !existsSync(readme)) continue;
    let manifest: { version?: string; description?: string } = {};
    try {
      manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    } catch {
      /* a lib without a readable manifest still gets its README */
    }
    libs.push(parseLib(entry, readFileSync(readme, "utf8"), manifest));
  }
  return { libs };
}

if (import.meta.main) {
  const root = process.argv[2] ?? join(import.meta.dir, "../../../../libs");
  const data = parseLibs(root);
  const outDir = join(import.meta.dir, "../src/data");
  mkdirSync(outDir, { recursive: true });
  writeFileSync(join(outDir, "libs.json"), JSON.stringify(data, null, 1) + "\n");
  for (const l of data.libs) {
    const n = l.modules.reduce((a, m) => a + m.members.length, 0);
    const o = Object.keys(l.objects).length;
    console.log(`${l.name} ${l.version}: ${l.modules.map((m) => `${m.spec} (${m.members.length})`).join(", ")}; ${o} object table(s) — ${n} members`);
  }
}
