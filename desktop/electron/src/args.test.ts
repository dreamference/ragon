import { describe, expect, it } from "vitest";

import { auditPreview, auditSeconds, linkTarget, workTarget } from "./args";

const args = (text: string) => text.split(/\s+/).filter(Boolean);

describe("what ragon app asks for", () => {
  it("opens Chat with no arguments, as before", () => {
    expect(workTarget([])).toBeNull();
        expect(workTarget(args("--ozone-platform=x11"))).toBeNull();
  });

  it("opens Work on a folder or a thread", () => {
    expect(workTarget(args("--work"))).toEqual({ cwd: null, thread: null });
    expect(workTarget(args("--cwd /home/u/project"))).toEqual({ cwd: "/home/u/project", thread: null });
    expect(workTarget(args("--work --thread 019a-thread"))).toEqual({ cwd: null, thread: "019a-thread" });
  });

  it("reads a ragon:// link", () => {
    expect(linkTarget("ragon://thread/019a-thread")).toEqual({ cwd: null, thread: "019a-thread" });
    expect(linkTarget("ragon://work?cwd=%2Fhome%2Fu%2Fp")).toEqual({ cwd: "/home/u/p", thread: null });
    // An id that is not an id is dropped (it is only ever sent to the server as a thread id, never joined to a path).
    expect(linkTarget("ragon://thread/not%20an%20id")).toEqual({ cwd: null, thread: null });
    expect(workTarget(["ragon://thread/t1"])).toEqual({ cwd: null, thread: "t1" });
  });

  it("reads the ragon:// links of DESKTOP_PARITY §2.20", () => {
    expect(linkTarget("ragon://threads/019a-thread")).toEqual({ cwd: null, thread: "019a-thread" });
    expect(linkTarget("ragon://threads/new")).toEqual({ cwd: null, thread: null, prompt: null, page: "new" });
    expect(linkTarget("ragon://new?prompt=Fix%20the%20build&path=%2Fhome%2Fu%2Fapp")).toEqual({ cwd: "/home/u/app", thread: null, prompt: "Fix the build", page: "new" });
    expect(linkTarget("ragon://settings")).toEqual({ cwd: null, thread: null, page: "settings" });
    expect(linkTarget("ragon://night")).toEqual({ cwd: null, thread: null, page: "night" });
    expect(linkTarget("ragon://pets")).toEqual({ cwd: null, thread: null, page: "pets" });
    expect(linkTarget("ragon://pets/install?url=https://example.com/p.zip")).toEqual({ cwd: null, thread: null, page: "pets" });
    expect(linkTarget("ragon://plugins/acme.lint")).toEqual({ cwd: null, thread: null, page: "plugins", plugin: "acme.lint" });
    expect(workTarget(["ragon://threads/t2"])).toEqual({ cwd: null, thread: "t2" });
  });

  it("knows the audit's session", () => {
    expect(auditSeconds({})).toBeNull();
    expect(auditSeconds({ RAGON_APP_AUDIT: "x" })).toBeNull();
    expect(auditSeconds({ RAGON_APP_AUDIT: "45" })).toBe(45);
    expect(auditPreview({ RAGON_APP_AUDIT_PREVIEW: "http://127.0.0.1:4100/page" })).toBe("http://127.0.0.1:4100/page");
    expect(auditPreview({ RAGON_APP_AUDIT_PREVIEW: "https://example.com/" })).toBeNull();
    expect(auditPreview({ RAGON_APP_AUDIT_PREVIEW: "http://192.168.1.2/" })).toBeNull();
    expect(auditPreview({})).toBeNull();
  });
});

describe("Quick Entry's links (DESKTOP_PARITY_CLAUDE §2.9)", () => {
  it("reads ragon://ask with a prompt for the composer, and ragon://quick for the box", () => {
    expect(linkTarget("ragon://ask?prompt=What%20is%20NVFP4%3F")).toEqual({ cwd: null, thread: null, page: "ask", prompt: "What is NVFP4?" });
    expect(linkTarget("ragon://ask")).toEqual({ cwd: null, thread: null, page: "ask", prompt: null });
    expect(linkTarget(`ragon://ask?prompt=${"x".repeat(5000)}`).prompt).toHaveLength(4000);
    expect(linkTarget("ragon://quick")).toEqual({ cwd: null, thread: null, page: "quick" });
  });

  it("reads the Codex app's other links (DESKTOP §16 B19)", () => {
    expect(linkTarget("ragon://threads/new?prompt=Fix%20it&path=%2Fw%2Fapp")).toEqual({ cwd: "/w/app", thread: null, prompt: "Fix it", page: "new" });
    expect(linkTarget("ragon://new?originUrl=git%40github.com%3Aacme%2Fapp.git&prompt=x")).toEqual({ cwd: null, thread: null, prompt: "x", page: "new", origin: "git@github.com:acme/app.git" });
    expect(linkTarget("ragon://settings/notifications")).toEqual({ cwd: null, thread: null, page: "settings", settingsPage: "notifications" });
    expect(linkTarget("ragon://settings/Bad_Page")).toEqual({ cwd: null, thread: null, page: "settings" });
    expect(linkTarget("ragon://automations")).toEqual({ cwd: null, thread: null, page: "night" });
    expect(linkTarget("ragon://plugins/lint@team")).toEqual({ cwd: null, thread: null, page: "plugins", plugin: "lint@team" });
  });
});

