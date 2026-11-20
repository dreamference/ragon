import { defineConfig } from "vitest/config";

// On Windows a stand-in `ragon` starts through cmd.exe and a fresh Node, and the first start of a
// newly written script can wait seconds on the virus scanner: a Windows QA run timed out one
// spawning test at vitest's 5 s while the same tree passed three times in under 1 s. The limit
// there is above `night.ts`'s 30 s `execFile` timeout, so a child that really hangs reports itself.
export default defineConfig({
  test: { environment: "node", include: ["src/**/*.test.ts"], testTimeout: process.platform === "win32" ? 45_000 : 5_000 },
});
