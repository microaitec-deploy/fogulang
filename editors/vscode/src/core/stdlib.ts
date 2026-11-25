// Standard library data and completion context detection (no vscode imports: unit-tested).
import data from "../data/stdlib.json";
import libData from "../data/libs.json";

export interface Member {
  name: string;
  signature: string;
  returns?: string;
  doc?: string;
  kind: "function" | "property";
}
export interface ModuleInfo {
  name: string;
  alias: string;
  summary: string;
  members: Member[];
}
export interface StdlibData {
  globals: Member[];
  fogu: Member[];
  modules: ModuleInfo[];
}

export const stdlib = data as StdlibData;

/** A library published on the Fogu server (libs/<name>: `import mcu from "mcu"`). */
export interface LibModule {
  spec: string;
  alias: string;
  members: Member[];
}
export interface LibInfo {
  name: string;
  version: string;
  summary: string;
  modules: LibModule[];
  objects: Record<string, Member[]>;
}
export interface LibsData {
  libs: LibInfo[];
}

export const libs = libData as unknown as LibsData;

/** The module specs of the libraries: `mcu`, `desktop`, `datascience/array`, … */
export function libSpecs(): LibModule[] {
  return libs.libs.flatMap((l) => l.modules);
}

/** The library a module spec belongs to (`datascience/array` → datascience). */
export function libOf(spec: string): LibInfo | undefined {
  const name = spec.split("/")[0];
  return libs.libs.find((l) => l.name === name);
}

/** A library module by its spec, as a ModuleInfo (name = spec, no `fogu:`). */
export function libModule(spec: string): ModuleInfo | undefined {
  const bare = spec.replace(/\.fg$/, "");
  for (const l of libs.libs) {
    const m = l.modules.find((x) => x.spec === bare);
    if (m) return { name: m.spec, alias: m.alias, summary: l.summary, members: m.members };
  }
  return undefined;
}

export const KEYWORDS = [
  "let", "const", "fn", "return", "if", "else", "while", "for", "in", "loop", "break", "continue",
  "import", "export", "from", "as", "struct", "interface", "embed", "new", "this", "match", "case",
  "default", "fg", "chan", "select", "true", "false", "nil",
];

export const TYPES = [
  "int", "float", "str", "char", "bool", "void", "nil", "duration", "list", "map", "tuple", "range", "chan", "error", "fn", "any",
  "buffer", "struct", "object",
  // sized numbers: `list<u8>` … `list<f64>` and `list<char>` also store their elements with that size
  "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
  "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "float32", "float64", "byte", "rune",
];

/** A standard library module (`fs`), or a library module (`lib:mcu`, `lib:datascience/array`). */
export function moduleByName(name: string): ModuleInfo | undefined {
  if (name.startsWith("lib:")) return libModule(name.slice(4));
  return stdlib.modules.find((m) => m.name === name);
}

/**
 * alias → module name, from the import lines of a document: `fogu:mod` gives
 * `mod`; a bare specifier of a published library (`"mcu"`, `"desktop/update.fg"`)
 * gives `lib:<spec>`. Relative and absolute paths are not modules.
 */
export function importedModules(text: string): Map<string, string> {
  const map = new Map<string, string>();
  const re = /\bimport\s+(?:\*\s+as\s+)?([A-Za-z_]\w*)\s+from\s+["']([^"'\n]+)["']/g;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    const spec = m[2];
    if (spec.startsWith("fogu:")) {
      map.set(m[1], spec.slice(5));
      continue;
    }
    if (/^[A-Za-z0-9_-]+(\/[\w./-]+)?$/.test(spec) && libModule(spec)) map.set(m[1], "lib:" + spec.replace(/\.fg$/, ""));
  }
  return map;
}

export type CompletionContext =
  | { kind: "module"; prefix: string }
  | { kind: "library"; prefix: string }
  | { kind: "member"; object: string; prefix: string }
  | { kind: "named-import"; module: string; prefix: string }
  | { kind: "named-local"; spec: string; prefix: string; listed: string[] }
  | { kind: "path"; typed: string }
  | { kind: "none" }
  | { kind: "general"; prefix: string };

