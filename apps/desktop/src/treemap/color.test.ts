import { describe, expect, it } from "vitest";
import { parseHexColor } from "./color";

describe("parseHexColor", () => {
  it("reads six digits", () => {
    expect(parseHexColor("#ff0080")).toEqual([1, 0, 128 / 255]);
  });

  it("reads the short form a CSS minifier may produce", () => {
    expect(parseHexColor("#fff")).toEqual([1, 1, 1]);
    expect(parseHexColor(" #0f0 ")).toEqual([0, 1, 0]);
  });
});
