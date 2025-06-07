// The module 'vscode' contains the VS Code extensibility API
// Import the module and reference it with the alias vscode in your code below
import path from "path";
import v from "vscode";
import { ExtensionContext } from "vscode";

import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  Executable,
} from "vscode-languageclient/node";

let client: LanguageClient;

// This method is called when your extension is activated
// Your extension is activated the very first time the command is executed
export function activate(context: ExtensionContext) {
  console.log("ADL Language Server is starting...");

  const adlWorkingDirectories = context.globalState.get(
    "adl.workingDirectories"
  );
  let adlRoot: string;
  if (adlWorkingDirectories instanceof Array) {
    // TODO: handle multiple working directories
    adlRoot = adlWorkingDirectories[0];
  } else {
    adlRoot = "adl";
  }

  let adlLspPath = context.globalState.get("adl.lspPath");
  if (typeof adlLspPath !== "string") {
    adlLspPath = "adl-lsp";
  }

  console.error("adlLspPath", adlLspPath);

  // Debug mode
  // const run: Executable = {
  //   command: "cargo",
  //   args: ["run", "--bin", "adl-lsp", "--", "--adl-root", adlRoot], // TODO: pass working directories as an argument
  //   options: {
  //     cwd: "/Users/alexytsu/Develop/Repositories/adl-lang/adl-lsp/rust/adl-lsp",
  //   },
  // };

  const run: Executable = {
    command: "adl-lsp",
    args: ["--adl-root", adlRoot], // TODO: pass working directories as an argument
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
