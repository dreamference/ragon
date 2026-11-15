// Redaction for the main process's log lines, the same rules as ragon-log and the Python writer
// (specs/DREAMFERENCE_LOGGING.md §7.4): the token shapes of ragon-rs/vectors/logging/secrets.txt
// (log-redact.test.ts compares this list with that file), private-key blocks, Authorization
// headers, bearer tokens and other users' home folders.

// name, keyword that must appear in the text, pattern: one row per line of secrets.txt.
export const TOKEN_SHAPES: ReadonlyArray<readonly [string, string, string]> = [
  ["github", "gh", String.raw`\bgh[pousr]_[A-Za-z0-9]{20,}`],
  ["github-pat", "github_pat_", String.raw`\bgithub_pat_[A-Za-z0-9_]{20,}`],
  ["slack", "xox", String.raw`\bxox[abposr]-[A-Za-z0-9-]{10,}`],
  ["openai", "sk-", String.raw`\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}`],
  ["jwt", "eyJ", String.raw`\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}`],
  ["aws", "A", String.raw`\b(?:AKIA|ASIA)[A-Z0-9]{16}\b`],
  ["google", "AIza", String.raw`\bAIza[A-Za-z0-9_-]{35}\b`],
  ["gitlab", "glpat-", String.raw`\bglpat-[A-Za-z0-9_-]{20,}`],
  ["npm", "npm_", String.raw`\bnpm_[A-Za-z0-9]{36}\b`],
  ["pypi", "pypi-", String.raw`\bpypi-[A-Za-z0-9_-]{20,}`],
  ["huggingface", "hf_", String.raw`\bhf_[A-Za-z0-9]{30,}`],
  ["telegram", ":", String.raw`\b[0-9]{8,10}:[A-Za-z0-9_-]{35}\b`],
];

const SHAPES = TOKEN_SHAPES.map(([, keyword, pattern]) => [keyword, new RegExp(pattern, "g")] as const);
const KEY_BLOCK = /-----BEGIN [A-Z0-9 ]+-----[\s\S]*?-----END [A-Z0-9 ]+-----/g;
const AUTH_HEADER = /\bAuthorization\s*:[^\r\n]*/gi;
const BEARER = /\b(?:Bearer|Basic)\s+[A-Za-z0-9._~+/=-]{8,}/gi;
const HOMES = /(?:\/home\/|\/Users\/)[^/\s"']+|[A-Za-z]:\\Users\\[^\\\s"']+/g;

/** Returns the text with secrets, tokens and home folders replaced by `[redacted]`. */
export function redactLine(text: string): string {
  let out = text.replace(KEY_BLOCK, "[redacted]").replace(AUTH_HEADER, "Authorization: [redacted]");
  out = out.replace(BEARER, "[redacted]");
  for (const [keyword, pattern] of SHAPES) {
    if (out.includes(keyword)) out = out.replace(pattern, "[redacted]");
  }
  return out.replace(HOMES, (m) => (m.startsWith("/Users/") ? "/Users/[user]" : m.startsWith("/home/") ? "/home/[user]" : "[user]"));
}

type Method = "log" | "info" | "warn" | "error" | "debug";
const METHODS: Method[] = ["log", "info", "warn", "error", "debug"];

function render(arg: unknown): string {
  if (typeof arg === "string") return arg;
  if (arg instanceof Error) return arg.stack ?? arg.message;
  try {
    return typeof arg === "object" ? JSON.stringify(arg) : String(arg);
  } catch {
    return String(arg);
  }
}

let installed = false;

/** Routes every console line through `redactLine`; installing twice does nothing. */
export function installLogRedaction(target: Pick<Console, Method> = console): void {
  if (installed && target === console) return;
  if (target === console) installed = true;
  for (const name of METHODS) {
    const original = target[name].bind(target);
    target[name] = (...args: unknown[]) => original(redactLine(args.map(render).join(" ")));
  }
}
