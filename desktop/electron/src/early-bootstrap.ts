// The entry point (`main` in package.json), as the Codex app's `early-bootstrap.js`: everything
// that must happen before Electron is ready — the privileged scheme, the switches that keep
// Chromium quiet on the network, the name and data folder — then `bootstrap`. `--version` comes
// first and is all that runs (src/version.ts).
//
// No GPU switches: Electron's defaults, as the Codex app runs
// (specs/DREAMFERENCE_RAGON_DESKTOP_ELECTRON.md §3).

import fs from "node:fs";
import { app } from "electron";

import appConfig from "../app.json";
import { registerScheme } from "./app-protocol";
import { bootstrap } from "./bootstrap";
import { dataFolder } from "./data-folder";
import { applySwitches } from "./egress";
import { versionText, wantsVersion } from "./version";

function start(): void {
  applySwitches();
  registerScheme();

  // The data folder is `ragon app`'s rule (data-folder.ts): `~/.local/share/<identifier>`, or
  // `%LOCALAPPDATA%\<identifier>` on Windows.
  app.setName(appConfig.productName);
  const data = dataFolder(process.platform, process.env, appConfig.identifier);
  if (data) app.setPath("userData", data);

  app.whenReady().then(bootstrap).catch((error) => {
    console.error(`ragon-app: ${error instanceof Error ? error.message : String(error)}`);
    app.exit(1);
  });
}

if (wantsVersion(process.argv.slice(1))) {
  // Written synchronously: app.exit does not wait for a pending stdout write.
  fs.writeSync(1, versionText(appConfig.productName, app.getVersion(), app.isPackaged ? process.resourcesPath : null));
  app.exit(0);
} else {
  start();
}
