// A theme is a JSON document (`.ropeway-theme`). Built-in themes use the
// same format, so any of them can be the starting point for a new one.

import { IS_LINUX, IS_MAC, IS_MOBILE } from "../platform";

/** Component styling family: shapes, shadows, motion character. */
export type Base = "fluent" | "material" | "neu" | "glass" | "retro" | "ropeway";
/** Arrangement of navigation and content. */
export type Layout = "navview" | "rail" | "soft" | "sidebar";
export type IconSet = "fluent" | "material" | "puffy" | "glass" | "retro" | "ropeway";
export type Mode = "light" | "dark";

/** Colors a theme sets per mode; anything left out is derived. */
export interface ModeColors {
  /** Window background. */
  bg?: string;
  /** Cards, message bubbles, dialogs. */
  surface?: string;
  /** Navigation / device list pane. */
  sidebar?: string;
  text?: string;
  muted?: string;
  border?: string;
}

export interface ThemeDef {
  format: "ropeway-theme";
  version: 1;
  id: string;
  name: string;
  author?: string;
  description?: string;
  base: Base;
  layout: Layout;
  icons: IconSet;
  /** Modes the theme supports; the first is used when the other is asked for. */
  modes: Mode[];
  /** Accent used when the user keeps "theme color". */
  accent: string;
  colors: Partial<Record<Mode, ModeColors>>;
  /** Corner radius of cards and dialogs, px. Smaller parts scale from it. */
  radius: number;
  /** CSS font-family list. */
  font: string;
  /** Animation speed: 1 normal, 0.5 twice as fast, 2 twice as slow. */
  motion: number;
  /** Windows 11 Mica backdrop. */
  mica: boolean;
  /** Extra CSS appended after the theme's own styles. */
  css?: string;
}

export type AccentChoice = "theme" | "windows" | string;
export type ModeChoice = "system" | Mode;
export type MotionChoice = "full" | "reduced" | "off";

export interface Appearance {
  theme: string;
  mode: ModeChoice;
  accent: AccentChoice;
  motion: MotionChoice;
}

/** Fluent on Windows, Material on phones, Ropeway's own look on Linux,
 *  Liquid Glass on the Mac. */
export const DEFAULT_APPEARANCE: Appearance = {
  theme: IS_MOBILE ? "material" : IS_LINUX ? "ropeway" : IS_MAC ? "liquid-glass" : "fluent",
  mode: "system",
  accent: "theme",
  motion: "full",
};
