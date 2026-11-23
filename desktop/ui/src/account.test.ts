// The window's own account (account.ts, AccountGate.tsx): which face each state shows, the terminal
// flow's checks in its order, the refusals in the user's words, every word in every language the
// launcher ships (its string table, ragon-rs/tests/fixtures/account-text.json), and the faces as
// the test renderer draws them: a labelled modal dialog, the error announced at its field, no
// password in the markup. With RAGON_ACCOUNT_SHOTS=<dir> it also writes each face as a page with
// the window's stylesheet, for the screenshots of the report.
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import {
  accountView, checkForm, dismissible, ENGLISH, isGate, refusalText, say, validUsername,
  type AccountState, type AccountView, type SignInAnswer,
} from "./account";
import { AccountChip, AccountGate, pressSend, wrapFocus, type FormStatus, type Submit } from "./AccountGate";
import gateSource from "./AccountGate.tsx?raw";
import accountSource from "./account.ts?raw";
import tableText from "../../../ragon-rs/tests/fixtures/account-text.json?raw";
import css from "./styles.css?raw";

const TABLES = JSON.parse(tableText) as Record<string, Record<string, string>>;
const LANGS = ["en", "pt", "es", "de", "fr"];

const state = (over: Partial<AccountState> = {}, lang = "en"): AccountState => ({
  lang, text: TABLES[lang], reachable: true, accounts: true, setup: false, local: true, node: "main-box", primary: "main-box",
  signedIn: false, user: null, role: null, apiKey: false, error: null, ...over,
});

describe("which face the window shows", () => {
  it("the first account, sign in, the account, or nothing", () => {
    expect(accountView(null)).toEqual({ kind: "none" });
    expect(accountView(state({ setup: true }))).toEqual({ kind: "createFirst" });
    expect(accountView(state({ setup: true, local: false }))).toEqual({ kind: "remoteSetup" });
    expect(accountView(state())).toEqual({ kind: "signIn" });
    expect(accountView(state({ signedIn: true, user: "ann", role: "admin" }))).toEqual({ kind: "signedIn", user: "ann", role: "admin" });
    // A sign-in kept from before shows while the node is away.
    expect(accountView(state({ reachable: false, accounts: false, signedIn: true, user: "ann", role: "user" }))).toMatchObject({ kind: "signedIn" });
    expect(accountView(state({ accounts: false }))).toEqual({ kind: "none" });
    expect(accountView(state({ apiKey: true }))).toEqual({ kind: "none" });
    expect(accountView(state({ reachable: false, accounts: false }))).toEqual({ kind: "none" });
    expect(accountView(state({ reachable: false, accounts: false, error: "The certificate of main-box has changed." }))).toEqual({
      kind: "unreachable", error: "The certificate of main-box has changed.",
    });
  });

  it("puts away only the notes, never the first account or the sign-in", () => {
    const faces: AccountView[] = [{ kind: "createFirst" }, { kind: "signIn" }, { kind: "remoteSetup" }, { kind: "unreachable", error: "x" }];
    expect(faces.map(isGate)).toEqual([true, true, true, true]);
    expect(faces.map(dismissible)).toEqual([false, false, true, true]);
    expect(isGate({ kind: "signedIn", user: "ann", role: null })).toBe(false);
    expect(isGate({ kind: "none" })).toBe(false);
  });
});

