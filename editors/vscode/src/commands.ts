import * as vscode from "vscode";
import { readFileSync } from "node:fs";
import { foguPath, output, warnMissing } from "./config";
import { parseBoards, run } from "./core/proc";
import type { Checker } from "./diagnostics";
import { makeTask, projectDir, readManifest, runTask } from "./tasks";

export const MCU_IMPORT = /\bfrom\s+["']mcu(?:\/[^"']*)?["']/;

/** The target document: the given resource, else the active Fogu editor. */
async function targetFile(uri?: vscode.Uri): Promise<vscode.Uri | undefined> {
  if (uri instanceof vscode.Uri && uri.scheme === "file") {
    const open = vscode.workspace.textDocuments.find((d) => d.uri.toString() === uri.toString());
    if (open?.isDirty) await open.save();
    return uri;
  }
  const ed = vscode.window.activeTextEditor;
  if (!ed || ed.document.languageId !== "fogu") {
    vscode.window.showWarningMessage("Fogu: open a .fg file first.");
    return undefined;
  }
  if (ed.document.isUntitled) {
    const saved = await ed.document.save();
    if (!saved || ed.document.isUntitled) return undefined;
  } else if (ed.document.isDirty) {
    await ed.document.save();
  }
  return ed.document.uri;
}

function folderOf(uri?: vscode.Uri): vscode.WorkspaceFolder | vscode.TaskScope {
  return (uri && vscode.workspace.getWorkspaceFolder(uri)) ?? vscode.workspace.workspaceFolders?.[0] ?? vscode.TaskScope.Workspace;
}

function workspaceDir(uri?: vscode.Uri): string | undefined {
  const f = folderOf(uri);
  return typeof f === "object" ? f.uri.fsPath : undefined;
}

function isWsl(): boolean {
  if (vscode.env.remoteName === "wsl") return true;
  if (process.platform !== "linux") return false;
  try {
    return /microsoft/i.test(readFileSync("/proc/version", "utf8"));
  } catch {
    return false;
  }
}

/** The project folder for desktop commands: the active file's project, else the first folder with a desktop manifest. */
function desktopDir(): string | undefined {
  const ed = vscode.window.activeTextEditor;
  if (ed?.document.uri.scheme === "file") {
    const d = projectDir(ed.document.uri);
    if (readManifest(d)?.desktop) return d;
  }
  for (const f of vscode.workspace.workspaceFolders ?? []) {
    if (readManifest(f.uri.fsPath)?.desktop) return f.uri.fsPath;
  }
  return workspaceDir(ed?.document.uri);
}

let boardsCache: { name: string; description: string; detail: string }[] | undefined;

async function pickBoard(ctx: vscode.ExtensionContext, uri: vscode.Uri): Promise<string | undefined> {
  const configured = vscode.workspace.getConfiguration("fogu", uri).get<string>("mcu.board", "");
  if (configured) return configured;
  const last = ctx.workspaceState.get<string>("fogu.mcu.lastBoard");
  if (!boardsCache) {
    const bin = foguPath(uri);
    const r = await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: "fogu mcu boards" },
      () => run(bin, ["mcu", "boards"], { cwd: projectDir(uri), timeoutMs: 120000 })
    );
    if (r.error?.code === "ENOENT") {
      warnMissing(bin);
      return undefined;
    }
    const boards = parseBoards(r.stdout + "\n" + r.stderr);
    if (boards.length) boardsCache = boards;
    else output.appendLine(`fogu mcu boards:\n${r.stdout}${r.stderr}`);
  }
  const items: (vscode.QuickPickItem & { board: string })[] = (boardsCache ?? []).map((b) => ({
    label: b.name,
    board: b.name,
    description: b.description,
    detail: b.detail || undefined,
  }));
  if (last) items.sort((a, b) => (a.board === last ? -1 : b.board === last ? 1 : 0));
  items.push({ label: "$(edit) Other board…", board: "", description: "type a board name" });
  const pick = await vscode.window.showQuickPick(items, { title: "Fogu MCU: board", matchOnDescription: true, matchOnDetail: true });
  if (!pick) return undefined;
  let board = pick.board;
  if (!board) board = (await vscode.window.showInputBox({ title: "Board name", value: last ?? "" }))?.trim() ?? "";
  if (!board) return undefined;
  await ctx.workspaceState.update("fogu.mcu.lastBoard", board);
  return board;
}

