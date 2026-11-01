/**
 * Wire format of one tile: `x, y, w, h: f32`, then `style, nodeId: u32`, little-endian.
 * The style holds the category in its low byte, the depth in the next one, and
 * bit 16 marks a folder.
 */
export const TILE_BYTES = 24;

const TILE_WORDS = TILE_BYTES / 4;
const STYLE_FOLDER = 1 << 16;

export interface TileRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Read-only view over a tile buffer received from Rust. */
export class TileBuffer {
  readonly count: number;
  private readonly floats: Float32Array;
  private readonly words: Uint32Array;

  constructor(readonly bytes: ArrayBuffer) {
    if (bytes.byteLength % TILE_BYTES !== 0) {
      throw new Error(`${bytes.byteLength} bytes is not a whole number of tiles`);
    }
    this.count = bytes.byteLength / TILE_BYTES;
    this.floats = new Float32Array(bytes);
    this.words = new Uint32Array(bytes);
  }

  /**
   * Finds the deepest tile under a layout-space point, with no IPC call.
   * Parents come before their children, so the last match is the deepest.
   */
  indexAt(x: number, y: number): number {
    const { floats } = this;
    for (let index = this.count - 1; index >= 0; index--) {
      const word = index * TILE_WORDS;
      const left = floats[word];
      const top = floats[word + 1];
      if (x >= left && y >= top && x < left + floats[word + 2] && y < top + floats[word + 3]) {
        return index;
      }
    }
    return -1;
  }

  nodeOf(index: number): number {
    return this.words[index * TILE_WORDS + 5];
  }

  isFolder(index: number): boolean {
    return (this.words[index * TILE_WORDS + 4] & STYLE_FOLDER) !== 0;
  }

  rectOf(index: number): TileRect {
    const word = index * TILE_WORDS;
    const { floats } = this;
    return { x: floats[word], y: floats[word + 1], w: floats[word + 2], h: floats[word + 3] };
  }

  indexOfNode(node: number): number {
    for (let index = 0; index < this.count; index++) {
      if (this.nodeOf(index) === node) {
        return index;
      }
    }
    return -1;
  }
}
