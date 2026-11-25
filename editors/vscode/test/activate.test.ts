// Smoke test of the bundled extension (dist/extension.js) against a minimal mock of the
// vscode API: activation, then the providers it registers on a real document.
import { beforeAll, describe, expect, mock, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { FOGU } from "./fogubin";

const registered: Record<string, any> = {};
const commands = new Map<string, (...a: any[]) => any>();
const diagnostics = new Map<string, any[]>();
const executedTasks: any[] = [];
const disposable = { dispose() {} };
const event = () => disposable;

class Position {
  constructor(public line: number, public character: number) {}
  translate(dl: number, dc: number) {
    return new Position(this.line + dl, this.character + dc);
  }
}
class Range {
  start: Position;
  end: Position;
  constructor(a: any, b: any, c?: any, d?: any) {
    if (typeof a === "number") {
      this.start = new Position(a, b);
      this.end = new Position(c, d);
    } else {
      this.start = a;
      this.end = b;
    }
  }
}
class Uri {
  constructor(public scheme: string, public fsPath: string) {}
  static file(p: string) {
    return new Uri("file", p);
  }
  static parse(s: string) {
    return new Uri("https", s);
  }
  toString() {
    return `${this.scheme}://${this.fsPath}`;
  }
}

function makeDoc(file: string, text: string) {
  const lines = text.split("\n");
  const offsetAt = (p: Position) => lines.slice(0, p.line).reduce((a, l) => a + l.length + 1, 0) + p.character;
  return {
    uri: Uri.file(file),
    fileName: file,
    languageId: "fogu",
    version: 1,
    isDirty: false,
    isClosed: false,
    isUntitled: false,
    lineCount: lines.length,
    getText: (r?: Range) => (r ? text.slice(offsetAt(r.start), offsetAt(r.end)) : text),
    lineAt: (n: number) => ({ text: lines[n], range: new Range(n, 0, n, lines[n].length) }),
    offsetAt,
    positionAt: (off: number) => {
      let line = 0;
      while (line < lines.length - 1 && off > lines[line].length) {
        off -= lines[line].length + 1;
        line++;
      }
      return new Position(line, off);
    },
    getWordRangeAtPosition: (p: Position, re: RegExp) => {
      const g = new RegExp(re.source, "g");
      for (let m = g.exec(lines[p.line]); m; m = g.exec(lines[p.line])) {
        if (m.index <= p.character && p.character <= m.index + m[0].length) return new Range(p.line, m.index, p.line, m.index + m[0].length);
      }
      return undefined;
    },
    save: async () => true,
  };
}

const vscodeMock = {
  Position,
  Range,
  Uri,
  EventEmitter: class {
    event = event;
    fire() {}
  },
  MarkdownString: class {
    constructor(public value: string) {}
  },
  SnippetString: class {
    constructor(public value: string) {}
  },
  CompletionItem: class {
    constructor(public label: string, public kind?: number) {}
  },
  CompletionItemKind: { Function: 2, Property: 9, Module: 8, Keyword: 13, Class: 6, TypeParameter: 24, Constant: 20, Variable: 5, Struct: 21, Interface: 7, Folder: 18, File: 16 },
  SymbolKind: { Function: 11, Constant: 13, Variable: 12, Struct: 22, Interface: 10, Field: 7, Constructor: 8, Method: 5 },
  DocumentSymbol: class {
    children: any[] = [];
    constructor(public name: string, public detail: string, public kind: number, public range: Range, public selectionRange: Range) {}
  },
  Location: class {
    constructor(public uri: Uri, public range: Position) {}
  },
  SignatureHelp: class {
    signatures: any[] = [];
    activeSignature = 0;
    activeParameter = 0;
  },
  SignatureInformation: class {
    parameters: any[] = [];
    constructor(public label: string, public documentation?: any) {}
  },
  ParameterInformation: class {
    constructor(public label: any) {}
  },
  CompletionTriggerKind: { Invoke: 0, TriggerCharacter: 1 },
  CodeLens: class {
    constructor(public range: Range, public command: any) {}
  },
  Hover: class {
    constructor(public contents: any, public range?: Range) {}
  },
  TextEdit: { replace: (range: Range, newText: string) => ({ range, newText }) },
  Diagnostic: class {
    source?: string;
    code?: string;
    tags?: number[];
    constructor(public range: Range, public message: string, public severity: number) {}
  },
  DiagnosticSeverity: { Error: 0, Warning: 1 },
  DiagnosticTag: { Unnecessary: 1 },
  ShellExecution: class {
    constructor(public command: string, public args: string[], public options?: any) {}
  },
  Task: class {
    presentationOptions: any;
    group: any;
    constructor(public definition: any, public scope: any, public name: string, public source: string, public execution: any, public problemMatchers: string[]) {}
  },
  TaskScope: { Global: 1, Workspace: 2 },
  TaskRevealKind: { Always: 1 },
  TaskPanelKind: { Shared: 1, Dedicated: 2 },
  TaskGroup: { Build: "build", Test: "test" },
  ProgressLocation: { Window: 10 },
  env: { remoteName: undefined, openExternal: () => undefined },
  window: {
    createOutputChannel: () => ({ appendLine() {}, show() {}, dispose() {} }),
    showWarningMessage: async () => undefined,
    setStatusBarMessage: () => disposable,
    activeTextEditor: undefined as any,
    onDidChangeActiveTextEditor: event,
    withProgress: (_o: any, f: any) => f(),
    showQuickPick: async () => undefined,
    showInputBox: async () => undefined,
  },
  workspace: {
    isTrusted: true,
    workspaceFolders: [] as any[],
    textDocuments: [] as any[],
    getConfiguration: () => ({ get: (k: string, d: any) => (k === "path" ? FOGU : d) }),
    getWorkspaceFolder: () => undefined,
    asRelativePath: (u: any) => (typeof u === "string" ? u : u.fsPath),
    onDidOpenTextDocument: event,
    onDidSaveTextDocument: event,
    onDidChangeTextDocument: event,
    onDidCloseTextDocument: event,
    onDidGrantWorkspaceTrust: event,
    onDidChangeConfiguration: event,
    onDidChangeWorkspaceFolders: event,
    createFileSystemWatcher: () => ({ onDidChange: event, onDidCreate: event, onDidDelete: event, dispose() {} }),
  },
  languages: {
    createDiagnosticCollection: () => ({
      set: (uri: Uri, d: any[]) => diagnostics.set(uri.toString(), d),
      delete: () => undefined,
      dispose() {},
    }),
    registerDocumentFormattingEditProvider: (_s: any, p: any) => ((registered.format = p), disposable),
    registerCompletionItemProvider: (_s: any, p: any, ...chars: string[]) => ((registered.completion = p), (registered.triggers = chars), disposable),
    registerHoverProvider: (_s: any, p: any) => ((registered.hover = p), disposable),
    registerCodeLensProvider: (_s: any, p: any) => ((registered.lens = p), disposable),
    registerDefinitionProvider: (_s: any, p: any) => ((registered.definition = p), disposable),
    registerSignatureHelpProvider: (_s: any, p: any) => ((registered.signature = p), disposable),
    registerDocumentSymbolProvider: (_s: any, p: any) => ((registered.symbols = p), disposable),
  },
  tasks: {
    registerTaskProvider: (_t: string, p: any) => ((registered.tasks = p), disposable),
    executeTask: async (t: any) => (executedTasks.push(t), {}),
  },
  commands: {
    registerCommand: (id: string, f: any) => (commands.set(id, f), disposable),
    executeCommand: async () => undefined,
  },
};

mock.module("vscode", () => vscodeMock);

const src = join(import.meta.dir, "fixtures/check/undefined.fg");
let ext: any;

beforeAll(async () => {
  ext = await import(join(import.meta.dir, "../dist/extension.js"));
  const ctx = { subscriptions: [] as any[], extension: { packageJSON: { version: "test" } }, workspaceState: { get: () => undefined, update: async () => undefined } };
  ext.activate(ctx);
});

describe("bundled extension", () => {
  test("activates and registers commands and providers", () => {
    const pkg = JSON.parse(readFileSync(join(import.meta.dir, "../package.json"), "utf8"));
    for (const c of pkg.contributes.commands) expect(commands.has(c.command)).toBe(true);
    for (const k of ["format", "completion", "hover", "lens", "tasks", "definition", "signature", "symbols"]) expect(registered[k]).toBeDefined();
  });

  test("diagnostics for a document", async () => {
    // reach the Checker through the "fogu.check" command on the active editor
    const doc = makeDoc(src, readFileSync(src, "utf8"));
    vscodeMock.window.activeTextEditor = { document: doc };
    await commands.get("fogu.check")!();
    const d = diagnostics.get(doc.uri.toString())!;
    expect(d).toHaveLength(1);
    expect(d[0].message).toContain("variable 'y' is not defined");
    expect([d[0].range.start.line, d[0].range.start.character, d[0].range.end.character]).toEqual([2, 9, 10]);
    expect(d[0].code).toBe("E0100");
  });

  test("diagnostics for unsaved text go through a temporary file", async () => {
    const doc = { ...makeDoc("/does/not/exist/new.fg", "let unused_var = 1\nprint(nope)\n"), isDirty: true };
    vscodeMock.window.activeTextEditor = { document: doc };
    await commands.get("fogu.check")!();
    const d = diagnostics.get(doc.uri.toString())!;
    expect(d.map((x: any) => x.message.split("\n")[0])).toEqual([
      "unused variable 'unused_var' (prefix it with '_' to silence this)",
      "variable 'nope' is not defined",
    ]);
  });

  test("formatting", async () => {
    const doc = makeDoc("/tmp/x.fg", "fn   f(a,b){return a+b}\n");
    const edits = await registered.format.provideDocumentFormattingEdits(doc);
    expect(edits).toHaveLength(1);
    expect(edits[0].newText).toBe("fn f(a, b) { return a + b }\n");
    expect(await registered.format.provideDocumentFormattingEdits(makeDoc("/tmp/y.fg", "const x = 1\n"))).toEqual([]);
  });

  test("completion: modules, members, keywords", () => {
    const text = 'import fs from "fogu:fs"\nconst t = fs.re\nimport h from "fogu:\n';
    const doc = makeDoc("/tmp/c.fg", text);
    const invoke = { triggerKind: 0 };
    const members = registered.completion.provideCompletionItems(doc, new Position(1, 15), {}, invoke);
    expect(members.map((i: any) => i.label)).toContain("readText");
    const mods = registered.completion.provideCompletionItems(doc, new Position(2, 20), {}, { triggerKind: 1, triggerCharacter: ":" });
    expect(mods.map((i: any) => i.label)).toContain("http");
    const general = registered.completion.provideCompletionItems(doc, new Position(1, 3), {}, invoke);
    expect(general.map((i: any) => i.label)).toEqual(expect.arrayContaining(["const", "print", "Fogu", "fs"]));
    // a ':' typed in ordinary code does not pop the list
    expect(registered.completion.provideCompletionItems(doc, new Position(1, 3), {}, { triggerKind: 1, triggerCharacter: ":" })).toBeUndefined();
  });

  test("hover on a module member and on a module specifier", () => {
    const doc = makeDoc("/tmp/h.fg", 'import fs from "fogu:fs"\nconst t = fs.readText("a")\n');
    const h = registered.hover.provideHover(doc, new Position(1, 16));
    expect(h.contents.value).toContain("fs.readText(path)");
    const m = registered.hover.provideHover(doc, new Position(0, 20));
    expect(m.contents.value).toContain("fogu:fs");
  });

  test("the user's code: imports from files with their types, members, hints, definitions, outline", () => {
    const file = join(import.meta.dir, "fixtures/project/main.fg");
    const shapes = join(import.meta.dir, "fixtures/project/shapes.fg");
    const text = readFileSync(file, "utf8") + 'import { , circle } from "./shapes.fg"\nimport z from "./\nconst d = origin.\nprint(circle(1, 2), shapes.parse("1", ), new Point(1, ), origin.move(1, ))\n';
    const doc = makeDoc(file, text);
    const lines = text.split("\n");
    const at = (needle: string, delta = 0) => {
      const line = lines.findIndex((l) => l.includes(needle));
      return new Position(line, lines[line].indexOf(needle) + delta);
    };
    const invoke = { triggerKind: 0 };
    const labels = (pos: Position, trigger: any = invoke) => registered.completion.provideCompletionItems(doc, pos, {}, trigger)?.map((i: any) => i.label);
    // import { | } from "./shapes.fg": its exports, without the ones already listed
    const named = registered.completion.provideCompletionItems(doc, at("{ , circle }", 2), {}, invoke);
    expect(named.map((i: any) => i.label).sort()).toEqual(["PI", "Pixel", "Point", "Shape", "add", "counter", "parse"]);
    expect(named.find((i: any) => i.label === "parse").detail).toContain("fn parse(text: str, strict = false): (int, error)");
    // from "./ : the files of the folder
    expect(labels(at('import z from "./', 17), { triggerKind: 1, triggerCharacter: "/" }).sort()).toEqual(["index.fg", "shapes.fg"]);
    // members of a value whose struct is known, of a module alias
    expect(labels(at("const d = origin.", 17), { triggerKind: 1, triggerCharacter: "." })).toEqual(["x", "y", "label", "norm", "move"]);
    // general: own and imported declarations
    const general = registered.completion.provideCompletionItems(doc, at("const origin", 2), {}, invoke);
    const byLabel = (l: string) => general.find((i: any) => i.label === l);
    expect(byLabel("show").detail).toBe("fn show(s: Shape, p: Point): str");
    expect(byLabel("parseInt").detail).toContain("(./shapes.fg)");
    expect(byLabel("Point").kind).toBe(21);
    expect(byLabel("shapes").detail).toContain("module shapes.fg");
    expect(byLabel("u8").detail).toBe("type");
    // hover: an imported function, a method, an import path
    expect(registered.hover.provideHover(doc, at("circle, parse as", 1)).contents.value).toContain("fn circle(r: float): float");
    expect(registered.hover.provideHover(doc, at("p.norm()", 3)).contents.value).toContain("Point.norm(): float");
    expect(registered.hover.provideHover(doc, at('"./index.fg"', 3)).contents.value).toContain("const VERSION: str");
    // parameter hints
    const sig = (needle: string, delta: number) => registered.signature.provideSignatureHelp(doc, at(needle, delta));
    expect(sig("circle(1, 2)", 10).signatures[0].label).toBe("fn circle(r: float): float");
    expect(sig("circle(1, 2)", 10).activeParameter).toBe(1);
    const parse = sig('shapes.parse("1", )', 18);
    expect(parse.signatures[0].label).toBe("fn parse(text: str, strict = false): (int, error)");
    expect(parse.activeParameter).toBe(1);
    expect(parse.signatures[0].parameters.map((x: any) => x.label)).toEqual([[9, 18], [20, 34]]);
    expect(sig("new Point(1, )", 13).signatures[0].label).toBe("new Point(x: int, y: int = 0)");
    expect(sig("origin.move(1, )", 15).signatures[0].label).toBe("move(dx: int, dy: int)");
    expect(registered.signature.provideSignatureHelp(makeDoc("/tmp/s.fg", "print(len("), new Position(0, 10)).signatures[0].label).toContain("len(");
    // definitions: an imported name, a member, a local function, an import path
    const def = (needle: string, delta: number) => registered.definition.provideDefinition(doc, at(needle, delta));
    expect(def("circle, parse as", 1).uri.fsPath).toBe(shapes);
    expect(def("circle, parse as", 1).range.line).toBe(5);
    expect(def("p.norm()", 3).uri.fsPath).toBe(shapes);
    expect(def("show(s: Shape", 1).uri.fsPath).toBe(file);
    expect(def('"./index.fg"', 3).uri.fsPath).toBe(join(import.meta.dir, "fixtures/project/index.fg"));
    // outline
    const outline = registered.symbols.provideDocumentSymbols(makeDoc(shapes, readFileSync(shapes, "utf8")));
    expect(outline.map((x: any) => x.name)).toEqual(["PI", "counter", "circle", "parse", "helper", "_private", "Point", "Pixel", "Shape", "Mini"]);
    expect(outline.find((x: any) => x.name === "Point").children.map((c: any) => c.name)).toEqual(["x", "y", "label", "new", "norm", "move"]);
  });

  test("code lenses and run command", async () => {
    const doc = makeDoc("/tmp/app.fg", "print(1)\n");
    const lenses = registered.lens.provideCodeLenses(doc);
    expect(lenses.map((l: any) => l.command.command)).toEqual(["fogu.run", "fogu.check"]);
    const t = makeDoc("/tmp/a.test.fg", 'import { test } from "fogu:test"\ntest("adds", () => {})\n');
    const tl = registered.lens.provideCodeLenses(t);
    expect(tl.map((l: any) => l.command.arguments?.[1])).toEqual([undefined, "adds"]);
    await commands.get("fogu.run")!(Uri.file("/tmp/app.fg"));
    const task = executedTasks.pop();
    expect(task.execution.command).toBe(FOGU);
    expect(task.execution.args).toEqual(["/tmp/app.fg"]);
    expect(task.problemMatchers).toEqual(["$fogu", "$fogu-warning"]);
  });
});