async function pickPort(ctx: vscode.ExtensionContext, uri: vscode.Uri): Promise<string | undefined> {
  const configured = vscode.workspace.getConfiguration("fogu", uri).get<string>("mcu.port", "");
  if (configured) return configured;
  const last = ctx.workspaceState.get<string>("fogu.mcu.lastPort") ?? "";
  const port = await vscode.window.showInputBox({
    title: "Fogu MCU: serial port",
    prompt: "e.g. COM5 or /dev/ttyACM0 — leave empty to let fogu mcu choose",
    value: last,
  });
  if (port === undefined) return undefined;
  await ctx.workspaceState.update("fogu.mcu.lastPort", port.trim());
  return port.trim();
}

export function registerCommands(ctx: vscode.ExtensionContext, checker: Checker): void {
  const reg = (id: string, fn: (...args: any[]) => unknown) => ctx.subscriptions.push(vscode.commands.registerCommand(id, fn));

  reg("fogu.run", async (uri?: vscode.Uri) => {
    const file = await targetFile(uri);
    if (!file) return;
    const cwd = projectDir(file);
    await runTask(makeTask({ type: "fogu", command: file.fsPath }, folderOf(file), { cwd, name: `run ${vscode.workspace.asRelativePath(file)}`, uri: file, dedicated: true }));
  });

  reg("fogu.check", async () => {
    const ed = vscode.window.activeTextEditor;
    if (!ed || ed.document.languageId !== "fogu") return vscode.window.showWarningMessage("Fogu: open a .fg file first.");
    const n = await checker.check(ed.document);
    if (n === 0) vscode.window.setStatusBarMessage("$(check) fogu check: no problems", 4000);
    else if (n !== undefined) vscode.commands.executeCommand("workbench.actions.view.problems");
  });

  reg("fogu.checkWorkspace", async () => {
    const f = folderOf(vscode.window.activeTextEditor?.document.uri);
    await runTask(makeTask({ type: "fogu", command: "check" }, f, { name: "check" }));
  });

  reg("fogu.test", async () => {
    const uri = vscode.window.activeTextEditor?.document.uri;
    const f = folderOf(uri);
    await runTask(makeTask({ type: "fogu", command: "test" }, f, { name: "test" }));
  });

  reg("fogu.testFile", async (uri?: vscode.Uri, name?: string) => {
    const file = await targetFile(uri);
    if (!file) return;
    const cwd = workspaceDir(file) ?? projectDir(file);
    const rel = vscode.workspace.asRelativePath(file, false);
    const args = [file.fsPath];
    if (name) args.push("-t", name);
    await runTask(makeTask({ type: "fogu", command: "test", args }, folderOf(file), { cwd, name: name ? `test ${rel} -t ${name}` : `test ${rel}`, uri: file }));
  });

  reg("fogu.format", () => vscode.commands.executeCommand("editor.action.formatDocument"));

  reg("fogu.showOutput", () => output.show());

  reg("fogu.desktopDev", async () => {
    const dir = desktopDir();
    if (!dir) return vscode.window.showWarningMessage("Fogu: open a desktop project (fogu.json with a \"desktop\" section).");
    const args = ["dev"];
    if (isWsl()) {
      const pick = await vscode.window.showQuickPick(
        [
          { label: "Windows window", description: "--target windows (recommended under WSL)", target: "windows" },
          { label: "Linux window (WSLg)", description: "native", target: "" },
        ],
        { title: "Fogu desktop dev: where to open the window" }
      );
      if (!pick) return;
      if (pick.target) args.push("--target", pick.target);
    }
    await runTask(makeTask({ type: "fogu", command: "desktop", args }, folderOf(vscode.Uri.file(dir)), { cwd: dir, name: "desktop dev", dedicated: true }));
  });

  reg("fogu.desktopBuild", async () => {
    const dir = desktopDir();
    if (!dir) return vscode.window.showWarningMessage("Fogu: open a desktop project (fogu.json with a \"desktop\" section).");
    const items = [
      { label: "This platform", description: "fogu desktop build", args: [] as string[] },
      { label: "Windows (.exe)", description: "--target windows", args: ["--target", "windows"] },
      { label: "Debug build", description: "--debug (devtools, faster)", args: ["--debug"] },
      { label: "With installer", description: "--installer (needs the Tauri CLI)", args: ["--installer"] },
    ];
    if (isWsl()) items.unshift(items.splice(1, 1)[0]);
    const pick = await vscode.window.showQuickPick(items, { title: "Fogu desktop build" });
    if (!pick) return;
    await runTask(makeTask({ type: "fogu", command: "desktop", args: ["build", ...pick.args] }, folderOf(vscode.Uri.file(dir)), { cwd: dir, name: "desktop build" }));
  });

  reg("fogu.desktopCi", async () => {
    const dir = desktopDir();
    if (!dir) return vscode.window.showWarningMessage("Fogu: open a desktop project (fogu.json with a \"desktop\" section).");
    const picks = await vscode.window.showQuickPick(
      [
        { label: "windows", picked: true },
        { label: "macos", picked: true },
        { label: "linux", picked: true },
        { label: "macos-intel", picked: false },
      ],
      { title: "Fogu desktop ci: which systems should GitHub Actions build?", canPickMany: true }
    );
    if (!picks || picks.length === 0) return;
    const installer = await vscode.window.showQuickPick(
      [
        { label: "Single executable", description: "fogu desktop build", args: [] as string[] },
        { label: "Installers", description: "--installer (.msi/.exe, .dmg, .deb/.AppImage)", args: ["--installer"] },
      ],
      { title: "Fogu desktop ci: what should the workflow produce?" }
    );
    if (!installer) return;
    const args = ["ci", "--os", picks.map((p) => p.label).join(","), ...installer.args];
    await runTask(makeTask({ type: "fogu", command: "desktop", args }, folderOf(vscode.Uri.file(dir)), { cwd: dir, name: "desktop ci" }));
  });

  reg("fogu.desktopKeygen", async () => {
    const dir = desktopDir();
    if (!dir) return vscode.window.showWarningMessage("Fogu: open a desktop project (fogu.json with a \"desktop\" section).");
    const endpoint = await vscode.window.showInputBox({
      title: "Fogu desktop keygen: where will latest.json be served?",
      prompt: "The update endpoint written to fogu.json (desktop.update.endpoint); leave empty to set it later",
      placeHolder: "https://example.com/my-app/latest.json",
    });
    if (endpoint === undefined) return;
    const args = ["keygen"];
    if (endpoint.trim()) args.push("--endpoint", endpoint.trim());
    await runTask(makeTask({ type: "fogu", command: "desktop", args }, folderOf(vscode.Uri.file(dir)), { cwd: dir, name: "desktop keygen" }));
  });

  /** The file the MCU commands work for: the given one, the active editor's, else the first workspace folder. */
  const mcuContext = (uri?: vscode.Uri): vscode.Uri | undefined => {
    if (uri instanceof vscode.Uri && uri.scheme === "file") return uri;
    const active = vscode.window.activeTextEditor?.document.uri;
    if (active?.scheme === "file") return active;
    return vscode.workspace.workspaceFolders?.[0]?.uri;
  };

  reg("fogu.mcuDetect", async (uri?: vscode.Uri) => {
    const file = mcuContext(uri);
    if (!file) return vscode.window.showWarningMessage("Fogu: open a folder or a .fg file first.");
    const board = await pickBoard(ctx, file);
    if (!board) return;
    const args = ["detect", "--board", board];
    const port = await pickPort(ctx, file);
    if (port === undefined) return;
    if (port) args.push("--port", port);
    await runTask(makeTask({ type: "fogu", command: "mcu", args }, folderOf(file), { cwd: projectDir(file), name: `mcu detect (${board})` }));
  });

  reg("fogu.mcuMonitor", async (uri?: vscode.Uri) => {
    const file = mcuContext(uri);
    if (!file) return vscode.window.showWarningMessage("Fogu: open a folder or a .fg file first.");
    const port = await pickPort(ctx, file);
    if (port === undefined) return;
    const args = ["monitor"];
    if (port) args.push("--port", port);
    await runTask(makeTask({ type: "fogu", command: "mcu", args }, folderOf(file), { cwd: projectDir(file), name: "mcu monitor", dedicated: true }));
  });

  const mcu = async (action: "run" | "build", uri?: vscode.Uri) => {
    const file = await targetFile(uri);
    if (!file) return;
    const board = await pickBoard(ctx, file);
    if (!board) return;
    const args = [action, file.fsPath, "--board", board];
    if (action === "run") {
      const port = await pickPort(ctx, file);
      if (port === undefined) return;
      if (port) args.push("--port", port);
    }
    await runTask(
      makeTask({ type: "fogu", command: "mcu", args }, folderOf(file), {
        cwd: projectDir(file),
        name: `mcu ${action} ${vscode.workspace.asRelativePath(file)} (${board})`,
        uri: file,
        dedicated: action === "run",
      })
    );
  };
  reg("fogu.mcuRun", (uri?: vscode.Uri) => mcu("run", uri));
  reg("fogu.mcuBuild", (uri?: vscode.Uri) => mcu("build", uri));
}

