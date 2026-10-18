const UNITS = {
  ru: ["Б", "КБ", "МБ", "ГБ", "ТБ"],
  en: ["B", "KB", "MB", "GB", "TB"],
};

export function fmtSize(bytes: number, lang: string) {
  const units = lang === "en" ? UNITS.en : UNITS.ru;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toLocaleString(lang, { maximumFractionDigits: digits, minimumFractionDigits: digits })} ${units[unit]}`;
}

export function fmtSpeed(bytesPerSec: number, lang: string) {
  return `${fmtSize(bytesPerSec, lang)}/${lang === "en" ? "s" : "с"}`;
}

export function fmtDate(ms: number, lang: string) {
  const d = new Date(ms);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  return sameDay
    ? d.toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleDateString(lang, {
        day: "numeric",
        month: "short",
        year: d.getFullYear() === now.getFullYear() ? undefined : "numeric",
      });
}

/** "3 файла", "1 file" … */
export function plural(n: number, lang: string, forms: { ru: [string, string, string]; en: [string, string] }) {
  if (lang === "en") return `${n} ${n === 1 ? forms.en[0] : forms.en[1]}`;
  const mod10 = n % 10;
  const mod100 = n % 100;
  const form = mod10 === 1 && mod100 !== 11 ? 0 : mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14) ? 1 : 2;
  return `${n} ${forms.ru[form]}`;
}

export const FILES = { ru: ["файл", "файла", "файлов"] as [string, string, string], en: ["file", "files"] as [string, string] };
