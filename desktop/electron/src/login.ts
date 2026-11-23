// The desktop app's two-choice sign-in form (specs/DREAMFERENCE_RAGON_AUTH.md §5.8): "sign in" or
// "create an account", both answered by the app-server's `account/ragon/signIn` request (patch
// 0042), which saves into the active entry and reloads the server's auth, so the server is not
// restarted and a thread that is open stays open. The request is the app's own (`AppServer.request`):
// it is not on the policy shared with `ragon web`, since a browser has no business sending a password
// this way. Name and password go through as typed, untrimmed (the window checks the name first, as
// the terminal form does; policy.json's `desktopAccount` names the fields). The error's code is
// passed on with the server's words: the window says it in the user's language.
// No Electron here, so this tests on its own.

import type { AppServer } from "./server";

export type SignInMode = "signIn" | "create";

/** What the page learns of a sign-in: who the user is now, or what to say. */
export type SignInResult =
  | { ok: true; username: string; role: string; mustChangePassword: boolean }
  | { ok: false; code: number | null; message: string };

/** The server's error codes (signin_rpc.rs); the window puts them in the user's language. */
export const WRONG_CREDENTIALS = -32001;
export const NAME_TAKEN = -32002;
export const UNREACHABLE = -32003;

/** The request's parameters: the fields verbatim. */
export function signInParams(mode: SignInMode, username: string, password: string): { mode: SignInMode; username: string; password: string } {
  return { mode, username, password };
}

/** Reads the server's answer to `account/ragon/signIn`. */
export function readSignIn(answer: unknown): SignInResult {
  const value = (answer ?? {}) as { result?: Record<string, unknown>; error?: { code?: unknown; message?: unknown } };
  if (value.error) {
    const code = typeof value.error.code === "number" ? value.error.code : null;
    return { ok: false, code, message: typeof value.error.message === "string" ? value.error.message : "Signing in failed." };
  }
  const result = value.result;
  if (result && result.type === "ragon" && typeof result.username === "string") {
    return { ok: true, username: result.username, role: String(result.role ?? ""), mustChangePassword: result.mustChangePassword === true };
  }
  return { ok: false, code: null, message: "The server's answer was not understood." };
}

/** Signs in (or creates the account) through the running app-server; never throws. */
export async function signIn(server: Pick<AppServer, "request">, mode: SignInMode, username: string, password: string): Promise<SignInResult> {
  try {
    return readSignIn(await server.request("account/ragon/signIn", signInParams(mode, username, password)));
  } catch (error) {
    return { ok: false, code: null, message: error instanceof Error ? error.message : String(error) };
  }
}
