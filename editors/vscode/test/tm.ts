// Headless TextMate tokenizer used by the grammar tests (vscode-textmate + vscode-oniguruma).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import * as vsctm from "vscode-textmate";
import * as oniguruma from "vscode-oniguruma";

const root = join(import.meta.dir, "..");
const grammarFiles: Record<string, string> = {
  "source.fogu": join(root, "syntaxes/fogu.tmLanguage.json"),
  "markdown.fogu.codeblock": join(root, "syntaxes/fogu.markdown.tmLanguage.json"),
};

let registry: vsctm.Registry | undefined;

async function getRegistry(): Promise<vsctm.Registry> {
  if (registry) return registry;
  const wasm = readFileSync(join(root, "node_modules/vscode-oniguruma/release/onig.wasm"));
  await oniguruma.loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));
  registry = new vsctm.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: (patterns: string[]) => new oniguruma.OnigScanner(patterns),
      createOnigString: (s: string) => new oniguruma.OnigString(s),
    }),
    loadGrammar: async (scopeName) => {
      if (scopeName === "text.html.markdown") {
        // stub host grammar so the fenced-code injection can be tested without the real Markdown grammar
        return vsctm.parseRawGrammar(JSON.stringify({ scopeName, patterns: [] }), "markdown.json");
      }
      const file = grammarFiles[scopeName];
      if (!file) return null;
      return vsctm.parseRawGrammar(readFileSync(file, "utf8"), file);
    },
    getInjections: (scopeName) => (scopeName === "text.html.markdown" ? ["markdown.fogu.codeblock"] : undefined),
  });
  return registry;
}

export interface Tok {
  text: string;
  scopes: string[];
  line: number;
}

export async function tokenize(src: string, scope = "source.fogu"): Promise<Tok[]> {
  const reg = await getRegistry();
  const grammar = await reg.loadGrammar(scope);
  if (!grammar) throw new Error(`grammar ${scope} not found`);
  const out: Tok[] = [];
  let state = vsctm.INITIAL;
  src.split("\n").forEach((line, i) => {
    const r = grammar.tokenizeLine(line, state);
    for (const t of r.tokens) {
      out.push({ text: line.slice(t.startIndex, t.endIndex), scopes: t.scopes, line: i });
    }
    state = r.ruleStack;
  });
  return out;
}

/** Scopes of the first token whose text is exactly `text` (optionally the n-th occurrence). */
export function scopesOf(toks: Tok[], text: string, nth = 0): string[] {
  const hits = toks.filter((t) => t.text === text);
  if (!hits[nth]) throw new Error(`token ${JSON.stringify(text)} #${nth} not found in: ${toks.map((t) => t.text).join("|")}`);
  return hits[nth].scopes;
}

export function dump(toks: Tok[]): string {
  return toks
    .filter((t) => t.text.trim() !== "")
    .map((t) => `${t.line}: ${JSON.stringify(t.text).padEnd(24)} ${t.scopes.slice(1).join(" ")}`)
    .join("\n");
}

if (import.meta.main) {
  const file = process.argv[2];
  const scope = process.argv[3] ?? "source.fogu";
  console.log(dump(await tokenize(readFileSync(file, "utf8"), scope)));
}
