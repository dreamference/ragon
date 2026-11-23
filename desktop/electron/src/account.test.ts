// The desktop window's own account (account.ts): the state from a scripted `ragon login status
// --json`, the app's language when the environment names none, the sign-out through the server or
// `ragon logout`, and a sentinel password that reaches the server's stdin and nowhere else (no
// argument, no log line, no window, no answer).

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { accountState, parseState, runRagon, signOut, withLocale, type Run } from "./account";
import { signIn } from "./login";
import { vetDesktopAccount } from "./policy";
import { AppServer } from "./server";
import { nodeScript } from "./test-ragon";

const STATE = {
  lang: "de", text: { SignInAction: "Anmelden", CreateFirstTitle: "Das erste Konto auf {0} anlegen" }, reachable: true, accounts: true,
  setup: true, local: true, node: "box", primary: "box", signedIn: false, user: null, role: null, apiKey: false, error: null,
};

describe("the window's account state", () => {
  it("reads the last JSON line and keeps only what it knows", () => {
    const state = parseState(`a notice\n${JSON.stringify({ ...STATE, text: { ...STATE.text, Odd: 3 }, extra: "x" })}\n`);
    expect(state).toEqual({ ...STATE, text: STATE.text });
    expect(parseState("{}")).toMatchObject({ lang: "en", text: {}, accounts: false, signedIn: false, user: null });
    expect(() => parseState("Signed in as ann\n")).toThrow(/older than the app/);
  });

  it("gives the launcher the app's language only when the environment names none", () => {
    expect(withLocale({ PATH: "/bin" }, "de-DE")).toEqual({ PATH: "/bin", LANG: "de.UTF-8" });
    expect(withLocale({ LANG: "" , LC_ALL: "" }, "pt-BR").LANG).toBe("pt.UTF-8");
    expect(withLocale({ LANG: "fr_FR.UTF-8" }, "de-DE").LANG).toBe("fr_FR.UTF-8");
    expect(withLocale({ LC_MESSAGES: "es_ES" }, "de-DE")).toEqual({ LC_MESSAGES: "es_ES" });
    expect(withLocale({}, "")).toEqual({});
  });

  it("asks `ragon login status --json`, nothing else", async () => {
    const calls: Array<[string[], string | undefined]> = [];
    const run: Run = async (_ragon, args, env) => {
      calls.push([args, env.LANG]);
      return JSON.stringify(STATE);
    };
    expect(await accountState("ragon", "de-DE", run, {})).toEqual(STATE);
    expect(calls).toEqual([[["login", "status", "--json"], "de.UTF-8"]]);
  });

  it.skipIf(process.platform === "win32")("runs a real launcher with arguments and the language", async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ragon-account-"));
    try {
      const ragon = nodeScript(dir, "ragon", `process.stdout.write(JSON.stringify({ argv: process.argv.slice(2), lang: process.env.LANG }) + "\\n");`);
      const printed = JSON.parse(await runRagon(ragon, ["login", "status", "--json"], withLocale({ PATH: process.env.PATH }, "fr-FR")));
      expect(printed).toEqual({ argv: ["login", "status", "--json"], lang: "fr.UTF-8" });
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("signing out", () => {
  it("asks the running server, which tells every window", async () => {
    const asked: Array<[string, unknown]> = [];
    const server = { request: async (method: string, params: unknown) => { asked.push([method, params]); return { id: "app␟1", result: {} }; } };
    const run = vi.fn<Run>();
    expect(await signOut(server, "ragon", run)).toEqual({ ok: true, message: null });
    expect(asked).toEqual([["account/logout", undefined]]);
    expect(run).not.toHaveBeenCalled();
  });

  it("says the server's refusal", async () => {
    const server = { request: async () => ({ error: { code: -32600, message: "logout failed: busy" } }) };
    expect(await signOut(server, "ragon")).toEqual({ ok: false, message: "logout failed: busy" });
  });

  it("runs `ragon logout` when no server runs", async () => {
    const server = { request: () => Promise.reject(new Error("the agent's server is not running")) };
    const run = vi.fn<Run>(async () => "");
    expect(await signOut(server, "ragon", run, {})).toEqual({ ok: true, message: null });
    expect(run.mock.calls.map((call) => call[1])).toEqual([["logout"]]);
    expect(await signOut(server, null, run)).toEqual({ ok: false, message: "the agent's server is not running" });
  });
});

// The server records what reached it: its arguments and every stdin line.
const RECORDING_SERVER = `
const fs = require("fs");
const log = process.env.RAGON_TEST_RECORD;
fs.appendFileSync(log, JSON.stringify({ argv: process.argv.slice(2) }) + "\\n");
const rl = require("readline").createInterface({ input: process.stdin });
const out = (o) => process.stdout.write(JSON.stringify(o) + "\\n");
rl.on("line", (line) => {
  fs.appendFileSync(log, line + "\\n");
  const m = JSON.parse(line);
  if (m.method === "account/ragon/signIn") {
    if (m.params.password === "right-" + "sentinel") { out({ id: m.id, result: { type: "ragon", username: m.params.username, role: "admin", mustChangePassword: false } }); out({ method: "account/updated", params: {} }); }
    else out({ id: m.id, error: { code: -32001, message: "Wrong username or password." } });
  }
});
`;

describe("the password", () => {
  const saved = { CODEX_HOME: process.env.CODEX_HOME, RAGON_BIN: process.env.RAGON_BIN, RAGON_TEST_RECORD: process.env.RAGON_TEST_RECORD };
  afterEach(() => {
    vi.restoreAllMocks();
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  });

  it.skipIf(process.platform === "win32")("reaches the server's stdin in the request body and nowhere else", async () => {
    const SENTINEL = "right-sentinel";
    const WRONG = "wrong-sentinel-Zq9";
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ragon-account-pw-"));
    const record = path.join(dir, "record.jsonl");
    process.env.CODEX_HOME = path.join(dir, "home");
    process.env.RAGON_BIN = nodeScript(dir, "ragon", RECORDING_SERVER);
    process.env.RAGON_TEST_RECORD = record;
    const logged: string[] = [];
    for (const level of ["log", "info", "warn", "error", "debug"] as const) {
      vi.spyOn(console, level).mockImplementation((...args: unknown[]) => { logged.push(args.map(String).join(" ")); });
    }
    const delivered: string[] = [];
    const server = new AppServer(null, { deliver: (message) => delivered.push(JSON.stringify(message)), busyChanged: () => {} });
    try {
      server.start();
      // As main does: the policy vets the window's message, then login.ts sends it.
      const first = vetDesktopAccount({ type: "account/signIn", mode: "create", username: "ann", password: SENTINEL }) as { mode: "create"; username: string; password: string };
      const created = await signIn(server, first.mode, first.username, first.password);
      const refused = await signIn(server, "signIn", "ann", WRONG);
      expect(created).toEqual({ ok: true, username: "ann", role: "admin", mustChangePassword: false });
      expect(refused).toEqual({ ok: false, code: -32001, message: "Wrong username or password." });
      const lines = fs.readFileSync(record, "utf8").trim().split("\n").map((line) => JSON.parse(line));
      const argv = lines.filter((line) => line.argv).flatMap((line) => line.argv as string[]);
      const requests = lines.filter((line) => line.method === "account/ragon/signIn");
      expect(requests.map((request) => request.params.password)).toEqual([SENTINEL, WRONG]);
      for (const secret of [SENTINEL, WRONG]) {
        expect(argv.join(" ")).not.toContain(secret);
        expect(logged.join("\n")).not.toContain(secret);
        expect(delivered.join("\n")).not.toContain(secret);
        expect(JSON.stringify([created, refused])).not.toContain(secret);
      }
      expect(() => vetDesktopAccount({ type: "account/signIn", mode: "create", username: "ann", password: SENTINEL, confirm: SENTINEL })).toThrow(
        expect.objectContaining({ message: expect.not.stringContaining(SENTINEL) }),
      );
    } finally {
      server.stop();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
