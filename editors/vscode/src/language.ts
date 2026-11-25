import * as path from "node:path";
import * as vscode from "vscode";
import {
  completionContext,
  importedModules,
  KEYWORDS,
  libOf,
  libSpecs,
  memberMarkdown,
  membersOf,
  moduleByName,
  stdlib,
  TYPES,
  type Member,
} from "./core/stdlib";
import {
  callAt,
  enclosingStruct,
  isPathSpec,
  localMembers,
  membersOfType,
  moduleExports,
  parseSymbols,
  pathCandidates,
  resolveImport,
  scopeOf,
  splitParams,
  symbolMarkdown,
  typeOfName,
  type Scope,
  type StructMember,
  type Sym,
} from "./core/symbols";

/** The file of a document on disk (imports are resolved from it). */
function fileOf(doc: vscode.TextDocument): string | undefined {
  return doc.uri.scheme === "file" ? doc.uri.fsPath : undefined;
}

/** `./shapes.fg` for a symbol imported from another file. */
function relFile(doc: vscode.TextDocument, sym: Sym): string | undefined {
  const file = fileOf(doc);
  if (!sym.file || !file || sym.file === file) return undefined;
  const rel = path.relative(path.dirname(file), sym.file).split(path.sep).join("/");
  return rel.startsWith(".") ? rel : "./" + rel;
}

/** The completion item of a published library's module (`mcu`, `datascience/array`). */
function libItem(spec: string, alias: string, summary: string, version: string): vscode.CompletionItem {
  const item = new vscode.CompletionItem(spec, vscode.CompletionItemKind.Module);
  item.detail = `library${version ? " " + version : ""} (fogu add ${spec.split("/")[0]})`;
  item.documentation = new vscode.MarkdownString(`${summary}\n\n\`import ${alias} from "${spec}"\``);
  return item;
}

function memberItem(m: Member): vscode.CompletionItem {
  const item = new vscode.CompletionItem(
    m.name,
    m.kind === "function" ? vscode.CompletionItemKind.Function : vscode.CompletionItemKind.Property
  );
  item.detail = m.signature + (m.returns ? ` → ${m.returns}` : "");
  if (m.doc) item.documentation = new vscode.MarkdownString(m.doc);
  if (m.kind === "function") {
    item.insertText = new vscode.SnippetString(`${m.name}($0)`);
    item.command = { title: "", command: "editor.action.triggerParameterHints" };
  }
  return item;
}

const SYMBOL_KIND: Record<Sym["kind"], vscode.CompletionItemKind> = {
  function: vscode.CompletionItemKind.Function,
  const: vscode.CompletionItemKind.Constant,
  let: vscode.CompletionItemKind.Variable,
  struct: vscode.CompletionItemKind.Struct,
  interface: vscode.CompletionItemKind.Interface,
};

/** A declaration of the user's code (of this file, or imported from another one). */
function symbolItem(doc: vscode.TextDocument, name: string, sym: Sym, scope: Scope, call = true): vscode.CompletionItem {
  const item = new vscode.CompletionItem(name, SYMBOL_KIND[sym.kind]);
  const from = relFile(doc, sym);
  item.detail = sym.signature + (from ? `  (${from})` : "");
  item.documentation = new vscode.MarkdownString(symbolMarkdown(sym, scope, from));
  if (call && sym.kind === "function") {
    item.insertText = new vscode.SnippetString(`${name}($0)`);
    item.command = { title: "", command: "editor.action.triggerParameterHints" };
  }
  return item;
}