/** Context keys: fogu.isDesktopProject, fogu.isMcuProject, fogu.fileImportsMcu. */
export function trackContext(ctx: vscode.ExtensionContext): void {
  const updateProject = () => {
    let desktop = false;
    let mcuDep = false;
    for (const f of vscode.workspace.workspaceFolders ?? []) {
      const m = readManifest(f.uri.fsPath);
      if (m?.desktop) desktop = true;
      if (m?.dependencies && typeof m.dependencies === "object" && "mcu" in m.dependencies) mcuDep = true;
    }
    vscode.commands.executeCommand("setContext", "fogu.isDesktopProject", desktop);
    vscode.commands.executeCommand("setContext", "fogu.isMcuProject", mcuDep);
  };
  const updateFile = () => {
    const doc = vscode.window.activeTextEditor?.document;
    const imports = !!doc && doc.languageId === "fogu" && MCU_IMPORT.test(doc.getText());
    vscode.commands.executeCommand("setContext", "fogu.fileImportsMcu", imports);
    if (!imports && doc?.languageId === "fogu" && doc.uri.scheme === "file") {
      // a desktop project opened below the workspace root
      const m = readManifest(projectDir(doc.uri));
      if (m?.desktop) vscode.commands.executeCommand("setContext", "fogu.isDesktopProject", true);
    }
  };
  updateProject();
  updateFile();
  const watcher = vscode.workspace.createFileSystemWatcher("**/fogu.json");
  watcher.onDidChange(updateProject);
  watcher.onDidCreate(updateProject);
  watcher.onDidDelete(updateProject);
  ctx.subscriptions.push(
    watcher,
    vscode.workspace.onDidChangeWorkspaceFolders(updateProject),
    vscode.window.onDidChangeActiveTextEditor(updateFile),
    vscode.workspace.onDidSaveTextDocument((d) => {
      if (d === vscode.window.activeTextEditor?.document) updateFile();
    })
  );
}

