// The declarations of a Fogu source file and what it exports, read from its
// text (no vscode imports: unit-tested). This is what gives completion, hover,
// signature help and go-to-definition for the user's own code: functions with
// their typed parameters, structs with their fields and methods, interfaces,
// and the names a file imports from other files (`import { a } from "./x.fg"`).
//
// It is a reader of declarations, not a parser of the language: it looks at
// the statements at the top level of the file (and one level inside structs
// and interfaces), which is where everything that can be exported lives.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import type { Member } from "./stdlib";

export type SymbolKind = "function" | "const" | "let" | "struct" | "interface";

export interface Sym {
  name: string;
  kind: SymbolKind;
  /** What hover shows: `fn add(a: int, b: int): int`, `const PORT: int`, `struct Point` … */
  signature: string;
  /** The parameters of a function, as written (`a: int, b = 2`). */
  params?: string;
  /** The declared return type of a function, or the declared/inferred type of a variable. */
  type?: string;
  /** The `//` comment lines right above the declaration. */
  doc?: string;
  exported: boolean;
  /** 0-based line of the declaration. */
  line: number;
  /** Fields, methods and the constructor of a struct; the requirements of an interface. */
  members?: StructMember[];
  /** `embed Other` of a struct or an interface. */
  embeds?: string[];
  /** The file the symbol is declared in (set for imported symbols). */
  file?: string;
}

export interface StructMember {
  name: string;
  kind: "field" | "method" | "constructor";
  signature: string;
  params?: string;
  type?: string;
  doc?: string;
  line: number;
}

export interface ModuleSymbols {
  /** Every top-level declaration, exported or not. */
  symbols: Sym[];
  /** Exported name → symbol (`export { a as b }` is under `b`). */
  exports: Map<string, Sym>;
  /** `export default …`. */
  default?: Sym;
  /** `export * from "./x.fg"` / `export { a } from "./x.fg"` (followed by [`moduleExports`]). */
  reexports: { spec: string; names?: [string, string][] }[];
}

// ---------------------------------------------------------------- scanning

interface Scan {
  /** The text with the contents of strings and comments replaced by spaces (same offsets). */
  code: string;
  lineStarts: number[];
  /** Brace depth at the start of each line. */
  depth: number[];
}

/** Blanks strings and comments, and records the `{}` depth at each line start. */
export function scan(text: string): Scan {
  const out: string[] = [];
  const lineStarts = [0];
  const depth = [0];
  let d = 0;
  // template strings: a stack of the brace depths their `${` opened at
  const templates: number[] = [];
  let i = 0;
  const n = text.length;
  const blank = (c: string) => (c === "\n" ? "\n" : c === "\r" ? "\r" : " ");
  const newline = (at: number) => {
    lineStarts.push(at + 1);
    depth.push(d);
  };
  const template = () => {
    // inside `…`: until the closing backtick or a `${`
    while (i < n) {
      const c = text[i];
      if (c === "\\") {
        out.push(" ", blank(text[i + 1] ?? " "));
        if (text[i + 1] === "\n") newline(i + 1);
        i += 2;
        continue;
      }
      if (c === "`") {
        out.push(" ");
        i++;
        return;
      }
      if (c === "$" && text[i + 1] === "{") {
        out.push(" ", "{");
        i += 2;
        d++;
        templates.push(d);
        return;
      }
      out.push(blank(c));
      if (c === "\n") newline(i);
      i++;
    }
  };
  while (i < n) {
    const c = text[i];
    if (c === "/" && text[i + 1] === "/") {
      while (i < n && text[i] !== "\n") {
        out.push(" ");
        i++;
      }
      continue;
    }
    if (c === "/" && text[i + 1] === "*") {
      while (i < n && !(text[i] === "*" && text[i + 1] === "/")) {
        out.push(blank(text[i]));
        if (text[i] === "\n") newline(i);
        i++;
      }
      out.push(" ", " ");
      i += 2;
      continue;
    }
    if (c === '"' || c === "'") {
      out.push(c);
      i++;
      while (i < n && text[i] !== c && text[i] !== "\n") {
        if (text[i] === "\\") {
          out.push(" ");
          i++;
        }
        out.push(" ");
        i++;
      }
      if (text[i] === c) {
        out.push(c);
        i++;
      }
      continue;
    }
    if (c === "`") {
      out.push(" ");
      i++;
      template();
      continue;
    }
    if (c === "{") d++;
    else if (c === "}") {
      if (templates.length && templates[templates.length - 1] === d) {
        // the end of a `${ … }`: back inside the template
        templates.pop();
        d--;
        out.push("}");
        i++;
        template();
        continue;
      }
      d = Math.max(0, d - 1);
    }
    out.push(c);
    if (c === "\n") newline(i);
    i++;
  }
  return { code: out.join(""), lineStarts, depth };
}

