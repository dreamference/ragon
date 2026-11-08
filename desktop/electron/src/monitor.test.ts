import { describe, expect, it } from "vitest";

import { MONITOR_URL, chipFrom, monitorChip } from "./monitor";

const DOWN = { node: { state: "down" }, items: [{ name: "engine", state: "ok", detail: "healthy" }, { name: "disk", state: "down", detail: "2% free" }] };

describe("the monitor chip", () => {
  it("takes the node state and lists the items that are not ok", () => {
    expect(chipFrom(DOWN)).toEqual({ state: "down", lines: ["disk down: 2% free"] });
    expect(chipFrom({ node: { state: "ok" }, items: [] })).toEqual({ state: "ok", lines: [] });
  });
  it("is unknown for anything it cannot read", () => {
    for (const bad of [null, "x", 3, {}, { node: { state: "bogus" } }]) expect(chipFrom(bad).state).toBe("unknown");
  });
  it("asks the loopback route and is unknown when it fails", async () => {
    const urls: string[] = [];
    const ok = (async (url: string) => (urls.push(url), new Response(JSON.stringify(DOWN)))) as unknown as typeof fetch;
    expect((await monitorChip(ok)).state).toBe("down");
    expect(urls).toEqual([MONITOR_URL]);
    expect(MONITOR_URL).toMatch(/^http:\/\/127\.0\.0\.1:/);
    expect((await monitorChip((async () => new Response("", { status: 404 })) as unknown as typeof fetch)).state).toBe("unknown");
    expect((await monitorChip((async () => { throw new Error("refused"); }) as unknown as typeof fetch)).state).toBe("unknown");
  });
});
