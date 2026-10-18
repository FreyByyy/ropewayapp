import type { ThemeDef } from "./types";

const SEGOE = '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';

export const FLUENT: ThemeDef = {
  format: "ropeway-theme",
  version: 1,
  id: "fluent",
  name: "Fluent",
  description: "Как приложения Windows 11: Mica, спокойные слои, навигация слева",
  base: "fluent",
  layout: "navview",
  icons: "fluent",
  modes: ["light", "dark"],
  accent: "#0067c0",
  colors: {
    light: {
      bg: "#f3f3f3",
      surface: "rgba(255, 255, 255, 0.72)",
      sidebar: "transparent",
      text: "#1b1b1b",
      muted: "#5f5f5f",
      border: "rgba(0, 0, 0, 0.08)",
    },
    dark: {
      bg: "#202020",
      surface: "rgba(255, 255, 255, 0.05)",
      sidebar: "transparent",
      text: "#ffffff",
      muted: "#c8c8c8",
      border: "rgba(255, 255, 255, 0.08)",
    },
  },
  radius: 8,
  font: SEGOE,
  motion: 1,
  mica: true,
};

export const MATERIAL: ThemeDef = {
  format: "ropeway-theme",
  version: 1,
  id: "material",
  name: "Material",
  description: "Material You: палитра из акцента, навигационный рельс, капсулы",
  base: "material",
  layout: "rail",
  icons: "material",
  modes: ["light", "dark"],
  accent: "#6750a4",
  // Everything else is a tonal palette of the accent.
  colors: {},
  radius: 16,
  font: '"Roboto", "Segoe UI", system-ui, sans-serif',
  motion: 1,
  mica: false,
};

export const NEUMORPHISM: ThemeDef = {
  format: "ropeway-theme",
  version: 1,
  id: "neumorphism",
  name: "Neumorphism",
  description: "Одна мягкая поверхность, выдавленные панели и вдавливающиеся кнопки",
  base: "neu",
  layout: "soft",
  icons: "puffy",
  modes: ["light", "dark"],
  accent: "#5b6cf0",
  colors: {
    light: { bg: "#dde3eb", text: "#2c3440", muted: "#667080" },
    dark: { bg: "#2a2e35", text: "#e2e6ec", muted: "#99a2ae" },
  },
  radius: 20,
  font: '"Nunito", "Segoe UI", system-ui, sans-serif',
  motion: 1,
  mica: false,
};

export const LIQUID_GLASS: ThemeDef = {
  format: "ropeway-theme",
  version: 1,
  id: "liquid-glass",
  name: "Liquid Glass",
  description: "Стеклянные панели с бликами по краям над цветными обоями из акцента",
  base: "glass",
  layout: "soft",
  icons: "glass",
  modes: ["light", "dark"],
  accent: "#0a84ff",
  // Panes are translucent; the wallpaper behind them comes from the accent.
  colors: {},
  radius: 26,
  font: '"Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif',
  motion: 1,
  mica: false,
};

export const RETRO: ThemeDef = {
  format: "ropeway-theme",
  version: 1,
  id: "retro",
  name: "Retro",
  description: "Windows 98: серые объёмные окна, синие заголовки, бирюзовый рабочий стол",
  base: "retro",
  layout: "soft",
  icons: "retro",
  modes: ["light"],
  accent: "#000080",
  colors: {},
  radius: 0,
  font: 'Tahoma, "Microsoft Sans Serif", "MS Sans Serif", sans-serif',
  motion: 1,
  mica: false,
};

export const ROPEWAY: ThemeDef = {
  format: "ropeway-theme",
  version: 1,
  id: "ropeway",
  name: "Ropeway",
  description: "Исходное оформление: бирюзовый акцент и список устройств слева",
  base: "ropeway",
  layout: "sidebar",
  icons: "ropeway",
  modes: ["light", "dark"],
  accent: "#0e7c6b",
  colors: {
    light: {
      bg: "#f4f6f7",
      surface: "#ffffff",
      sidebar: "#eaeef0",
      text: "#142023",
      muted: "#586a70",
      border: "#d9e0e3",
    },
    dark: {
      bg: "#111618",
      surface: "#171d20",
      sidebar: "#0d1214",
      text: "#e3eaec",
      muted: "#93a4a9",
      border: "#253033",
    },
  },
  radius: 12,
  font: SEGOE,
  motion: 1,
  mica: false,
};

export const BUILTIN_THEMES: ThemeDef[] = [FLUENT, MATERIAL, NEUMORPHISM, LIQUID_GLASS, RETRO, ROPEWAY];

export const FONT_CHOICES: { label: string; value: string }[] = [
  { label: "Segoe UI", value: SEGOE },
  { label: "Roboto", value: '"Roboto", "Segoe UI", system-ui, sans-serif' },
  { label: "Nunito", value: '"Nunito", "Segoe UI", system-ui, sans-serif' },
  { label: "Tahoma", value: 'Tahoma, "Microsoft Sans Serif", "MS Sans Serif", sans-serif' },
  { label: "Georgia", value: 'Georgia, "Times New Roman", serif' },
  { label: "Cascadia Mono", value: '"Cascadia Mono", Consolas, monospace' },
];

/** Accent swatches offered in settings. */
export const ACCENT_SWATCHES = [
  "#0067c0",
  "#6750a4",
  "#0e7c6b",
  "#2e7d32",
  "#c2185b",
  "#d84315",
  "#f59e0b",
  "#5b6cf0",
  "#00838f",
  "#455a64",
  "#000080",
  "#0a84ff",
];
