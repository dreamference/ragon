// Settings › Node (specs/DREAMFERENCE_RAGON_NODE.md §6.4): the Ragon nodes on the local
// network and the one this machine uses, through the launcher's own commands, as a shell has them:
// `ragon node list --json` (the same browse as `ragon node list`, names only: no address, URL or
// configured host is in it) and `ragon node use <name>`, which remembers the node in
// `$CODEX_HOME/node.json` as it always has (the last address stays its fallback after a failed
// browse). The Settings picker only sends a name, or a node's id when it advertises no name, from the
// list it was last given. Startup recovery confirms a manually entered address separately. Nothing browses until the page asks: an mDNS query at
// start-up would be a finding of the egress audit's app session. No Electron here.

import { execFile } from "node:child_process";

import { readFile } from "node:fs/promises";
import path from "node:path";
import { isIP } from "node:net";
import { command, programOnPath } from "./platform";

export interface NodeEntry {
  name: string;
  id: string;
  /** What `ragon node use` is given to choose this node; null when it can be chosen by no name. */
  choice: string | null;
  inUse: boolean;
  model: string | null;
  contextTokens: number | null;
  state: string;
  version: string;
  coding: boolean;
}

export interface NodeList {
  /** `client` chooses among the nodes; a `node` uses its own model server; `configured` names one. */
  thisMachine: "client" | "node" | "configured";
  nodes: NodeEntry[];
  /** The remembered node, when it did not answer this browse. */
  rememberedAway: { name: string; id: string } | null;
  /** A line about the network (Wi-Fi power saving) when a browse missed. */
  hint: string | null;
}

/** A browse waits up to a few seconds, and each answering node's model server two more. */
const TIMEOUT_MS = 30_000;

/** A name or id `ragon node use` takes, and nothing that reads as an address or an option. */
const CHOICE = /^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$/;

function run(ragon: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    const started = command(ragon, args);
    execFile(started.file, started.args, { timeout: TIMEOUT_MS, shell: started.shell, windowsHide: true }, (error, stdout, stderr) => {
      if (error) reject(new Error(String(stderr || stdout || error.message).trim()));
      else resolve(String(stdout));
    });
  });
}

/** The list's JSON: the last line of `ragon node list --json` that is an object (a notice may come first). */
export function parseList(stdout: string): NodeList {
  const line = stdout.split("\n").map((text) => text.trim()).filter((text) => text.startsWith("{")).pop();
  if (!line) throw new Error("ragon node list --json printed no list: this ragon is older than the app");
  const parsed = JSON.parse(line) as Partial<NodeList>;
  const machine = parsed.thisMachine === "node" || parsed.thisMachine === "configured" ? parsed.thisMachine : "client";
  const nodes = Array.isArray(parsed.nodes) ? parsed.nodes.filter((node): node is NodeEntry => typeof node === "object" && node !== null) : [];
  return {
    thisMachine: machine,
    nodes: nodes.map((node) => ({
      name: String(node.name ?? ""),
      id: String(node.id ?? ""),
      // Its name, else its id: whichever passes as a plain word (an mDNS name may hold a space or a dot).
      choice: [node.choice, node.name, node.id].find((text): text is string => typeof text === "string" && CHOICE.test(text)) ?? null,
      inUse: node.inUse === true,
      model: typeof node.model === "string" ? node.model : null,
      contextTokens: typeof node.contextTokens === "number" ? node.contextTokens : null,
      state: String(node.state ?? ""),
      version: String(node.version ?? ""),
      coding: node.coding === true,
    })),
    rememberedAway: parsed.rememberedAway && typeof parsed.rememberedAway === "object"
      ? { name: String(parsed.rememberedAway.name ?? ""), id: String(parsed.rememberedAway.id ?? "") }
      : null,
    hint: typeof parsed.hint === "string" ? parsed.hint : null,
  };
}

/** Lists the nodes and remembers which choices the window may send back. */
export class Nodes {
  private choices = new Set<string>();

  async list(ragon: string): Promise<NodeList> {
    const list = parseList(await run(ragon, ["node", "list", "--json"]));
    this.choices = new Set(list.thisMachine === "client" ? list.nodes.flatMap((node) => (node.choice ? [node.choice] : [])) : []);
    return list;
  }

  /** `ragon node use <choice>` for a node of the last list; resolves with what it printed. */
  use(ragon: string, choice: unknown): Promise<string> {
    if (typeof choice !== "string" || !CHOICE.test(choice) || !this.choices.has(choice)) {
      return Promise.reject(new Error("choose a node from the list"));
    }
    return run(ragon, ["node", "use", choice]).then((output) => output.trim());
  }
}

/** GUI launches may omit the directory where remote join installed NetBird. */
export function clientPath(): string {
  const installed = process.platform === "win32"
    ? path.join(process.env.ProgramFiles || "C:\\Program Files", "Ragon", "NetBird")
    : "/usr/local/bin";
  return [process.env.PATH, installed].filter(Boolean).join(path.delimiter);
}

const DNS_NAME = /^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/i;

/** Only a remembered overlay name carrying the remembered node's own id can become a server URL. */
export async function vpnRoute(home: string | null, client = programOnPath("netbird", process.platform, clientPath())) {
  if (!home) throw new Error("The remembered node is unavailable.");
  let record;
  try {
    record = JSON.parse(await readFile(path.join(home, "node.json"), "utf8"));
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw new Error("Could not read the remembered node.");
  }
  const { node, remote, model_port: port } = record ?? {};
  const remembered = typeof node === "string" && node && typeof remote === "string" && DNS_NAME.test(remote)
    && remote.toLowerCase().startsWith(`ragon-${node.toLowerCase()}.`);
  if (!client) return { state: remembered ? "missing" : "unenrolled" } as const;
  try {
    const status = JSON.parse(await run(client, ["status", "--json"]));
    if (status.daemonStatus === "NeedsLogin") return { state: remembered ? "down" : "unenrolled" } as const;
    if (status.management?.connected === true) {
      if (remembered) {
        const modelPort = Number.isInteger(port) && port > 0 && port <= 65535 ? port : 8000;
        return { state: "ready", host: `http://${remote}:${modelPort}` } as const;
      }
      // After a reinstall or a peer rename, only the launcher may select a node from the peers.
      if (Array.isArray(status.peers?.details) && status.peers.details.length) return { state: "discover" } as const;
      return { state: "unenrolled" } as const;
    }
  } catch {
    // The CLI can include peer and management details in errors. Only expose the connection state.
  }
  return { state: "down" } as const;
}

/** Accept a single address in the launcher's node-use syntax, including IPv6 and a model port. */
export function nodeAddress(value: unknown): string {
  if (typeof value === "string" && value.length <= 256) {
    const address = value.replace(/^http:\/\//i, "").replace(/\/$/, "");
    if (isIP(address)) return address;
    const match = /^(?:\[([a-f0-9:.]+)\]|([a-z0-9.-]+))(?::([0-9]{1,5}))?$/i.exec(address);
    if (match && (match[1] ? isIP(match[1]) === 6 : DNS_NAME.test(match[2]))
        && (!match[3] || (+match[3] > 0 && +match[3] <= 65535))) return address;
  }
  throw new Error("Enter a node address, optionally followed by its model port.");
}

export async function useAddress(ragon: string, address: string): Promise<void> {
  try {
    await run(ragon, ["node", "use", nodeAddress(address)]);
  } catch {
    throw new Error("The node address could not be saved.");
  }
}
