import * as vscode from "vscode";
import { FoguCodeLens } from "./codelens";
import { registerCommands, trackContext } from "./commands";
import { output, resetFoguPath, resetWarning } from "./config";
import { Checker } from "./diagnostics";
import { Formatter } from "./formatting";
import { FoguCompletion, FoguDefinition, FoguDocumentSymbols, FoguHover, FoguSignatureHelp } from "./language";
import { FoguTaskProvider } from "./tasks";

const SELECTOR: vscode.DocumentSelector = [
  { language: "fogu", scheme: "file" },
  { language: "fogu", scheme: "untitled" },
];

export function activate(ctx: vscode.ExtensionContext): void {
  output.appendLine(`Fogu extension ${ctx.extension.packageJSON.version} (${process.platform}${vscode.env.remoteName ? ", remote " + vscode.env.remoteName : ""})`);

  const checker = new Checker();
  const lenses = new FoguCodeLens();
  ctx.subscriptions.push(
    output,
    checker,
    vscode.languages.registerDocumentFormattingEditProvider(SELECTOR, new Formatter()),
    vscode.languages.registerCompletionItemProvider(SELECTOR, new FoguCompletion(), ".", ":", '"', "{", "/"),
    vscode.languages.registerHoverProvider(SELECTOR, new FoguHover()),
    vscode.languages.registerDefinitionProvider(SELECTOR, new FoguDefinition()),
    vscode.languages.registerSignatureHelpProvider(SELECTOR, new FoguSignatureHelp(), "(", ","),
    vscode.languages.registerDocumentSymbolProvider(SELECTOR, new FoguDocumentSymbols()),
    vscode.languages.registerCodeLensProvider({ language: "fogu", scheme: "file" }, lenses),
    vscode.tasks.registerTaskProvider("fogu", new FoguTaskProvider()),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("fogu.path")) {
        resetFoguPath();
        resetWarning();
      }
      if (e.affectsConfiguration("fogu.codeLens")) lenses.refresh();
    })
  );
  registerCommands(ctx, checker);
  trackContext(ctx);
}

export function deactivate(): void {
  /* disposables are released by VS Code */
}
