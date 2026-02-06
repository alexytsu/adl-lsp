import v from "vscode";
import path from "path";
import os from "os";
import fs from "fs";
import { Executable } from "vscode-languageclient/node";

/**
 *
 * @returns A list of directories to search fo ADL files (not necessarily ADL package roots, just the whole world we care about)
 */
export function getSearchDirs(): string[] {
  const adlPackageRootsConfig = v.workspace
    .getConfiguration("adl")
    .get("searchDirs");

  let _searchDirs: string[];
  if (adlPackageRootsConfig instanceof Array) {
    _searchDirs = adlPackageRootsConfig;
  } else {
    _searchDirs = ["."];
  }

  return _searchDirs.map((root) => {
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

function expandHomePath(input: string): string {
  if (
    input.startsWith("~") ||
    input.startsWith("${userHome}") ||
    input.startsWith("$HOME")
  ) {
    // HACK path substitution
    return input
      .replace("~", os.homedir())
      .replace("${userHome}", os.homedir())
      .replace("$HOME", os.homedir());
  }

  return input;
}

export function getLspPath(): string {
  const adlLspPath: string =
    v.workspace.getConfiguration("adl").get("lspPath") ?? "adl-lsp";
  return expandHomePath(adlLspPath);
}

function getCargoPath(): string {
  const configured = v.workspace
    .getConfiguration("adl")
    .get<string>("cargoPath");
  if (configured && configured.trim().length > 0) {
    return expandHomePath(configured);
  }

  const envCargo = process.env.CARGO;
  if (envCargo && fs.existsSync(envCargo)) {
    return envCargo;
  }

  const candidates = [
    path.join(os.homedir(), ".cargo", "bin", "cargo"),
    path.join(os.homedir(), ".local", "share", "cargo", "bin", "cargo"),
  ];

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  return "cargo";
}

function findDevCwd(extensionPath?: string): string | undefined {
  const candidates: string[] = [];

  if (extensionPath) {
    candidates.push(path.resolve(extensionPath, "..", "..", "rust", "adl-lsp"));
  }

  const workspaceRoot = v.workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (workspaceRoot) {
    candidates.push(path.join(workspaceRoot, "rust", "adl-lsp"));
  }

  for (const candidate of candidates) {
    if (fs.existsSync(path.join(candidate, "Cargo.toml"))) {
      return candidate;
    }
  }

  return undefined;
}

export function getLspExecutable(extensionPath?: string): {
  dev: Executable;
  prod: Executable;
} {
  const adlSearchDirs = getSearchDirs();
  const adlLspPath = getLspPath();
  const cargoPath = getCargoPath();
  const devCwd = findDevCwd(extensionPath);

  const adlLspArgs = [
    "--client",
    "vscode",
    "--search-dirs",
    adlSearchDirs.join(","),
  ];

  return {
    dev: {
      command: cargoPath,
      args: ["run", "--bin", "adl-lsp", "--", ...adlLspArgs],
      ...(devCwd ? { options: { cwd: devCwd } } : {}),
    },
    prod: {
      command: adlLspPath,
      args: [...adlLspArgs],
    },
  };
}
