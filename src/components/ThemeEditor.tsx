import { useEffect, useState } from "react";
import { NOT_WINDOWS } from "../platform";
import { useI18n, type MessageKey } from "../i18n";
import { IconX } from "../icons";
import { FONT_CHOICES } from "../theme/builtin";
import { isHex } from "../theme/color";
import type { AppearanceApi } from "../theme/useAppearance";
import type { Base, IconSet, Layout, Mode, ModeColors, ThemeDef } from "../theme/types";

const BASES: [Base, MessageKey][] = [
  ["fluent", "baseFluent"],
  ["material", "baseMaterial"],
  ["neu", "baseNeu"],
  ["glass", "baseGlass"],
  ["retro", "baseRetro"],
  ["ropeway", "baseRopeway"],
];
const LAYOUTS: [Layout, MessageKey][] = [
  ["navview", "layoutNavview"],
  ["rail", "layoutRail"],
  ["soft", "layoutSoft"],
  ["sidebar", "layoutSidebar"],
];
const ICONS: [IconSet, MessageKey][] = [
  ["fluent", "iconsFluent"],
  ["material", "iconsMaterial"],
  ["puffy", "iconsPuffy"],
  ["glass", "iconsGlass"],
  ["retro", "iconsRetro"],
  ["ropeway", "iconsRopeway"],
];
const COLOR_FIELDS: [keyof ModeColors, MessageKey][] = [
  ["bg", "colorBg"],
  ["surface", "colorSurface"],
  ["sidebar", "colorSidebar"],
  ["text", "colorText"],
  ["muted", "colorMuted"],
  ["border", "colorBorder"],
];

/**
 * Side drawer for editing a theme. Every change is applied to the app at
 * once (preview); Cancel restores the previous theme.
 */
