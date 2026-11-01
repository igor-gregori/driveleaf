import { describe, expect, it } from "vitest";
import { TILE_BYTES, TileBuffer } from "./tiles";

const FOLDER = 1 << 16;

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

describe("TileBuffer", () => {
  const buffer = new TileBuffer(
    encode([
      [0, 0, 100, 100, FOLDER, 1],
      [10, 10, 40, 80, 3, 7],
      [50, 10, 40, 80, 4, 9],
    ]),
  );

  it("counts tiles", () => {
    expect(buffer.count).toBe(3);
  });

  it("finds the deepest tile under a point", () => {
    expect(buffer.nodeOf(buffer.indexAt(20, 20))).toBe(7);
    expect(buffer.nodeOf(buffer.indexAt(60, 20))).toBe(9);
    expect(buffer.nodeOf(buffer.indexAt(5, 5))).toBe(1);
    expect(buffer.indexAt(150, 5)).toBe(-1);
    expect(buffer.indexAt(-1, 5)).toBe(-1);
  });

  it("treats the right and bottom edges as outside", () => {
    expect(buffer.nodeOf(buffer.indexAt(50, 20))).toBe(9);
    expect(buffer.indexAt(100, 50)).toBe(-1);
  });

  it("knows folders from files", () => {
    expect(buffer.isFolder(0)).toBe(true);
    expect(buffer.isFolder(1)).toBe(false);
  });

  it("gives back rectangles and finds nodes", () => {
    expect(buffer.rectOf(1)).toEqual({ x: 10, y: 10, w: 40, h: 80 });
    expect(buffer.indexOfNode(9)).toBe(2);
    expect(buffer.indexOfNode(123)).toBe(-1);
  });

  it("rejects a truncated buffer", () => {
    expect(() => new TileBuffer(new ArrayBuffer(TILE_BYTES + 1))).toThrow();
  });

  it("handles an empty buffer", () => {
    expect(new TileBuffer(new ArrayBuffer(0)).indexAt(0, 0)).toBe(-1);
  });
});
