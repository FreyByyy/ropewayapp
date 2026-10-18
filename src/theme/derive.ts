import { alpha, lightness, onColor, shift, tone, toneAt } from "./color";
import type { Mode, ThemeDef } from "./types";

/**
 * Turns a theme, a mode and an accent into the CSS variables the app's
 * styles read. Colors the theme leaves out are derived from the accent.
 */
export function themeVars(t: ThemeDef, mode: Mode, accent: string, mica: boolean): Record<string, string> {
  const dark = mode === "dark";
  const c = t.colors[mode] ?? {};

  // Base colors.
  let bg: string, surface: string, sidebar: string, text: string, muted: string, border: string;
  if (t.base === "material") {
    // Material You: every surface is a faint tint of the accent's hue.
    bg = c.bg ?? (dark ? tone(accent, 0.145, 0.012) : tone(accent, 0.94, 0.016));
    surface = c.surface ?? (dark ? tone(accent, 0.225, 0.018) : tone(accent, 0.99, 0.006));
    sidebar = c.sidebar ?? (dark ? tone(accent, 0.18, 0.015) : tone(accent, 0.955, 0.014));
    text = c.text ?? (dark ? tone(accent, 0.93, 0.01) : tone(accent, 0.2, 0.02));
    muted = c.muted ?? (dark ? tone(accent, 0.76, 0.02) : tone(accent, 0.45, 0.025));
    border = c.border ?? (dark ? tone(accent, 0.36, 0.02) : tone(accent, 0.86, 0.02));
  } else if (t.base === "neu") {
    // One material: panels are the background, shaped only by light.
    bg = c.bg ?? (dark ? "#2a2e35" : "#dde3eb");
    surface = c.surface ?? bg;
    sidebar = c.sidebar ?? bg;
    text = c.text ?? (dark ? "#e2e6ec" : "#2c3440");
    muted = c.muted ?? (dark ? "#99a2ae" : "#667080");
    border = c.border ?? "transparent";
  } else if (t.base === "glass") {
    // Translucent panes over a wallpaper drawn from the accent.
    bg = c.bg ?? (dark ? "#0d0f15" : "#e8ecf4");
    surface = c.surface ?? (dark ? "rgba(34, 37, 46, 0.46)" : "rgba(255, 255, 255, 0.42)");
    sidebar = c.sidebar ?? surface;
    text = c.text ?? (dark ? "#f3f5f9" : "#0f1217");
    muted = c.muted ?? (dark ? "#aab2bf" : "#4d5563");
    border = c.border ?? (dark ? "rgba(255, 255, 255, 0.12)" : "rgba(255, 255, 255, 0.6)");
  } else if (t.base === "retro") {
    // Windows 98: button-face gray, white fields, one light scheme.
    bg = c.bg ?? "#c0c0c0";
    surface = c.surface ?? "#ffffff";
    sidebar = c.sidebar ?? "#c0c0c0";
    text = c.text ?? "#000000";
    muted = c.muted ?? "#404040";
    border = c.border ?? "#808080";
  } else {
    bg = c.bg ?? (dark ? "#16191b" : "#f4f5f6");
    surface = c.surface ?? (dark ? "#1f2326" : "#ffffff");
    sidebar = c.sidebar ?? (dark ? "#121517" : "#ebedef");
    text = c.text ?? (dark ? "#eceff1" : "#161a1d");
    muted = c.muted ?? (dark ? "#9aa4ab" : "#5b656d");
    border = c.border ?? (dark ? "#2b3135" : "#dde2e6");
  }

  // Accent, kept readable in dark mode.
  let primary = accent;
  if (t.base === "material") primary = dark ? tone(accent, 0.8, 0.11) : tone(accent, 0.45);
  else if (t.base === "retro") primary = accent;
  else if (dark && lightness(accent) < 0.6) primary = tone(accent, 0.72);
  const onAccent = t.base === "material" && dark ? tone(accent, 0.24, 0.08) : onColor(primary);
  const accentHover = shift(primary, dark ? 0.05 : -0.05);
  const accentSoft = dark ? tone(accent, 0.33, 0.07) : tone(accent, 0.92, 0.05);

  const r = Math.max(0, Math.min(32, t.radius));
  const rSm = Math.round(r * 0.5);
  const rMd = Math.round(r * 0.75);

  const v: Record<string, string> = {
    "--app-bg": mica ? "transparent" : bg,
    "--bg": bg,
    "--panel": surface,
    "--sidebar": sidebar,
    "--text": text,
    "--muted": muted,
    "--faint": `color-mix(in oklab, ${muted} 72%, ${dark ? "#000" : "#fff"})`,
    "--border": border,
    "--hover": `color-mix(in oklab, ${text} ${dark ? 7 : 5}%, transparent)`,
    "--active": `color-mix(in oklab, ${primary} ${dark ? 22 : 13}%, transparent)`,
    "--accent": primary,
    "--accent-hover": accentHover,
    "--accent-soft": accentSoft,
    "--on-accent": onAccent,
    "--bubble-in":
      t.base === "fluent"
        ? dark
          ? "rgba(255, 255, 255, 0.07)"
          : "#ffffff"
        : t.base === "glass"
          ? dark
            ? "rgba(255, 255, 255, 0.09)"
            : "rgba(255, 255, 255, 0.66)"
          : t.base === "retro"
            ? "#ffffe1"
            : surface,
    "--bubble-out": primary,
    "--bubble-out-text": onAccent,
    "--danger": dark ? "#ef6b5f" : "#c43a2f",
    "--danger-soft": dark ? "#3a1d1b" : "#fbe5e3",
    "--online": dark ? "#3ccf86" : "#1fa463",
    "--font": t.font,
    "--font-display": t.font,
    "--r-sm": `${rSm}px`,
    "--r-md": `${rMd}px`,
    "--r-lg": `${r}px`,
    "--radius": `${rMd}px`,
    "--m": String(Math.max(0.25, Math.min(3, t.motion || 1))),
  };

  switch (t.base) {
    case "fluent":
      v["--shadow"] = dark ? "0 2px 4px rgba(0, 0, 0, 0.26)" : "0 2px 4px rgba(0, 0, 0, 0.04), 0 0 1px rgba(0, 0, 0, 0.1)";
      v["--shadow-lg"] = dark ? "0 16px 48px rgba(0, 0, 0, 0.55)" : "0 16px 48px rgba(0, 0, 0, 0.2)";
      v["--layer"] = dark ? "rgba(255, 255, 255, 0.045)" : "rgba(255, 255, 255, 0.5)";
      v["--layer-stroke"] = dark ? "rgba(255, 255, 255, 0.07)" : "rgba(0, 0, 0, 0.06)";
      v["--control"] = dark ? "rgba(255, 255, 255, 0.06)" : "rgba(255, 255, 255, 0.7)";
      break;
    case "material":
      v["--shadow"] = "none";
      v["--shadow-lg"] = dark
        ? "0 4px 8px 3px rgba(0, 0, 0, 0.3), 0 1px 3px rgba(0, 0, 0, 0.3)"
        : "0 4px 8px 3px rgba(0, 0, 0, 0.12), 0 1px 3px rgba(0, 0, 0, 0.18)";
      v["--surface-high"] = dark ? tone(accent, 0.27, 0.02) : tone(accent, 0.95, 0.02);
      v["--surface-highest"] = dark ? tone(accent, 0.31, 0.022) : tone(accent, 0.925, 0.024);
      break;
    case "neu": {
      const L = lightness(bg);
      const light = tone(bg, Math.min(1, L + (dark ? 0.075 : 0.09)));
      const shade = tone(bg, Math.max(0, L - (dark ? 0.12 : 0.16)));
      v["--neu-light"] = light;
      v["--neu-dark"] = shade;
      v["--neu-out"] = `7px 7px 16px ${shade}, -7px -7px 16px ${light}`;
      v["--neu-out-sm"] = `3px 3px 7px ${shade}, -3px -3px 7px ${light}`;
      v["--neu-in"] = `inset 5px 5px 10px ${shade}, inset -5px -5px 10px ${light}`;
      v["--neu-in-sm"] = `inset 3px 3px 6px ${shade}, inset -3px -3px 6px ${light}`;
      // Pressed-in surfaces sit a little darker, raised controls a little lighter.
      v["--neu-sunk"] = `color-mix(in oklab, ${bg} ${dark ? 70 : 72}%, ${shade})`;
      v["--neu-raised"] = `color-mix(in oklab, ${bg} ${dark ? 80 : 65}%, ${light})`;
      v["--neu-stroke"] = dark ? "rgba(255, 255, 255, 0.07)" : "rgba(255, 255, 255, 0.65)";
      v["--shadow"] = v["--neu-out-sm"];
      v["--shadow-lg"] = `14px 14px 30px ${shade}, -10px -10px 26px ${light}`;
      break;
    }
    case "glass": {
      // Four soft blobs around the accent's hue; the panes blur them.
      const blob = (dh: number) => (dark ? toneAt(accent, dh, 0.42, 0.14) : toneAt(accent, dh, 0.8, 0.12));
      v["--glass-wall"] = [
        `radial-gradient(60% 70% at 8% 12%, ${blob(0)} 0%, transparent 70%)`,
        `radial-gradient(50% 60% at 92% 6%, ${blob(55)} 0%, transparent 70%)`,
        `radial-gradient(60% 60% at 78% 96%, ${blob(-60)} 0%, transparent 70%)`,
        `radial-gradient(45% 55% at 22% 92%, ${blob(150)} 0%, transparent 70%)`,
        `linear-gradient(160deg, ${dark ? "#10131b" : "#eef1f8"}, ${dark ? "#07080c" : "#dfe5f0"})`,
      ].join(", ");
      v["--glass-fill"] = surface;
      v["--glass-fill-strong"] = dark ? "rgba(255, 255, 255, 0.13)" : "rgba(255, 255, 255, 0.72)";
      v["--glass-fill-soft"] = dark ? "rgba(255, 255, 255, 0.06)" : "rgba(255, 255, 255, 0.34)";
      v["--glass-rim"] = dark
        ? "linear-gradient(150deg, rgba(255, 255, 255, 0.34), rgba(255, 255, 255, 0.06) 38%, rgba(255, 255, 255, 0.03) 62%, rgba(255, 255, 255, 0.2))"
        : "linear-gradient(150deg, rgba(255, 255, 255, 0.95), rgba(255, 255, 255, 0.35) 38%, rgba(255, 255, 255, 0.2) 62%, rgba(255, 255, 255, 0.8))";
      v["--glass-hi"] = dark ? "rgba(255, 255, 255, 0.16)" : "rgba(255, 255, 255, 0.85)";
      v["--glass-glow"] = alpha(primary, dark ? 0.45 : 0.35);
      v["--shadow"] = dark ? "0 6px 20px rgba(0, 0, 0, 0.35)" : "0 6px 20px rgba(30, 40, 70, 0.1)";
      v["--shadow-lg"] = dark ? "0 24px 70px rgba(0, 0, 0, 0.55)" : "0 24px 70px rgba(30, 40, 80, 0.22)";
      v["--hover"] = dark ? "rgba(255, 255, 255, 0.07)" : "rgba(255, 255, 255, 0.45)";
      break;
    }
    case "retro":
      // Bevel edges: highlight, light, shadow, dark frame.
      v["--rt-hi"] = "#ffffff";
      v["--rt-light"] = "#dfdfdf";
      v["--rt-shade"] = "#808080";
      v["--rt-dark"] = "#0a0a0a";
      v["--rt-desktop"] = "#008080";
      v["--rt-title"] = `linear-gradient(90deg, ${primary}, ${tone(primary, Math.min(0.72, lightness(primary) + 0.32))})`;
      v["--rt-title-off"] = "linear-gradient(90deg, #808080, #b5b5b5)";
      v["--rt-raised"] = "inset -1px -1px #0a0a0a, inset 1px 1px #ffffff, inset -2px -2px #808080, inset 2px 2px #dfdfdf";
      v["--rt-sunken"] = "inset -1px -1px #ffffff, inset 1px 1px #808080, inset -2px -2px #dfdfdf, inset 2px 2px #0a0a0a";
      v["--rt-field"] = "inset -1px -1px #ffffff, inset 1px 1px #808080, inset -2px -2px #dfdfdf, inset 2px 2px #0a0a0a";
      v["--rt-thin-up"] = "inset -1px -1px #808080, inset 1px 1px #ffffff";
      v["--rt-thin-down"] = "inset -1px -1px #ffffff, inset 1px 1px #808080";
      v["--rt-window"] = "inset -1px -1px #0a0a0a, inset 1px 1px #dfdfdf, inset -2px -2px #808080, inset 2px 2px #ffffff";
      v["--shadow"] = "none";
      v["--shadow-lg"] = "none";
      v["--hover"] = "transparent";
      v["--active"] = primary;
      v["--accent-soft"] = "#c0c0c0";
      v["--faint"] = "#808080";
      v["--danger"] = "#c00000";
      v["--danger-soft"] = "#ffe0e0";
      v["--online"] = "#008000";
      break;
    default:
      v["--shadow"] = dark ? "0 1px 2px rgba(0, 0, 0, 0.3)" : "0 1px 2px rgba(16, 32, 36, 0.06), 0 2px 8px rgba(16, 32, 36, 0.05)";
      v["--shadow-lg"] = dark ? "0 20px 60px rgba(0, 0, 0, 0.55)" : "0 20px 60px rgba(10, 25, 28, 0.25)";
  }
  return v;
}

/** Resolves the mode a theme is shown in. */
export function resolveMode(t: ThemeDef, wanted: Mode): Mode {
  return t.modes.includes(wanted) ? wanted : (t.modes[0] ?? "light");
}
