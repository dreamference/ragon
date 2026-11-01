// Opening a cited document: only an id reaches `ragon docs locate`, and only its answer is opened.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { docsAdd, docsReindex, docsRemove, docsStatus, locate } from "./docs";
import { nodeScript } from "./test-ragon";

describe("a cited document", () => {
  it("is found by its id through ragon docs locate, and nothing else is asked", async () => {
    const home = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "ragon-docs-open-")));
    const log = path.join(home, "calls.txt");
    const file = path.join(home, "report.pdf");
    const ragon = nodeScript(home, "ragon", `require("fs").appendFileSync(${JSON.stringify(log)}, process.argv.slice(2).join(" ") + "\\n");
if (process.argv[4] === "documents/42") console.log(JSON.stringify({ path: ${JSON.stringify(file)}, kind: "pdf" }));
else { console.error("ragon-docs: no document documents:7"); process.exit(1); }
`);
    await expect(locate(ragon, "documents/42")).resolves.toBe(file);
    await expect(locate(ragon, "documents/7")).rejects.toThrow("no document documents:7");
    for (const bad of ["../etc/1", "/etc/passwd", "documents/1.pdf", "documents", 42]) {
      await expect(locate(ragon, bad)).rejects.toThrow("not a document");
    }
    expect(fs.readFileSync(log, "utf8").trim().split("\n")).toEqual(["docs locate documents/42 --json", "docs locate documents/7 --json"]);
  });

  it("manages the collections through ragon docs, with names and full paths only", async () => {
    const home = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "ragon-docs-settings-")));
    const log = path.join(home, "calls.txt");
    const ragon = nodeScript(home, "ragon", `require("fs").appendFileSync(${JSON.stringify(log)}, process.argv.slice(2).join(" ") + "\\n");
if (process.argv[3] === "status") console.log(JSON.stringify({ collections: [{ name: "documents", counts: { indexed: 3 } }], runtime_missing: null }));
else if (process.argv[3] === "add" && process.argv[4] === "/home/x/Downloads/ssh") { console.error("ragon-docs: /home/x/.ssh is never indexed"); process.exit(1); }
else console.log("done");
`);
    await expect(docsStatus(ragon)).resolves.toEqual({ collections: [{ name: "documents", counts: { indexed: 3 } }], runtime_missing: null });
    await expect(docsAdd(ragon, "/home/x/Notes")).resolves.toBe("done");
    await expect(docsAdd(ragon, "/home/x/Downloads/ssh")).rejects.toThrow("never indexed");
    await expect(docsRemove(ragon, "notes")).resolves.toBe("done");
    await expect(docsReindex(ragon, "documents")).resolves.toBe("done");
    await expect(docsAdd(ragon, "relative/folder")).rejects.toThrow("full path");
    await expect(docsRemove(ragon, "../x")).rejects.toThrow("not a collection");
    expect(fs.readFileSync(log, "utf8").trim().split("\n")).toEqual(["docs status --json", "docs add /home/x/Notes", "docs add /home/x/Downloads/ssh", "docs remove notes", "docs reindex documents"]);
  });
});
