import v from "vscode";
export const REQUIRED_MAJOR_VERSION = 0;
export const REQUIRED_MINOR_VERSION = 7;
export const REQUIRED_PATCH_VERSION = 0;

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

export function checkVersionAndNotify(serverVersion: string | undefined) {
  let checkResult = checkVersion(serverVersion);

  const requiredVersion = `${REQUIRED_MAJOR_VERSION}.${REQUIRED_MINOR_VERSION}.${REQUIRED_PATCH_VERSION}`;
  const requiredVersionMessage = `adl-lsp ${serverVersion} is not supported. Please update to version ${requiredVersion} or later.\nYou can update by running cargo install adl-lsp`;

  if (checkResult !== "version-supported") {
    v.window.showErrorMessage(requiredVersionMessage);
    console.error("checkResult: ", checkResult);
    console.error(requiredVersionMessage);
  }
}
