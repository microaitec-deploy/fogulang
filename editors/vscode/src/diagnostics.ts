import * as vscode from "vscode";
import { createHash } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";
import { foguPath, output, warnMissing } from "./config";
import { parseCheckOutput } from "./core/diagparse";
import { checkWithFogu } from "./core/proc";

const tempRoot = join(tmpdir(), `fogu-vscode-${process.pid}`);

/** Runs `fogu check` on open/save/type and publishes the diagnostics. */
export class Checker implements vscode.Disposable {
  readonly collection = vscode.languages.createDiagnosticCollection("fogu");
  private timers = new Map<string, NodeJS.Timeout>();
  private generation = new Map<string, number>();
  private subs: vscode.Disposable[] = [];

  constructor() {
    this.subs.push(
      vscode.workspace.onDidOpenTextDocument((d) => this.onEvent(d, "open")),
      vscode.workspace.onDidSaveTextDocument((d) => this.onEvent(d, "save")),
      vscode.workspace.onDidChangeTextDocument((e) => {
        if (e.contentChanges.length) this.onEvent(e.document, "type");
      }),
      vscode.workspace.onDidCloseTextDocument((d) => {
        this.cancel(d);
        this.collection.delete(d.uri);
      }),
      vscode.workspace.onDidGrantWorkspaceTrust(() => this.checkOpen())
    );
    this.checkOpen();
  }

  private checkOpen(): void {
    for (const d of vscode.workspace.textDocuments) this.onEvent(d, "open");
  }

  private onEvent(doc: vscode.TextDocument, why: "open" | "save" | "type"): void {
    if (doc.languageId !== "fogu" || !vscode.workspace.isTrusted) return;
    if (doc.uri.scheme !== "file" && doc.uri.scheme !== "untitled") return;
    const cfg = vscode.workspace.getConfiguration("fogu", doc.uri);
    const onSave = cfg.get<boolean>("checkOnSave", true);
    const onType = cfg.get<boolean>("checkOnType", true);
    if (why === "type" && !onType) return;
    if (why !== "type" && !onSave && !onType) return;
    const delay = why === "type" ? Math.max(50, cfg.get<number>("checkDelay", 400)) : 0;
    this.schedule(doc, delay);
  }

  private cancel(doc: vscode.TextDocument): void {
    const key = doc.uri.toString();
    const t = this.timers.get(key);
    if (t) clearTimeout(t);
    this.timers.delete(key);
  }

  schedule(doc: vscode.TextDocument, delay = 0): void {
    this.cancel(doc);
    const key = doc.uri.toString();
    this.timers.set(
      key,
      setTimeout(() => {
        this.timers.delete(key);
        void this.check(doc);
      }, delay)
    );
  }

  /** Checks a document now; resolves with the number of problems found (or undefined on failure). */
  async check(doc: vscode.TextDocument): Promise<number | undefined> {
    if (doc.isClosed) return undefined;
    const key = doc.uri.toString();
    const gen = (this.generation.get(key) ?? 0) + 1;
    this.generation.set(key, gen);
    const version = doc.version;
    const text = doc.getText();

    // Saved files are checked in place; unsaved text through a temporary copy.
    let file: string;
    let temp: string | undefined;
    const cwd = doc.uri.scheme === "file" ? dirname(doc.uri.fsPath) : undefined;
    if (doc.uri.scheme === "file" && !doc.isDirty) {
      file = doc.uri.fsPath;
    } else {
      const name = doc.uri.scheme === "file" ? basename(doc.uri.fsPath) : "untitled.fg";
      const dir = join(tempRoot, createHash("sha1").update(key).digest("hex").slice(0, 12));
      mkdirSync(dir, { recursive: true });
      temp = join(dir, name.endsWith(".fg") ? name : name + ".fg");
      writeFileSync(temp, text, "utf8");
      file = temp;
    }

    const bin = foguPath(doc.uri);
    const r = await checkWithFogu(bin, file, cwd);
    if (r.error) {
      output.appendLine(`fogu check failed: ${r.error.message}`);
      if (r.error.code === "ENOENT") warnMissing(bin);
      return undefined;
    }
    // A newer check started or the text changed meanwhile: drop this result.
    if (this.generation.get(key) !== gen || doc.isClosed || doc.version !== version) return undefined;

    const lines = text.split(/\r?\n/);
    const parsed = parseCheckOutput(r.stdout + "\n" + r.stderr, (n) => lines[n]);
    const diags = parsed.map((p) => {
      const line = Math.min(p.line, Math.max(0, doc.lineCount - 1));
      const d = new vscode.Diagnostic(
        new vscode.Range(line, p.start, line, p.end),
        p.message,
        p.severity === "error" ? vscode.DiagnosticSeverity.Error : vscode.DiagnosticSeverity.Warning
      );
      d.source = "fogu";
      if (p.code) d.code = p.code;
      if (p.unnecessary) d.tags = [vscode.DiagnosticTag.Unnecessary];
      return d;
    });
    if (r.code !== 0 && r.code !== null && diags.length === 0) {
      output.appendLine(`fogu check ${file} exited with ${r.code}:\n${r.stdout}${r.stderr}`);
    }
    this.collection.set(doc.uri, diags);
    return diags.length;
  }

  dispose(): void {
    for (const t of this.timers.values()) clearTimeout(t);
    this.subs.forEach((s) => s.dispose());
    this.collection.dispose();
    try {
      rmSync(tempRoot, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
  }
}
