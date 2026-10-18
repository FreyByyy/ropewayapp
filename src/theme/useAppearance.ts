import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../api";
import { BUILTIN_THEMES, FLUENT } from "./builtin";
import { resolveMode, themeVars } from "./derive";
import { normalizeAppearance, normalizeTheme } from "./normalize";
import { DEFAULT_APPEARANCE, type Appearance, type Mode, type MotionChoice, type ThemeDef } from "./types";
import { IS_MOBILE, NOT_WINDOWS } from "../platform";

const media = (q: string) => window.matchMedia(q);

/** `rgb(…)` / `#rrggbb` to `#rrggbb`; transparent and unknown give null. */
function toHex(color: string): string | null {
  if (/^#[0-9a-f]{6}$/i.test(color)) return color;
  const m = /^rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)$/.exec(color);
  if (!m || (m[4] !== undefined && Number(m[4]) < 0.5)) return null;
  return "#" + m.slice(1, 4).map((v) => Number(v).toString(16).padStart(2, "0")).join("");
}

function styleTag(id: string) {
  let el = document.getElementById(id) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement("style");
    el.id = id;
    document.head.appendChild(el);
  }
  return el;
}

/** Writes a theme onto the document: attributes for the CSS, variables, custom CSS. */
export function applyTheme(theme: ThemeDef, mode: Mode, accent: string, motion: MotionChoice, mica: boolean) {
  const root = document.documentElement;
  root.dataset.base = theme.base;
  root.dataset.layout = theme.layout;
  root.dataset.mode = mode;
  root.dataset.motion = motion;
  root.dataset.mica = String(mica);
  root.style.colorScheme = mode;
  const vars = themeVars(theme, mode, accent, mica);
  const level = motion === "off" ? 0 : motion === "reduced" ? 0.5 : 1;
  styleTag("rw-theme-vars").textContent =
    `html:root{${Object.entries(vars)
      .map(([k, v]) => `${k}:${v};`)
      .join("")}--motion:${level};}`;
  // Appended after the app's styles, so a theme can override anything.
  styleTag("rw-theme-css").textContent = theme.css ?? "";
}

const RIPPLE_TARGETS =
  ".btn, .nav-item, .tool, .foot-btn, .rail-item, .rail-fab, .source, .source-row, .icon-btn, .send, .segmented button, .menu button, .theme-card";

/** Material's touch ripple, drawn from the pointer position. */
function onRipplePointer(e: PointerEvent) {
  const el = (e.target as HTMLElement | null)?.closest<HTMLElement>(RIPPLE_TARGETS);
  if (!el || (el as HTMLButtonElement).disabled) return;
  const rect = el.getBoundingClientRect();
  const size = Math.max(rect.width, rect.height) * 2.2;
  const dot = document.createElement("span");
  dot.className = "ripple";
  dot.style.width = dot.style.height = `${size}px`;
  dot.style.left = `${e.clientX - rect.left - size / 2}px`;
  dot.style.top = `${e.clientY - rect.top - size / 2}px`;
  el.appendChild(dot);
  const remove = () => dot.remove();
  dot.addEventListener("animationend", remove);
  setTimeout(remove, 1200);
}

