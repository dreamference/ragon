// The app window's side of `ragon app-server` (specs/DREAMFERENCE_RAGON_DESKTOP.md §4.3), the
// pure half.
//
// The window speaks Codex's app-server protocol: one JSON object per line on the server's stdin
// and stdout, without JSON-RPC's `"jsonrpc"` field. Everything the window sends passes the policy
// first (policy.ts, the rules of `ragon-rs/web/policy.json` that `ragon web` enforces too), so a bug
// in the UI cannot reach a method that would replace Ragon's prompt, provider or policy, pair
// the machine with the upstream vendor, or upload anything. Everything the server writes passes
// `classify`, and `BusyTracker` keeps the marker Night Shift reads to know whether a turn is
// running (§8.3). No Electron here, so this tests on its own.

import fs from "node:fs";
import path from "node:path";

type JsonObject = Record<string, unknown>;

/** One thing the server wrote on stdout. */
export type Incoming =
  | { kind: "response" }
  | { kind: "serverRequest"; key: string; method: string }
  | { kind: "notification"; method: string }
  /** A line that is not a protocol message: something wrote where it must not. */
  | { kind: "notProtocol" };

const isObject = (value: unknown): value is JsonObject => typeof value === "object" && value !== null && !Array.isArray(value);

/** Reads a line the server wrote. */
export function classify(line: string): [Incoming, unknown] {
  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch {
    return [{ kind: "notProtocol" }, undefined];
  }
  if (!isObject(value)) return [{ kind: "notProtocol" }, undefined];
  const method = typeof value.method === "string" ? value.method : undefined;
  const hasId = "id" in value;
  if (method !== undefined && hasId) return [{ kind: "serverRequest", key: JSON.stringify(value.id), method }, value];
  if (method !== undefined) return [{ kind: "notification", method }, value];
  if (hasId && ("result" in value || "error" in value)) return [{ kind: "response" }, value];
  return [{ kind: "notProtocol" }, value];
}

/** The threads with a turn running, from the server's own notifications, and the marker that tells Night Shift about them (§8.3). */
export class BusyTracker {
  private readonly threads = new Set<string>();

  /** Notes a notification. Returns true when the set of busy threads changed. */
  observe(method: string, params: unknown): boolean {
    const thread = isObject(params) && typeof params.threadId === "string" ? params.threadId : null;
    if (thread === null) return false;
    switch (method) {
      case "turn/started":
        if (this.threads.has(thread)) return false;
        this.threads.add(thread);
        return true;
      case "turn/completed":
      case "thread/closed":
        return this.threads.delete(thread);
      default:
        return false;
    }
  }

  get isBusy(): boolean {
    return this.threads.size > 0;
  }

  /** The marker's contents: the busy threads, for a person reading the folder. */
  markerText(): string {
    return JSON.stringify({ threads: [...this.threads].sort() });
  }

  /** Writes the marker while a turn runs and removes it when none does. */
  syncMarker(marker: string): void {
    if (this.isBusy) {
      fs.mkdirSync(path.dirname(marker), { recursive: true });
      fs.writeFileSync(marker, this.markerText());
    } else {
      fs.rmSync(marker, { force: true });
    }
  }
}

/** `ragon`'s home folder: `$CODEX_HOME`, else `~/.ragon` (`ragon-rs/src/home.rs`; on Windows the profile folder's). */
export function codexHome(env: NodeJS.ProcessEnv = process.env): string | null {
  if (env.CODEX_HOME) return env.CODEX_HOME;
  const home = env.HOME || env.USERPROFILE;
  return home ? path.join(home, ".ragon") : null;
}

/** Where the busy marker of the app-server with process id `pid` goes; Night Shift reads the folder (`NightShiftHost.busy_app_server_pids`). */
export function busyMarker(home: string, pid: number): string {
  return path.join(home, "night", "busy", String(pid));
}

/** The model the launcher's catalog names (`$CODEX_HOME/model_catalog.json`), which the window passes in every `thread/start` (§5). */
export function servedModel(home: string): string | null {
  try {
    const catalog = JSON.parse(fs.readFileSync(path.join(home, "model_catalog.json"), "utf8"));
    const first = catalog?.models?.[0];
    const name = first?.slug ?? first?.id;
    return typeof name === "string" ? name : null;
  } catch {
    return null;
  }
}

const isFile = (file: string) => {
  try {
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
};

/**
 * The `ragon` executable: `$RAGON_BIN`, then the one bundled beside the app (`resourcesPath`,
 * then the unpacked asar), then `ragon` on PATH, then `~/.local/bin/ragon`. Never a bare `codex`: the
 * launcher is what brings Ragon's model server, prompt and home (§4.3).
 */
export function findRagon(resourcesPath: string | null, env: NodeJS.ProcessEnv = process.env, platform: NodeJS.Platform = process.platform): string | null {
  if (env.RAGON_BIN && isFile(env.RAGON_BIN)) return env.RAGON_BIN;
  // On Windows `ragon.exe`, and where install.ps1 puts it rather than ~/.local/bin (DESKTOP_PARITY Phase 3 item 12).
  const windows = platform === "win32";
  const name = windows ? "ragon.exe" : "ragon";
  if (resourcesPath) {
    for (const candidate of [path.join(resourcesPath, name), path.join(resourcesPath, "app.asar.unpacked", name)]) {
      if (isFile(candidate)) return candidate;
    }
  }
  for (const dir of (env.PATH ?? env.Path ?? "").split(windows ? ";" : path.delimiter).filter(Boolean)) {
    const candidate = path.join(dir, name);
    if (isFile(candidate)) return candidate;
  }
  const local = windows
    ? (env.RAGON_INSTALL_DIR || (env.LOCALAPPDATA ? path.join(env.LOCALAPPDATA, "Programs", "Ragon", "bin") : null))
    : env.HOME ? path.join(env.HOME, ".local", "bin") : null;
  return local && isFile(path.join(local, name)) ? path.join(local, name) : null;
}
