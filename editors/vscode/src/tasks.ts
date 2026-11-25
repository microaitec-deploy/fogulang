import * as vscode from "vscode";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { foguPath } from "./config";

export interface FoguTaskDefinition extends vscode.TaskDefinition {
  type: "fogu";
  command: string;
  args?: string[];
}

const MATCHERS = ["$fogu", "$fogu-warning"];

/** fogu.json of a folder, if any. */
export function readManifest(dir: string): any | undefined {
  const p = join(dir, "fogu.json");
  if (!existsSync(p)) return undefined;
  try {
    return JSON.parse(readFileSync(p, "utf8"));
  } catch {
    return undefined;
  }
}

/** The nearest folder with a fogu.json above `file` (inside its workspace folder), else the file's folder. */
export function projectDir(file: vscode.Uri): string {
  const ws = vscode.workspace.getWorkspaceFolder(file)?.uri.fsPath;
  let dir = dirname(file.fsPath);
  for (;;) {
    if (existsSync(join(dir, "fogu.json"))) return dir;
    const up = dirname(dir);
    if (up === dir || (ws && !up.startsWith(ws))) break;
    dir = up;
  }
  return dirname(file.fsPath);
}

/** Builds a task running `fogu <command> <args…>`. */
export function makeTask(
  def: FoguTaskDefinition,
  scope: vscode.WorkspaceFolder | vscode.TaskScope,
  opts: { cwd?: string; name?: string; uri?: vscode.Uri; dedicated?: boolean } = {}
): vscode.Task {
  const bin = foguPath(opts.uri ?? (typeof scope === "object" ? scope.uri : undefined));
  const args = [def.command, ...(def.args ?? [])].filter((a) => a !== "");
  const cwd = opts.cwd ?? (typeof scope === "object" ? scope.uri.fsPath : undefined);
  const exec = new vscode.ShellExecution(bin, args, cwd ? { cwd } : undefined);
  const name = opts.name ?? ["fogu", ...args].join(" ");
  const task = new vscode.Task(def, scope, name, "fogu", exec, MATCHERS);
  task.presentationOptions = {
    reveal: vscode.TaskRevealKind.Always,
    panel: opts.dedicated ? vscode.TaskPanelKind.Dedicated : vscode.TaskPanelKind.Shared,
    clear: true,
    showReuseMessage: false,
  };
  if (def.command === "check") task.group = vscode.TaskGroup.Build;
  if (def.command === "test") task.group = vscode.TaskGroup.Test;
  return task;
}

/** Tasks shown in "Run Task": check, test, fmt --check, run and the scripts of fogu.json. */
export class FoguTaskProvider implements vscode.TaskProvider {
  provideTasks(): vscode.Task[] {
    const tasks: vscode.Task[] = [];
    for (const folder of vscode.workspace.workspaceFolders ?? []) {
      tasks.push(makeTask({ type: "fogu", command: "check" }, folder, { name: "check" }));
      tasks.push(makeTask({ type: "fogu", command: "test" }, folder, { name: "test" }));
      tasks.push(makeTask({ type: "fogu", command: "fmt", args: ["--check"] }, folder, { name: "fmt --check" }));
      const manifest = readManifest(folder.uri.fsPath);
      if (manifest?.entry) {
        tasks.push(makeTask({ type: "fogu", command: "run" }, folder, { name: `run (${manifest.entry})`, dedicated: true }));
      }
      if (manifest?.scripts && typeof manifest.scripts === "object") {
        for (const script of Object.keys(manifest.scripts)) {
          if (["check", "test", "run", "fmt"].includes(script)) continue;
          tasks.push(makeTask({ type: "fogu", command: script }, folder, { name: `script: ${script}`, dedicated: true }));
        }
      }
      if (manifest?.desktop) {
        tasks.push(makeTask({ type: "fogu", command: "desktop", args: ["dev"] }, folder, { name: "desktop dev", dedicated: true }));
        tasks.push(makeTask({ type: "fogu", command: "desktop", args: ["build"] }, folder, { name: "desktop build" }));
      }
    }
    return tasks;
  }

  resolveTask(task: vscode.Task): vscode.Task | undefined {
    const def = task.definition as FoguTaskDefinition;
    if (!def.command) return undefined;
    const scope = task.scope ?? vscode.TaskScope.Workspace;
    const t = makeTask(def, scope as vscode.WorkspaceFolder | vscode.TaskScope, { name: task.name });
    // keep the definition object identity (VS Code requires it for tasks.json entries)
    const resolved = new vscode.Task(def, scope as vscode.WorkspaceFolder | vscode.TaskScope, task.name, "fogu", t.execution, t.problemMatchers);
    resolved.presentationOptions = t.presentationOptions;
    resolved.group = t.group;
    return resolved;
  }
}

export function runTask(task: vscode.Task): Thenable<vscode.TaskExecution> {
  return vscode.tasks.executeTask(task);
}