export function useAppearance() {
  const [appearance, setAppearanceState] = useState<Appearance>(DEFAULT_APPEARANCE);
  const [custom, setCustom] = useState<ThemeDef[]>([]);
  const [windowsAccent, setWindowsAccent] = useState<string | null>(null);
  const [systemDark, setSystemDark] = useState(() => media("(prefers-color-scheme: dark)").matches);
  const [systemReduced, setSystemReduced] = useState(() => media("(prefers-reduced-motion: reduce)").matches);
  const [preview, setPreview] = useState<ThemeDef | null>(null);
  const [loaded, setLoaded] = useState(false);

  const reloadThemes = useCallback(async () => {
    const list = await api.listThemes().catch(() => []);
    setCustom(list.map(normalizeTheme).filter((t): t is ThemeDef => t !== null));
  }, []);

  useEffect(() => {
    Promise.all([api.getAppearance().catch(() => null), reloadThemes(), api.windowsAccent().catch(() => null)]).then(
      ([saved, , accent]) => {
        setAppearanceState(normalizeAppearance(saved));
        setWindowsAccent(accent);
        setLoaded(true);
      },
    );
    const dark = media("(prefers-color-scheme: dark)");
    const reduced = media("(prefers-reduced-motion: reduce)");
    const onDark = () => setSystemDark(dark.matches);
    const onReduced = () => setSystemReduced(reduced.matches);
    // The Windows accent can change while the app runs.
    const onFocus = () => api.windowsAccent().then(setWindowsAccent).catch(() => {});
    dark.addEventListener("change", onDark);
    reduced.addEventListener("change", onReduced);
    window.addEventListener("focus", onFocus);
    return () => {
      dark.removeEventListener("change", onDark);
      reduced.removeEventListener("change", onReduced);
      window.removeEventListener("focus", onFocus);
    };
  }, [reloadThemes]);

  const themes = useMemo(() => [...BUILTIN_THEMES, ...custom], [custom]);
  const theme = preview ?? themes.find((t) => t.id === appearance.theme) ?? FLUENT;
  const mode = resolveMode(theme, appearance.mode === "system" ? (systemDark ? "dark" : "light") : appearance.mode);
  const accent =
    appearance.accent === "windows" || (appearance.accent === "theme" && theme.base === "fluent")
      ? (windowsAccent ?? theme.accent)
      : appearance.accent === "theme"
        ? theme.accent
        : appearance.accent;
  // Windows' "show animations" off counts as "reduced" unless the user chose off.
  const motion: MotionChoice = appearance.motion === "full" && systemReduced ? "reduced" : appearance.motion;

  // Mica is a Windows window material; phones draw the theme's background.
  const mica = theme.mica && !NOT_WINDOWS;
  useLayoutEffect(() => {
    applyTheme(theme, mode, accent, motion, mica);
  }, [theme, mode, accent, motion, mica]);

  // Phone: the status and navigation bars take the page's background.
  useEffect(() => {
    if (!IS_MOBILE) return;
    const bg = theme.base === "retro" ? "#008080" : getComputedStyle(document.body).backgroundColor;
    api.setSystemBars(toHex(bg) ?? (mode === "dark" ? "#000000" : "#ffffff"), mode === "dark").catch(() => {});
  }, [theme, mode, accent]);

  // The backdrop belongs to the window, not the page.
  const backdrop = useRef("");
  useEffect(() => {
    const key = `${mica}:${mode}`;
    if (backdrop.current === key) return;
    backdrop.current = key;
    api.setBackdrop(mica, mode === "dark").catch(() => {});
  }, [mica, mode]);

  // Title bar follows an explicit choice; "system" leaves it to Windows so
  // prefers-color-scheme keeps reporting the OS setting.
  const windowTheme = appearance.mode === "system" ? null : mode;
  useEffect(() => {
    getCurrentWindow().setTheme(windowTheme).catch(() => {});
  }, [windowTheme]);

  useEffect(() => {
    if (theme.base !== "material" || motion === "off") return;
    document.addEventListener("pointerdown", onRipplePointer);
    return () => document.removeEventListener("pointerdown", onRipplePointer);
  }, [theme.base, motion]);

  const update = useCallback((patch: Partial<Appearance>) => {
    setAppearanceState((prev) => {
      const next = { ...prev, ...patch };
      api.setAppearance(next).catch(console.error);
      return next;
    });
  }, []);

  const saveTheme = useCallback(
    async (t: ThemeDef) => {
      await api.saveTheme(t);
      await reloadThemes();
    },
    [reloadThemes],
  );

  const deleteTheme = useCallback(
    async (id: string) => {
      await api.deleteTheme(id);
      await reloadThemes();
      setAppearanceState((prev) => {
        if (prev.theme !== id) return prev;
        const next = { ...prev, theme: FLUENT.id };
        api.setAppearance(next).catch(console.error);
        return next;
      });
    },
    [reloadThemes],
  );

  return {
    loaded,
    appearance,
    update,
    theme,
    mode,
    accent,
    motion,
    themes,
    custom,
    saveTheme,
    deleteTheme,
    setPreview,
    windowsAccent,
    systemDark,
  };
}

export type AppearanceApi = ReturnType<typeof useAppearance>;
