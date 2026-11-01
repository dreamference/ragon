// The app's data folder (Electron's `userData`: Chromium's profile, the window state, the
// preferences), by one rule with `ragon app`, which empties its `Cache` (ragon-rs/src/app.rs,
// `data_dir_for`); both are tested against ragon-rs/vectors/app-data-folder.json.
//
// On Windows it is `%LOCALAPPDATA%\<identifier>` (`%USERPROFILE%\AppData\Local\<identifier>`
// without the variable), where a Windows app keeps its own data. Elsewhere it is
// `~/.local/share/<identifier>`, where `ragon app` and `ragon-admin desktop` expect it. No Electron here, so the
// tests run the rule for every platform on any machine.

/** The environment variables the rule reads. */
export type Env = Record<string, string | undefined>;

function set(env: Env, name: string): string | null {
  const value = env[name];
  return value ? value : null;
}

/** `base` without separators at its end, then `parts`, joined with the platform's separator. */
function joined(base: string, windows: boolean, parts: string[]): string {
  const trimmed = base.replace(windows ? /[\\/]+$/ : /\/+$/, "");
  return [trimmed, ...parts].join(windows ? "\\" : "/");
}

/** The app's data folder on `platform` (a `process.platform` value) with `env`, or null without a home. */
export function dataFolder(platform: string, env: Env, identifier: string): string | null {
  if (platform === "win32") {
    const local = set(env, "LOCALAPPDATA");
    if (local) return joined(local, true, [identifier]);
    const profile = set(env, "USERPROFILE");
    return profile ? joined(profile, true, ["AppData", "Local", identifier]) : null;
  }
  const home = set(env, "HOME") ?? set(env, "USERPROFILE");
  return home ? joined(home, false, [".local", "share", identifier]) : null;
}
