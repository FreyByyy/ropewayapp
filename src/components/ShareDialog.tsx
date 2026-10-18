import { useEffect, useState } from "react";
import { api, DEFAULT_VIDEO, type CaptureSource, type PeerView, type SourceInfo, type VideoSettings } from "../api";
import { useI18n, type Translate } from "../i18n";
import { IconMonitor } from "../icons";
import { Avatar } from "./Avatar";
import { Modal } from "./Modals";
import { VideoFields } from "./VideoFields";
import { IS_MOBILE } from "../platform";

interface Props {
  peers: PeerView[];
  /** Devices checked when the dialog opens. */
  preselected: string[];
  /** Start with "everyone on the network". */
  everyone: boolean;
  /** Settings used last time. */
  initialVideo?: VideoSettings;
  onClose: () => void;
}

function sourceLabel(s: SourceInfo, t: Translate) {
  if (IS_MOBILE) return t("phoneScreen");
  // Wayland: the system's own dialog picks the screen.
  if (s.id === "portal") return t("portalScreen");
  if (s.kind === "monitor") return `${t("monitor")} ${s.index}`;
  return s.name;
}

export function ShareDialog({ peers, preselected, everyone, initialVideo, onClose }: Props) {
  const { t } = useI18n();
  const online = peers.filter((p) => p.online);
  const [sources, setSources] = useState<SourceInfo[] | null>(null);
  const [picked, setPicked] = useState<SourceInfo | null>(null);
  const [systemAudio, setSystemAudio] = useState(true);
  const [mic, setMic] = useState(false);
  const [video, setVideo] = useState<VideoSettings>(initialVideo ?? DEFAULT_VIDEO);
  const [isPublic, setIsPublic] = useState(everyone);
  const [selected, setSelected] = useState<Set<string>>(new Set(preselected));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .screenSources()
      .then((list) => {
        setSources(list);
        setPicked(list.find((s) => s.kind === "monitor" && s.primary) ?? list[0] ?? null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const toggle = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const canStart = !!picked && (isPublic || selected.size > 0) && !busy;

  const start = async () => {
    if (!picked) return;
    setBusy(true);
    setError(null);
    const source: CaptureSource = { kind: picked.kind, id: picked.id, label: sourceLabel(picked, t) };
    try {
      // Android asks the user every time; a refusal just keeps the dialog open.
      if (!(await api.requestCapture())) {
        setBusy(false);
        return;
      }
      api.setScreenVideo(video).catch(() => {});
      const audio = !IS_MOBILE;
      await api.startShare(
        { video, source, system_audio: audio && systemAudio, microphone: audio && mic },
        [...selected],
        isPublic,
      );
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const monitors = sources?.filter((s) => s.kind === "monitor") ?? [];
  const windows = sources?.filter((s) => s.kind === "window") ?? [];

  return (
    <Modal title={t("shareTitle")} onClose={onClose}>
      <div className="share-dialog">
        <div className="share-section">
          <div className="share-label">{t("shareSource")}</div>
          {!sources ? (
            <p className="dim">{t("loadingSources")}</p>
          ) : sources.length === 0 ? (
            <p className="dim">{t("noSources")}</p>
          ) : (
            <div className="source-list">
              {monitors.length > 0 && <div className="source-group">{t("screens")}</div>}
              {monitors.map((s) => (
                <SourceRow key={s.kind + s.id} s={s} picked={picked} onPick={setPicked} t={t} />
              ))}
              {windows.length > 0 && <div className="source-group">{t("windows")}</div>}
              {windows.map((s) => (
                <SourceRow key={s.kind + s.id} s={s} picked={picked} onPick={setPicked} t={t} />
              ))}
            </div>
          )}
        </div>

        <div className="share-section">
          <div className="share-label">{t("videoQuality")}</div>
          <VideoFields value={video} onChange={setVideo} />
        </div>

        {!IS_MOBILE && (
        <div className="share-section">
          <div className="share-label">{t("sound")}</div>
          <label className="check">
            <input type="checkbox" checked={systemAudio} onChange={(e) => setSystemAudio(e.target.checked)} />
            {t("systemSound")}
          </label>
          <label className="check">
            <input type="checkbox" checked={mic} onChange={(e) => setMic(e.target.checked)} />
            {t("microphone")}
          </label>
        </div>
        )}

        <div className="share-section">
          <div className="share-label">{t("recipients")}</div>
          <label className="check">
            <input type="checkbox" checked={isPublic} onChange={(e) => setIsPublic(e.target.checked)} />
            <span>
              {t("everyone")}
              <span className="dim small"> — {t("everyoneHint")}</span>
            </span>
          </label>
          {!isPublic &&
            (online.length === 0 ? (
              <p className="dim small">{t("noOneOnline")}</p>
            ) : (
              online.map((p) => (
                <label key={p.id} className="check peer-check">
                  <input type="checkbox" checked={selected.has(p.id)} onChange={() => toggle(p.id)} />
                  <Avatar name={p.name} id={p.id} size={22} />
                  {p.name}
                </label>
              ))
            ))}
        </div>
      </div>
      {error && <div className="form-error">{error}</div>}
      <div className="modal-actions">
        <button className="btn ghost" onClick={onClose}>
          {t("cancel")}
        </button>
        <button className="btn primary" onClick={start} disabled={!canStart} title={canStart ? "" : t("pickRecipient")}>
          {t("startShare")}
        </button>
      </div>
    </Modal>
  );
}

function SourceRow({
  s,
  picked,
  onPick,
  t,
}: {
  s: SourceInfo;
  picked: SourceInfo | null;
  onPick: (s: SourceInfo) => void;
  t: Translate;
}) {
  const on = picked?.kind === s.kind && picked.id === s.id;
  return (
    <button className={`source-row ${on ? "on" : ""}`} onClick={() => onPick(s)}>
      <span className="source-icon">
        <IconMonitor size={16} />
      </span>
      <span className="source-text">
        <span className="truncate">
          {sourceLabel(s, t)}
          {s.kind === "monitor" && s.primary && s.id !== "portal" && <span className="dim"> ({t("primary")})</span>}
        </span>
        <span className="dim small truncate">
          {s.id === "portal"
            ? t("portalScreenHint")
            : s.kind === "monitor"
              ? `${s.name ? s.name + " · " : ""}${s.width}×${s.height}`
              : s.process}
        </span>
      </span>
    </button>
  );
}
