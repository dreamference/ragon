// The sign-in form's request against the real `AppServer` and a scripted `ragon app-server`
// (test-ragon.ts): the answer comes back to the form, a thread opened before stays open, and
// account/read shows the user at once, all in one server process (no restart).

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { readSignIn, signIn, signInParams } from "./login";
import { AppServer } from "./server";
import { nodeScript } from "./test-ragon";

const SERVER = `
if (process.argv[2] === "airgapped") { process.stdout.write('{"level":"off","tier":"default","source":"stub","invalid":[]}\\n'); process.exit(0); }
const rl = require("readline").createInterface({ input: process.stdin });
const pid = process.pid;
let user = null;
const out = (o) => process.stdout.write(JSON.stringify(o) + "\\n");
rl.on("line", (line) => {
  const m = JSON.parse(line);
  if (m.method === "thread/start") out({ id: m.id, result: { thread: { id: "t1" }, pid } });
  else if (m.method === "account/ragon/signIn") {
    const p = m.params;
    if (p.username === "taken") out({ id: m.id, error: { code: -32002, message: "name taken" } });
    else if (p.password !== "pw ") out({ id: m.id, error: { code: -32001, message: "wrong" } });
    else { user = p.username; out({ id: m.id, result: { type: "ragon", username: p.username, role: "user", mustChangePassword: false } }); out({ method: "account/updated", params: {} }); }
  } else if (m.method === "account/read") out({ id: m.id, result: { account: user ? { type: "ragon", username: user } : null, pid } });
  else if (m.method === "thread/loaded/list") out({ id: m.id, result: { data: ["t1"], pid } });
});
`;

describe("the sign-in form", () => {
  const saved = { CODEX_HOME: process.env.CODEX_HOME, RAGON_BIN: process.env.RAGON_BIN };
  afterEach(() => {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  });

  it("keeps the fields as typed", () => {
    expect(signInParams("create", "", " a ")).toEqual({ mode: "create", username: "", password: " a " });
  });

  it("puts the server's errors in words", () => {
    expect(readSignIn({ error: { code: -32001, message: "x" } })).toMatchObject({ ok: false, code: -32001, message: expect.stringContaining("do not match") });
    expect(readSignIn({ error: { code: -32002 } })).toMatchObject({ ok: false, message: expect.stringContaining("taken") });
    expect(readSignIn({ error: { code: -32003 } })).toMatchObject({ ok: false, message: expect.stringContaining("primary") });
    expect(readSignIn({ result: { type: "ragon", username: "ann", role: "admin", mustChangePassword: false } })).toEqual({ ok: true, username: "ann", role: "admin", mustChangePassword: false });
    expect(readSignIn({ result: {} })).toMatchObject({ ok: false });
  });

  it("says so when the server is not running", async () => {
    const server = new AppServer(null, { deliver: () => {}, busyChanged: () => {} });
    expect(await signIn(server, "signIn", "a", "b")).toMatchObject({ ok: false, message: expect.stringContaining("not running") });
  });

  it.skipIf(process.platform === "win32")("signs in without a restart: the thread stays open and account/read shows the user", async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ragon-login-"));
    process.env.CODEX_HOME = path.join(dir, "home");
    process.env.RAGON_BIN = nodeScript(dir, "ragon", SERVER);
    const messages: any[] = [];
    const server = new AppServer(null, { deliver: (m) => { if (m.channel === "work://message") messages.push(m.payload); }, busyChanged: () => {} });
    const next = async (id: number) => {
      for (let i = 0; i < 100; i++) {
        const found = messages.find((m) => m.id === id);
        if (found) return found;
        await new Promise((r) => setTimeout(r, 50));
      }
      throw new Error(`no answer to ${id}`);
    };
    try {
      server.start();
      await server.send({ id: 1, method: "initialize", params: { clientInfo: { name: "test", version: "0" } } });
      await server.send({ method: "initialized" });
      await server.send({ id: 2, method: "thread/start", params: { cwd: dir } }).catch((e) => { throw new Error("send: " + e.message); });
      const pid = (await next(2)).result.pid;
      expect((await signIn(server, "signIn", "ann", "bad"))).toMatchObject({ ok: false, code: -32001 });
      expect((await signIn(server, "create", "taken", "pw "))).toMatchObject({ ok: false, code: -32002 });
      expect(await signIn(server, "signIn", "ann", "pw ")).toEqual({ ok: true, username: "ann", role: "user", mustChangePassword: false });
      // The answers to the form's own requests reached no window.
      expect(messages.some((m) => typeof m.id === "string")).toBe(false);
      await server.send({ id: 3, method: "account/read", params: {} });
      const account = await next(3);
      expect(account.result.account.username).toBe("ann");
      expect(account.result.pid).toBe(pid);
      await server.send({ id: 4, method: "thread/loaded/list", params: {} });
      expect((await next(4)).result.data).toEqual(["t1"]);
    } finally {
      server.stop();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