export class FoguCompletion implements vscode.CompletionItemProvider {
  provideCompletionItems(
    doc: vscode.TextDocument,
    pos: vscode.Position,
    _token: vscode.CancellationToken,
    trigger: vscode.CompletionContext
  ): vscode.CompletionItem[] | undefined {
    const line = doc.lineAt(pos.line).text;
    const before = line.slice(0, pos.character);
    const ctx = completionContext(before, line);
    // trigger characters (. : " { /) only open the list where they mean something
    if (trigger.triggerKind === vscode.CompletionTriggerKind.TriggerCharacter && ctx.kind === "general") return undefined;
    const file = fileOf(doc);
    switch (ctx.kind) {
      case "none":
        return undefined;
      case "module":
        return stdlib.modules.map((m) => {
          const item = new vscode.CompletionItem(m.name, vscode.CompletionItemKind.Module);
          item.detail = `fogu:${m.name}`;
          item.documentation = new vscode.MarkdownString(m.summary + (m.alias ? `\n\n\`import ${m.alias} from "fogu:${m.name}"\`` : ""));
          return item;
        });
      case "library": {
        const items: vscode.CompletionItem[] = [];
        for (const m of libSpecs()) {
          const lib = libOf(m.spec);
          items.push(libItem(m.spec, m.alias, lib?.summary ?? "", lib?.version ?? ""));
        }
        const std = new vscode.CompletionItem("fogu:", vscode.CompletionItemKind.Folder);
        std.detail = "the standard library (fogu:fs, fogu:http, …)";
        std.command = { title: "", command: "editor.action.triggerSuggest" };
        items.push(std);
        if (file) {
          const here = new vscode.CompletionItem("./", vscode.CompletionItemKind.Folder);
          here.detail = "a file of this project";
          here.command = { title: "", command: "editor.action.triggerSuggest" };
          items.push(here);
        }
        return items;
      }
      case "path": {
        if (!file) return undefined;
        // only the last segment is replaced: what follows the last `/`
        const typedName = ctx.typed.slice(ctx.typed.lastIndexOf("/") + 1);
        const range = new vscode.Range(pos.translate(0, -typedName.length), pos);
        return pathCandidates(file, ctx.typed).map((c) => {
          const item = new vscode.CompletionItem(c.label, c.folder ? vscode.CompletionItemKind.Folder : vscode.CompletionItemKind.File);
          item.range = range;
          if (c.folder) {
            item.insertText = c.label + "/";
            item.command = { title: "", command: "editor.action.triggerSuggest" };
          } else if (c.label.endsWith(".fg")) {
            const target = resolveImport(file, ctx.typed.slice(0, ctx.typed.lastIndexOf("/") + 1) + c.label);
            const exp = target ? moduleExports(target) : undefined;
            if (exp) item.detail = [...exp.exports.keys()].slice(0, 8).join(", ") + (exp.exports.size > 8 ? ", …" : "");
          }
          return item;
        });
      }
      case "named-import": {
        const mod = moduleByName(ctx.module);
        return mod?.members.map(memberItem).map((i) => {
          i.insertText = i.label as string;
          i.command = undefined;
          return i;
        });
      }
      case "named-local": {
        const target = file ? resolveImport(file, ctx.spec) : undefined;
        const exp = target ? moduleExports(target) : undefined;
        if (!exp) return undefined;
        const scope: Scope = { names: exp.exports, modules: new Map() };
        return [...exp.exports]
          .filter(([name]) => !ctx.listed.includes(name))
          .map(([name, sym]) => symbolItem(doc, name, sym, scope, false));
      }
      case "member": {
        const std = membersOf(ctx.object, importedModules(doc.getText()));
        if (std) return std.map(memberItem);
        return localMembers(doc.getText(), file, ctx.object, doc.offsetAt(pos))?.map(memberItem);
      }
      case "general": {
        const items: vscode.CompletionItem[] = [];
        const text = doc.getText();
        const scope = scopeOf(text, file);
        for (const k of KEYWORDS) items.push(new vscode.CompletionItem(k, vscode.CompletionItemKind.Keyword));
        for (const g of stdlib.globals) {
          if (!scope.names.has(g.name)) items.push(memberItem(g));
        }
        const fogu = new vscode.CompletionItem("Fogu", vscode.CompletionItemKind.Class);
        fogu.detail = "The Fogu runtime object";
        fogu.documentation = new vscode.MarkdownString(stdlib.fogu.map((m) => "`" + m.signature + "`").join(" · "));
        items.push(fogu);
        for (const t of TYPES) {
          if (KEYWORDS.includes(t) || stdlib.globals.some((g) => g.name === t) || scope.names.has(t)) continue;
          const i = new vscode.CompletionItem(t, vscode.CompletionItemKind.TypeParameter);
          i.detail = "type";
          items.push(i);
        }
        // after `new`, a type annotation or `<`, a name is a type: no call parentheses
        const typePosition = /(\bnew\s+|:\s*|<\s*|\bembed\s+)\w*$/.test(before);
        for (const [name, sym] of scope.names) items.push(symbolItem(doc, name, sym, scope, !typePosition));
        for (const [alias, mod] of scope.modules) {
          const i = new vscode.CompletionItem(alias, vscode.CompletionItemKind.Module);
          i.detail = `module ${path.basename(mod.file)}: ${[...mod.exports.keys()].slice(0, 8).join(", ")}`;
          items.push(i);
        }
        if (enclosingStruct(text, doc.offsetAt(pos))) items.push(new vscode.CompletionItem("this", vscode.CompletionItemKind.Keyword));
        for (const [alias, mod] of importedModules(text)) {
          const i = new vscode.CompletionItem(alias, vscode.CompletionItemKind.Module);
          i.detail = mod.startsWith("lib:") ? `library ${mod.slice(4)}` : `fogu:${mod}`;
          items.push(i);
        }
        return items;
      }
    }
  }
}

