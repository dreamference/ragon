import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

import { RAGON_VERSION_FILE, versionText, wantsVersion } from "./version";

describe("--version", () => {
  it("is asked for by --version only", () => {
    expect(wantsVersion(["--version"])).toBe(true);
    expect(wantsVersion(["--work", "--version"])).toBe(true);
    expect(wantsVersion([])).toBe(false);
    expect(wantsVersion(["--work", "--cwd", "/home/u/p"])).toBe(false);
  });

  it("prints the app's version and the bundled ragon's, read from the stamp", () => {
    const read = (file: string) => {
      expect(file).toBe(path.join("/opt/res", RAGON_VERSION_FILE));
      return "ragon 0.9.0\n";
    };
    expect(versionText("Ragon", "0.9.0", "/opt/res", read)).toBe("Ragon 0.9.0\nragon 0.9.0\n");
  });

  it("says when no ragon is bundled or the app is not packaged", () => {
    const missing = () => {
      throw new Error("ENOENT");
    };
    expect(versionText("Ragon", "0.9.0", "/opt/res", missing)).toBe("Ragon 0.9.0\nragon: not bundled\n");
    expect(versionText("Ragon", "0.9.0", null, missing)).toBe("Ragon 0.9.0\nragon: not bundled\n");
    expect(versionText("Ragon", "0.9.0", "/opt/res", () => "\n")).toBe("Ragon 0.9.0\nragon: version unknown\n");
  });

  it("reads the file forge.config.js writes", () => {
    const config = fs.readFileSync(path.join(__dirname, "..", "forge.config.js"), "utf8");
    expect(config).toContain(`"${RAGON_VERSION_FILE}"`);
  });
});