export function ThemeEditor({
  initial,
  appearance,
  onClose,
}: {
  initial: ThemeDef;
  appearance: AppearanceApi;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<ThemeDef>(initial);
  const [tab, setTab] = useState<Mode>(appearance.mode);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { setPreview } = appearance;

  useEffect(() => {
    setPreview(draft);
  }, [draft, setPreview]);
  useEffect(() => () => setPreview(null), [setPreview]);

  const set = <K extends keyof ThemeDef>(key: K, value: ThemeDef[K]) => setDraft((d) => ({ ...d, [key]: value }));
  const setColor = (mode: Mode, key: keyof ModeColors, value: string | undefined) =>
    setDraft((d) => {
      const colors = { ...d.colors, [mode]: { ...(d.colors[mode] ?? {}) } };
      if (value === undefined || value === "") delete colors[mode]![key];
      else colors[mode]![key] = value;
      return { ...d, colors };
    });
  const toggleMode = (mode: Mode) =>
    setDraft((d) => {
      const has = d.modes.includes(mode);
      const modes = has ? d.modes.filter((m) => m !== mode) : [...d.modes, mode];
      return { ...d, modes: modes.length ? modes : d.modes };
    });

  const save = async () => {
    if (!draft.name.trim()) return;
    setBusy(true);
    try {
      await appearance.saveTheme({ ...draft, name: draft.name.trim() });
      appearance.update({ theme: draft.id });
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const colors = draft.colors[tab] ?? {};
  const fontKnown = FONT_CHOICES.some((f) => f.value === draft.font);

  return (
    <aside className="drawer" role="dialog" aria-label={t("editorTitle")}>
      <header className="drawer-head">
        <div>
          <h2>{t("editorTitle")}</h2>
          <p className="field-hint">{t("editorHint")}</p>
        </div>
        <button className="icon-btn" onClick={onClose} aria-label={t("cancel")}>
          <IconX size={16} />
        </button>
      </header>

      <div className="drawer-body">
        <label className="ed-field">
          <span>{t("themeName")}</span>
          <input value={draft.name} maxLength={60} onChange={(e) => set("name", e.target.value)} />
        </label>

        <div className="ed-grid">
          <label className="ed-field">
            <span>{t("themeBase")}</span>
            <select value={draft.base} onChange={(e) => set("base", e.target.value as Base)}>
              {BASES.map(([v, k]) => (
                <option key={v} value={v}>
                  {t(k)}
                </option>
              ))}
            </select>
          </label>
          <label className="ed-field">
            <span>{t("themeLayout")}</span>
            <select value={draft.layout} onChange={(e) => set("layout", e.target.value as Layout)}>
              {LAYOUTS.map(([v, k]) => (
                <option key={v} value={v}>
                  {t(k)}
                </option>
              ))}
            </select>
          </label>
          <label className="ed-field">
            <span>{t("themeIcons")}</span>
            <select value={draft.icons} onChange={(e) => set("icons", e.target.value as IconSet)}>
              {ICONS.map(([v, k]) => (
                <option key={v} value={v}>
                  {t(k)}
                </option>
              ))}
            </select>
          </label>
          <label className="ed-field">
            <span>{t("themeAccent")}</span>
            <span className="color-pair">
              <input type="color" value={draft.accent} onChange={(e) => set("accent", e.target.value)} />
              <input
                className="mono"
                value={draft.accent}
                onChange={(e) => isHex(e.target.value) && set("accent", e.target.value)}
              />
            </span>
          </label>
        </div>

        <div className="ed-field">
          <span>{t("themeModes")}</span>
          <div className="ed-checks">
            <label className="check">
              <input type="checkbox" checked={draft.modes.includes("light")} onChange={() => toggleMode("light")} />
              {t("modeLight")}
            </label>
            <label className="check">
              <input type="checkbox" checked={draft.modes.includes("dark")} onChange={() => toggleMode("dark")} />
              {t("modeDark")}
            </label>
          </div>
        </div>

        <div className="ed-field">
          <span>{t("themeColors")}</span>
          <div className="segmented small">
            {(["light", "dark"] as Mode[]).map((m) => (
              <button key={m} className={tab === m ? "on" : ""} onClick={() => setTab(m)} disabled={!draft.modes.includes(m)}>
                {m === "light" ? t("modeLight") : t("modeDark")}
              </button>
            ))}
          </div>
          <div className="ed-colors">
            {COLOR_FIELDS.map(([key, label]) => {
              const value = colors[key];
              return (
                <div key={key} className="ed-color">
                  <span className="ed-color-label">{t(label)}</span>
                  <span className="color-pair">
                    <input
                      type="color"
                      value={value && isHex(value) ? value : "#808080"}
                      onChange={(e) => setColor(tab, key, e.target.value)}
                    />
                    <input
                      className="mono"
                      placeholder={t("colorAuto")}
                      title={value ? undefined : t("colorAutoHint")}
                      value={value ?? ""}
                      onChange={(e) => setColor(tab, key, e.target.value)}
                    />
                  </span>
                </div>
              );
            })}
          </div>
        </div>

        <label className="ed-field">
          <span>
            {t("themeRadius")} <b className="ed-value">{draft.radius} px</b>
          </span>
          <input type="range" min={0} max={32} value={draft.radius} onChange={(e) => set("radius", Number(e.target.value))} />
        </label>

        <label className="ed-field">
          <span>{t("themeFont")}</span>
          <select
            value={fontKnown ? draft.font : "__custom"}
            onChange={(e) => e.target.value !== "__custom" && set("font", e.target.value)}
          >
            {FONT_CHOICES.map((f) => (
              <option key={f.label} value={f.value}>
                {f.label}
              </option>
            ))}
            {!fontKnown && <option value="__custom">{draft.font}</option>}
          </select>
        </label>

        <label className="ed-field">
          <span>
            {t("themeMotion")} <b className="ed-value">×{(1 / draft.motion).toFixed(1)}</b>
          </span>
          <input
            type="range"
            min={-1}
            max={1}
            step={0.1}
            value={-Math.log2(draft.motion)}
            onChange={(e) => set("motion", Number((2 ** -Number(e.target.value)).toFixed(2)))}
          />
          <span className="range-ends">
            <span>{t("motionSlower")}</span>
            <span>{t("motionFaster")}</span>
          </span>
        </label>

        {!NOT_WINDOWS && (
          <label className="check">
            <input type="checkbox" checked={draft.mica} onChange={(e) => set("mica", e.target.checked)} />
            {t("themeMica")}
          </label>
        )}

        <label className="ed-field">
          <span>{t("themeCss")}</span>
          <textarea
            className="mono css-input"
            spellCheck={false}
            rows={8}
            value={draft.css ?? ""}
            onChange={(e) => set("css", e.target.value)}
            placeholder={".bubble {\n  border-radius: 4px;\n}"}
          />
          <span className="field-hint">{t("themeCssHint")}</span>
        </label>

        {error && <div className="form-error">{error}</div>}
      </div>

      <footer className="drawer-foot">
        <button className="btn ghost" onClick={onClose}>
          {t("cancel")}
        </button>
        <button className="btn primary" onClick={save} disabled={busy || !draft.name.trim()}>
          {t("saveTheme")}
        </button>
      </footer>
    </aside>
  );
}
