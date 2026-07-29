import * as v from "vscode";
import {
  DidChangeConfigurationNotification,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";
import { checkVersionAndNotify } from "./check-version";
import { registerCommands } from "./commands";
import { getLspExecutable, getSearchDirs } from "./config";

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
    if (e.affectsConfiguration("adl.searchDirs")) {
      client.sendNotification(DidChangeConfigurationNotification.type, {
        settings: {
          searchDirs: getSearchDirs(),
        },
      });
    }
  });

  await client.start();

  const serverVersion = client.initializeResult?.serverInfo?.version;
  console.log("Server version: ", serverVersion);
  checkVersionAndNotify(serverVersion);

  registerCommands(client, context);
}

export function deactivate() {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
