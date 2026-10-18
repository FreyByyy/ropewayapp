import { useEffect, useMemo, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type InputEvent, type Language, type MonitorChoice, type VideoSettings } from "../api";
import { I18n, makeT } from "../i18n";
import { AudioPlayer } from "./audio";
import { attachControl, pressWinKey, switchMonitor, tapKey } from "./control";
import { codecFromKeyframe, parsePacket, PKT_AUDIO, PKT_INFO, PKT_VIDEO } from "./packets";
import { Panel } from "./Panel";
import { MobileKeyboard } from "./MobileKeyboard";
import { IS_MOBILE } from "../platform";

type Status = "connecting" | "waiting" | "live" | "ended" | "error";

interface Info {
  width: number;
  height: number;
  audio: boolean;
  monitors?: MonitorChoice[];
  monitor?: string | null;
  video?: VideoSettings;
}

function params() {
  const query = window.location.hash.split("?")[1] ?? "";
  const p = new URLSearchParams(query);
  return { peer: p.get("peer") ?? "", session: p.get("session") ?? "" };
}

/**
 * Shows a shared screen. Desktop: a window of its own, told what to show by
 * its URL. Phone: drawn over the app with `peer`/`session` and `onClose`.
 */
export function Viewer(props: { peer?: string; session?: string; onClose?: () => void } = {}) {
  const fromUrl = useMemo(params, []);
  const peer = props.peer ?? fromUrl.peer;
  const session = props.session ?? fromUrl.session;
  const onClose = props.onClose;
  const [lang, setLang] = useState<Language>("ru");
  const t = useMemo(() => makeT(lang), [lang]);
  const [status, setStatus] = useState<Status>("connecting");
  const [error, setError] = useState("");
  const [peerName, setPeerName] = useState("");
  const [phone, setPhone] = useState(false);
  const [linux, setLinux] = useState(false);
  const [mac, setMac] = useState(false);
  /** `null` until known; then whether this window may control. */
  const [canControl, setCanControl] = useState<boolean | null>(null);
  /** May change the stream's quality (controller, or asked for this share). */
  const [canAdjust, setCanAdjust] = useState(false);
  const [controlOn, setControlOn] = useState(true);
  const [size, setSize] = useState<{ w: number; h: number } | null>(null);
  const [info, setInfo] = useState<Info | null>(null);
  const [muted, setMuted] = useState(false);
  const [volume, setVolume] = useState(1);
  const [audioBlocked, setAudioBlocked] = useState(false);
  const [fit, setFit] = useState(true);
  const [fullscreen, setFullscreen] = useState(false);
  const [fps, setFps] = useState(0);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const audioRef = useRef<AudioPlayer | null>(null);
  const controlOnRef = useRef(controlOn);
  controlOnRef.current = controlOn;

  // Who is this, and may we control?
  useEffect(() => {
    api.settings().then((s) => setLang(s.language)).catch(() => {});
    Promise.all([api.screen(), api.peers()])
      .then(([screen, peers]) => {
        const share = screen.available.find((s) => s.id === session);
        setPeerName(share?.peer_name ?? peers.find((p) => p.id === peer)?.name ?? "");
        const os = peers.find((p) => p.id === peer)?.os;
        setPhone(os === "android");
        setLinux(os === "linux");
        setMac(os === "macos");
        setCanControl(!!share?.control);
        setCanAdjust(!!share?.control || !!share?.requested);
      })
      .catch(() => setCanControl(false));
  }, [peer, session]);

  useEffect(() => {
    const title = peerName ? t("screenOf", { name: peerName }) : "Ropeway";
    document.title = title;
    getCurrentWindow().setTitle(title).catch(() => {});
  }, [peerName, t]);

  useEffect(() => {
    const onChange = () => setFullscreen(!!document.fullscreenElement);
    document.addEventListener("fullscreenchange", onChange);
    return () => document.removeEventListener("fullscreenchange", onChange);
  }, []);

  // Input goes to the remote computer while control is on.
  useEffect(() => {
    if (!canControl || !canvasRef.current) return;
    // A phone takes typed text rather than key codes.
    const detach = attachControl(canvasRef.current, () => controlOnRef.current, phone);
    canvasRef.current.focus();
    return detach;
  }, [canControl, phone]);

  useEffect(() => {
    if (canControl === null) return;
    if (typeof VideoDecoder === "undefined") {
      setStatus("error");
      setError("no-decoder");
      return;
    }
    const canvas = canvasRef.current!;
    const ctx = canvas.getContext("2d", { alpha: false })!;
    const audio = new AudioPlayer();
    audioRef.current = audio;
    let decoder: VideoDecoder | null = null;
    let codec = "";
    let waitingForKey = true;
    let frames = 0;
    let disposed = false;

    const fpsTimer = setInterval(() => {
      setFps(frames);
      frames = 0;
      setAudioBlocked(audio.blocked);
    }, 1000);

    const resetDecoder = () => {
      if (decoder && decoder.state !== "closed") decoder.close();
      decoder = null;
      waitingForKey = true;
    };

    const makeDecoder = (c: string) => {
      resetDecoder();
      decoder = new VideoDecoder({
        output: (frame) => {
          if (canvas.width !== frame.displayWidth || canvas.height !== frame.displayHeight) {
            canvas.width = frame.displayWidth;
            canvas.height = frame.displayHeight;
            setSize({ w: frame.displayWidth, h: frame.displayHeight });
          }
          ctx.drawImage(frame, 0, 0);
          frame.close();
          frames++;
          setStatus((s) => (s === "live" ? s : "live"));
        },
        error: (e) => {
          console.error("decoder", e);
          resetDecoder();
        },
      });
      decoder.configure({ codec: c, optimizeForLatency: true });
      codec = c;
    };

    const channel = new Channel<ArrayBuffer | number[]>();
    channel.onmessage = (message) => {
      if (disposed) return;
      const bytes = message instanceof ArrayBuffer ? new Uint8Array(message) : Uint8Array.from(message);
      const packet = parsePacket(bytes);
      if (!packet) return;
      switch (packet.kind) {
        case PKT_INFO: {
          setInfo(JSON.parse(new TextDecoder().decode(packet.payload)) as Info);
          setStatus((s) => (s === "connecting" ? "waiting" : s));
          break;
        }
        case PKT_VIDEO: {
          if (packet.keyframe) {
            const c = codecFromKeyframe(packet.payload);
            if (c && (c !== codec || !decoder || decoder.state === "closed")) makeDecoder(c);
            waitingForKey = false;
          }
          if (waitingForKey || !decoder || decoder.state !== "configured") return;
          decoder.decode(
            new EncodedVideoChunk({ type: packet.keyframe ? "key" : "delta", timestamp: packet.ts, data: packet.payload }),
          );
          break;
        }
        case PKT_AUDIO:
          audio.play(packet.payload);
          break;
      }
    };

    invoke("watch_screen", { peer, session, control: canControl, channel })
      .then(() => {
        if (!disposed) setStatus("ended");
      })
      .catch((e) => {
        if (disposed) return;
        setStatus("error");
        setError(String(e));
      });

    return () => {
      disposed = true;
      clearInterval(fpsTimer);
      resetDecoder();
      audio.close();
      // A phone's viewer lives in the main window, which stays open.
      if (IS_MOBILE) api.stopWatch().catch(() => {});
    };
  }, [peer, session, canControl]);

  useEffect(() => {
    audioRef.current?.setVolume(muted ? 0 : volume);
  }, [muted, volume]);

  const toggleFullscreen = () => {
    if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
    else document.documentElement.requestFullscreen().catch(() => {});
  };

  const controlling = !!canControl && controlOn;
  const monitors = info?.monitors ?? [];

  const statusText =
    status === "connecting"
      ? t("viewerConnecting")
      : status === "waiting"
        ? t("viewerWaitingPicture")
        : status === "ended"
          ? t("viewerEnded")
          : status === "error"
            ? error === "no-decoder"
              ? t("viewerNoDecoder")
              : t("viewerError", { error })
            : null;

  return (
    <I18n.Provider value={{ t, lang }}>
      <div
        className={`viewer ${fit ? "fit" : "actual"} ${controlling ? "controlling" : ""}`}
        onDoubleClick={controlling ? undefined : toggleFullscreen}
      >
        <div className="viewer-stage">
          <canvas ref={canvasRef} tabIndex={0} className={status === "live" ? "" : "hidden"} />
        </div>

        {statusText && (
          <div className="viewer-status">
            {status === "connecting" || status === "waiting" ? <span className="spinner" /> : null}
            <span>{statusText}</span>
          </div>
        )}

        {audioBlocked && info?.audio && !muted && (
          <button className="viewer-unblock" onClick={() => audioRef.current?.resume()}>
            {t("enableSound")}
          </button>
        )}

        {onClose && (
          <button className="viewer-fab close" onClick={onClose} aria-label={t("closeViewer")} title={t("closeViewer")}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              <path d="M6 6l12 12M18 6 6 18" />
            </svg>
          </button>
        )}
        {IS_MOBILE && controlling && status === "live" && <MobileKeyboard />}

        {canControl !== null && (
          <Panel
            title={peerName}
            meta={size && status === "live" ? `${size.w}×${size.h} · ${fps} fps` : null}
            canControl={canControl}
            controlOn={controlOn}
            onToggleControl={() => setControlOn((v) => !v)}
            onWin={pressWinKey}
            phone={phone}
            linux={linux}
            mac={mac}
            onPhoneKey={(key) =>
              tapKey(key === "back" ? "BrowserBack" : key === "home" ? "BrowserHome" : "ContextMenu")
            }
            monitors={monitors}
            monitor={info?.monitor ?? null}
            onMonitor={switchMonitor}
            hasAudio={!!info?.audio}
            muted={muted}
            volume={volume}
            onMute={() => setMuted((m) => !m)}
            onVolume={(v) => {
              setVolume(v);
              setMuted(false);
            }}
            fit={fit}
            onFit={() => setFit((f) => !f)}
            fullscreen={fullscreen}
            onFullscreen={toggleFullscreen}
            video={info?.video ?? null}
            canAdjust={canAdjust}
            onVideo={(video) => {
              const event: InputEvent = { t: "set_video", video };
              api.sendInput(event).catch(() => {});
              setInfo((i) => (i ? { ...i, video } : i));
            }}
          />
        )}
      </div>
    </I18n.Provider>
  );
}
