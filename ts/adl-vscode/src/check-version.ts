import v from "vscode";
import { LanguageClient } from "vscode-languageclient/node";

export const REQUIRED_MAJOR_VERSION = 0;
export const REQUIRED_MINOR_VERSION = 9;
export const REQUIRED_PATCH_VERSION = 1;
export const REQUIRED_VERSION = `${REQUIRED_MAJOR_VERSION}.${REQUIRED_MINOR_VERSION}.${REQUIRED_PATCH_VERSION}`;

export const INSTALL_COMMAND = "cargo install adl-lsp";
export const RESTART_COMMAND = "adl-vscode.restart-language-server";
export const UPDATE_COMMAND = "adl-vscode.update-language-server";

export type CheckVersionResult =
  | "version-not-specified"
  | "version-not-supported"
  | "version-supported";

export function checkVersion(
  serverVersion: string | undefined
): CheckVersionResult {
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

let statusBarItem: v.StatusBarItem | undefined;
let lastProblem: string | undefined;

/**
 * Keeps a warning in the status bar for as long as the server needs attention,
 * so the problem stays visible after the notification has been dismissed.
 */
function setServerProblem(problem: string | undefined, label?: string) {
  lastProblem = problem;
  if (!problem) {
    statusBarItem?.hide();
    return;
  }

  if (!statusBarItem) {
    statusBarItem = v.window.createStatusBarItem(v.StatusBarAlignment.Left);
    statusBarItem.command = UPDATE_COMMAND;
    statusBarItem.backgroundColor = new v.ThemeColor(
      "statusBarItem.warningBackground"
    );
  }
  statusBarItem.text = `$(warning) ${label ?? "adl-lsp"}`;
  statusBarItem.tooltip = `${problem}\nClick to update.`;
  statusBarItem.show();
}

/**
 * Asks the user to install or update adl-lsp. The notification has buttons, so
 * VS Code keeps it on screen until it is answered or dismissed.
 */
export async function promptToUpdateServer(problem?: string) {
  const message =
    problem ?? lastProblem ?? `Install the latest adl-lsp with ${INSTALL_COMMAND}.`;

  const updateInTerminal = "Update in Terminal";
  const copyCommand = "Copy Command";
  const choice = await v.window.showErrorMessage(
    message,
    updateInTerminal,
    copyCommand
  );

  if (choice === copyCommand) {
    await v.env.clipboard.writeText(INSTALL_COMMAND);
    v.window.showInformationMessage(
      `Copied "${INSTALL_COMMAND}" to the clipboard.`
    );
    return;
  }
  if (choice !== updateInTerminal) {
    return;
  }

  const terminal = v.window.createTerminal("Update adl-lsp");
  terminal.show();
  terminal.sendText(INSTALL_COMMAND);

  const restart = "Restart Language Server";
  const restartChoice = await v.window.showInformationMessage(
    "Restart the ADL language server once the install has finished.",
    restart
  );
  if (restartChoice === restart) {
    await v.commands.executeCommand(RESTART_COMMAND);
  }
}

/**
 * Checks the version the running server reported and, if it is too old, tells
 * the user in the output channel, the status bar and a notification.
 *
 * @returns whether the server is supported
 */
export function checkVersionAndNotify(client: LanguageClient): boolean {
  const serverVersion = client.initializeResult?.serverInfo?.version;
  const checkResult = checkVersion(serverVersion);

  if (checkResult === "version-supported") {
    client.outputChannel.appendLine(
      `[adl-vscode] adl-lsp ${serverVersion} is supported (requires ${REQUIRED_VERSION} or later)`
    );
    setServerProblem(undefined);
    return true;
  }

  const found = serverVersion ? `adl-lsp ${serverVersion}` : "This adl-lsp";
  const problem = `${found} is not supported by this version of the ADL extension. Update to ${REQUIRED_VERSION} or later with ${INSTALL_COMMAND}.`;

  client.outputChannel.appendLine(`[adl-vscode] ${problem}`);
  console.error(problem);
  setServerProblem(problem, `adl-lsp ${serverVersion ?? "unknown"} is outdated`);
  void promptToUpdateServer(problem);
  return false;
}

/**
 * Reports that the server could not be started at all, which usually means it
 * is not installed or `adl.lspPath` points at the wrong place.
 */
export function notifyServerFailedToStart(command: string, error: unknown) {
  const reason = error instanceof Error ? error.message : String(error);
  const problem = `Could not start the ADL language server "${command}": ${reason}. Install it with ${INSTALL_COMMAND}, or set adl.lspPath.`;

  console.error(problem);
  setServerProblem(problem, "adl-lsp is not running");
  void promptToUpdateServer(problem);
}

export function disposeVersionStatus() {
  statusBarItem?.dispose();
  statusBarItem = undefined;
}
