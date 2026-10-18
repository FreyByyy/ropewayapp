import { isHex } from "./color";
import { FLUENT } from "./builtin";
import { DEFAULT_APPEARANCE, type Appearance, type Base, type IconSet, type Layout, type Mode, type ThemeDef } from "./types";

const BASES: Base[] = ["fluent", "material", "neu", "glass", "retro", "ropeway"];
const LAYOUTS: Layout[] = ["navview", "rail", "soft", "sidebar"];
const ICONS: IconSet[] = ["fluent", "material", "puffy", "glass", "retro", "ropeway"];
const COLOR_KEYS = ["bg", "surface", "sidebar", "text", "muted", "border"] as const;

const str = (v: unknown, max = 200) => (typeof v === "string" ? v.slice(0, max) : undefined);
const pick = <T extends string>(v: unknown, list: T[], fallback: T): T =>
  list.includes(v as T) ? (v as T) : fallback;

/** CSS color values only: no `;`, braces or url(), so a value cannot inject rules. */
function safeColor(v: unknown): string | undefined {
  const s = str(v, 80)?.trim();
  if (!s) return undefined;
  return /^[#a-zA-Z0-9(),.%\s-]+$/.test(s) && !/url/i.test(s) ? s : undefined;
}

/** Accepts anything shaped like a theme and returns a complete, safe one. */
export function normalizeTheme(raw: unknown): ThemeDef | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  if (r.format !== "ropeway-theme" || typeof r.id !== "string" || typeof r.name !== "string") return null;
  const modes = (Array.isArray(r.modes) ? r.modes : ["light", "dark"]).filter(
    (m): m is Mode => m === "light" || m === "dark",
  );
  const colors: ThemeDef["colors"] = {};
  const rawColors = (r.colors ?? {}) as Record<string, Record<string, unknown>>;
  for (const mode of ["light", "dark"] as Mode[]) {
    const src = rawColors[mode];
    if (!src || typeof src !== "object") continue;
    const out: Record<string, string> = {};
    for (const k of COLOR_KEYS) {
      const c = safeColor(src[k]);
      if (c) out[k] = c;
    }
    colors[mode] = out;
  }
  const font = str(r.font, 200)?.replace(/[;{}]/g, "") || FLUENT.font;
  return {
    format: "ropeway-theme",
    version: 1,
    id: r.id.replace(/[^a-zA-Z0-9_-]/g, "").slice(0, 64) || "theme",
    name: r.name.slice(0, 60),
    author: str(r.author, 60),
    description: str(r.description, 200),
    base: pick(r.base, BASES, "fluent"),
    layout: pick(r.layout, LAYOUTS, "navview"),
    icons: pick(r.icons, ICONS, "fluent"),
    modes: modes.length ? [...new Set(modes)] : ["light"],
    accent: typeof r.accent === "string" && isHex(r.accent) ? r.accent : FLUENT.accent,
    colors,
    radius: typeof r.radius === "number" ? Math.max(0, Math.min(32, r.radius)) : 8,
    font,
    motion: typeof r.motion === "number" ? Math.max(0.25, Math.min(3, r.motion)) : 1,
    mica: r.mica === true,
    css: str(r.css, 200_000),
  };
}

export function normalizeAppearance(raw: unknown): Appearance {
  if (!raw || typeof raw !== "object") return DEFAULT_APPEARANCE;
  const r = raw as Record<string, unknown>;
  const accent =
    r.accent === "theme" || r.accent === "windows" || (typeof r.accent === "string" && isHex(r.accent))
      ? (r.accent as string)
      : DEFAULT_APPEARANCE.accent;
  return {
    theme: typeof r.theme === "string" ? r.theme : DEFAULT_APPEARANCE.theme,
    mode: r.mode === "light" || r.mode === "dark" || r.mode === "system" ? r.mode : "system",
    accent,
    motion: r.motion === "reduced" || r.motion === "off" || r.motion === "full" ? r.motion : "full",
  };
}

export function newThemeId() {
  return `custom-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
}
