import en from "./en.json";
import ptBR from "./pt-BR.json";

export type TextKey = keyof typeof en;

const dictionaries: Record<string, Record<TextKey, string>> = { en, "pt-BR": ptBR };

/** Picks the closest translation for a browser language tag. */
export function pickLocale(language: string): keyof typeof dictionaries {
  return language.toLowerCase().startsWith("pt") ? "pt-BR" : "en";
}

/** The language used for numbers and dates, too. */
export const locale = pickLocale(typeof navigator === "undefined" ? "en" : navigator.language);

export function translate(
  language: string,
  key: TextKey,
  values?: Record<string, string | number>,
): string {
  const template = dictionaries[language][key];
  if (!values) {
    return template;
  }
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in values ? String(values[name]) : match,
  );
}

export function t(key: TextKey, values?: Record<string, string | number>): string {
  return translate(locale, key, values);
}
