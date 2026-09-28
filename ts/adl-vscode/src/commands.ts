import v, { ExtensionContext } from "vscode";
import { LanguageClient } from "vscode-languageclient/node";
import {
  RESTART_COMMAND,
  UPDATE_COMMAND,
  checkVersionAndNotify,
  notifyServerFailedToStart,
  promptToUpdateServer,
} from "./check-version";

export function registerCommands(
  client: LanguageClient,
  context: ExtensionContext,
  serverCommand: string
) {
  context.subscriptions.push(
    v.commands.registerCommand(RESTART_COMMAND, async () => {
      try {
        // `restart` only works on a running client; after a failed start the
        // client has to be started instead.
        if (client.isRunning()) {
          await client.restart();
        } else {
          await client.start();
        }
      } catch (error) {
        notifyServerFailedToStart(serverCommand, error);
        return;
      }

      // The restart may have picked up a newly installed server.
      if (checkVersionAndNotify(client)) {
        const version = client.initializeResult?.serverInfo?.version;
        v.window.showInformationMessage(
          `Restarted ADL Language Server (adl-lsp ${version})`
        );
      }
    }),
    v.commands.registerCommand(UPDATE_COMMAND, () => promptToUpdateServer())
  );
}
