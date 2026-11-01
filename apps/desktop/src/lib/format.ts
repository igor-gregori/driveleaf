const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** Sizes in powers of 1024, labeled like Windows Explorer labels them. */
export function formatBytes(bytes: number, locale: string): string {
  if (bytes < 1024) {
    return `${bytes.toLocaleString(locale)} B`;
  }
  const unit = Math.min(Math.floor(Math.log2(bytes) / 10), UNITS.length - 1);
  const value = bytes / 1024 ** unit;
  const digits = value < 10 ? 2 : value < 100 ? 1 : 0;
  const text = value.toLocaleString(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
  return `${text} ${UNITS[unit]}`;
}

export function formatCount(count: number, locale: string): string {
  return count.toLocaleString(locale);
}

export function formatShare(share: number, locale: string): string {
  return share.toLocaleString(locale, {
    style: "percent",
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  });
}

export function formatDate(millis: number | null, locale: string): string {
  if (millis === null) {
    return "";
  }
  return new Date(millis).toLocaleDateString(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export function formatSeconds(seconds: number, locale: string): string {
  return `${seconds.toLocaleString(locale, {
    minimumFractionDigits: seconds < 10 ? 2 : 1,
    maximumFractionDigits: seconds < 10 ? 2 : 1,
  })} s`;
}
