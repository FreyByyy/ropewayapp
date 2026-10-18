import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { IS_LINUX, IS_MAC, IS_MOBILE } from "../platform";
import { BackButton } from "./MobileNav";
import { api, type Folders, type Language, type MeView, type PeerView, type Settings } from "../api";
import { useI18n } from "../i18n";
import type { Dispatch } from "../store";
import { IconExternal } from "../icons";
import { VideoFields } from "./VideoFields";
import { AppearanceCard } from "./AppearanceCard";
import type { AppearanceApi } from "../theme/useAppearance";
import { Avatar } from "./Avatar";

interface Props {
  me: MeView;
  settings: Settings;
  folders: Folders;
  peers: PeerView[];
  dispatch: Dispatch;
  appearance: AppearanceApi;
  onUntrust: (peer: PeerView) => void;
}

export function SettingsView({ me, settings, folders, peers, dispatch, onUntrust, appearance }: Props) {
  const { t } = useI18n();
  const [name, setName] = useState(settings.device_name);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [version, setVersion] = useState("");

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  const reload = async () => {
    const [s, f] = await Promise.all([api.settings(), api.folders()]);
    dispatch({ type: "settings", settings: s });
    dispatch({ type: "folders", folders: f });
  };

  const pickFolder = async (apply: (path: string) => Promise<void>, current: string) => {
    const picked = await open({ directory: true, multiple: false, defaultPath: current });
    if (typeof picked !== "string") return;
    try {
      await apply(picked);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
    await reload();
  };

  const setSharing = async (enabled: boolean) => {
    await api.setSharing(enabled);
    await reload();
  };

  const setTrustedOnly = async (enabled: boolean) => {
    await api.setSharedTrustedOnly(enabled);
    await reload();
  };

  const saveName = async () => {
    try {
      await api.setDeviceName(name);
      await reload();
      setError(null);
      setSaved(true);
      setTimeout(() => setSaved(false), 1600);
    } catch (e) {
      setError(String(e));
    }
  };

  const setLanguage = async (language: Language) => {
    await api.setLanguage(language);
    await reload();
  };

  const setAutostart = async (enabled: boolean) => {
    try {
      await api.setAutostart(enabled);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
    await reload();
  };

  const copyCode = async () => {
    await navigator.clipboard.writeText(me.code);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  const trustedPeers: PeerView[] = settings.trusted.map(
    (tr) =>
      peers.find((p) => p.id === tr.id) ?? {
        id: tr.id,
        name: tr.name,
        os: "",
        address: null,
        online: false,
        trusted: true,
        code: tr.id.slice(0, 12).toUpperCase().replace(/(.{4})(?=.)/g, "$1-"),
      },
  );

  return (
    <section className="settings">
      <header className="settings-head">
        <BackButton />
        <h1>{t("settings")}</h1>
      </header>
      <div className="settings-body">
        <div className="card device-card">
          <Avatar name={me.name} id={me.id} size={52} />
          <div className="device-info">
            <span className="eyebrow">{t("thisDevice")}</span>
            <span className="device-name">{me.name}</span>
          </div>
          <button className="code-chip" onClick={copyCode} title={t("copyCode")}>
            <span className="eyebrow">{t("deviceCode")}</span>
            <span className="mono code">{copied ? t("copied") : me.code}</span>
          </button>
        </div>

        <h2 className="settings-section">{t("appearance")}</h2>
        <AppearanceCard appearance={appearance} peers={peers} />

        <div className="card">
          <label className="field-label" htmlFor="device-name">
            {t("deviceName")}
          </label>
          <p className="field-hint">{t("deviceNameHint")}</p>
          <div className="inline-form">
            <input
              id="device-name"
              value={name}
              maxLength={64}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && saveName()}
            />
            <button
              className="btn primary"
              onClick={saveName}
              disabled={!name.trim() || name.trim() === settings.device_name}
            >
              {saved ? t("saved") : t("save")}
            </button>
          </div>
        </div>

        <div className="card">
          <span className="field-label">{t("language")}</span>
          <div className="segmented">
            {(["ru", "en"] as const).map((l) => (
              <button key={l} className={settings.language === l ? "on" : ""} onClick={() => setLanguage(l)}>
                {l === "ru" ? "Русский" : "English"}
              </button>
            ))}
          </div>
        </div>

        {!IS_MOBILE && (
        <div className="card row-card">
          <div>
            <span className="field-label">{t(IS_LINUX || IS_MAC ? "autostartLinux" : "autostart")}</span>
            <p className="field-hint">{t("autostartHint")}</p>
          </div>
          <label className="switch">
            <input type="checkbox" checked={settings.autostart} onChange={(e) => setAutostart(e.target.checked)} />
            <span />
          </label>
        </div>
        )}

        <div className="card">
          <span className="field-label">{t("folders")}</span>
          <div className="folder-row">
            <div className="folder-text">
              <span className="folder-title">{t("downloadDir")}</span>
              <span className="folder-path mono" title={folders.download_dir}>
                {folders.download_dir}
              </span>
              <span className="field-hint">{t("downloadDirHint")}</span>
            </div>
            {!IS_MOBILE && (
              <button className="icon-btn" title={t("openFolder")} onClick={() => api.openPath(folders.download_dir).catch(() => {})}>
                <IconExternal size={16} />
              </button>
            )}
            {!IS_MOBILE && (
              <button className="btn ghost small" onClick={() => pickFolder((p) => api.setDownloadDir(p), folders.download_dir)}>
                {t("change")}
              </button>
            )}
          </div>
          <div className="folder-row">
            <div className="folder-text">
              <span className="folder-title">{t("sharedDirLabel")}</span>
              <span className="folder-path mono" title={folders.shared_dir}>
                {folders.shared_dir}
              </span>
            </div>
            {!IS_MOBILE && (
              <button className="icon-btn" title={t("openFolder")} onClick={() => api.openPath(folders.shared_dir).catch(() => {})}>
                <IconExternal size={16} />
              </button>
            )}
            {!IS_MOBILE && (
              <button className="btn ghost small" onClick={() => pickFolder((p) => api.setSharedDir(p), folders.shared_dir)}>
                {t("change")}
              </button>
            )}
          </div>
          <div className="folder-row toggle-row">
            <div className="folder-text">
              <span className="folder-title">{t("sharingToggle")}</span>
              <span className="field-hint">{t("sharingToggleHint")}</span>
            </div>
            <label className="switch">
              <input type="checkbox" checked={folders.sharing_enabled} onChange={(e) => setSharing(e.target.checked)} />
              <span />
            </label>
          </div>
          <div className={`folder-row toggle-row ${folders.sharing_enabled ? "" : "disabled"}`}>
            <div className="folder-text">
              <span className="folder-title">{t("sharedTrustedOnly")}</span>
              <span className="field-hint">{t("sharedTrustedOnlyHint")}</span>
            </div>
            <label className="switch">
              <input
                type="checkbox"
                checked={folders.shared_trusted_only}
                disabled={!folders.sharing_enabled}
                onChange={(e) => setTrustedOnly(e.target.checked)}
              />
              <span />
            </label>
          </div>
        </div>

        <div className="card">
          <span className="field-label">{t("screenDefaults")}</span>
          <p className="field-hint">{t("screenDefaultsHint")}</p>
          <div className="video-fields-wrap">
            <VideoFields
              value={settings.screen_video}
              onChange={async (v) => {
                await api.setScreenVideo(v);
                await reload();
              }}
            />
          </div>
        </div>

        <div className="card">
          <span className="field-label">{t("trustedDevices")}</span>
          {trustedPeers.length === 0 ? (
            <p className="field-hint">{t("noTrusted")}</p>
          ) : (
            <ul className="trusted-list">
              {trustedPeers.map((p) => (
                <li key={p.id}>
                  <Avatar name={p.name} id={p.id} online={p.online} size={30} />
                  <span className="truncate">{p.name}</span>
                  <span className="mono dim">{p.code}</span>
                  <button className="btn ghost small" onClick={() => onUntrust(p)}>
                    {t("remove")}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="card">
          <span className="field-label">{t("network")}</span>
          <dl className="facts">
            <dt>{t("addresses")}</dt>
            <dd className="mono">{me.addresses.length ? me.addresses.join(", ") : "—"}</dd>
            <dt>{t("port")}</dt>
            <dd className="mono">{me.port} (UDP)</dd>
            <dt>{t("version")}</dt>
            <dd className="mono">{version}</dd>
            <dt>{t("about")}</dt>
            <dd>{t("iconsCredit")} — icons8.com</dd>
          </dl>
        </div>

        {error && <div className="form-error">{error}</div>}
      </div>
    </section>
  );
}
