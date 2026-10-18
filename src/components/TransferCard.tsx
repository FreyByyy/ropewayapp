import { api, type TransferView } from "../api";
import { fmtSize, fmtSpeed, FILES, plural } from "../format";
import { useI18n, type MessageKey } from "../i18n";
import { IconDownload, IconFiles, IconFolder, IconPalette, IconUpload } from "../icons";
import { isThemeFile, useThemeInstall } from "../theme/install";

const ACTIVE_LIKE = new Set(["active", "waiting", "pending", "interrupted"]);

export function TransferCard({ t }: { t: TransferView }) {
  const { t: tr, lang } = useI18n();
  const installTheme = useThemeInstall();
  const themeFile =
    t.direction === "incoming" && t.status === "completed" && t.file_count === 1 && isThemeFile(t.open_path);
  const incoming = t.direction === "incoming";
  const percent = t.total > 0 ? Math.min(100, (t.transferred / t.total) * 100) : t.status === "completed" ? 100 : 0;
  const title =
    t.root_count > 1 ? `${t.names[0]} +${t.root_count - 1}` : (t.names[0] ?? plural(t.file_count, lang, FILES));
  const isFolder = t.file_count > 1 || t.root_count > 1;
  const run = (f: () => Promise<unknown>) => () => {
    f().catch((e) => console.error(e));
  };

  return (
    <div className={`row ${incoming ? "in" : "out"}`}>
      <div className={`transfer status-${t.status}`}>
        <div className="transfer-top">
          <span className="transfer-icon">{isFolder ? <IconFolder size={20} /> : <IconFiles size={20} />}</span>
          <div className="transfer-text">
            <div className="transfer-title truncate" title={t.names.join(", ")}>
              {title}
            </div>
            <div className="transfer-meta">
              {plural(t.file_count, lang, FILES)} · {fmtSize(t.total, lang)}
            </div>
          </div>
          <span className="transfer-dir" title={incoming ? "↓" : "↑"}>
            {incoming ? <IconDownload size={15} /> : <IconUpload size={15} />}
          </span>
        </div>

        {ACTIVE_LIKE.has(t.status) && t.status !== "pending" && (
          <div className="progress">
            <div className="progress-bar" style={{ width: `${percent}%` }} />
          </div>
        )}

        <div className="transfer-status">
          <span className="truncate">{statusLine(t, tr, lang)}</span>
        </div>
        {t.error && (t.status === "failed" || t.status === "interrupted") && (
          <div className="transfer-error" title={t.error}>
            {t.error}
          </div>
        )}

        <div className="transfer-actions">
          {incoming && t.status === "pending" && (
            <>
              <button className="btn primary small" onClick={run(() => api.acceptTransfer(t.id))}>
                {tr("accept")}
              </button>
              <button className="btn ghost small" onClick={run(() => api.rejectTransfer(t.id))}>
                {tr("decline")}
              </button>
            </>
          )}
          {incoming && t.status === "interrupted" && (
            <button className="btn ghost small" onClick={run(() => api.retryTransfer(t.id))}>
              {tr("resume")}
            </button>
          )}
          {!incoming && t.status === "failed" && (
            <button className="btn ghost small" onClick={run(() => api.retryTransfer(t.id))}>
              {tr("sendAgain")}
            </button>
          )}
          {["active", "waiting", "interrupted"].includes(t.status) && (
            <button className="btn ghost small" onClick={run(() => api.cancelTransfer(t.id))}>
              {tr("cancelTransfer")}
            </button>
          )}
          {themeFile && (
            <button className="btn primary small" onClick={() => installTheme(t.open_path!)}>
              <IconPalette size={14} /> {tr("installTheme")}
            </button>
          )}
          {incoming && t.status === "completed" && t.open_path && (
            <>
              <button className="btn ghost small" onClick={run(() => api.openPath(t.open_path!))}>
                {tr("open")}
              </button>
              <button className="btn ghost small" onClick={run(() => api.revealPath(t.open_path!))}>
                {tr("showInFolder")}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function statusLine(t: TransferView, tr: (k: MessageKey) => string, lang: string) {
  const incoming = t.direction === "incoming";
  switch (t.status) {
    case "waiting":
      return tr("trWaiting");
    case "pending":
      return tr("trPending");
    case "active": {
      const parts = [`${fmtSize(t.transferred, lang)} ${tr("of")} ${fmtSize(t.total, lang)}`];
      if (t.speed > 0) parts.push(fmtSpeed(t.speed, lang));
      return parts.join(" · ");
    }
    case "interrupted":
      return `${tr("trInterrupted")} · ${fmtSize(t.transferred, lang)} ${tr("of")} ${fmtSize(t.total, lang)}`;
    case "completed":
      return incoming ? tr("trReceived") : tr("trDelivered");
    case "rejected":
      return tr("trRejected");
    case "cancelled":
      return tr("trCancelled");
    case "failed":
      return tr("trFailed");
  }
}
