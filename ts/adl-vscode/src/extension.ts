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

const REQUIRED_MAJOR_VERSION = 0;
const REQUIRED_MINOR_VERSION = 3;
const REQUIRED_PATCH_VERSION = 0;

export async function activate(context: ExtensionContext) {
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

  // Publish mode
  const run: Executable = {
    command: adlLspPath,
    args: [...adlLspArgs],
    options: {
      cwd: "/Users/alexytsu/Develop/Repositories/adl-lang/adl-lsp/rust/adl-lsp",
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

  await client.start();

  const serverVersion = client.initializeResult?.serverInfo?.version;
  console.log("Server version: ", serverVersion);

  let checkResult = checkVersion(serverVersion);

  const requiredVersion = `${REQUIRED_MAJOR_VERSION}.${REQUIRED_MINOR_VERSION}.${REQUIRED_PATCH_VERSION}`;
  const requiredVersionMessage = `adl-lsp ${serverVersion} is not supported. Please update to version ${requiredVersion} or later.\nYou can update by running cargo install adl-lsp`;

  if (checkResult !== "version-supported") {
    v.window.showErrorMessage(requiredVersionMessage);
    console.error("checkResult: ", checkResult);
    console.error(requiredVersionMessage);
  }

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

type CheckVersionResult =
  | "version-not-specified"
  | "version-not-supported"
  | "version-supported";

function checkVersion(serverVersion: string | undefined): CheckVersionResult {
  if (!serverVersion) {
    return "version-not-specified";
  }

  const [serverMajorVersion, serverMinorVersion, serverPatchVersion] =
    serverVersion.split(".").map((v) => parseInt(v)) ?? [];

  if (serverMajorVersion < REQUIRED_MAJOR_VERSION) {
    return "version-not-supported";
  } else if (serverMajorVersion === REQUIRED_MAJOR_VERSION) {
    if (serverMinorVersion < REQUIRED_MINOR_VERSION) {
      return "version-not-supported";
    } else if (serverMinorVersion === REQUIRED_MINOR_VERSION) {
      if (serverPatchVersion < REQUIRED_PATCH_VERSION) {
        return "version-not-supported";
      }
      return "version-supported";
    }
    return "version-supported";
  }
  return "version-supported";
}
