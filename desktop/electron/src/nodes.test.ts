// Settings › Node's calls (nodes.ts) through a stand-in `ragon node`: the list read from its JSON,
// and a choice sent back only when the last list offered it, never an address.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { Nodes, nodeAddress, parseList, useAddress, vpnRoute } from "./nodes";
import { nodeScript } from "./test-ragon";

const LIST = {
  thisMachine: "client",
  nodes: [
    { name: "spark-1", id: "11111111-aaaa", choice: "spark-1", inUse: true, model: "RadixArk/Qwen3.8-27B-NVFP4", contextTokens: 262144, state: "ready", version: "0.9.1", coding: true },
    { name: "", id: "22222222-bbbb", choice: "22222222-bbbb", inUse: false, model: null, contextTokens: null, state: "stopped", version: "0.9.1", coding: true },
    { name: "lab box.local", id: "", choice: "lab box.local", inUse: false, model: null, contextTokens: null, state: "", version: "", coding: false },
  ],
  rememberedAway: null,
  hint: null,
};

describe("Settings › Node", () => {
  it("reads the list from the last JSON line, and keeps only plain names and ids as choices", () => {
    const list = parseList(`Note: node spark-1 runs Ragon 0.9.0, this machine 0.9.1.\n${JSON.stringify(LIST)}\n`);
    expect(list.thisMachine).toBe("client");
    expect(list.nodes.map((node) => node.choice)).toEqual(["spark-1", "22222222-bbbb", null]);
    expect(list.nodes[0].inUse).toBe(true);
    expect(() => parseList("No Ragon node answered on this network.\n")).toThrow("older than the app");
  });

  it("runs ragon node list --json and ragon node use <name>, and refuses what the list did not offer", async () => {
    const home = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "ragon-nodes-")));
    const log = path.join(home, "calls.txt");
    const ragon = nodeScript(home, "ragon", `require("fs").appendFileSync(${JSON.stringify(log)}, process.argv.slice(2).join(" ") + "\\n");
if (process.argv[3] === "list") console.log(${JSON.stringify(JSON.stringify(LIST))});
else console.log("Using node " + process.argv[4] + " at http://192.168.50.106:8000.");
`);
    const nodes = new Nodes();
    // Nothing listed yet: nothing may be chosen.
    await expect(nodes.use(ragon, "spark-1")).rejects.toThrow("from the list");
    const list = await nodes.list(ragon);
    expect(list.nodes).toHaveLength(3);
    await expect(nodes.use(ragon, "22222222-bbbb")).resolves.toContain("Using node");
    for (const refused of ["192.168.50.106", "spark-9", "--help", "spark-1 ; rm", "lab box.local", 7]) {
      await expect(nodes.use(ragon, refused)).rejects.toThrow("from the list");
    }
    expect(fs.readFileSync(log, "utf8").trim().split("\n")).toEqual(["node list --json", "node use 22222222-bbbb"]);
  });

  it("offers no choice on a node or with a configured host", async () => {
    const home = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "ragon-nodes-")));
    const ragon = nodeScript(home, "ragon", `console.log(${JSON.stringify(JSON.stringify({ ...LIST, thisMachine: "node" }))});\n`);
    const nodes = new Nodes();
    expect((await nodes.list(ragon)).thisMachine).toBe("node");
    await expect(nodes.use(ragon, "spark-1")).rejects.toThrow("from the list");
  });
});

describe("startup node recovery", () => {
  it("starts only with the enrolled node's overlay URL and connected management", async () => {
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "ragon-vpn-"));
    try {
      const file = path.join(home, "node.json");
      const client = nodeScript(home, "netbird", `if (process.argv.slice(2).join(" ") !== "status --json") process.exit(1);
console.log(JSON.stringify({management: {connected: true}}));`);
      await expect(vpnRoute(home, client)).resolves.toEqual({ state: "unenrolled" });
      const peers = nodeScript(home, "peers", 'console.log(JSON.stringify({management:{connected:true},peers:{details:[{fqdn:"peer.example"}]}}))');
      await expect(vpnRoute(home, peers)).resolves.toEqual({ state: "discover" });
      const record = { node: "abcd", remote: "ragon-abcd.overlay.test", model_port: 8123 };
      fs.writeFileSync(file, JSON.stringify({ ...record, node: "other" }));
      await expect(vpnRoute(home, client)).resolves.toEqual({ state: "unenrolled" });
      fs.writeFileSync(file, JSON.stringify(record));
      await expect(vpnRoute(home, client)).resolves.toEqual({ state: "ready", host: "http://ragon-abcd.overlay.test:8123" });
      await expect(vpnRoute(home, null)).resolves.toEqual({ state: "missing" });
      const loggedOut = nodeScript(home, "logged-out", 'console.log(JSON.stringify({daemonStatus:"NeedsLogin"}))');
      await expect(vpnRoute(home, loggedOut)).resolves.toEqual({ state: "down" });
      const down = nodeScript(home, "down", 'console.log(JSON.stringify({peers:{connected:2}, management:{connected:false}}))');
      await expect(vpnRoute(home, down)).resolves.toEqual({ state: "down" });
    } finally {
      fs.rmSync(home, { recursive: true, force: true });
    }
  });

  it("saves a named address with node use, accepting only a host and optional port", async () => {
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "ragon-address-"));
    try {
      const log = path.join(home, "args");
      const ragon = nodeScript(home, "ragon", `require("fs").writeFileSync(${JSON.stringify(log)}, JSON.stringify(process.argv.slice(2)))`);
      await useAddress(ragon, "http://model.example:8123/");
      expect(JSON.parse(fs.readFileSync(log, "utf8"))).toEqual(["node", "use", "model.example:8123"]);
      expect(nodeAddress("[::1]:8123")).toBe("[::1]:8123");
      for (const bad of ["--help", "model.example?token=x", "user@node.test", "node.test;id", "http://node.test/file", "node.test:99999"]) {
        expect(() => nodeAddress(bad)).toThrow("Enter a node address");
      }
    } finally {
      fs.rmSync(home, { recursive: true, force: true });
    }
  });
});
