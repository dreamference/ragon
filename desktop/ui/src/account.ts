// The desktop window's own account (docs/dev/desktop.md, "Accounts"; specs/DREAMFERENCE_AUTH.md
// §8.3): which of the three faces the window shows (create the first account, sign in, the account
// with sign out), the terminal flow's checks in its order (the name, the passwords twice, then
// "continue without a password?"), and the error codes in the user's words. The words are the
// launcher's string table (ragon-rs/src/auth/strings.rs) that `ragon login status --json` sends in
// the user's language, so the window says what the terminal says, in each language the app ships.
// No React and no bridge here: AccountGate.tsx draws this, account.test.ts tests it.

/** `ragon login status --json` (desktop/electron/src/account.ts `AccountState`). */
export interface AccountState {
  lang: string;
  text: Record<string, string>;
  reachable: boolean;
  accounts: boolean;
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

/** What the window shows of the account. */
export type AccountView =
  /** Accounts off, an API key in use, or nothing known: no gate and no chip. */
  | { kind: "none" }
  /** Accounts on, no account yet, and this machine is the node: the first account (the admin). */
  | { kind: "createFirst" }
  /** No account yet but the node is another machine: its first account is made there. */
  | { kind: "remoteSetup" }
  /** Accounts on and signed out. */
  | { kind: "signIn" }
  /** The node could not be asked (a changed certificate, say): its words, Try again or Not now. */
  | { kind: "unreachable"; error: string }
  | { kind: "signedIn"; user: string; role: string | null };

export function accountView(state: AccountState | null): AccountView {
  if (!state) return { kind: "none" };
  if (state.signedIn && state.user) return { kind: "signedIn", user: state.user, role: state.role };
  if (state.reachable && state.accounts) {
    if (state.apiKey) return { kind: "none" };
    if (state.setup) return state.local ? { kind: "createFirst" } : { kind: "remoteSetup" };
    return { kind: "signIn" };
  }
  if (!state.reachable && state.error) return { kind: "unreachable", error: state.error };
  return { kind: "none" };
}

/** Whether the face is a dialog over the window (the signed-in face is a chip in the header). */
export const isGate = (view: AccountView) => view.kind !== "none" && view.kind !== "signedIn";

/** The face that cannot be put away: the agent answers nobody until someone signs in. */
export const dismissible = (view: AccountView) => view.kind === "remoteSetup" || view.kind === "unreachable";

/** The English table, for a launcher too old to send its own and for a key it lacks. */
export const ENGLISH: Record<string, string> = {
  AccountLabel: "Account",
  AccountsUnreachable: "The accounts server cannot be reached.",
  BackAction: "Back",
  BadName: "Usernames are 1-32 letters, digits, '.', '_' or '-'.",
  ContinueAction: "Continue",
  CreateAction: "Create account",
  CreateFirstHint: "The first account is the admin of {0}. Choose a username and a password.",
  CreateFirstTitle: "Create the first account on {0}",
  CreateInstead: "Create an account",
  CreateTitle: "Create an account on {0}",
  CreateHint: "Choose a username and a password",
  EmptyPasswordAsk: "Continue without a password?",
  FieldConfirmPassword: "Confirm password",
  FieldPassword: "Password",
  FieldUsername: "Username",
  FirstAccountOnNode: "{0} has no accounts yet. Its first account is made on {0} itself: run ragon in a terminal there.",
  NameTaken: "That username is taken.",
  NotNowAction: "Not now",
  PasswordMismatch: "The passwords do not match.",
  RetryAction: "Try again",
  SignInAction: "Sign in",
  SignInHint: "With the username and password of your account",
  SignInInstead: "I already have an account",
  SignInTitle: "Sign in to {0}",
  SignOutAction: "Sign out",
  SignedInAs: "Signed in as {0} ({1})",
  SignedInOnly: "Signed in to {0}",
  SigningIn: "Signing in…",
  WrongCredentials: "Wrong username or password.",
};

/** A text of the table with `{0}`, `{1}`… filled in, English when the table lacks it. */
export function say(text: Record<string, string>, key: string, ...args: string[]): string {
  const template = text[key] ?? ENGLISH[key] ?? key;
  return template.replace(/\{(\d)\}/g, (whole, index: string) => args[Number(index)] ?? whole);
}

/** `[A-Za-z0-9._-]{1,32}`, the store's rule (screen.rs `valid_username`). */
export const validUsername = (name: string) => /^[A-Za-z0-9._-]{1,32}$/.test(name);

/** Which form the dialog shows: the first account and an account of one's own are both `create`. */
export type FormMode = "signIn" | "create";

export interface FormFields {
  username: string;
  password: string;
  confirm: string;
}

/** The field a check sends the focus back to. */
export type FieldName = keyof FormFields;

export type FormCheck =
  | { kind: "error"; key: string; field: FieldName }
  /** An empty password on a new account: asked once more, as the terminal asks. */
  | { kind: "askEmpty"; username: string }
  | { kind: "send"; username: string };

/** The terminal's checks in its order (screen.rs `submit`): the name, the passwords twice, the empty password. */
export function checkForm(mode: FormMode, fields: FormFields): FormCheck {
  const username = fields.username.trim();
  if (!validUsername(username)) return { kind: "error", key: "BadName", field: "username" };
  if (mode === "create" && fields.password !== fields.confirm) return { kind: "error", key: "PasswordMismatch", field: "password" };
  if (mode === "create" && fields.password === "") return { kind: "askEmpty", username };
  return { kind: "send", username };
}

/** desktop/electron/src/login.ts: the server's codes (signin_rpc.rs). */
export const WRONG_CREDENTIALS = -32001;
export const NAME_TAKEN = -32002;
export const UNREACHABLE = -32003;

/** What `account/signIn` resolves with (login.ts `SignInResult`). */
export type SignInAnswer =
  | { ok: true; username: string; role: string | null; mustChangePassword: boolean }
  | { ok: false; code: number | null; message: string };

/** A refusal in the user's words: a known code by the table, any other in the server's words. */
export function refusalText(text: Record<string, string>, answer: { code: number | null; message: string }): string {
  if (answer.code === WRONG_CREDENTIALS) return say(text, "WrongCredentials");
  if (answer.code === NAME_TAKEN) return say(text, "NameTaken");
  if (answer.code === UNREACHABLE) return `${say(text, "AccountsUnreachable")} ${answer.message}`.trim();
  return answer.message;
}

/** The node's name for the titles: the primary the window signs in to, else the gate's label. */
export const nodeName = (state: AccountState) => state.primary ?? state.node ?? "";

/** The header chip's words: "Signed in as ann (admin)". */
export function chipText(state: AccountState, view: Extract<AccountView, { kind: "signedIn" }>): string {
  return view.role ? say(state.text, "SignedInAs", view.user, view.role) : view.user;
}
