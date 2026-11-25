import * as vscode from "vscode";
import { dirname } from "node:path";
import { foguPath, output, warnMissing } from "./config";
import { formatWithFogu } from "./core/proc";

/** Document formatting through `fogu fmt -` (stdin → stdout). */
export class Formatter implements vscode.DocumentFormattingEditProvider {
  async provideDocumentFormattingEdits(doc: vscode.TextDocument): Promise<vscode.TextEdit[]> {
    if (!vscode.workspace.isTrusted) return [];
    const bin = foguPath(doc.uri);
    const text = doc.getText();
    const r = await formatWithFogu(bin, text, doc.uri.scheme === "file" ? dirname(doc.uri.fsPath) : undefined);
    if (r.error !== undefined) {
      output.appendLine(`fogu fmt: ${r.error}`);
      if (r.error.startsWith("fogu not found")) warnMissing(bin);
      else vscode.window.setStatusBarMessage(`$(warning) fogu fmt: ${r.error.split("\n")[0]}`, 5000);
      return [];
    }
    const formatted = r.text ?? "";
    if (!formatted.trim() && text.trim()) return [];
    const normalized = text.replace(/\r\n/g, "\n");
    if (formatted === normalized) return [];
    const full = new vscode.Range(doc.positionAt(0), doc.positionAt(text.length));
    return [vscode.TextEdit.replace(full, formatted)];
  }
}
