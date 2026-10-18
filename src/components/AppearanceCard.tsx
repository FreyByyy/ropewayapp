import { useState } from "react";
import { NOT_WINDOWS } from "../platform";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, type PeerView } from "../api";
import { useI18n } from "../i18n";
import { IconPalette, IconPlus } from "../icons";
import { ACCENT_SWATCHES } from "../theme/builtin";
import { useThemeInstall, THEME_EXT } from "../theme/install";
import { newThemeId } from "../theme/normalize";
import type { AppearanceApi } from "../theme/useAppearance";
import type { ModeChoice, MotionChoice, ThemeDef } from "../theme/types";
import { Avatar } from "./Avatar";
import { Modal } from "./Modals";
import { ThemeEditor } from "./ThemeEditor";
import { ThemePreview } from "./ThemePreview";
import { revealChange } from "../theme/transition";

export function AppearanceCard({ appearance, peers }: { appearance: AppearanceApi; peers: PeerView[] }) {
  const { t } = useI18n();
  const install = useThemeInstall();
  const [editing, setEditing] = useState<ThemeDef | null>(null);
  const [sending, setSending] = useState<ThemeDef | null>(null);
  const [deleting, setDeleting] = useState<ThemeDef | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { appearance: a, theme, mode, themes, custom } = appearance;
  const customIds = new Set(custom.map((c) => c.id));
  const current = themes.find((x) => x.id === a.theme) ?? theme;

  const createFrom = (base: ThemeDef) =>
    setEditing({
      ...base,
      id: newThemeId(),
      name: t("themeCopyName", { name: base.name }),
      author: undefined,
      colors: structuredClone(base.colors),
    });

  const importTheme = async () => {
    const picked = await open({ multiple: false, filters: [{ name: "Ropeway", extensions: [THEME_EXT] }] });
    if (typeof picked === "string") install(picked);
  };

  const exportTheme = async (th: ThemeDef) => {
    const path = await save({ defaultPath: `${th.name}.${THEME_EXT}`, filters: [{ name: "Ropeway", extensions: [THEME_EXT] }] });
    if (!path) return;
    api.exportTheme(path, th).catch((e) => setError(String(e)));
  };

  const sendTheme = async (th: ThemeDef, peer: PeerView) => {
    setSending(null);
    try {
      const path = await api.themeFileForSending(th);
      await api.sendPaths(peer.id, [path]);
    } catch (e) {
      setError(String(e));
    }
  };

  const modes: [ModeChoice, string][] = [
    ["system", t("modeSystem")],
    ["light", t("modeLight")],
    ["dark", t("modeDark")],
  ];
  const motions: [MotionChoice, string][] = [
    ["full", t("motionFull")],
    ["reduced", t("motionReduced")],
    ["off", t("motionOff")],
  ];
  const customAccent = a.accent !== "theme" && a.accent !== "windows" ? a.accent : null;
  const online = peers.filter((p) => p.online);
  // Each card shows the accent it would get if picked.
  const accentFor = (th: ThemeDef) =>
    customAccent ??
    (a.accent === "windows" || th.base === "fluent" ? (appearance.windowsAccent ?? th.accent) : th.accent);

  return (
    <div className="card appearance-card">
      <span className="field-label">{t("theme")}</span>
      <div className="theme-grid">
        {themes.map((th) => (
          <div key={th.id} className={`theme-card ${th.id === a.theme ? "on" : ""}`}>
            <button className="theme-pick" onClick={(e) => revealChange(e, () => appearance.update({ theme: th.id }))} aria-pressed={th.id === a.theme}>
              <ThemePreview theme={th} mode={th.modes.includes(mode) ? mode : th.modes[0]} accent={accentFor(th)} />
              <span className="theme-name">{th.name}</span>
              <span className="theme-meta">{customIds.has(th.id) ? (th.author ?? "") : t("builtIn")}</span>
            </button>
          </div>
        ))}
      </div>

      <div className="appearance-row">
        <span className="appearance-label">{t("modeLabel")}</span>
        <div className="segmented">
          {modes.map(([m, label]) => {
            const supported = m === "system" || current.modes.includes(m);
            return (
              <button
                key={m}
                className={a.mode === m ? "on" : ""}
                disabled={!supported}
                title={supported ? undefined : t("modeUnsupported")}
                onClick={(e) => revealChange(e, () => appearance.update({ mode: m }))}
              >
                {label}
              </button>
            );
          })}
        </div>
      </div>

      <div className="appearance-row">
        <span className="appearance-label">{t("accentLabel")}</span>
        <div className="accent-row">
          <button
            className={`accent-chip ${a.accent === "theme" ? "on" : ""}`}
            onClick={() => appearance.update({ accent: "theme" })}
          >
            <i style={{ background: current.base === "fluent" ? (appearance.windowsAccent ?? current.accent) : current.accent }} />
            {t("accentTheme")}
          </button>
          {appearance.windowsAccent && (
            <button
              className={`accent-chip ${a.accent === "windows" ? "on" : ""}`}
              onClick={() => appearance.update({ accent: "windows" })}
            >
              <i style={{ background: appearance.windowsAccent }} />
              {t("accentWindows")}
            </button>
          )}
          <div className="swatches">
            {ACCENT_SWATCHES.map((c) => (
              <button
                key={c}
                className={`swatch ${a.accent === c ? "on" : ""}`}
                style={{ background: c }}
                aria-label={c}
                title={c}
                onClick={() => appearance.update({ accent: c })}
              />
            ))}
            <label className={`swatch custom ${customAccent && !ACCENT_SWATCHES.includes(customAccent) ? "on" : ""}`} title={t("accentCustom")}>
              <input type="color" value={customAccent ?? "#0067c0"} onChange={(e) => appearance.update({ accent: e.target.value })} />
              <IconPalette size={14} />
            </label>
          </div>
        </div>
      </div>

      <div className="appearance-row">
        <span className="appearance-label">{t("motionLabel")}</span>
        <div>
          <div className="segmented">
            {motions.map(([m, label]) => (
              <button key={m} className={a.motion === m ? "on" : ""} onClick={() => appearance.update({ motion: m })}>
                {label}
              </button>
            ))}
          </div>
          <p className="field-hint">{t(NOT_WINDOWS ? "motionHintMobile" : "motionHint")}</p>
        </div>
      </div>

      <div className="my-themes">
        <span className="field-label">{t("myThemes")}</span>
        <div className="my-themes-actions">
          <button className="btn primary small" onClick={() => createFrom(current)}>
            <IconPlus size={14} /> {t("createTheme")}
          </button>
          <button className="btn ghost small" onClick={() => importTheme().catch((e) => setError(String(e)))}>
            {t("importTheme")}
          </button>
          {!customIds.has(current.id) && (
            <button className="btn ghost small" onClick={() => exportTheme(current)}>
              {t("exportTheme")}
            </button>
          )}
        </div>
        {custom.length > 0 && (
          <ul className="my-theme-list">
            {custom.map((th) => (
              <li key={th.id}>
                <span className={`theme-dot ${th.id === a.theme ? "on" : ""}`} style={{ background: th.accent }} />
                <span className="truncate">{th.name}</span>
                <span className="my-theme-buttons">
                  <button className="btn ghost small" onClick={() => setEditing(structuredClone(th))}>
                    {t("editTheme")}
                  </button>
                  <button className="btn ghost small" onClick={() => exportTheme(th)}>
                    {t("exportTheme")}
                  </button>
                  <button className="btn ghost small" onClick={() => setSending(th)} disabled={online.length === 0}>
                    {t("sendTheme")}
                  </button>
                  <button className="btn ghost small danger-text" onClick={() => setDeleting(th)}>
                    {t("deleteTheme")}
                  </button>
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      {error && <div className="form-error">{error}</div>}

      {editing && <ThemeEditor initial={editing} appearance={appearance} onClose={() => setEditing(null)} />}

      {sending && (
        <Modal title={t("sendThemeTitle", { name: sending.name })} onClose={() => setSending(null)}>
          <p className="modal-text">{t("sendThemeBody")}</p>
          <div className="pick-list">
            {online.map((p) => (
              <button key={p.id} className="pick-row" onClick={() => sendTheme(sending, p)}>
                <Avatar name={p.name} id={p.id} size={28} />
                <span className="truncate">{p.name}</span>
              </button>
            ))}
          </div>
        </Modal>
      )}

      {deleting && (
        <Modal title={t("deleteThemeTitle", { name: deleting.name })} onClose={() => setDeleting(null)}>
          <p className="modal-text">{t("deleteThemeBody")}</p>
          <div className="modal-actions">
            <button className="btn ghost" onClick={() => setDeleting(null)}>
              {t("cancel")}
            </button>
            <button
              className="btn danger"
              onClick={() => {
                const id = deleting.id;
                setDeleting(null);
                appearance.deleteTheme(id).catch((e) => setError(String(e)));
              }}
            >
              {t("deleteTheme")}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}
