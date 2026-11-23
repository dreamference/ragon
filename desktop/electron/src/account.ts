// The desktop window's own account (docs/dev/desktop.md, "Accounts"; specs/DREAMFERENCE_AUTH.md
// §8.3): what the window shows comes from `ragon login status --json`, the launcher's reading of the
// same auth.json the agent's server reads, with the string table in the user's language; a sign-in
// or a first account goes through the app-server's `account/ragon/signIn` (login.ts), a sign-out
// through its `account/logout`, so the running server reloads its auth and no thread closes.
//
// The window's messages are vetted against ragon-rs/web/policy.json (`desktopAccount`) before they
// reach here; the password is only ever in the request's body on the server's stdin, never in an
// argument, a log line or an answer. No Electron here, so this tests on its own.

import { execFile } from "node:child_process";

import type { AppServer } from "./server";
import { command } from "./platform";

/** What the window is drawn from. */
export interface AccountState {
  /** The language of `text`, a two-letter code. */
  lang: string;
  /** The launcher's string table in `lang` (strings.rs), placeholders `{0}`… unfilled. */
  text: Record<string, string>;
  /** The gate answered its info page. */
  reachable: boolean;
  /** The node runs accounts; without them the window shows nothing of this. */
  accounts: boolean;
  /** The node still needs its first account, and `local`: this machine is the node itself. */
  setup: boolean;
  local: boolean;
  node: string | null;
  primary: string | null;
  signedIn: boolean;
  user: string | null;
  role: string | null;
  apiKey: boolean;
  error: string | null;
}

/** `ragon login status --json` asks the node once; a slow gate waits ten seconds. */
const TIMEOUT_MS = 20_000;

const str = (value: unknown): string | null => (typeof value === "string" ? value : null);

/** The state's JSON: the last line of the output that is an object (a notice may come first). */
export function parseState(stdout: string): AccountState {
  const line = stdout.split("\n").map((text) => text.trim()).filter((text) => text.startsWith("{")).pop();
  if (!line) throw new Error("ragon login status --json printed nothing: this ragon is older than the app");
  const value = JSON.parse(line) as Record<string, unknown>;
  const text: Record<string, string> = {};
  if (value.text && typeof value.text === "object") {
    for (const [key, template] of Object.entries(value.text as Record<string, unknown>)) {
      if (typeof template === "string") text[key] = template;
    }
  }
  return {
    lang: str(value.lang) ?? "en",
    text,
    reachable: value.reachable === true,
    accounts: value.accounts === true,
    setup: value.setup === true,
    local: value.local === true,
    node: str(value.node),
    primary: str(value.primary),
    signedIn: value.signedIn === true,
    user: str(value.user),
    role: str(value.role),
    apiKey: value.apiKey === true,
    error: str(value.error),
  };
}

/**
 * The launcher's environment with the app's language when the user's own environment names none:
 * a desktop app started from the dock or the start menu has no `LANG`.
 */
export function withLocale(env: NodeJS.ProcessEnv, locale: string): NodeJS.ProcessEnv {
  if (["LC_ALL", "LC_MESSAGES", "LANG"].some((name) => (env[name] ?? "") !== "")) return env;
  const lang = /^[A-Za-z]{2,3}/.exec(locale)?.[0]?.toLowerCase();
  return lang ? { ...env, LANG: `${lang}.UTF-8` } : env;
}

/** Runs `ragon` with arguments only (never a password); resolves with what it printed. */
export type Run = (ragon: string, args: string[], env: NodeJS.ProcessEnv) => Promise<string>;

export const runRagon: Run = (ragon, args, env) =>
  new Promise((resolve, reject) => {
    const started = command(ragon, args);
    execFile(started.file, started.args, { env, timeout: TIMEOUT_MS, shell: started.shell, windowsHide: true }, (error, stdout, stderr) => {
      if (error) reject(new Error(String(stderr || stdout || error.message).trim()));
      else resolve(String(stdout));
    });
  });

/** The window's state: `ragon login status --json` in the app's language. */
export async function accountState(ragon: string, locale: string, run: Run = runRagon, env: NodeJS.ProcessEnv = process.env): Promise<AccountState> {
  return parseState(await run(ragon, ["login", "status", "--json"], withLocale(env, locale)));
}

/**
 * Signs out: the running server's `account/logout` (it reloads its auth and tells every window with
 * `account/updated`), else `ragon logout` when no server runs. Resolves with whether it worked.
 */
export async function signOut(server: Pick<AppServer, "request">, ragon: string | null, run: Run = runRagon, env: NodeJS.ProcessEnv = process.env): Promise<{ ok: boolean; message: string | null }> {
  try {
    const answer = (await server.request("account/logout", undefined)) as { error?: { message?: unknown } } | undefined;
    if (answer?.error) return { ok: false, message: str(answer.error.message) ?? "Signing out failed." };
    return { ok: true, message: null };
  } catch (error) {
    if (!ragon) return { ok: false, message: error instanceof Error ? error.message : String(error) };
    try {
      await run(ragon, ["logout"], env);
      return { ok: true, message: null };
    } catch (failed) {
      return { ok: false, message: failed instanceof Error ? failed.message : String(failed) };
    }
  }
}
