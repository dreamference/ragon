// The app-server's process as AppServer runs it, against a stand-in `ragon` (test-ragon.ts).

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { AppServer } from "./server";
import { nodeScript } from "./test-ragon";

describe("the app-server's process", () => {
  const saved = { CODEX_HOME: process.env.CODEX_HOME, RAGON_BIN: process.env.RAGON_BIN };
  afterEach(() => {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  });

  it("starts the agent with the enrolled node as its model host", async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ragon-vpn-server-"));
    process.env.CODEX_HOME = dir;
    process.env.RAGON_BIN = nodeScript(dir, "ragon", 'console.error(process.env.DREAMFERENCE_VLLM_HOST);');
    const lines: string[] = [];
    let finished!: () => void;
    const exit = new Promise<void>(resolve => { finished = resolve; });
    const server = new AppServer(null, {
      deliver: event => {
        if (event.channel === "work://stderr") lines.push(event.payload);
        if (event.channel === "work://exit") finished();
      },
      busyChanged() {},
    });
    try {
      const host = "http://ragon-abcd.overlay.test:8123";
      expect(server.start(host).started).toBe(true);
      await exit;
      expect(lines).toEqual([host]);
    } finally {
      server.stop();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it.skipIf(process.platform === "win32")("closing its input throws nothing in this process", async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ragon-server-closed-"));
    process.env.CODEX_HOME = path.join(dir, "home");
    // Closes its stdin and lives on a while, as a server going down does.
    process.env.RAGON_BIN = nodeScript(dir, "ragon", 'require("fs").closeSync(0); setTimeout(() => {}, 3000);\n');
    const server = new AppServer(null, { deliver: () => {}, busyChanged: () => {} });
    const uncaught: unknown[] = [];
    const record = (error: unknown) => uncaught.push(error);
    process.on("uncaughtException", record);
    try {
      server.start();
      await new Promise((resolve) => setTimeout(resolve, 500));
      await server.send({ id: 1, method: "initialize", params: { clientInfo: { name: "test", version: "0" } } }).catch(() => undefined);
      await new Promise((resolve) => setTimeout(resolve, 200));
      expect(uncaught).toEqual([]);
    } finally {
      process.off("uncaughtException", record);
      server.stop();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
