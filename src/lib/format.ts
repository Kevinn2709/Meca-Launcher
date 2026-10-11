const LOCALE = "es-MX";

const oneDecimal = new Intl.NumberFormat(LOCALE, { maximumFractionDigits: 1 });
const integer = new Intl.NumberFormat(LOCALE, { maximumFractionDigits: 0 });
const longDate = new Intl.DateTimeFormat(LOCALE, {
  day: "numeric",
  month: "long",
  year: "numeric",
  timeZone: "UTC",
});

/** 222298112 -> "212 MB", 1932735283 -> "1.8 GB" */
export function formatBytes(bytes: number): string {
  const mb = bytes / 1024 / 1024;
  if (mb >= 1024) return `${oneDecimal.format(mb / 1024)} GB`;
  if (mb >= 10) return `${integer.format(mb)} MB`;
  return `${oneDecimal.format(mb)} MB`;
}

/** 13002342 -> "12.4 MB/s" */
export function formatSpeed(bytesPerSecond: number): string {
  return `${oneDecimal.format(bytesPerSecond / 1024 / 1024)} MB/s`;
}

export function formatCount(value: number): string {
  return integer.format(value);
}

/** "2026-10-05" -> "5 de octubre de 2026" */
export function formatDate(isoDate: string): string {
  const date = new Date(`${isoDate}T00:00:00Z`);
  return Number.isNaN(date.getTime()) ? isoDate : longDate.format(date);
}