/** From an opening bracket at `open`: the offset of its closing one (or -1). */
function matching(code: string, open: number): number {
  const pairs: Record<string, string> = { "(": ")", "[": "]", "{": "}", "<": ">" };
  const o = code[open];
  const c = pairs[o];
  let depth = 0;
  const limit = Math.min(code.length, open + 4000);
  for (let i = open; i < limit; i++) {
    const ch = code[i];
    if (ch === o) depth++;
    else if (ch === c) {
      depth--;
      if (depth === 0) return i;
    } else if (o !== "<" && (ch === "(" || ch === "[" || ch === "{") && ch !== o) {
      const j = matching(code, i);
      if (j < 0) return -1;
      i = j;
    }
  }
  return -1;
}

const squeeze = (s: string) => s.replace(/\s+/g, " ").trim();

/** `(params)` and an optional `: type` starting at the `(` at `open`, read from the original text. */
function readSignature(text: string, code: string, open: number): { params: string; type?: string; end: number } | undefined {
  const close = matching(code, open);
  if (close < 0) return undefined;
  const params = squeeze(text.slice(open + 1, close));
  // `: type` up to the body's `{`, `=>` or the end of the line
  const rest = /^\s*:\s*/.exec(code.slice(close + 1));
  if (!rest) return { params, end: close + 1 };
  let i = close + 1 + rest[0].length;
  const start = i;
  let angle = 0;
  let paren = 0;
  while (i < code.length) {
    const ch = code[i];
    if (ch === "\n") break;
    if (ch === "<") angle++;
    else if (ch === ">") {
      if (code[i - 1] === "=") break; // `=>`
      angle = Math.max(0, angle - 1);
    } else if (ch === "(" || ch === "[") paren++;
    else if (ch === ")" || ch === "]") {
      if (paren === 0) break;
      paren--;
    } else if ((ch === "{" || ch === "}") && angle === 0 && paren === 0) break;
    else if (ch === "=" && code[i + 1] === ">") break;
    else if ((ch === "," || ch === ";") && angle === 0 && paren === 0) break;
    i++;
  }
  const type = squeeze(text.slice(start, i));
  return { params, type: type || undefined, end: i };
}

/** The `//` comment block that ends on the line before `line`. */
function docAbove(lines: string[], line: number): string | undefined {
  const out: string[] = [];
  for (let i = line - 1; i >= 0; i--) {
    const m = /^\s*\/\/\s?(.*)$/.exec(lines[i]);
    if (!m) break;
    out.unshift(m[1]);
  }
  const doc = out.join("\n").trim();
  return doc || undefined;
}

// ------------------------------------------------------------- declarations

const IDENT = "[A-Za-z_]\\w*";

