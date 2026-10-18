import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import type { MonitorChoice, VideoSettings } from "../api";
import { VideoFields } from "../components/VideoFields";
import { useI18n } from "../i18n";

const OPEN_KEY = "ropeway.viewer.panelOpen";
const POS_KEY = "ropeway.viewer.panelPos";

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage unavailable: the panel just forgets */
  }
}

interface Props {
  title: string;
  meta: string | null;
  canControl: boolean;
  controlOn: boolean;
  onToggleControl: () => void;
  onWin: () => void;
  /** The shared screen is a phone: system buttons instead of the Windows key. */
  phone: boolean;
  /** The shared screen is a Linux computer: its Windows key is "Super". */
  linux: boolean;
  /** A Mac: the Windows key is Command. */
  mac: boolean;
  onPhoneKey: (key: "back" | "home" | "recents") => void;
  monitors: MonitorChoice[];
  monitor: string | null;
  onMonitor: (id: string) => void;
  hasAudio: boolean;
  muted: boolean;
  volume: number;
  onMute: () => void;
  onVolume: (v: number) => void;
  fit: boolean;
  onFit: () => void;
  fullscreen: boolean;
  onFullscreen: () => void;
  /** Current stream settings and whether this viewer may change them. */
  video: VideoSettings | null;
  canAdjust: boolean;
  onVideo: (v: VideoSettings) => void;
}

/**
 * Viewer controls along the top edge. Collapses to a small tab and can be
 * dragged sideways, so it never has to cover the remote taskbar or a
 * window's buttons.
 */
export function Panel(p: Props) {
  const { t } = useI18n();
  // Controlling: start collapsed so the whole remote screen is reachable.
  const [open, setOpen] = useState<boolean>(() => load(OPEN_KEY, !p.canControl));
  const [pos, setPos] = useState<number>(() => load(POS_KEY, 0.5));
  const [settingsOpen, setSettingsOpen] = useState(false);
  const drag = useRef<{ startX: number; moved: boolean } | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [panelWidth, setPanelWidth] = useState(0);
  const [viewport, setViewport] = useState(window.innerWidth);

  useEffect(() => save(OPEN_KEY, open), [open]);

  // Keep the whole panel inside the window, whatever its width.
  useLayoutEffect(() => {
    const el = panelRef.current;
    if (!el) return;
    const measure = () => setPanelWidth(el.offsetWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [open]);

  useEffect(() => {
    const onResize = () => setViewport(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const margin = 8;
  const half = panelWidth / 2;
  const center = Math.min(Math.max(pos * viewport, half + margin), Math.max(half + margin, viewport - half - margin));

  const onGripDown = (e: ReactPointerEvent) => {
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag.current = { startX: e.clientX, moved: false };
  };
  const onGripMove = (e: ReactPointerEvent) => {
    if (!drag.current) return;
    if (Math.abs(e.clientX - drag.current.startX) > 3) drag.current.moved = true;
    if (drag.current.moved) setPos(Math.min(0.95, Math.max(0.05, e.clientX / window.innerWidth)));
  };
  const onGripUp = (e: ReactPointerEvent) => {
    const d = drag.current;
    drag.current = null;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    if (d?.moved) save(POS_KEY, pos);
    else if (!open) setOpen(true);
  };

  const grip = (
    <span
      className="vgrip"
      title={t("panelDrag")}
      onPointerDown={onGripDown}
      onPointerMove={onGripMove}
      onPointerUp={onGripUp}
    >
      ⋮⋮
    </span>
  );

  return (
    <div
      ref={panelRef}
      className={`vpanel ${open ? "open" : "closed"}`}
      style={{ left: `${center}px` }}
      onDoubleClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
    >
      {!open ? (
        <div className="vtab">
          {grip}
          <button className="vtab-title" onClick={() => setOpen(true)} title={t("panelExpand")}>
            {p.title || "Ropeway"} <span className="chev">▾</span>
          </button>
        </div>
      ) : (
        <>
          <div className="vbar">
            {grip}
            <span className="viewer-title">{p.title}</span>
            {p.meta && <span className="viewer-meta">{p.meta}</span>}
            <Sep />
            {p.canControl && (
              <>
                <button
                  className={`vbtn text toggle ${p.controlOn ? "on" : ""}`}
                  title={t("controlToggleHint")}
                  onClick={p.onToggleControl}
                >
                  {p.controlOn ? t("controlOn") : t("controlOff")}
                </button>
                {p.phone ? (
                  <>
                    <button className="vbtn text" onClick={() => p.onPhoneKey("back")}>
                      {t("phoneBack")}
                    </button>
                    <button className="vbtn text" onClick={() => p.onPhoneKey("home")}>
                      {t("phoneHome")}
                    </button>
                    <button className="vbtn text" onClick={() => p.onPhoneKey("recents")}>
                      {t("phoneRecents")}
                    </button>
                  </>
                ) : (
                  <button className="vbtn text" title={t("winKeyHint")} onClick={p.onWin}>
                    {t(p.linux ? "superKey" : p.mac ? "cmdKey" : "winKey")}
                  </button>
                )}
                {p.monitors.length > 1 && (
                  <select
                    className="vselect"
                    title={t("monitorSwitch")}
                    value={p.monitor ?? ""}
                    onChange={(e) => p.onMonitor(e.target.value)}
                  >
                    {p.monitors.map((m) => (
                      <option key={m.id} value={m.id}>
                        {t("monitor")} {m.label}
                        {m.primary ? ` (${t("primary")})` : ""}
                      </option>
                    ))}
                  </select>
                )}
                <Sep />
              </>
            )}
            {p.hasAudio && (
              <>
                <button className="vbtn" onClick={p.onMute} title={p.muted ? t("unmute") : t("mute")}>
                  {p.muted ? "🔇" : "🔊"}
                </button>
                <input
                  className="volume"
                  type="range"
                  min={0}
                  max={1}
                  step={0.05}
                  value={p.muted ? 0 : p.volume}
                  onChange={(e) => p.onVolume(Number(e.target.value))}
                />
              </>
            )}
            {p.canAdjust && p.video && (
              <button
                className={`vbtn ${settingsOpen ? "pressed" : ""}`}
                title={t("streamSettings")}
                onClick={() => setSettingsOpen((v) => !v)}
              >
                ⚙
              </button>
            )}
            <button className="vbtn text" onClick={p.onFit}>
              {p.fit ? t("actualSize") : t("fit")}
            </button>
            <button className="vbtn text" onClick={p.onFullscreen}>
              {p.fullscreen ? t("exitFullscreen") : t("fullscreen")}
            </button>
            <button
              className="vbtn"
              title={t("panelCollapse")}
              onClick={() => {
                setSettingsOpen(false);
                setOpen(false);
              }}
            >
              ▴
            </button>
          </div>
          {settingsOpen && p.video && (
            <Popover>
              <div className="vpop-title">{t("streamSettings")}</div>
              <VideoFields dark value={p.video} onChange={p.onVideo} />
            </Popover>
          )}
        </>
      )}
    </div>
  );
}

function Sep() {
  return <span className="vsep" />;
}

function Popover({ children }: { children: ReactNode }) {
  return <div className="vpop">{children}</div>;
}
