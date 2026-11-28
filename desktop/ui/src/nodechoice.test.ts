import { describe, expect, it } from "vitest";

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { VpnControls } from "./VpnRecovery";
import type { NodeEntry } from "./bridge";
import { needsNodeChoice, pickable } from "./nodechoice";

describe("the start-up node picker", () => {
  it("asks only when the launcher refused for several nodes", () => {
    expect(needsNodeChoice(["Error: Several Ragon nodes are on this network:", "  1. a"])).toBe(true);
    expect(needsNodeChoice(["the model server stopped"])).toBe(false);
    expect(needsNodeChoice([])).toBe(false);
  });

  it("offers the nodes that can be chosen by a name", () => {
    const node = (name: string, choice: string | null) => ({ name, id: name, choice }) as NodeEntry;
    expect(pickable([node("a", "a"), node("b", null)]).map((n) => n.name)).toEqual(["a"]);
  });
});

describe("VPN recovery controls", () => {
  const render = (phase: Parameters<typeof VpnControls>[0]["phase"], address = "") => renderToStaticMarkup(createElement(VpnControls, {
    phase, address, error: "", onVpn() {}, onAddress() {}, onSave() {},
  }));
  it("offers VPN recovery and announces progress while checking", () => {
    expect(render("idle")).toContain("Connect through VPN");
    expect(render("checking")).toContain("Checking the VPN connection");
    expect(render("connecting")).toContain("Connecting to your node through VPN");
  });
  it("offers a retry when NetBird is disconnected", () => {
    expect(render("down")).toContain("Retry VPN connection");
    expect(render("down")).toContain("NetBird is disconnected");
    expect(render("unreachable")).toContain("node did not answer");
  });
  it("explains enrolment, links the guide and accepts a named address", () => {
    expect(render("unenrolled")).toContain("Enrol this computer once at home");
    expect(render("unenrolled")).toContain('href="#vpn-enrolment"');
    expect(render("unenrolled", "model.example")).toContain("model.example");
    expect(render("saving")).toContain("Saving the node address");
  });
});