describe("the words", () => {
  const USED = new Set(
    [accountSource, gateSource].flatMap((source) =>
      [...source.matchAll(/say\([^,]+, (?:create \? |first \? )?"(\w+)"(?: : "(\w+)")?/g)].flatMap((m) => [m[1], m[2]]).filter(Boolean),
    ),
  );

  it("every word the window says is in every language the launcher ships", () => {
    expect(Object.keys(TABLES).sort()).toEqual([...LANGS].sort());
    expect(USED.size).toBeGreaterThanOrEqual(15);
    for (const key of [...USED, ...Object.keys(ENGLISH)]) {
      for (const lang of LANGS) expect(TABLES[lang][key], `${lang} ${key}`).toBeTruthy();
    }
    for (const lang of LANGS.filter((code) => code !== "en")) {
      expect(TABLES[lang].SignInAction).not.toBe(TABLES.en.SignInAction);
      expect(TABLES[lang].CreateFirstTitle).not.toBe(TABLES.en.CreateFirstTitle);
    }
  });

  it("the English fallback is the launcher's English", () => {
    for (const [key, text] of Object.entries(ENGLISH)) expect(text, key).toBe(TABLES.en[key]);
  });

  it("fills the placeholders and falls back to English", () => {
    expect(say(TABLES.de, "CreateFirstTitle", "main-box")).toContain("main-box");
    expect(say(TABLES.fr, "SignedInAs", "ann", "admin")).toContain("ann");
    expect(say({}, "SignInTitle", "box")).toBe("Sign in to box");
    expect(say({}, "NoSuchKey")).toBe("NoSuchKey");
  });

  it("says a refusal by its code, any other in the server's words", () => {
    expect(refusalText(TABLES.de, { code: -32001, message: "Wrong username or password." })).toBe(TABLES.de.WrongCredentials);
    expect(refusalText(TABLES.pt, { code: -32002, message: "That username is taken." })).toBe(TABLES.pt.NameTaken);
    expect(refusalText(TABLES.es, { code: -32003, message: "main-box answered 503." })).toBe(`${TABLES.es.AccountsUnreachable} main-box answered 503.`);
    expect(refusalText(TABLES.en, { code: null, message: "the agent's server is not running" })).toBe("the agent's server is not running");
  });
});

describe("the form's checks, in the terminal's order", () => {
  it("the name, then the passwords twice, then the empty password", () => {
    expect(validUsername("ann.b_c-1")).toBe(true);
    expect(["", "a".repeat(33), "ann b", "änn", "ann@box"].map(validUsername)).toEqual([false, false, false, false, false]);
    expect(validUsername("a".repeat(32))).toBe(true);
    expect(checkForm("create", { username: "bad name", password: "a", confirm: "b" })).toEqual({ kind: "error", key: "BadName", field: "username" });
    expect(checkForm("create", { username: "ann", password: "a", confirm: "b" })).toEqual({ kind: "error", key: "PasswordMismatch", field: "password" });
    expect(checkForm("create", { username: "ann", password: "", confirm: "" })).toEqual({ kind: "askEmpty", username: "ann" });
    expect(checkForm("create", { username: " ann ", password: "pw", confirm: "pw" })).toEqual({ kind: "send", username: "ann" });
    // Signing in has no second field and asks nothing of an empty password.
    expect(checkForm("signIn", { username: "ann", password: "", confirm: "other" })).toEqual({ kind: "send", username: "ann" });
    expect(checkForm("signIn", { username: "", password: "pw", confirm: "" })).toMatchObject({ kind: "error", key: "BadName" });
  });
});

describe("the keyboard in the dialog", () => {
  it("Tab and Shift+Tab stay inside it", () => {
    const items = ["username", "password", "send", "switch"];
    expect(wrapFocus(items, "switch", false)).toBe("username");
    expect(wrapFocus(items, "username", true)).toBe("switch");
    expect(wrapFocus(items, "password", false)).toBeNull();
    expect(wrapFocus(items, "password", true)).toBeNull();
    // The focus is on the page behind (the window's own controls): back into the dialog.
    expect(wrapFocus(items, "sidebar", false)).toBe("username");
    expect(wrapFocus(items, "sidebar", true)).toBe("switch");
    expect(wrapFocus([], "sidebar", false)).toBeNull();
  });
});

describe("pressing the dialog's button", () => {
  const fresh = (mode: FormStatus["mode"]): FormStatus => ({ mode, error: null, field: null, askEmpty: false, busy: true });
  const ok: SignInAnswer = { ok: true, username: "ann", role: "admin", mustChangePassword: false };

  it("stops at a check before anything is sent", async () => {
    const submit = vi.fn<Submit>();
    const bad = await pressSend(TABLES.de, fresh("create"), { username: "ann!", password: "pw", confirm: "pw" }, false, submit);
    expect(bad).toEqual({ status: { mode: "create", error: TABLES.de.BadName, field: "username", askEmpty: false, busy: false }, clear: false, done: false });
    const mismatch = await pressSend(TABLES.fr, fresh("create"), { username: "ann", password: "pw", confirm: "pW" }, false, submit);
    expect(mismatch.status).toMatchObject({ error: TABLES.fr.PasswordMismatch, field: "password" });
    expect(submit).not.toHaveBeenCalled();
  });

  it("asks once about an empty password, then sends it", async () => {
    const submit = vi.fn<Submit>(async () => ok);
    const asked = await pressSend(TABLES.en, fresh("create"), { username: "ann", password: "", confirm: "" }, false, submit);
    expect(asked.status.askEmpty).toBe(true);
    expect(submit).not.toHaveBeenCalled();
    const sent = await pressSend(TABLES.en, asked.status, { username: "ann", password: "", confirm: "" }, true, submit);
    expect(submit).toHaveBeenCalledWith("create", "ann", "");
    expect(sent).toMatchObject({ clear: true, done: true, status: { askEmpty: false, error: null } });
  });

  it("sends the first account, the trimmed name and the password once", async () => {
    const submit = vi.fn<Submit>(async () => ok);
    const done = await pressSend(TABLES.en, fresh("create"), { username: " ann ", password: "s3cret-pw", confirm: "s3cret-pw" }, false, submit);
    expect(submit.mock.calls).toEqual([["create", "ann", "s3cret-pw"]]);
    expect(done).toEqual({ status: { mode: "create", error: null, field: null, askEmpty: false, busy: false }, clear: true, done: true });
  });

  it("empties the passwords after a refusal and points at the field", async () => {
    const wrong = await pressSend(TABLES.es, fresh("signIn"), { username: "ann", password: "nope", confirm: "" }, false, async () => ({ ok: false, code: -32001, message: "Wrong username or password." }));
    expect(wrong).toEqual({ status: { mode: "signIn", error: TABLES.es.WrongCredentials, field: "password", askEmpty: false, busy: false }, clear: true, done: false });
    const taken = await pressSend(TABLES.en, fresh("create"), { username: "ann", password: "pw", confirm: "pw" }, false, async () => ({ ok: false, code: -32002, message: "That username is taken." }));
    expect(taken.status).toMatchObject({ error: "That username is taken.", field: "username" });
    expect(taken.clear).toBe(true);
    const down = await pressSend(TABLES.en, fresh("signIn"), { username: "ann", password: "pw", confirm: "" }, false, async () => { throw new Error("the agent's server is not running"); });
    expect(down.status.error).toBe("the agent's server is not running");
    expect(JSON.stringify([wrong, taken, down])).not.toMatch(/nope|"pw"/);
  });
});

const noop = () => {};
const gate = (s: AccountState, initial?: Partial<FormStatus>) =>
  renderToStaticMarkup(createElement(AccountGate, { state: s, view: accountView(s), submit: async (): Promise<SignInAnswer> => ({ ok: false, code: null, message: "" }), onSignedIn: noop, onRetry: noop, onDismiss: noop, initial }));
const chip = (s: AccountState) => {
  const view = accountView(s);
  if (view.kind !== "signedIn") throw new Error("not signed in");
  return renderToStaticMarkup(createElement(AccountChip, { state: s, view, onSignOut: noop }));
};

describe("the faces as drawn", () => {
  it("the first account: a labelled dialog with the name and the password twice", () => {
    const html = gate(state({ setup: true }, "de"));
    expect(html).toContain('role="dialog"');
    expect(html).toContain('aria-modal="true"');
    expect(html).toContain('aria-labelledby="account-title"');
    expect(html).toContain(`<h2 id="account-title">${say(TABLES.de, "CreateFirstTitle", "main-box")}</h2>`);
    expect(html).toContain(TABLES.de.FieldUsername);
    expect(html).toContain(TABLES.de.FieldConfirmPassword);
    expect(html.match(/type="password"/g)).toHaveLength(2);
    expect(html.match(/autoComplete="new-password"|autocomplete="new-password"/g)).toHaveLength(2);
    expect(html).toContain(TABLES.de.CreateAction);
    expect(html).not.toContain(TABLES.de.NotNowAction);
    expect(html).not.toContain(TABLES.de.SignInInstead);
  });

  it("sign in: one password, the current one, and the way to a new account", () => {
    const html = gate(state({}, "fr"));
    expect(html).toContain(say(TABLES.fr, "SignInTitle", "main-box"));
    expect(html.match(/type="password"/g)).toHaveLength(1);
    expect(html).toMatch(/autocomplete="current-password"/i);
    expect(html).toContain(TABLES.fr.CreateInstead);
    expect(html).not.toContain(TABLES.fr.NotNowAction);
  });

  it("an error is announced at its field and no password is ever in the markup", () => {
    const html = gate(state({}, "en"), { error: TABLES.en.WrongCredentials, field: "password" });
    expect(html).toContain(`<p id="account-error" class="account-error" role="alert">${TABLES.en.WrongCredentials}</p>`);
    expect(html).toMatch(/id="account-password"[^>]*aria-invalid="true"[^>]*aria-describedby="account-error"/);
    expect(html).not.toMatch(/id="account-username"[^>]*aria-invalid/);
    for (const input of html.match(/<input[^>]*>/g) ?? []) expect(input).toContain('value=""');
  });

  it("the empty-password question and the notes", () => {
    expect(gate(state({ setup: true }), { mode: "create", askEmpty: true })).toContain(`<p id="account-ask">${TABLES.en.EmptyPasswordAsk}</p>`);
    const remote = gate(state({ setup: true, local: false }, "pt"));
    expect(remote).toContain(say(TABLES.pt, "FirstAccountElsewhere", "main-box").replace(/</g, "&lt;").replace(/>/g, "&gt;"));
    expect(remote).toContain(TABLES.pt.NotNowAction);
    expect(remote).not.toContain("<input");
    const away = gate(state({ reachable: false, accounts: false, error: "The certificate of main-box has changed." }));
    expect(away).toContain('role="alert"');
    expect(away).toContain(TABLES.en.RetryAction);
  });

  it("signed in: the account's name and Sign out", () => {
    const html = chip(state({ signedIn: true, user: "ann", role: "admin" }, "es"));
    expect(html).toContain(say(TABLES.es, "SignedInAs", "ann", "admin"));
    expect(html).toContain(`>${TABLES.es.SignOutAction}</button>`);
    expect(html).toContain(`aria-label="${TABLES.es.AccountLabel}"`);
  });

  // The window has no Node types: the shots' writer takes Node's fs at run time, by name.
  const shots = import.meta.env.RAGON_ACCOUNT_SHOTS as string | undefined;
  it.skipIf(!shots)("writes each face as a page for the screenshots", async () => {
    const dir = shots as string;
    const fsName = "node:fs";
    const fs = (await import(/* @vite-ignore */ fsName)) as {
      mkdirSync(dir: string, options: { recursive: boolean }): void;
      writeFileSync(file: string, text: string): void;
      readdirSync(dir: string): string[];
    };
    fs.mkdirSync(dir, { recursive: true });
    const page = (body: string) => `<!doctype html><html><head><meta charset="utf-8"><style>${css}</style></head><body><div id="root">${body}</div></body></html>`;
    const header = (inner: string) => `<header class="topbar" style="display:flex;gap:8px;padding:10px 14px;justify-content:flex-end">${inner}</header>`;
    const faces: Record<string, string> = {
      "1-create-first": gate(state({ setup: true })),
      "1-create-first-de": gate(state({ setup: true }, "de")),
      "2-sign-in": gate(state()),
      "2-sign-in-error": gate(state(), { error: TABLES.en.WrongCredentials, field: "password" }),
      "3-signed-in": header(chip(state({ signedIn: true, user: "ann", role: "admin" }))),
    };
    for (const [name, body] of Object.entries(faces)) fs.writeFileSync(`${dir}/${name}.html`, page(body));
    expect(fs.readdirSync(dir).filter((file) => file.endsWith(".html"))).toHaveLength(5);
  });
});