/** What the word at a position names in the user's code. */
interface Found {
  sym?: Sym;
  member?: StructMember;
  /** The file the declaration is in (this document when undefined). */
  file?: string;
  line: number;
  markdown: string;
}

function findLocal(doc: vscode.TextDocument, range: vscode.Range, word: string): Found | undefined {
  const text = doc.getText();
  const file = fileOf(doc);
  const scope = scopeOf(text, file);
  const before = doc.lineAt(range.start.line).text.slice(0, range.start.character);
  const obj = /([A-Za-z_]\w*)\s*\??\.\s*$/.exec(before);
  if (obj) {
    // module.name
    const mod = scope.modules.get(obj[1]);
    if (mod) {
      const sym = mod.exports.get(word);
      return sym && { sym, file: sym.file, line: sym.line, markdown: symbolMarkdown(sym, scope, relFile(doc, sym)) };
    }
    // value.member, where the value's struct (or interface) is declared
    const offset = doc.offsetAt(range.start);
    const type = obj[1] === "this" ? enclosingStruct(text, offset) : (typeOfName(text, obj[1], offset) ?? scope.names.get(obj[1])?.type);
    const owner = type ? scope.names.get(type.replace(/<.*$/, "").replace(/\?$/, "")) : undefined;
    const member = type ? membersOfType(type, scope)?.find((m) => m.name === word && m.kind !== "constructor") : undefined;
    if (!owner || !member) return undefined;
    // an embedded member lives in the embedded type's file
    const holder = owner.members?.includes(member) ? owner : (owner.embeds ?? []).map((e) => scope.names.get(e)).find((s) => s?.members?.includes(member));
    const md = "```fogu\n" + `${owner.name}.${member.signature}` + "\n```" + (member.doc ? "\n\n" + member.doc : "");
    return { member, file: (holder ?? owner).file, line: member.line, markdown: md };
  }
  if (/\.\s*$/.test(before)) return undefined;
  const sym = scope.names.get(word);
  if (sym) return { sym, file: sym.file, line: sym.line, markdown: symbolMarkdown(sym, scope, relFile(doc, sym)) };
  const mod = scope.modules.get(word);
  if (mod) {
    const rel = file ? "./" + path.relative(path.dirname(file), mod.file).split(path.sep).join("/") : mod.file;
    const list = [...mod.exports.values()].map((s) => "    " + s.signature).join("\n");
    return { file: mod.file, line: 0, markdown: `**module** \`${rel}\`\n\n` + "```fogu\n" + list + "\n```" };
  }
  return undefined;
}

