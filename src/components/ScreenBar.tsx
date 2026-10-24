import { useState } from "react";
import { api, type PeerView, type ScreenView } from "../api";
import { Modal } from "./Modals";
import { useI18n } from "../i18n";
import { IconCast } from "../icons";
import { IS_MOBILE } from "../platform";
import { explainError } from "../errors";

/** Live screen sharing state, shown above every view of the main window. */
export function ScreenBar({ screen, peers }: { screen: ScreenView; peers: PeerView[] }) {
  const { t } = useI18n();
  const [needControl, setNeedControl] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const run = (f: () => Promise<unknown>) => () => {
    f().catch((e) => setFailure(explainError(e, t)));
  };
  const nameOf = (id: string) => peers.find((p) => p.id === id)?.name ?? id.slice(0, 8);

  if (!screen.sharing.length && !screen.available.length && !screen.asks.length && !screen.asked.length) {
    return null;
  }

  return (
    <>
    {failure && (
      <Modal title={t("screenErrorTitle")} onClose={() => setFailure(null)}>
        <p className="modal-text pre-line">{failure}</p>
        <div className="modal-actions">
          <button className="btn primary" onClick={() => setFailure(null)}>
            OK
          </button>
        </div>
      </Modal>
    )}
    {needControl && (
      <Modal title={t("controlSetupTitle")} onClose={() => setNeedControl(false)}>
        <p className="modal-text">{t("controlSetupText")}</p>
        <div className="modal-actions">
          <button className="btn ghost" onClick={() => setNeedControl(false)}>
            {t("cancel")}
          </button>
          <button
            className="btn primary"
            onClick={() => {
              setNeedControl(false);
              api.openControlSettings().catch(console.error);
            }}
          >
            {t("controlSetupOpen")}
          </button>
        </div>
      </Modal>
    )}
    <div className="screen-bar">
      {screen.sharing.map((s) => (
        <div key={s.id} className="sb-row sharing">
          <span className="live-dot" />
          <span className="sb-text">
            {s.controllers.length > 0 ? (
              <b>{t("controlledBy", { names: s.controllers.map((v) => v.name).join(", ") })}</b>
            ) : (
              <>
                <b>{t("youShare")}</b>
                {s.title && <span className="dim"> · {s.title}</span>}
                <span className="dim">
                  {" · "}
                  {s.viewers.length
                    ? t("watchingNow", { names: s.viewers.map((v) => v.name).join(", ") })
                    : t("nobodyWatching")}
                </span>
              </>
            )}
          </span>
          <button className="btn danger small" onClick={run(() => api.stopShare(s.id))}>
            {t("stopShare")}
          </button>
        </div>
      ))}
      {screen.asks.map((a) => (
        <div key={a.id} className="sb-row ask">
          <IconCast size={16} />
          <span className="sb-text">
            <b>{a.peer_name}</b> {a.control ? t(IS_MOBILE ? "wantsToControlPhone" : "wantsToControl") : t("wantsToSee")}
          </span>
          <button
            className="btn primary small"
            onClick={run(async () => {
              if (IS_MOBILE && a.control && !(await api.controlEnabled())) {
                setNeedControl(true);
                return;
              }
              if (IS_MOBILE && !(await api.requestCapture())) return;
              await api.answerAsk(a.id, true);
            })}
          >
            {t("allow")}
          </button>
          <button className="btn ghost small" onClick={run(() => api.answerAsk(a.id, false))}>
            {t("decline")}
          </button>
        </div>
      ))}
      {screen.available.map((s) => (
        <div key={s.id} className="sb-row available">
          <IconCast size={16} />
          <span className="sb-text">
            <b>{s.peer_name}</b> {t("sharesWithYou")}
            {s.title && <span className="dim"> · {s.title}</span>}
          </span>
          <button className="btn primary small" onClick={run(() => api.openViewer(s.peer, s.id))}>
            {t("watch")}
          </button>
        </div>
      ))}
      {screen.asked.map((peer) => (
        <div key={peer} className="sb-row asked">
          <span className="spinner small" />
          <span className="sb-text dim">{t("waitingAnswer", { name: nameOf(peer) })}</span>
        </div>
      ))}
    </div>
    </>
  );
}
