import * as assert from "assert";
import * as vscode from "vscode";
import { LanguageClient } from "vscode-languageclient/node";
import {
  INSTALL_COMMAND,
  REQUIRED_VERSION,
  checkVersion,
  checkVersionAndNotify,
  disposeVersionStatus,
  promptToUpdateServer,
} from "../check-version";

type ShowMessage = typeof vscode.window.showErrorMessage;

/** Stand-in for a started client that reported `version` during initialize. */
function clientReporting(version: string | undefined) {
  const output: string[] = [];
  const client = {
    initializeResult: { serverInfo: version ? { version } : undefined },
    outputChannel: { appendLine: (line: string) => output.push(line) },
  } as unknown as LanguageClient;
  return { client, output };
}

suite("server version check", () => {
  const [major, minor, patch] = REQUIRED_VERSION.split(".").map(Number);
  const originalShowError = vscode.window.showErrorMessage;
  const originalShowInformation = vscode.window.showInformationMessage;
  let prompts: { message: string; buttons: string[] }[];
  let answer: string | undefined;

  setup(() => {
    prompts = [];
    answer = undefined;
    (vscode.window as { showErrorMessage: unknown }).showErrorMessage = (
      message: string,
      ...buttons: string[]
    ) => {
      prompts.push({ message, buttons });
      return Promise.resolve(answer);
    };
    (
      vscode.window as { showInformationMessage: unknown }
    ).showInformationMessage = () => Promise.resolve(undefined);
  });

  teardown(() => {
    (vscode.window as { showErrorMessage: ShowMessage }).showErrorMessage =
      originalShowError;
    (
      vscode.window as { showInformationMessage: ShowMessage }
    ).showInformationMessage = originalShowInformation;
    disposeVersionStatus();
  });

  test("compares against the required version", () => {
    assert.strictEqual(checkVersion(undefined), "version-not-specified");
    assert.strictEqual(checkVersion("0.8.2"), "version-not-supported");
    assert.strictEqual(checkVersion(REQUIRED_VERSION), "version-supported");
    assert.strictEqual(
      checkVersion(`${major}.${minor}.${patch + 1}`),
      "version-supported"
    );
    assert.strictEqual(
      checkVersion(`${major}.${minor + 1}.0`),
      "version-supported"
    );
    assert.strictEqual(checkVersion(`${major + 1}.0.0`), "version-supported");
  });

  test("a supported server raises no prompt", () => {
    const { client, output } = clientReporting(REQUIRED_VERSION);
    assert.strictEqual(checkVersionAndNotify(client), true);
    assert.deepStrictEqual(prompts, []);
    assert.match(output.join("\n"), /is supported/);
  });

  test("an outdated server raises a prompt with buttons", () => {
    const { client, output } = clientReporting("0.8.2");
    assert.strictEqual(checkVersionAndNotify(client), false);

    assert.strictEqual(prompts.length, 1);
    assert.match(prompts[0].message, /adl-lsp 0\.8\.2 is not supported/);
    assert.ok(prompts[0].message.includes(REQUIRED_VERSION));
    // Buttons are what keep the notification on screen until it is answered.
    assert.deepStrictEqual(prompts[0].buttons, [
      "Update in Terminal",
      "Copy Command",
    ]);
    // The same message lands in the channel where the server's logs are read.
    assert.match(output.join("\n"), /adl-lsp 0\.8\.2 is not supported/);
  });

  test("a server that reports no version raises a prompt", () => {
    const { client } = clientReporting(undefined);
    assert.strictEqual(checkVersionAndNotify(client), false);
    assert.strictEqual(prompts.length, 1);
  });

  test("Copy Command puts the install command on the clipboard", async () => {
    await vscode.env.clipboard.writeText("");
    answer = "Copy Command";
    await promptToUpdateServer("adl-lsp 0.8.2 is not supported");
    assert.strictEqual(await vscode.env.clipboard.readText(), INSTALL_COMMAND);
  });
});
