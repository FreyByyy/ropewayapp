import type { CSSProperties } from "react";
import { themeVars } from "../theme/derive";
import type { Mode, ThemeDef } from "../theme/types";

/**
 * A small drawing of the theme: its layout, surfaces, bubbles and accent,
 * in the given mode. Built from the same variables the app uses.
 */
export function ThemePreview({ theme, mode, accent }: { theme: ThemeDef; mode: Mode; accent: string }) {
  const v = themeVars(theme, mode, accent, false);
  const style = { ...v, fontFamily: theme.font } as CSSProperties;
  return (
    <div className="tp" data-tp-base={theme.base} data-tp-layout={theme.layout} style={style} aria-hidden="true">
      {theme.layout === "rail" && (
        <div className="tp-rail">
          <i className="tp-fab" />
          <i className="tp-dot on" />
          <i className="tp-dot" />
        </div>
      )}
      <div className="tp-side">
        <i className="tp-row on" />
        <i className="tp-row" />
        <i className="tp-row" />
      </div>
      <div className="tp-main">
        <i className="tp-bubble in" />
        <i className="tp-bubble out" />
        <i className="tp-bubble in short" />
        <i className="tp-input" />
      </div>
    </div>
  );
}