export class FoguHover implements vscode.HoverProvider {
  provideHover(doc: vscode.TextDocument, pos: vscode.Position): vscode.Hover | undefined {
    const line = doc.lineAt(pos.line).text;
    // "fogu:http" in an import
    const modRange = doc.getWordRangeAtPosition(pos, /["'](?:fogu:[\w-]+|[A-Za-z0-9_-]+(?:\/[\w./-]+)?)["']/);
    if (modRange && /\bfrom\s*$/.test(line.slice(0, modRange.start.character))) {
      const spec = doc.getText(modRange).slice(1, -1);
      const mod = spec.startsWith("fogu:") ? moduleByName(spec.slice(5)) : moduleByName("lib:" + spec);
      if (!mod) return undefined;
      const names = mod.members.map((m) => m.name).join(", ");
      const title = spec.startsWith("fogu:") ? `fogu:${mod.name}` : `${spec} (library, \`fogu add ${spec.split("/")[0]}\`)`;
      return new vscode.Hover(new vscode.MarkdownString(`**${title}** — ${mod.summary}${names ? `\n\nMembers: ${names}` : ""}`), modRange);
    }
    // "./file.fg" in an import: what it exports
    const fileRange = doc.getWordRangeAtPosition(pos, /["'](?:\.{1,2}\/|\/)[^"'\n]*["']/);
    const file = fileOf(doc);
    if (fileRange && file && /\bfrom\s*$/.test(line.slice(0, fileRange.start.character))) {
      const target = resolveImport(file, doc.getText(fileRange).slice(1, -1));
      const exp = target ? moduleExports(target) : undefined;
      if (!exp) return undefined;
      const list = [...exp.exports.values()].map((s) => "    " + s.signature);
      if (exp.default) list.unshift("    export default " + exp.default.signature);
      return new vscode.Hover(new vscode.MarkdownString("**exports**\n\n```fogu\n" + list.join("\n") + "\n```"), fileRange);
    }
    const range = doc.getWordRangeAtPosition(pos, /[A-Za-z_]\w*/);
    if (!range) return undefined;
    const word = doc.getText(range);
    const before = line.slice(0, range.start.character);
    const imports = importedModules(doc.getText());
    const obj = /([A-Za-z_]\w*)\s*\??\.\s*$/.exec(before);
    if (obj) {
      const m = membersOf(obj[1], imports)?.find((x) => x.name === word);
      if (m) return new vscode.Hover(new vscode.MarkdownString(memberMarkdown(m)), range);
      const local = findLocal(doc, range, word);
      return local ? new vscode.Hover(new vscode.MarkdownString(local.markdown), range) : undefined;
    }
    if (/\.\s*$/.test(before)) return undefined;
    const mod = imports.get(word);
    if (mod) {
      const info = moduleByName(mod);
      if (info) {
        const title = mod.startsWith("lib:") ? `${info.name} (library)` : `fogu:${info.name}`;
        return new vscode.Hover(new vscode.MarkdownString(`**${title}** — ${info.summary}`), range);
      }
    }
    if (word === "Fogu") return new vscode.Hover(new vscode.MarkdownString("**Fogu** — runtime helpers available everywhere, without an import."), range);
    const local = findLocal(doc, range, word);
    if (local) return new vscode.Hover(new vscode.MarkdownString(local.markdown), range);
    const g = stdlib.globals.find((x) => x.name === word);
    if (g && /^\s*\(/.test(line.slice(range.end.character))) return new vscode.Hover(new vscode.MarkdownString(memberMarkdown(g)), range);
    return undefined;
  }
}

/** Go to the declaration of a function, struct, member or imported name; an import path opens its file. */
export class FoguDefinition implements vscode.DefinitionProvider {
  provideDefinition(doc: vscode.TextDocument, pos: vscode.Position): vscode.Location | undefined {
    const file = fileOf(doc);
    const fileRange = doc.getWordRangeAtPosition(pos, /["'](?:\.{1,2}\/|\/)[^"'\n]*["']/);
    if (fileRange && file) {
      const spec = doc.getText(fileRange).slice(1, -1);
      const target = isPathSpec(spec) ? resolveImport(file, spec) : undefined;
      if (target) return new vscode.Location(vscode.Uri.file(target), new vscode.Position(0, 0));
    }
    const range = doc.getWordRangeAtPosition(pos, /[A-Za-z_]\w*/);
    if (!range) return undefined;
    const found = findLocal(doc, range, doc.getText(range));
    if (!found) return undefined;
    const uri = found.file ? vscode.Uri.file(found.file) : doc.uri;
    return new vscode.Location(uri, new vscode.Position(found.line, 0));
  }
}

/** The parameters of the call the cursor is in: the user's functions, methods and constructors, and the standard library's. */
export class FoguSignatureHelp implements vscode.SignatureHelpProvider {
  provideSignatureHelp(doc: vscode.TextDocument, pos: vscode.Position): vscode.SignatureHelp | undefined {
    const offset = doc.offsetAt(pos);
    const text = doc.getText();
    const call = callAt(text.slice(Math.max(0, offset - 3000), offset));
    if (!call) return undefined;
    let label: string | undefined;
    let params: string | undefined;
    let docText: string | undefined;
    const paramsOf = (signature: string) => {
      const open = signature.indexOf("(");
      const close = signature.lastIndexOf(")");
      return open >= 0 && close > open ? signature.slice(open + 1, close) : "";
    };
    const scope = scopeOf(text, fileOf(doc));
    if (call.object) {
      const std = membersOf(call.object, importedModules(text))?.find((m) => m.name === call.name);
      const mod = scope.modules.get(call.object)?.exports.get(call.name);
      if (std) {
        label = std.signature + (std.returns ? ` → ${std.returns}` : "");
        params = paramsOf(std.signature);
        docText = std.doc;
      } else if (mod?.params !== undefined) {
        label = mod.signature;
        params = mod.params;
        docText = mod.doc;
      } else {
        const type = call.object === "this" ? enclosingStruct(text, offset) : (typeOfName(text, call.object, offset) ?? scope.names.get(call.object)?.type);
        const m = type ? membersOfType(type, scope)?.find((x) => x.name === call.name && x.kind === "method") : undefined;
        if (m) {
          label = m.signature;
          params = m.params ?? "";
          docText = m.doc;
        }
      }
    } else {
      const sym = scope.names.get(call.name);
      const ctor = sym?.kind === "struct" ? membersOfType(sym.name, scope)?.find((m) => m.kind === "constructor") : undefined;
      if (ctor) {
        label = ctor.signature;
        params = ctor.params ?? "";
        docText = sym?.doc;
      } else if (sym?.params !== undefined) {
        label = sym.signature;
        params = sym.params;
        docText = sym.doc;
      } else if (!sym) {
        const g = stdlib.globals.find((x) => x.name === call.name);
        if (g) {
          label = g.signature + (g.returns ? ` → ${g.returns}` : "");
          params = paramsOf(g.signature);
          docText = g.doc;
        }
      }
    }
    if (label === undefined || params === undefined) return undefined;
    const info = new vscode.SignatureInformation(label, docText ? new vscode.MarkdownString(docText) : undefined);
    const list = splitParams(params);
    // each parameter is found in the label by its position, so equal texts do not collide
    let from = label.indexOf("(") + 1;
    for (const p of list) {
      const at = label.indexOf(p, from);
      if (at < 0) {
        info.parameters.push(new vscode.ParameterInformation(p));
        continue;
      }
      info.parameters.push(new vscode.ParameterInformation([at, at + p.length]));
      from = at + p.length;
    }
    const help = new vscode.SignatureHelp();
    help.signatures = [info];
    help.activeSignature = 0;
    // a `...rest` parameter takes every argument from its position on
    const rest = list.length > 0 && list[list.length - 1].startsWith("...");
    help.activeParameter = rest ? Math.min(call.arg, list.length - 1) : call.arg;
    return help;
  }
}

/** The outline of a file: its functions, variables, structs (with fields and methods) and interfaces. */
export class FoguDocumentSymbols implements vscode.DocumentSymbolProvider {
  provideDocumentSymbols(doc: vscode.TextDocument): vscode.DocumentSymbol[] {
    const kinds: Record<Sym["kind"], vscode.SymbolKind> = {
      function: vscode.SymbolKind.Function,
      const: vscode.SymbolKind.Constant,
      let: vscode.SymbolKind.Variable,
      struct: vscode.SymbolKind.Struct,
      interface: vscode.SymbolKind.Interface,
    };
    const lineRange = (line: number) => doc.lineAt(Math.min(line, doc.lineCount - 1)).range;
    const seen = new Set<Sym>();
    const out: vscode.DocumentSymbol[] = [];
    for (const sym of parseSymbols(doc.getText()).symbols) {
      if (seen.has(sym)) continue;
      seen.add(sym);
      const lastLine = Math.max(sym.line, ...(sym.members ?? []).map((m) => m.line));
      const full = new vscode.Range(lineRange(sym.line).start, lineRange(lastLine).end);
      const item = new vscode.DocumentSymbol(sym.name, sym.signature.replace(/^\w+\s+/, "").replace(sym.name, "").trim(), kinds[sym.kind], full, lineRange(sym.line));
      for (const m of sym.members ?? []) {
        const kind = m.kind === "field" ? vscode.SymbolKind.Field : m.kind === "constructor" ? vscode.SymbolKind.Constructor : vscode.SymbolKind.Method;
        item.children.push(new vscode.DocumentSymbol(m.name, m.signature.replace(m.name, "").trim(), kind, lineRange(m.line), lineRange(m.line)));
      }
      out.push(item);
    }
    return out;
  }
}


