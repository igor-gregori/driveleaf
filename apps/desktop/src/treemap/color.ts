/** Reads a `#rgb` or `#rrggbb` color into channels from 0 to 1. */
export function parseHexColor(text: string): [number, number, number] {
  const hex = text.trim().replace(/^#/, "");
  const full = hex.length === 3 ? [...hex].map((digit) => digit + digit).join("") : hex;
  const value = Number.parseInt(full, 16);
  return [((value >> 16) & 255) / 255, ((value >> 8) & 255) / 255, (value & 255) / 255];
}
