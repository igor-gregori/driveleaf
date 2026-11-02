import { describe, expect, it } from "vitest";
import { fitText, labelCandidates } from "./labels";
import { TILE_BYTES, TileBuffer } from "./tiles";

const FOLDER = 1 << 16;
const OPENED = 1 << 17;

function encode(tiles: [number, number, number, number, number, number][]): ArrayBuffer {
  const bytes = new ArrayBuffer(tiles.length * TILE_BYTES);
  const view = new DataView(bytes);
  tiles.forEach(([x, y, w, h, style, nodeId], index) => {
    const base = index * TILE_BYTES;
    view.setFloat32(base, x, true);
    view.setFloat32(base + 4, y, true);
    view.setFloat32(base + 8, w, true);
    view.setFloat32(base + 12, h, true);
    view.setUint32(base + 16, style, true);
    view.setUint32(base + 20, nodeId, true);
  });
  return bytes;
}

describe("labelCandidates", () => {
  it("names the biggest uncovered tiles first", () => {
    const tiles = new TileBuffer(
      encode([
        [0, 0, 800, 600, FOLDER | OPENED, 1],
        [0, 0, 300, 200, 3, 2],
        [300, 0, 500, 400, FOLDER, 3],
        [0, 200, 20, 10, 3, 4],
      ]),
    );
    expect(labelCandidates(tiles)).toEqual([2, 1]);
  });
});

describe("fitText", () => {
  const measure = (text: string) => text.length * 10;

  it("keeps text that fits", () => {
    expect(fitText("report.pdf", 100, measure)).toBe("report.pdf");
  });

  it("cuts the rest with an ellipsis", () => {
    expect(fitText("a-very-long-name.iso", 60, measure)).toBe("a-ver…");
  });

  it("gives up when not even one letter fits", () => {
    expect(fitText("abc", 5, measure)).toBe("");
  });
});
