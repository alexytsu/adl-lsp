import v from "vscode";
import { ExtensionContext } from "vscode";
import os from "os";
import path from "path";

import {
  LanguageClient,
  LanguageClientOptions,
  Executable,
} from "vscode-languageclient/node";

let client: LanguageClient;

export function activate(context: ExtensionContext) {
  console.log("ADL Language Server is starting...");

  const adlPackageRootsConfig = v.workspace
    .getConfiguration("adl")
    .get("packageRoots");

  let _adlPackageRoots: string[];
  if (adlPackageRootsConfig instanceof Array) {
    _adlPackageRoots = adlPackageRootsConfig;
  } else {
    _adlPackageRoots = ["adl"];
  }

  const adlPackageRoots = _adlPackageRoots.map((root) => {
    const relativePath = v.workspace.asRelativePath(root, true);

    // Get the workspace root
    const workspaceFolders = v.workspace.workspaceFolders;
    if (!workspaceFolders) {
      console.error("No workspace folder found");
      return relativePath;
    }

    // Construct absolute path using workspace root
    const workspaceRoot = workspaceFolders[0].uri.fsPath;
    const absolutePath = path.join(workspaceRoot, relativePath);
    console.log("absolute package root: ", absolutePath);
    return absolutePath;
  });

  let adlLspPath: string =
    v.workspace.getConfiguration("adl").get("lspPath") ?? "adl-lsp";

  const adlLspArgs = [
    "--client",
    "vscode",
    "--package-roots",
    adlPackageRoots.join(","),
  ];

  // Publish mode
  const run: Executable = {
    command: adlLspPath,
    args: [...adlLspArgs],
    options: {
      cwd: "/Users/alexytsu/Develop/Repositories/adl-lang/adl-lsp/rust/adl-lsp",
    },
  };

  // Debug mode
  // const run: Executable = {
  //   command: "cargo",
  //   args: ["run", "--bin", "adl-lsp", "--", ...adlLspArgs],
  //   options: {
  //     cwd: "/Users/alexytsu/Develop/Repositories/adl-lang/adl-lsp/rust/adl-lsp",
  //   },
  // };

  // HACK path substitution
  if (
    adlLspPath.startsWith("~") ||
    adlLspPath.startsWith("${userHome}") ||
    adlLspPath.startsWith("$HOME")
  ) {
    adlLspPath = adlLspPath.replace("~", os.homedir());
    adlLspPath = adlLspPath.replace("${userHome}", os.homedir());
    adlLspPath = adlLspPath.replace("$HOME", os.homedir());
  }

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
