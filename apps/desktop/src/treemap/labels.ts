import type { Palette } from "./renderer";
import type { TileBuffer } from "./tiles";

/** Labels cost an IPC round trip and a text draw each, so only the largest get one. */
export const MAX_LABELS = 150;
const MIN_WIDTH = 56;
const MIN_HEIGHT = 18;
/** Tall enough for the size on a second line. */
const TWO_LINE_HEIGHT = 34;
const PADDING = 5;
const NAME_FONT = '600 11px "Geist Variable", sans-serif';
const DETAIL_FONT = '400 10px "Geist Variable", sans-serif';

export interface Label {
  index: number;
  name: string;
  detail: string;
}

/**
 * The largest tiles with nothing drawn on top of them, biggest first. Opened
 * folders are left out, since their contents cover them.
 */
export function labelCandidates(tiles: TileBuffer): number[] {
  const found: { index: number; area: number }[] = [];
  for (let index = 0; index < tiles.count; index++) {
    if (tiles.isOpened(index)) continue;
    const { w, h } = tiles.rectOf(index);
    if (w >= MIN_WIDTH && h >= MIN_HEIGHT) found.push({ index, area: w * h });
  }
  found.sort((a, b) => b.area - a.area);
  return found.slice(0, MAX_LABELS).map((entry) => entry.index);
}

/** Cuts text to fit a width, ending in an ellipsis. */
export function fitText(text: string, maxWidth: number, measure: (text: string) => number): string {
  if (measure(text) <= maxWidth) return text;
  let low = 0;
  let high = text.length;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if (measure(`${text.slice(0, middle)}…`) <= maxWidth) {
      low = middle;
    } else {
      high = middle - 1;
    }
  }
  return low === 0 ? "" : `${text.slice(0, low)}…`;
}

function isLight([red, green, blue]: [number, number, number]): boolean {
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue > 0.6;
}

/** Draws labels in layout space, which matches the canvas while the view is at rest. */
export function drawLabels(
  context: CanvasRenderingContext2D,
  tiles: TileBuffer,
  labels: Label[],
  palette: Palette,
  pixelRatio: number,
): void {
  context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
  context.textBaseline = "top";
  for (const { index, name, detail } of labels) {
    if (index >= tiles.count) continue;
    const rect = tiles.rectOf(index);
    const maxWidth = rect.w - 2 * PADDING;
    const light = isLight(palette.colors[tiles.categoryOf(index)] ?? palette.colors[0]);
    context.fillStyle = light ? "rgba(0, 0, 0, 0.78)" : "rgba(255, 255, 255, 0.96)";
    context.shadowColor = light ? "transparent" : "rgba(0, 0, 0, 0.35)";
    context.shadowBlur = light ? 0 : 2;

    context.font = NAME_FONT;
    const text = fitText(name, maxWidth, (value) => context.measureText(value).width);
    if (!text) continue;
    context.fillText(text, rect.x + PADDING, rect.y + PADDING);
    if (rect.h >= TWO_LINE_HEIGHT) {
      context.font = DETAIL_FONT;
      context.globalAlpha = 0.8;
      const size = fitText(detail, maxWidth, (value) => context.measureText(value).width);
      context.fillText(size, rect.x + PADDING, rect.y + PADDING + 14);
      context.globalAlpha = 1;
    }
  }
  context.shadowColor = "transparent";
}
