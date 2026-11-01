// The data folder's rule against the cases `ragon app` is held to too
// (ragon-rs/vectors/app-data-folder.json).

import { describe, expect, it } from "vitest";

import appConfig from "../app.json";
import { dataFolder } from "./data-folder";
import vectors from "../../../ragon-rs/vectors/app-data-folder.json";

interface Case {
  name: string;
  platform: string;
  env: Record<string, string>;
  expected: string | null;
}

describe("the data folder", () => {
  it("is named after the app's identifier", () => {
    expect(vectors.identifier).toBe(appConfig.identifier);
  });

  for (const each of vectors.cases as Case[]) {
    it(each.name, () => {
      expect(dataFolder(each.platform, each.env, vectors.identifier)).toBe(each.expected);
    });
  }
});
