import { describe, expect, it } from "vitest";
import { formatBytes, formatCount, formatDate, formatSeconds, formatShare } from "./format";

describe("formatBytes", () => {
  it("keeps small sizes in bytes", () => {
    expect(formatBytes(0, "en")).toBe("0 B");
    expect(formatBytes(1023, "en")).toBe("1,023 B");
  });

  it("uses powers of 1024 with sensible precision", () => {
    expect(formatBytes(1024, "en")).toBe("1.00 KB");
    expect(formatBytes(1536, "en")).toBe("1.50 KB");
    expect(formatBytes(15 * 1024 * 1024, "en")).toBe("15.0 MB");
    expect(formatBytes(150 * 1024 ** 3, "en")).toBe("150 GB");
    expect(formatBytes(2 * 1024 ** 4, "en")).toBe("2.00 TB");
  });

  it("follows the locale", () => {
    expect(formatBytes(1536, "pt-BR")).toBe("1,50 KB");
  });

  it("does not run out of units", () => {
    expect(formatBytes(2 * 1024 ** 6, "en")).toBe("2,048 PB");
  });
});

describe("the other formats", () => {
  it("formats counts and shares", () => {
    expect(formatCount(1234567, "en")).toBe("1,234,567");
    expect(formatShare(0.1234, "en")).toBe("12.3%");
  });

  it("formats dates and tolerates unknown ones", () => {
    expect(formatDate(null, "en")).toBe("");
    expect(formatDate(Date.UTC(2026, 5, 15, 12), "en")).toContain("2026");
  });

  it("formats durations", () => {
    expect(formatSeconds(0.394, "en")).toBe("0.39 s");
    expect(formatSeconds(12.34, "en")).toBe("12.3 s");
  });
});
