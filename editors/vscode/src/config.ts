import * as vscode from "vscode";
import { resolveFogu } from "./core/proc";

export const output = vscode.window.createOutputChannel("Fogu");

let cached: { key: string; bin: string } | undefined;

/** The fogu executable for a document (or the first workspace folder). */
export function foguPath(uri?: vscode.Uri): string {
  const cfg = vscode.workspace.getConfiguration("fogu", uri);
  const configured = cfg.get<string>("path", "");
  const folder = (uri && vscode.workspace.getWorkspaceFolder(uri)) ?? vscode.workspace.workspaceFolders?.[0];
  const key = `${configured}|${folder?.uri.fsPath ?? ""}`;
  if (cached?.key === key) return cached.bin;
  const bin = resolveFogu(configured, { workspaceFolder: folder?.uri.fsPath });
  cached = { key, bin };
  output.appendLine(`fogu executable: ${bin}`);
  return bin;
}

export function resetFoguPath(): void {
  cached = undefined;
}

let warned = false;

/** Shown once when the binary cannot be started. */
export function warnMissing(bin: string): void {
  if (warned) return;
  warned = true;
  const where = vscode.env.remoteName === "wsl" ? " inside WSL" : "";
  vscode.window
    .showWarningMessage(
      `Fogu: cannot run '${bin}'${where}. Install Fogu or set "fogu.path".`,
      "Open Settings",
      "Download"
    )
    .then((choice) => {
      if (choice === "Open Settings") vscode.commands.executeCommand("workbench.action.openSettings", "fogu.path");
      if (choice === "Download") vscode.env.openExternal(vscode.Uri.parse("https://fogulang.microaitec.com"));
    });
}

export function resetWarning(): void {
  warned = false;
}
