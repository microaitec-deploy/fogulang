// Builds src/data/stdlib.json (module names, members, signatures, docs) from the
// generated standard library reference lang/skills/fogu/stdlib.md.
// Run with: bun scripts/gen-stdlib.ts [path/to/stdlib.md]
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export interface Member {
  name: string;
  /** e.g. `fs.readText(path)` */
  signature: string;
  returns?: string;
  doc?: string;
  kind: "function" | "property";
}

export interface ModuleInfo {
  /** `fs` for `fogu:fs` */
  name: string;
  /** default import name used in the docs */
  alias: string;
  summary: string;
  members: Member[];
}

export interface StdlibData {
  globals: Member[];
  fogu: Member[];
  modules: ModuleInfo[];
}

const IDENT = /^[A-Za-z_][A-Za-z0-9_]*$/;

/** `- \`a.b(x) · a.c(y)\` → ret — doc` */
function parseBullet(line: string): { sigs: string[]; returns?: string; doc?: string } | undefined {
  const m = /^- `([^`]+)`(.*)$/.exec(line);
  if (!m) return undefined;
  let rest = m[2].trim();
  let returns: string | undefined;
  let doc: string | undefined;
  if (rest.startsWith("→")) {
    rest = rest.slice(1).trim();
    const i = rest.indexOf(" — ");
    if (i >= 0) {
      returns = rest.slice(0, i).trim();
      doc = rest.slice(i + 3).trim();
    } else {
      returns = rest.trim();
    }
  } else if (rest.startsWith("—")) {
    doc = rest.slice(1).trim();
  }
  if (returns === "—") returns = undefined;
  return { sigs: m[1].split(" · ").map((s) => s.trim()), returns, doc };
}

/** Adds the members of one bullet: `a.f(x) · a.g(y)`, `log.trace/debug(msg)`, `Fogu.signal.on(…) · off(name)`. */
function addBullet(list: Member[], prefix: string | undefined, sigs: string[], returns?: string, doc?: string) {
  let parent: string | undefined; // `Fogu.signal` for `Fogu.signal.on(...)`: continuations belong to it
  for (const raw of sigs) {
    let sig = raw;
    if (prefix && !sig.startsWith(prefix + ".")) {
      if (parent === undefined && /^[A-Za-z_]\w*\s*\(/.test(sig)) sig = `${prefix}.${sig}`; // `· debug(msg)`
      else if (parent !== undefined) {
        addMember(list, prefix, `${parent}.${sig}`, returns, doc);
        continue;
      } else continue;
    }
    const head = prefix ? sig.slice(prefix.length + 1) : sig;
    const nm = /^([A-Za-z_]\w*)\./.exec(head);
    parent = nm ? `${prefix}.${nm[1]}` : undefined;
    // `log.trace/debug/info(message)`
    const slash = /^([A-Za-z_]\w*(?:\/[A-Za-z_]\w*)+)(.*)$/.exec(head);
    if (slash) {
      for (const n of slash[1].split("/")) addMember(list, prefix, `${prefix ? prefix + "." : ""}${n}${slash[2]}`, returns, doc);
      continue;
    }
    addMember(list, prefix, sig, returns, doc);
  }
}

function addMember(list: Member[], prefix: string | undefined, sig: string, returns?: string, doc?: string) {
  if (prefix && !sig.startsWith(prefix + ".")) return;
  const name = prefix ? sig.slice(prefix.length + 1) : sig;
  const m = /^([A-Za-z_][A-Za-z0-9_]*)\s*(\()?/.exec(name);
  if (!m || !IDENT.test(m[1])) return;
  const member = m[1];
  // `Fogu.signal.wait(...)` → member `signal` (an object): keep the first segment
  const nested = name.slice(member.length).startsWith(".");
  const kind: Member["kind"] = m[2] && !nested ? "function" : "property";
  const existing = list.find((x) => x.name === member);
  if (existing) {
    if (!nested && existing.kind === "property" && kind === "function") existing.kind = kind;
    if (nested && !existing.doc?.includes(sig)) {
      const seen = !!doc && !!existing.doc?.includes(doc);
      const entry = `\`${sig}\`${returns ? ` → ${returns}` : ""}${doc && !seen ? ` — ${doc}` : ""}`;
      existing.doc = [existing.doc, entry].filter(Boolean).join(seen ? " · " : "\n\n");
    }
    return;
  }
  list.push({
    name: member,
    signature: nested ? `${prefix ? prefix + "." : ""}${member}` : sig,
    returns: nested ? undefined : returns,
    doc: nested ? `\`${sig}\`${returns ? ` → ${returns}` : ""}${doc ? ` — ${doc}` : ""}` : doc,
    kind: nested ? "property" : kind,
  });
}

export function parseStdlib(md: string): StdlibData {
  const data: StdlibData = { globals: [], fogu: [], modules: [] };
  const sections = md.split(/^## /m).slice(1);
  for (const sec of sections) {
    const lines = sec.split("\n");
    const title = lines[0].trim();
    const body = lines.slice(1);
    if (title === "Global functions") {
      for (const l of body) {
        const b = parseBullet(l);
        if (b) addBullet(data.globals, undefined, b.sigs, b.returns, b.doc);
      }
    } else if (title === "The Fogu object") {
      for (const l of body) {
        const b = parseBullet(l);
        if (b) addBullet(data.fogu, "Fogu", b.sigs, b.returns, b.doc);
      }
    } else if (title.startsWith("fogu:")) {
      const name = title.slice(5).trim();
      const imp = body.map((l) => /^import\s+([A-Za-z_]\w*)\s+from\s+"fogu:/.exec(l)).find(Boolean);
      const named = body.some((l) => /^import\s*\{/.test(l));
      // modules used with named imports only (fogu:test): bare member names
      const alias = imp ? imp[1] : named ? "" : name.replace(/[^A-Za-z0-9_]/g, "_");
      const summary =
        body.find((l) => l.trim() !== "" && !l.startsWith("```") && !l.startsWith("import ") && !l.startsWith("- ") && !l.startsWith("#"))?.trim() ?? "";
      const members: Member[] = [];
      let inCode = false;
      for (const l of body) {
        if (l.startsWith("```")) {
          inCode = !inCode;
          continue;
        }
        if (inCode) continue;
        const b = parseBullet(l);
        if (b) addBullet(members, alias || undefined, b.sigs, b.returns, b.doc);
      }
      data.modules.push({ name, alias, summary, members });
    }
  }
  return data;
}

if (import.meta.main) {
  const src = process.argv[2] ?? join(import.meta.dir, "../../../skills/fogu/stdlib.md");
  const data = parseStdlib(readFileSync(src, "utf8"));
  const outDir = join(import.meta.dir, "../src/data");
  mkdirSync(outDir, { recursive: true });
  writeFileSync(join(outDir, "stdlib.json"), JSON.stringify(data, null, 1) + "\n");
  const n = data.modules.reduce((a, m) => a + m.members.length, 0);
  console.log(`stdlib.json: ${data.globals.length} globals, ${data.fogu.length} Fogu members, ${data.modules.length} modules, ${n} module members`);
}
