import { describe, expect, it } from "vitest";
import { TILE_BYTES, TileBuffer } from "./tiles";

function encode(tiles: [number, number, number, number, number, number][]): ArrayBuffer {
  const bytes = new ArrayBuffer(tiles.length * TILE_BYTES);
  const view = new DataView(bytes);
  tiles.forEach(([x, y, w, h, color, nodeId], index) => {
    const base = index * TILE_BYTES;
    view.setFloat32(base, x, true);
    view.setFloat32(base + 4, y, true);
    view.setFloat32(base + 8, w, true);
    view.setFloat32(base + 12, h, true);
    view.setUint32(base + 16, color, true);
    view.setUint32(base + 20, nodeId, true);
  });
  return bytes;
}

describe("TileBuffer", () => {
  const buffer = new TileBuffer(
    encode([
      [0, 0, 10, 10, 0x112233ff, 7],
      [10, 0, 5, 10, 0x445566ff, 9],
    ]),
  );

  it("counts tiles", () => {
    expect(buffer.count).toBe(2);
  });

  it("hit tests against the raw buffer", () => {
    expect(buffer.nodeAt(1, 1)).toBe(7);
    expect(buffer.nodeAt(12, 5)).toBe(9);
    expect(buffer.nodeAt(15, 5)).toBeNull();
    expect(buffer.nodeAt(-1, 5)).toBeNull();
  });

  it("treats the right and bottom edges as outside", () => {
    expect(buffer.nodeAt(10, 5)).toBe(9);
    expect(buffer.nodeAt(5, 10)).toBeNull();
  });

  it("rejects a truncated buffer", () => {
    expect(() => new TileBuffer(new ArrayBuffer(TILE_BYTES + 1))).toThrow();
  });

  it("handles an empty buffer", () => {
    expect(new TileBuffer(new ArrayBuffer(0)).nodeAt(0, 0)).toBeNull();
  });
});
