import v from "vscode";
import path from "path";
import os from "os";
import { Executable } from "vscode-languageclient/node";

export function getPackageRoots(): string[] {
  const adlPackageRootsConfig = v.workspace
    .getConfiguration("adl")
    .get("packageRoots");

  let _adlPackageRoots: string[];
  if (adlPackageRootsConfig instanceof Array) {
    _adlPackageRoots = adlPackageRootsConfig;
  } else {
    _adlPackageRoots = ["adl"];
  }

  return _adlPackageRoots.map((root) => {
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
}

export function getLspPath(): string {
  let adlLspPath: string =
    v.workspace.getConfiguration("adl").get("lspPath") ?? "adl-lsp";

  if (
    adlLspPath.startsWith("~") ||
    adlLspPath.startsWith("${userHome}") ||
    adlLspPath.startsWith("$HOME")
  ) {
    // HACK path substitution
    adlLspPath = adlLspPath.replace("~", os.homedir());
    adlLspPath = adlLspPath.replace("${userHome}", os.homedir());
    adlLspPath = adlLspPath.replace("$HOME", os.homedir());
  }

  return adlLspPath;
}

export function getLspExecutable(): {
  dev: Executable;
  prod: Executable;
} {
  const adlPackageRoots = getPackageRoots();
  const adlLspPath = getLspPath();

  const adlLspArgs = [
    "--client",
    "vscode",
    "--package-roots",
    adlPackageRoots.join(","),
  ];

  return {
    dev: {
      command: "cargo",
      args: ["run", "--bin", "adl-lsp", "--", ...adlLspArgs],
      options: {
        cwd: "/Users/alexytsu/Develop/Repositories/alexytsu/adl-lsp/rust/adl-lsp",
      },
    },
    prod: {
      command: adlLspPath,
      args: [...adlLspArgs],
    },
  };
}
