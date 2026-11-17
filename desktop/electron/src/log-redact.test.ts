import fs from "node:fs";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";

import { TOKEN_SHAPES, installLogRedaction, redactLine } from "./log-redact";

const VECTOR = path.resolve(__dirname, "../../../ragon-rs/vectors/logging/secrets.txt");
const rows = fs
  .readFileSync(VECTOR, "utf8")
  .split(/\r?\n/)
  .filter((l) => l.trim() && !l.startsWith("#"))
  .map((l) => {
    const [name, keyword, ...rest] = l.split("|");
    return [name, keyword, rest.join("|")] as const;
  });

// A string that matches each shape, built so no whole token sits in this file.
const SAMPLES: Record<string, string> = {
  github: "gh" + "p_" + "a".repeat(30),
  "github-pat": "github_pat_" + "B".repeat(30),
  slack: "xox" + "b-" + "1234567890-abc",
  openai: "sk-" + "proj-" + "c".repeat(30),
  jwt: "eyJ" + "hbGciOi" + ".eyJzdWIiOi" + ".sig12345",
  aws: "AKIA" + "A".repeat(16),
  google: "AIza" + "d".repeat(35),
  gitlab: "glpat-" + "e".repeat(24),
  npm: "npm_" + "f".repeat(36),
  pypi: "pypi-" + "g".repeat(24),
  huggingface: "hf_" + "h".repeat(34),
  telegram: "123456789:" + "i".repeat(35),
};

describe("log redaction", () => {
  it("carries exactly the shapes of secrets.txt", () => {
    expect(TOKEN_SHAPES.map((s) => [...s])).toEqual(rows.map((r) => [...r]));
  });

  it.each(rows)("redacts the %s shape", (name) => {
    const sample = SAMPLES[name];
    expect(sample, `no sample for ${name}`).toBeTruthy();
    const out = redactLine(`token ${sample} end`);
    expect(out).not.toContain(sample);
    expect(out).toContain("[redacted]");
  });

  it("redacts headers, bearer tokens, key blocks and other homes", () => {
    expect(redactLine("Authorization: Basic abc")).toBe("Authorization: [redacted]");
    expect(redactLine("Bearer abcdefgh12345")).toBe("[redacted]");
    expect(redactLine("-----BEGIN PRIVATE KEY-----\nxx\n-----END PRIVATE KEY-----")).toBe("[redacted]");
    expect(redactLine("open /home/jane/x")).toBe("open /home/[user]/x");
  });

  it("no shape reaches a console line once installed", () => {
    const error = vi.fn();
    const sink = { log: vi.fn(), info: vi.fn(), warn: vi.fn(), error, debug: vi.fn() };
    installLogRedaction(sink as unknown as Console);
    for (const sample of Object.values(SAMPLES)) sink.error("failed", { t: sample }, new Error(sample));
    const seen = error.mock.calls.map((c) => String(c[0])).join("\n");
    for (const sample of Object.values(SAMPLES)) expect(seen).not.toContain(sample);
  });
});