/** The type a value expression obviously has (`new Point(…)` → `Point`, a literal → its type). */
function typeOfValue(value: string): string | undefined {
  const v = value.trim();
  const made = new RegExp(`^new\\s+(${IDENT})`).exec(v);
  if (made) return made[1];
  if (/^-?\d+$/.test(v) || /^-?0[xXbBoO][\da-fA-F_]+$/.test(v)) return "int";
  if (/^-?\d+\.\d+/.test(v)) return "float";
  if (/^["'`]/.test(v)) return "str";
  if (/^(true|false)\b/.test(v)) return "bool";
  if (v.startsWith("[")) return "list";
  if (v.startsWith("{")) return "map";
  if (/^\d+(ms|s|m|h)\b/.test(v)) return "duration";
  return undefined;
}

function fnSignature(name: string, params: string, type?: string): string {
  return `fn ${name}(${params})${type ? `: ${type}` : ""}`;
}

/** The members written one level inside the `{ … }` of a struct or an interface. */
function readMembers(text: string, s: Scan, lines: string[], owner: string, firstLine: number, lastLine: number, bodyDepth: number): { members: StructMember[]; embeds: string[] } {
  const members: StructMember[] = [];
  const embeds: string[] = [];
  const one = (raw: string, offset: number, line: number) => {
    const t = raw.trim();
    if (!t || t === "}" || t === "{") return;
    const lead = raw.length - raw.trimStart().length;
    const emb = new RegExp(`^embed\\s+(${IDENT})`).exec(t);
    if (emb) {
      embeds.push(emb[1]);
      return;
    }
    const call = new RegExp(`^(?:fn\\s+)?(${IDENT})\\s*(?:<[^>]*>)?\\s*\\(`).exec(t);
    if (call) {
      const open = offset + lead + call[0].length - 1;
      const sig = readSignature(text, s.code, open);
      if (!sig) return;
      const name = call[1];
      if (name === "new") {
        members.push({ name: "new", kind: "constructor", signature: `new ${owner}(${sig.params})`, params: sig.params, line, doc: docAbove(lines, line) });
      } else {
        members.push({ name, kind: "method", signature: `${name}(${sig.params})${sig.type ? `: ${sig.type}` : ""}`, params: sig.params, type: sig.type, line, doc: docAbove(lines, line) });
      }
      return;
    }
    const field = new RegExp(`^(${IDENT})\\s*(?::\\s*([^=]+?))?\\s*(?:=\\s*(.*))?$`).exec(t.replace(/[;,]\s*$/, ""));
    if (field && !["return", "if", "for", "while", "let", "const", "this"].includes(field[1])) {
      const type = field[2]?.trim() || (field[3] !== undefined ? typeOfValue(text.slice(offset, offset + raw.length).split("=").slice(1).join("=")) : undefined);
      members.push({ name: field[1], kind: "field", signature: `${field[1]}${type ? `: ${type}` : ""}`, type, line, doc: docAbove(lines, line) });
    }
  };
  for (let line = firstLine; line <= lastLine; line++) {
    const start = s.lineStarts[line];
    const end = line + 1 < s.lineStarts.length ? s.lineStarts[line + 1] : s.code.length;
    let raw = s.code.slice(start, end).replace(/\r?\n$/, "");
    let offset = start;
    if (line === firstLine) {
      // what follows the opening brace on the declaration's own line
      const brace = raw.indexOf("{");
      if (brace < 0) continue;
      offset = start + brace + 1;
      raw = raw.slice(brace + 1);
      // a one-line body: `interface A { a(); b() }`
      const close = matching(s.code, start + brace);
      if (close >= 0 && close < end) raw = s.code.slice(offset, close);
      for (const part of splitTop(raw)) one(part.text, offset + part.at, line);
      continue;
    }
    if (s.depth[line] !== bodyDepth) continue;
    for (const part of splitTop(raw)) one(part.text, offset + part.at, line);
  }
  return { members, embeds };
}

/** Splits a line of members at the `;` that are outside brackets. */
function splitTop(raw: string): { text: string; at: number }[] {
  const out: { text: string; at: number }[] = [];
  let depth = 0;
  let from = 0;
  for (let i = 0; i < raw.length; i++) {
    const c = raw[i];
    if (c === "(" || c === "[" || c === "{") depth++;
    else if (c === ")" || c === "]" || c === "}") depth--;
    else if (c === ";" && depth === 0) {
      out.push({ text: raw.slice(from, i), at: from });
      from = i + 1;
    }
  }
  out.push({ text: raw.slice(from), at: from });
  return out;
}

/** The declarations at the top level of a source text. */
export function parseSymbols(text: string): ModuleSymbols {
  const s = scan(text);
  const lines = text.split("\n");
  const symbols: Sym[] = [];
  const exports = new Map<string, Sym>();
  const reexports: ModuleSymbols["reexports"] = [];
  const pending: [string, string][] = []; // `export { local as name }`, resolved at the end
  let def: Sym | undefined;
  let defName: string | undefined;

  const lineEnd = (line: number) => (line + 1 < s.lineStarts.length ? s.lineStarts[line + 1] : s.code.length);
  for (let line = 0; line < s.lineStarts.length; line++) {
    if (s.depth[line] !== 0) continue;
    const start = s.lineStarts[line];
    const codeLine = s.code.slice(start, lineEnd(line)).replace(/\r?\n$/, "");
    const lead = codeLine.length - codeLine.trimStart().length;
    let t = codeLine.trim();
    if (!t) continue;
    let at = start + lead;
    let exported = false;
    let isDefault = false;
    const ex = /^export\s+/.exec(t);
    if (ex) {
      exported = true;
      t = t.slice(ex[0].length);
      at += ex[0].length;
      const d = /^default\s+/.exec(t);
      if (d) {
        isDefault = true;
        t = t.slice(d[0].length);
        at += d[0].length;
      }
      // export { a, b as c } [from "./x.fg"]  ·  export * from "./x.fg"
      if (t.startsWith("{")) {
        const close = matching(s.code, at);
        if (close > 0) {
          const names: [string, string][] = [];
          for (const part of s.code.slice(at + 1, close).split(",")) {
            const m = new RegExp(`^\\s*(${IDENT})(?:\\s+as\\s+(${IDENT}))?\\s*$`).exec(part);
            if (m) names.push([m[1], m[2] ?? m[1]]);
          }
          const from = /^\s*from\s*["']/.exec(s.code.slice(close + 1));
          if (from) {
            const q = close + 1 + from[0].length;
            const spec = text.slice(q, text.indexOf(text[q - 1], q));
            reexports.push({ spec, names });
          } else {
            pending.push(...names);
          }
        }
        continue;
      }
      if (t.startsWith("*")) {
        const from = /^\*\s*(?:as\s+[A-Za-z_]\w*\s+)?from\s*["']/.exec(t);
        if (from) {
          const q = at + from[0].length;
          reexports.push({ spec: text.slice(q, text.indexOf(text[q - 1], q)) });
        }
        continue;
      }
    }
    const add = (sym: Sym) => {
      sym.exported = exported && !isDefault;
      sym.doc = docAbove(lines, line);
      symbols.push(sym);
      if (isDefault) def = sym;
      else if (exported) exports.set(sym.name, sym);
    };

    const kw = /^(fn|struct|interface|let|const)\b\s*/.exec(t);
    if (!kw) {
      if (isDefault) {
        // `export default name` / `export default <expression>`
        const name = new RegExp(`^(${IDENT})\\s*$`).exec(t);
        if (name) defName = name[1];
        else def = { name: "default", kind: "const", signature: "export default", type: typeOfValue(text.slice(at, lineEnd(line))), exported: false, line };
      }
      continue;
    }
    const rest = t.slice(kw[0].length);
    const restAt = at + kw[0].length;
    switch (kw[1]) {
      case "fn": {
        const m = new RegExp(`^(${IDENT})?\\s*(<[^>]*>)?\\s*\\(`).exec(rest);
        if (!m) break;
        const sig = readSignature(text, s.code, restAt + m[0].length - 1);
        if (!sig) break;
        const name = m[1] ?? "default";
        add({ name, kind: "function", signature: fnSignature(name + (m[2] ?? ""), sig.params, sig.type), params: sig.params, type: sig.type, exported, line });
        break;
      }
      case "struct":
      case "interface": {
        const m = new RegExp(`^(${IDENT})?\\s*(<[^>]*>)?`).exec(rest);
        const name = m?.[1] ?? "default";
        // the body: from this line to the line where the depth is back to 0
        let last = line;
        const brace = s.code.indexOf("{", restAt);
        const close = brace >= 0 && brace < lineEnd(line) + 200 ? matching(s.code, brace) : -1;
        if (close >= 0) {
          while (last + 1 < s.lineStarts.length && s.lineStarts[last + 1] <= close) last++;
        }
        const body = readMembers(text, s, lines, name, line, last, 1);
        add({ name, kind: kw[1], signature: `${kw[1]} ${name}${m?.[2] ?? ""}`, members: body.members, embeds: body.embeds, exported, line });
        break;
      }
      default: {
        // let / const: a name, or a destructuring pattern
        const pat = /^(\[[^\]]*\]|\{[^}]*\})\s*=/.exec(rest);
        if (pat) {
          for (const part of pat[1].slice(1, -1).split(",")) {
            const n = new RegExp(`(${IDENT})\\s*$`).exec(part.replace(/=.*$/, ""));
            if (n && n[1] !== "_") add({ name: n[1], kind: kw[1] as SymbolKind, signature: `${kw[1]} ${n[1]}`, exported, line });
          }
          break;
        }
        const m = new RegExp(`^(${IDENT})\\s*(?::\\s*([^=]+?))?\\s*(?:=(?!=)\\s*|$)`).exec(rest);
        if (!m) break;
        const name = m[1];
        const valueAt = restAt + m[0].length;
        const value = s.code.slice(valueAt, lineEnd(line));
        // a function value: `fn(a, b) { … }`, `(a, b) => …`, `x => …`
        const fnv = /^(?:async\s+)?fn\s*(?:<[^>]*>)?\s*\(/.exec(value);
        const arrow = /^\(/.test(value) ? matching(s.code, valueAt) : -1;
        if (fnv) {
          const sig = readSignature(text, s.code, valueAt + fnv[0].length - 1);
          if (sig) {
            add({ name, kind: "function", signature: fnSignature(name, sig.params, sig.type), params: sig.params, type: sig.type, exported, line });
            break;
          }
        } else if (arrow > 0 && /^\s*(?::[^=]+)?=>/.test(s.code.slice(arrow + 1, arrow + 200))) {
          const sig = readSignature(text, s.code, valueAt);
          if (sig) {
            add({ name, kind: "function", signature: fnSignature(name, sig.params, sig.type), params: sig.params, type: sig.type, exported, line });
            break;
          }
        } else {
          const single = new RegExp(`^(${IDENT})\\s*=>`).exec(value);
          if (single) {
            add({ name, kind: "function", signature: fnSignature(name, single[1]), params: single[1], exported, line });
            break;
          }
        }
        const type = m[2]?.trim() || typeOfValue(text.slice(valueAt, lineEnd(line)));
        add({ name, kind: kw[1] as SymbolKind, signature: `${kw[1]} ${name}${type ? `: ${type}` : ""}`, type, exported, line });
      }
    }
  }
  for (const [local, name] of pending) {
    const sym = symbols.find((x) => x.name === local);
    if (!sym) continue;
    if (name === "default") def = sym;
    else exports.set(name, sym);
  }
  if (defName) def = symbols.find((x) => x.name === defName) ?? def;
  return { symbols, exports, default: def, reexports };
}

// ------------------------------------------------------------------ imports

export interface LocalImport {
  /** The path as written: `./util.fg`. */
  spec: string;
  /** `import x from` (the default export, or the module when it has none). */
  default?: string;
  /** `import * as x from` (the module). */
  namespace?: string;
  /** `import { a, b as c }` → [exported name, local name]. */
  names: [string, string][];
}

/** Is `spec` a file path (not `fogu:mod`, not a published library)? */
export function isPathSpec(spec: string): boolean {
  return spec.startsWith("./") || spec.startsWith("../") || spec.startsWith("/") || /^[A-Za-z]:[\\/]/.test(spec);
}

/** The imports of a document that name files. */
export function localImports(text: string): LocalImport[] {
  const out: LocalImport[] = [];
  const re = /\bimport\s+([^"'\n;]*?)\s*from\s+["']([^"'\n]+)["']/g;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    const spec = m[2];
    if (!isPathSpec(spec)) continue;
    const imp: LocalImport = { spec, names: [] };
    let clause = m[1];
    const braces = /\{([^}]*)\}/.exec(clause);
    if (braces) {
      for (const part of braces[1].split(",")) {
        const n = new RegExp(`^\\s*(${IDENT})(?:\\s+as\\s+(${IDENT}))?\\s*$`).exec(part);
        if (n) imp.names.push([n[1], n[2] ?? n[1]]);
      }
      clause = clause.replace(braces[0], "");
    }
    const ns = new RegExp(`\\*\\s*as\\s+(${IDENT})`).exec(clause);
    if (ns) {
      imp.namespace = ns[1];
      clause = clause.replace(ns[0], "");
    }
    const def = new RegExp(`(${IDENT})`).exec(clause);
    if (def) imp.default = def[1];
    out.push(imp);
  }
  return out;
}

/** The file an import names, from the file that imports it (as written, then with `.fg`). */
export function resolveImport(fromFile: string, spec: string): string | undefined {
  const base = isAbsolute(spec) ? spec : resolve(dirname(fromFile), spec);
  for (const p of [base, base + ".fg", join(base, "index.fg")]) {
    try {
      if (statSync(p).isFile()) return p;
    } catch {
      /* not there */
    }
  }
  return undefined;
}

const cache = new Map<string, { mtime: number; mod: ModuleSymbols }>();

/** The symbols of a file on disk (parsed again when it changes). */
export function fileSymbols(path: string): ModuleSymbols | undefined {
  try {
    const mtime = statSync(path).mtimeMs;
    const hit = cache.get(path);
    if (hit && hit.mtime === mtime) return hit.mod;
    const mod = parseSymbols(readFileSync(path, "utf8"));
    for (const sym of mod.symbols) sym.file = path;
    if (mod.default) mod.default.file = path;
    cache.set(path, { mtime, mod });
    return mod;
  } catch {
    return undefined;
  }
}

/** What a file exports, re-exports included (`export * from`, `export { a } from`). */
export function moduleExports(path: string, seen = new Set<string>()): { exports: Map<string, Sym>; default?: Sym } | undefined {
  if (seen.has(path)) return undefined;
  seen.add(path);
  const mod = fileSymbols(path);
  if (!mod) return undefined;
  const exports = new Map(mod.exports);
  for (const re of mod.reexports) {
    if (!isPathSpec(re.spec)) continue;
    const target = resolveImport(path, re.spec);
    const other = target ? moduleExports(target, seen) : undefined;
    if (!other) continue;
    if (re.names) {
      for (const [from, to] of re.names) {
        const sym = from === "default" ? other.default : other.exports.get(from);
        if (sym && !exports.has(to)) exports.set(to, sym);
      }
    } else {
      for (const [name, sym] of other.exports) if (!exports.has(name)) exports.set(name, sym);
    }
  }
  // `seen` guards against import cycles, not against two re-exports of one file
  seen.delete(path);
  return { exports, default: mod.default };
}

/** A module seen as a value (`import * as m`, or a default import of a file without a default export). */
export interface ModuleValue {
  file: string;
  exports: Map<string, Sym>;
}

/** Everything a document can name: its own declarations and what it imports from files. */
export interface Scope {
  /** local name → symbol (own declarations first). */
  names: Map<string, Sym>;
  /** local name → module object. */
  modules: Map<string, ModuleValue>;
}

export function scopeOf(text: string, file?: string): Scope {
  const names = new Map<string, Sym>();
  const modules = new Map<string, ModuleValue>();
  if (file) {
    for (const imp of localImports(text)) {
      const target = resolveImport(file, imp.spec);
      const mod = target ? moduleExports(target) : undefined;
      if (!target || !mod) continue;
      for (const [from, to] of imp.names) {
        const sym = from === "default" ? mod.default : mod.exports.get(from);
        if (sym) names.set(to, sym);
      }
      if (imp.namespace) modules.set(imp.namespace, { file: target, exports: mod.exports });
      if (imp.default) {
        if (mod.default) names.set(imp.default, mod.default);
        else modules.set(imp.default, { file: target, exports: mod.exports });
      }
    }
  }
  for (const sym of parseSymbols(text).symbols) names.set(sym.name, sym);
  return { names, modules };
}

// ----------------------------------------------------------- what a name is

/** The members of a struct or an interface, with those of what it embeds. */
export function membersOfType(type: string, scope: Scope, seen = new Set<string>()): StructMember[] | undefined {
  const name = type.replace(/<.*$/, "").replace(/\?$/, "").trim();
  if (seen.has(name)) return undefined;
  seen.add(name);
  const sym = scope.names.get(name);
  if (!sym || !sym.members) return undefined;
  const out = [...sym.members];
  for (const e of sym.embeds ?? []) {
    for (const m of membersOfType(e, scope, seen) ?? []) {
      if (m.kind !== "constructor" && !out.some((x) => x.name === m.name)) out.push(m);
    }
  }
  return out;
}

/**
 * The declared type of a variable or parameter, looking at the text before
 * `offset`: `let p: Point`, `const p = new Point(…)`, a parameter `p: Point`,
 * `for p in …` is not known. The nearest declaration above wins.
 */
export function typeOfName(text: string, name: string, offset: number): string | undefined {
  const before = text.slice(0, offset);
  const n = name.replace(/[$]/g, "\\$&");
  let found: string | undefined;
  let at = -1;
  const take = (re: RegExp, pick: (m: RegExpExecArray) => string | undefined) => {
    for (let m = re.exec(before); m; m = re.exec(before)) {
      const t = pick(m);
      if (t && m.index > at) {
        at = m.index;
        found = t;
      }
    }
  };
  // let p: Point = …   ·   const p = new Point(…)
  take(new RegExp(`\\b(?:let|const)\\s+${n}\\s*:\\s*([A-Za-z_][\\w<>, ?\\[\\]]*?)\\s*(?:=|$)`, "gm"), (m) => m[1].trim());
  take(new RegExp(`\\b(?:let|const)\\s+${n}\\s*=\\s*new\\s+(${IDENT})`, "g"), (m) => m[1]);
  // a parameter: `(…, p: Point, …)` or a struct field annotation
  take(new RegExp(`[(,]\\s*${n}\\s*:\\s*([A-Za-z_][\\w<>?]*)`, "g"), (m) => m[1]);
  return found;
}

/** The struct whose body contains `offset` (for `this.`). */
export function enclosingStruct(text: string, offset: number): string | undefined {
  const s = scan(text);
  const re = new RegExp(`\\bstruct\\s+(${IDENT})[^{\\n]*\\{`, "g");
  let found: string | undefined;
  for (let m = re.exec(s.code); m; m = re.exec(s.code)) {
    const open = m.index + m[0].length - 1;
    if (open > offset) break;
    const close = matching(s.code, open);
    if (close < 0 || close >= offset) found = m[1];
  }
  return found;
}

// ------------------------------------------------------- shapes for the editor

export function symbolMember(sym: Sym, name = sym.name): Member {
  return {
    name,
    signature: name === sym.name ? sym.signature : sym.signature.replace(sym.name, name),
    doc: sym.doc,
    kind: sym.kind === "function" ? "function" : "property",
  };
}

export function structMember(m: StructMember): Member {
  return { name: m.name, signature: m.signature, doc: m.doc, kind: m.kind === "field" ? "property" : "function" };
}

/**
 * The members after `object.` in the user's code: the exports of a module
 * alias, the fields and methods of a variable whose struct type is declared
 * (or of `this` inside a struct).
 */
export function localMembers(text: string, file: string | undefined, object: string, offset: number, scope = scopeOf(text, file)): Member[] | undefined {
  const mod = scope.modules.get(object);
  if (mod) return [...mod.exports].map(([name, sym]) => symbolMember(sym, name));
  const type = object === "this" ? enclosingStruct(text, offset) : (typeOfName(text, object, offset) ?? scope.names.get(object)?.type);
  if (!type) return undefined;
  return membersOfType(type, scope)
    ?.filter((m) => m.kind !== "constructor")
    .map(structMember);
}

/** Hover text of a symbol: its signature, its members when it is a type, its comment and where it is from. */
export function symbolMarkdown(sym: Sym, scope?: Scope, relFile?: string): string {
  let code = sym.signature;
  const members = sym.members && scope ? (membersOfType(sym.name, scope) ?? sym.members) : sym.members;
  if (members?.length) {
    code += " {\n" + members.map((m) => "    " + m.signature).join("\n") + "\n}";
  }
  let out = "```fogu\n" + code + "\n```";
  if (sym.doc) out += "\n\n" + sym.doc;
  if (relFile) out += `\n\n*from \`${relFile}\`*`;
  return out;
}

/** The `.fg` files and folders that can follow `from "<typed>` (typed: `./`, `../lib/`, `./ut`). */
export function pathCandidates(fromFile: string, typed: string): { label: string; folder: boolean }[] {
  const slash = typed.lastIndexOf("/");
  const dir = resolve(dirname(fromFile), typed.slice(0, slash + 1) || ".");
  const out: { label: string; folder: boolean }[] = [];
  try {
    if (!existsSync(dir)) return out;
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (e.name.startsWith(".") || e.name === "node_modules") continue;
      if (e.isDirectory()) out.push({ label: e.name, folder: true });
      else if (/\.(fg|json|ya?ml|toml|csv|env|txt)$/.test(e.name) && join(dir, e.name) !== fromFile) out.push({ label: e.name, folder: false });
    }
  } catch {
    /* unreadable folder */
  }
  return out;
}

/**
 * The call the cursor is inside of: the name being called (with its object,
 * `obj.method`) and the index of the argument at the cursor.
 */
export function callAt(before: string): { object?: string; name: string; arg: number; isNew: boolean } | undefined {
  const code = scan(before).code;
  let depth = 0;
  let arg = 0;
  for (let i = code.length - 1; i >= 0; i--) {
    const c = code[i];
    if (c === ")" || c === "]" || c === "}") depth++;
    else if (c === "[" || c === "{") {
      if (depth === 0) return undefined;
      depth--;
    } else if (c === "(") {
      if (depth > 0) {
        depth--;
        continue;
      }
      const m = new RegExp(`(?:(\\bnew)\\s+)?(?:(${IDENT})\\s*\\??\\.\\s*)?(${IDENT})\\s*(?:<[^>]*>)?\\s*$`).exec(code.slice(0, i));
      if (!m || ["if", "while", "for", "match", "fn", "return", "in"].includes(m[3])) return undefined;
      return { object: m[2], name: m[3], arg, isNew: !!m[1] };
    } else if (c === "," && depth === 0) arg++;
    else if (c === "\n" && depth === 0 && i < code.length - 2000) return undefined;
  }
  return undefined;
}

/** The parameters of a signature, split at its top-level commas. */
export function splitParams(params: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let from = 0;
  for (let i = 0; i < params.length; i++) {
    const c = params[i];
    if (c === "(" || c === "[" || c === "{" || c === "<") depth++;
    else if (c === ")" || c === "]" || c === "}" || c === ">") depth--;
    else if (c === "," && depth === 0) {
      out.push(params.slice(from, i).trim());
      from = i + 1;
    }
  }
  const last = params.slice(from).trim();
  if (last) out.push(last);
  return out;
}
