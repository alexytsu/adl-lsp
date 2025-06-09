import v, { ExtensionContext } from "vscode";
import { LanguageClient } from "vscode-languageclient/node";

export function registerCommands(
  client: LanguageClient,
  context: ExtensionContext
) {
  const disposable = v.commands.registerCommand(
    "adl-vscode.restart-language-server",
    async () => {
      // The code you place here will be executed every time your command is executed
      // Display a message box to the user
      await client.restart();
      v.window.showInformationMessage("Restarted ADL Language Server");
    }
  );

  context.subscriptions.push(disposable);
}