/**
 * What to complete at the end of `before` (the text of the line before the
 * cursor); `line` is the whole line (for `import { | } from "fogu:x"`).
 */
export function completionContext(before: string, line: string): CompletionContext {
  const mod = /["']fogu:([\w-]*)$/.exec(before);
  if (mod) return { kind: "module", prefix: mod[1] };
  // `import x from "mc` → the libraries (and `fogu:` itself)
  const bare = /\bimport\b[^"'\n]*\bfrom\s+["']([\w./-]*)$/.exec(before);
  if (bare && (bare[1].startsWith(".") || bare[1].startsWith("/"))) return { kind: "path", typed: bare[1] };
  if (bare) return { kind: "library", prefix: bare[1] };
  const named = /^\s*import\s*\{[^}]*?(\w*)$/.exec(before);
  if (named) {
    const from = /from\s+["']fogu:([\w-]+)["']/.exec(line);
    if (from) return { kind: "named-import", module: from[1], prefix: named[1] };
    // `import { a, | } from "./file.fg"`: the names that file exports
    const file = /from\s+["'](\.{0,2}\/[^"'\n]+)["']/.exec(line);
    if (file) {
      // every name between the braces, before and after the cursor, except the one being typed
      const all = (/\{([^}]*)\}?/.exec(line)?.[1] ?? "").split(",").map((x) => x.trim().split(/\s+/)[0]).filter(Boolean);
      const typing = all.indexOf(named[1]);
      const listed = named[1] && typing >= 0 ? all.filter((_, k) => k !== typing) : all;
      return { kind: "named-local", spec: file[1], prefix: named[1], listed };
    }
    return { kind: "none" };
  }
  // inside a string or a comment: nothing
  if (insideStringOrComment(before)) return { kind: "none" };
  const mem = /([A-Za-z_]\w*)\s*\??\.\s*(\w*)$/.exec(before);
  if (mem) return { kind: "member", object: mem[1], prefix: mem[2] };
  const word = /(\w*)$/.exec(before);
  return { kind: "general", prefix: word ? word[1] : "" };
}

/** Rough check on one line: inside "…", '…', `…` (outside ${…}) or after //. */
export function insideStringOrComment(before: string): boolean {
  let quote: string | null = null;
  let depth = 0; // ${ … } nesting inside a template
  for (let i = 0; i < before.length; i++) {
    const c = before[i];
    if (quote) {
      if (c === "\\") {
        i++;
        continue;
      }
      if (quote === "`" && c === "$" && before[i + 1] === "{") {
        quote = null;
        depth++;
        i++;
        continue;
      }
      if (c === quote) quote = null;
      continue;
    }
    if (c === "/" && before[i + 1] === "/") return true;
    if (c === '"' || c === "'" || c === "`") quote = c;
    else if (c === "}" && depth > 0) {
      depth--;
      quote = "`";
    }
  }
  return quote !== null;
}

/**
 * Members for `object.` given the document's imports (`Fogu.` included). An
 * object that is not an import may be one a library's README documents
 * (`pin.`, `card.`, `w.` …): its table is used when that library is imported.
 */
export function membersOf(object: string, imports: Map<string, string>): Member[] | undefined {
  if (object === "Fogu") return stdlib.fogu;
  const mod = imports.get(object);
  if (mod) return moduleByName(mod)?.members;
  for (const spec of imports.values()) {
    if (!spec.startsWith("lib:")) continue;
    const table = libOf(spec.slice(4))?.objects[object];
    if (table) return table;
  }
  return undefined;
}

export function memberMarkdown(m: Member): string {
  const sig = "```fogu\n" + m.signature + (m.returns ? ` → ${m.returns}` : "") + "\n```";
  return m.doc ? `${sig}\n\n${m.doc}` : sig;
}
