import v from "vscode";
import { ExtensionContext } from "vscode";
import os from "os";

import {
  LanguageClient,
  LanguageClientOptions,
  Executable,
} from "vscode-languageclient/node";

let client: LanguageClient;

export function activate(context: ExtensionContext) {
  console.log("ADL Language Server is starting...");

  const adlWorkingDirectories = v.workspace
    .getConfiguration("adl")
    .get("workingDirectories");
  let adlRoot: string;
  if (adlWorkingDirectories instanceof Array) {
    // TODO: handle multiple working directories
    adlRoot = adlWorkingDirectories[0];
  } else {
    adlRoot = "adl";
  }

  let adlLspPath: string =
    v.workspace.getConfiguration("adl").get("lspPath") ?? "adl-lsp";

  // Debug mode
  // const run: Executable = {
  //   command: "cargo",
  //   args: ["run", "--bin", "adl-lsp", "--", "--adl-root", adlRoot], // TODO: pass working directories as an argument
  //   options: {
  //     cwd: "/Users/alexytsu/Develop/Repositories/adl-lang/adl-lsp/rust/adl-lsp",
  //   },
  // };

  v.window.showInformationMessage(`adlLspPath: ${adlLspPath}`);
  v.window.showInformationMessage(`homeDir: ${os.homedir()}`);
  if (
    adlLspPath.startsWith("~") ||
    adlLspPath.startsWith("${userHome}") ||
    adlLspPath.startsWith("$HOME")
  ) {
    adlLspPath = adlLspPath.replace("~", os.homedir());
    adlLspPath = adlLspPath.replace("${userHome}", os.homedir());
    adlLspPath = adlLspPath.replace("$HOME", os.homedir());
  }

  const run: Executable = {
    command: adlLspPath,
    options: {
      env: {
        RUST_LOG: "debug",
      },
    },
  };

  const serverOptions = {
    run,
    debug: run,
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "adl" }],
    synchronize: {
      fileEvents: v.workspace.createFileSystemWatcher("**/.adl"),
    },
  };

  client = new LanguageClient(
    "adl-vscode",
    "ADL Language Server",
    serverOptions,
    clientOptions
  );

  client.start();

  // The command has been defined in the package.json file
  // Now provide the implementation of the command with registerCommand
  // The commandId parameter must match the command field in package.json
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

export function deactivate() {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
