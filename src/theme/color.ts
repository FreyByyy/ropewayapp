// Color math in OKLCH, so tones derived from any accent look even:
// a lightness of 0.9 is equally light whether the accent is blue or orange.

type Rgb = [number, number, number];

const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const toGamma = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

export function parseHex(hex: string): Rgb | null {
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join("") : m[1];
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255) as Rgb;
}

const toHex = ([r, g, b]: Rgb) =>
  "#" + [r, g, b].map((c) => Math.round(Math.min(1, Math.max(0, c)) * 255).toString(16).padStart(2, "0")).join("");

function rgbToOklch([r, g, b]: Rgb) {
  const [lr, lg, lb] = [toLinear(r), toLinear(g), toLinear(b)];
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return { L, C: Math.hypot(a, bb), h: (Math.atan2(bb, a) * 180) / Math.PI };
}

function oklchToRgb(L: number, C: number, h: number): Rgb {
  const a = C * Math.cos((h * Math.PI) / 180);
  const b = C * Math.sin((h * Math.PI) / 180);
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    toGamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    toGamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    toGamma(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
  ];
}

const inGamut = (rgb: Rgb) => rgb.every((c) => c >= -0.0005 && c <= 1.0005);

/**
 * The accent's hue at lightness `L` (0..1) and chroma `C` (`undefined`: the
 * accent's own chroma). Chroma is reduced until the color fits sRGB.
 */
export function tone(accent: string, L: number, C?: number): string {
  const rgb = parseHex(accent) ?? [0, 0.4, 0.75];
  const base = rgbToOklch(rgb);
  let c = C ?? base.C;
  let out = oklchToRgb(L, c, base.h);
  for (let i = 0; i < 24 && !inGamut(out); i++) {
    c *= 0.88;
    out = oklchToRgb(L, c, base.h);
  }
  return toHex(out);
}

/** Like `tone`, with the hue turned by `dh` degrees. */
export function toneAt(accent: string, dh: number, L: number, C: number): string {
  const rgb = parseHex(accent) ?? [0, 0.4, 0.75];
  const h = rgbToOklch(rgb).h + dh;
  let c = C;
  let out = oklchToRgb(L, c, h);
  for (let i = 0; i < 24 && !inGamut(out); i++) {
    c *= 0.88;
    out = oklchToRgb(L, c, h);
  }
  return toHex(out);
}

export function lightness(color: string): number {
  const rgb = parseHex(color);
  return rgb ? rgbToOklch(rgb).L : 0.5;
}

/** Moves a color's lightness by `delta`, keeping hue and chroma. */
export function shift(color: string, delta: number): string {
  const rgb = parseHex(color);
  if (!rgb) return color;
  return tone(color, Math.min(1, Math.max(0, rgbToOklch(rgb).L + delta)));
}

export function alpha(color: string, a: number): string {
  const rgb = parseHex(color);
  if (!rgb) return color;
  return `rgba(${rgb.map((c) => Math.round(c * 255)).join(", ")}, ${a})`;
}

/** Readable text color (white or near-black) on top of `bg`. */
export function onColor(bg: string): string {
  return lightness(bg) > 0.68 ? "#14171c" : "#ffffff";
}

export function isHex(s: string) {
  return parseHex(s) !== null;
}
