import * as vscode from "vscode";
import { MCU_IMPORT } from "./commands";

const TEST_CALL = /^\s*(test|skip)\s*\(\s*(["'`])((?:\\.|(?!\2).)*)\2/;

/** `Run | Check` at the top of a file, `Run test` above each test(...) of *.test.fg files. */
export class FoguCodeLens implements vscode.CodeLensProvider {
  private emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeCodeLenses = this.emitter.event;

  refresh(): void {
    this.emitter.fire();
  }

  provideCodeLenses(doc: vscode.TextDocument): vscode.CodeLens[] {
    if (!vscode.workspace.getConfiguration("fogu", doc.uri).get<boolean>("codeLens", true)) return [];
    if (doc.uri.scheme !== "file") return [];
    const top = new vscode.Range(0, 0, 0, 0);
    const lenses: vscode.CodeLens[] = [];
    const isTest = doc.fileName.endsWith(".test.fg");
    if (isTest) {
      lenses.push(new vscode.CodeLens(top, { title: "$(beaker) Run tests in file", command: "fogu.testFile", arguments: [doc.uri] }));
      for (let i = 0; i < doc.lineCount; i++) {
        const m = TEST_CALL.exec(doc.lineAt(i).text);
        if (m && m[1] === "test") {
          const name = m[3];
          lenses.push(
            new vscode.CodeLens(new vscode.Range(i, 0, i, 0), {
              title: "$(play) Run test",
              command: "fogu.testFile",
              arguments: [doc.uri, name],
            })
          );
        }
      }
      return lenses;
    }
    if (MCU_IMPORT.test(doc.getText())) {
      lenses.push(new vscode.CodeLens(top, { title: "$(circuit-board) Flash & monitor", command: "fogu.mcuRun", arguments: [doc.uri] }));
      lenses.push(new vscode.CodeLens(top, { title: "Build firmware", command: "fogu.mcuBuild", arguments: [doc.uri] }));
    } else {
      lenses.push(new vscode.CodeLens(top, { title: "$(play) Run", command: "fogu.run", arguments: [doc.uri] }));
    }
    lenses.push(new vscode.CodeLens(top, { title: "Check", command: "fogu.check" }));
    return lenses;
  }
}
