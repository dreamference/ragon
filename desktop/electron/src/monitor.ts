// The node monitor's chip (MONITORING §5.1): the main process reads `GET /api/monitor/status` from
// `ragon web` on this machine (loopback, port 3100, the route the web page's JSON comes from) and
// answers the chip's state and the items that are not ok. Nothing here is an app-server request,
// so ragon-rs/web/policy.json (which vets those) is not asked; the window's one channel carries
// only the message type `monitor/status`. No Electron here, so this tests on its own.

export const MONITOR_URL = "http://127.0.0.1:3100/api/monitor/status";
/** The status page the chip opens. */
export const MONITOR_PAGE = "http://127.0.0.1:3100/status";

export type MonitorState = "ok" | "degraded" | "down" | "off" | "unknown";

export interface MonitorChip {
  state: MonitorState;
  /** The items that are not ok, as `name: detail`, for the chip's tooltip. */
  lines: string[];
}

const STATES: ReadonlyArray<string> = ["ok", "degraded", "down", "off", "unknown"];

function stateOf(value: unknown): MonitorState {
  return typeof value === "string" && STATES.includes(value) ? (value as MonitorState) : "unknown";
}

/** The chip from the document of `monitor status --json`; unknown for anything else. */
export function chipFrom(document: unknown): MonitorChip {
  if (typeof document !== "object" || document === null) return { state: "unknown", lines: [] };
  const record = document as Record<string, unknown>;
  const node = record.node as Record<string, unknown> | undefined;
  const lines: string[] = [];
  if (Array.isArray(record.items)) {
    for (const item of record.items) {
      if (typeof item !== "object" || item === null) continue;
      const entry = item as Record<string, unknown>;
      if (typeof entry.name !== "string" || stateOf(entry.state) === "ok") continue;
      const detail = typeof entry.detail === "string" && entry.detail ? `: ${entry.detail}` : "";
      lines.push(`${entry.name.slice(0, 80)} ${stateOf(entry.state)}${detail.slice(0, 160)}`);
      if (lines.length >= 8) break;
    }
  }
  return { state: stateOf(node?.state), lines };
}

/** Asks `ragon web` for the monitor's status; unknown when it does not answer. */
export async function monitorChip(fetcher: typeof fetch): Promise<MonitorChip> {
  try {
    const response = await fetcher(MONITOR_URL, { signal: AbortSignal.timeout(20_000) });
    if (!response.ok) return { state: "unknown", lines: [] };
    return chipFrom(await response.json());
  } catch {
    return { state: "unknown", lines: [] };
  }
}
