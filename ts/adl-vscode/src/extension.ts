import * as v from "vscode";
import {
  DidChangeConfigurationNotification,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";
import {
  checkVersionAndNotify,
  disposeVersionStatus,
  notifyServerFailedToStart,
} from "./check-version";
import { registerCommands } from "./commands";
import { getLspExecutable, getSearchDirs, getStdlibDir } from "./config";

let client: LanguageClient;

export async function activate(context: v.ExtensionContext) {
  console.log("ADL Language Server is starting...");
  const { dev, prod } = getLspExecutable(context.extensionPath);

  // In the Extension Development Host (F5) run the server via `cargo run` from
  // the local checkout; installed builds always use the packaged/`adl.lspPath`
  // binary. Falls back to prod if the dev checkout isn't present.
  const useDev =
    context.extensionMode === v.ExtensionMode.Development &&
    dev.options?.cwd !== undefined;
  const executable = useDev ? dev : prod;
  console.log(
    `Launching ADL Language Server in ${useDev ? "dev (cargo run)" : "prod"} mode: ${executable.command}`,
  );

  const serverOptions: ServerOptions = {
    run: executable,
    debug: executable,
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "adl" }],
  };

  client = new LanguageClient(
    "adl-vscode",
    "ADL Language Server",
    serverOptions,
    clientOptions,
  );

  v.workspace.onDidChangeConfiguration(async (e) => {
    console.log("Configuration changed: ", e);
    if (
      e.affectsConfiguration("adl.searchDirs") ||
      e.affectsConfiguration("adl.stdlibDir")
    ) {
      client.sendNotification(DidChangeConfigurationNotification.type, {
        settings: {
          searchDirs: getSearchDirs(),
          // An empty string tells the server to go back to finding it itself.
          stdlibDir: getStdlibDir() ?? "",
        },
      });
    }
  });

  // Registered before the server starts so the restart and update commands
  // work even when it fails to.
  registerCommands(client, context, executable.command);

  try {
    await client.start();
  } catch (error) {
    notifyServerFailedToStart(executable.command, error);
    return;
  }

  checkVersionAndNotify(client);
}

export function deactivate() {
  disposeVersionStatus();
  if (!client) {
    return undefined;
  }
  return client.stop();
}
