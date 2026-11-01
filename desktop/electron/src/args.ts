// How `ragon app` asked for the window: `--work`, `--cwd <folder>` and `--thread <id>` open it on
// Work (the launcher passes them); nothing opens it on Ask, as before. A `ragon://` link
// (DESKTOP_PARITY §2.20; the app registers `ragon` and, for links made before, `ragon`) opens
// what it names: `ragon://threads/<id>`, `ragon://threads/new`, `ragon://new?prompt=…&path=…`,
// `ragon://settings[/<page>]`, `ragon://skills`, `ragon://night` (and `ragon://automations`), `ragon://plugins/<id>`; and the older
// `ragon://thread/<id>` and `ragon://work?cwd=<folder>`.

import app from "../app.json";

export interface WorkTarget {
  cwd: string | null;
  thread: string | null;
  /** Text for the composer of a new thread (`ragon://new?prompt=…`). */
  prompt?: string | null;
  /** A page of the window a link names (`ragon://settings`, `ragon://skills`, …), or `new` for a new thread. */
  page?: "new" | "settings" | "skills" | "night" | "plugins" | "pets" | "ask" | "quick";
  /** With `page: "ask"`: send the prompt as a new question at once (Quick Entry), rather than leave it in the composer. */
  send?: boolean;
  /** The plugin `ragon://plugins/<id>` names. */
  plugin?: string;
  /** `ragon://settings/<page>`: the Settings page to open (DESKTOP §16 B19). */
  settingsPage?: string;
  /** `ragon://new?originUrl=…`: a git remote; the new thread starts in the open project whose remote it is. */
  origin?: string;
}

/** The Work target in `args`, or `null` when they open Ask. Chromium's own switches are ignored. */
export function workTarget(args: readonly string[]): WorkTarget | null {
  const valueOf = (flag: string): string | null => {
    const at = args.indexOf(flag);
    return at >= 0 && at + 1 < args.length ? args[at + 1] : null;
  };
  const link = args.find((arg) => arg.startsWith(`${app.scheme}://`));
  if (link) return linkTarget(link);
  const target = { cwd: valueOf("--cwd"), thread: valueOf("--thread") };
  return args.includes("--work") || target.cwd !== null || target.thread !== null ? target : null;
}

const threadId = (value: string) => (/^[A-Za-z0-9-]+$/.test(value) ? value : null);

/** A link's target; anything it does not name opens Work plainly. */
export function linkTarget(link: string): WorkTarget {
  try {
    const url = new URL(link);
    const rest = decodeURIComponent(url.pathname.replace(/^\/+/, ""));
    switch (url.hostname) {
      case "thread":
      case "threads":
        // `ragon://threads/new?prompt=…&path=…` (DESKTOP §16 B19): as `ragon://new`.
        if (rest === "new") return { cwd: url.searchParams.get("path"), thread: null, prompt: url.searchParams.get("prompt")?.slice(0, 4000) ?? null, page: "new" };
        return { cwd: null, thread: threadId(rest) };
      case "new": {
        const origin = url.searchParams.get("originUrl");
        return { cwd: url.searchParams.get("path"), thread: null, prompt: url.searchParams.get("prompt")?.slice(0, 4000) ?? null, page: "new", ...(origin ? { origin: origin.slice(0, 500) } : {}) };
      }
      case "settings":
        return { cwd: null, thread: null, page: "settings", ...(/^[a-z]{1,32}$/.test(rest) ? { settingsPage: rest } : {}) };
      case "skills":
      case "night":
        return { cwd: null, thread: null, page: url.hostname };
      // The Codex app's name for scheduled tasks: Scheduled, over Night Shift.
      case "automations":
        return { cwd: null, thread: null, page: "night" };
      // `ragon://pets` and the vendor's `pets/install` shape: Settings › Pets (a pet is not installed from a link).
      case "pets":
        return { cwd: null, thread: null, page: "pets" };
      // `ragon://ask?prompt=…`: a new Ask question with the text in the composer, not sent (DESKTOP_PARITY_CLAUDE §2.9).
      case "ask":
        return { cwd: null, thread: null, page: "ask", prompt: url.searchParams.get("prompt")?.slice(0, 4000) ?? null };
      // `ragon://quick`: Quick Entry's box (quick.ts), for a desktop's own key binding.
      case "quick":
        return { cwd: null, thread: null, page: "quick" };
      case "plugins":
        return { cwd: null, thread: null, page: "plugins", ...(rest ? { plugin: rest.slice(0, 200) } : {}) };
      default:
        return { cwd: url.searchParams.get("cwd"), thread: null };
    }
  } catch {
    return { cwd: null, thread: null };
  }
}

/**
 * The page the audit's session opens in a preview tab (`RAGON_APP_AUDIT_PREVIEW`, DESKTOP_PARITY
 * §5): an http URL on loopback only, the audit's own page, which reaches for the internet so the
 * trace shows the preview's rules hold. Null otherwise.
 */
export function auditPreview(env: NodeJS.ProcessEnv = process.env): string | null {
  const value = env.RAGON_APP_AUDIT_PREVIEW;
  if (!value) return null;
  try {
    const url = new URL(value);
    return url.protocol === "http:" && (url.hostname === "127.0.0.1" || url.hostname === "localhost") ? url.toString() : null;
  } catch {
    return null;
  }
}

/** Whether this process was asked to run the audit's session (`RAGON_APP_AUDIT=<seconds>`): hidden windows, then quit. */
export function auditSeconds(env: NodeJS.ProcessEnv = process.env): number | null {
  const value = Number(env.RAGON_APP_AUDIT);
  return Number.isFinite(value) && value > 0 ? value : null;
}
