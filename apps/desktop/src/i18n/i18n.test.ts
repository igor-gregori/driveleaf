import { describe, expect, it } from "vitest";
import en from "./en.json";
import ptBR from "./pt-BR.json";
import { pickLocale, translate } from "./index";

const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();

describe("translations", () => {
  it("cover the same keys", () => {
    expect(Object.keys(ptBR).sort()).toEqual(Object.keys(en).sort());
  });

  it("use the same placeholders", () => {
    for (const [key, text] of Object.entries(en)) {
      expect(placeholders(ptBR[key as keyof typeof ptBR]), key).toEqual(placeholders(text));
    }
  });

  it("are all filled in", () => {
    for (const dictionary of [en, ptBR]) {
      for (const text of Object.values(dictionary)) {
        expect(text.trim()).not.toBe("");
      }
    }
  });
});

describe("translate", () => {
  it("fills in values", () => {
    expect(translate("en", "scan.counted", { count: "1,000" })).toBe("1,000 entries so far");
    expect(translate("pt-BR", "scan.counted", { count: "1.000" })).toBe("1.000 itens até agora");
  });

  it("leaves unknown placeholders visible", () => {
    expect(translate("en", "scan.counted")).toContain("{count}");
  });

  it("picks a language", () => {
    expect(pickLocale("pt-BR")).toBe("pt-BR");
    expect(pickLocale("pt-PT")).toBe("pt-BR");
    expect(pickLocale("en-US")).toBe("en");
    expect(pickLocale("de")).toBe("en");
  });
});
