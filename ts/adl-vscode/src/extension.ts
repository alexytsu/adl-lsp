import * as v from "vscode";
import {
  DidChangeConfigurationNotification,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";
import { checkVersionAndNotify } from "./check-version";
import { registerCommands } from "./commands";
import { getLspExecutable, getPackageRoots } from "./config";

let client: LanguageClient;

export async function activate(context: v.ExtensionContext) {
  console.log("ADL Language Server is starting...");

  const { dev, prod } = getLspExecutable();

  const serverOptions: ServerOptions = {
    run: prod,
    debug: prod,
  };

  // const serverOptions: ServerOptions = {
  //   run: dev,
  //   debug: dev,
  // };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "adl" }],
  };

  client = new LanguageClient(
    "adl-vscode",
    "ADL Language Server",
    serverOptions,
    clientOptions
  );

  v.workspace.onDidChangeConfiguration(async (e) => {
    console.log("Configuration changed: ", e);
    if (e.affectsConfiguration("adl.packageRoots")) {
      client.sendNotification(DidChangeConfigurationNotification.type, {
        settings: {
          packageRoots: getPackageRoots(),
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
