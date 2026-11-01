/** Wire format of one tile: `x, y, w, h: f32`, then `color, nodeId: u32`, little-endian. */
export const TILE_BYTES = 24;

const TILE_WORDS = TILE_BYTES / 4;

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

  /** Finds the node under a layout-space point, with no IPC call. */
  nodeAt(x: number, y: number): number | null {
    const { floats, words } = this;
    for (let word = (this.count - 1) * TILE_WORDS; word >= 0; word -= TILE_WORDS) {
      const left = floats[word];
      const top = floats[word + 1];
      if (x >= left && y >= top && x < left + floats[word + 2] && y < top + floats[word + 3]) {
        return words[word + 5];
      }
    }
    return null;
  }
}
