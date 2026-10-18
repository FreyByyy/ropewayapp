import { api, type UpdateInfo } from "../api";
import { useI18n } from "../i18n";
import { IS_ANDROID } from "../platform";
import { changelogSection, notesFor, type Updates } from "../updates";
import { Modal } from "./Modals";

function Notes({ items }: { items: string[] }) {
  if (items.length === 0) return null;
  return (
    <ul className="update-notes">
      {items.map((item, i) => (
        <li key={i}>{item}</li>
      ))}
    </ul>
  );
}

function megabytes(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** Install progress, or the error of the last attempt. */
export function UpdateProgress({ updates }: { updates: Updates }) {
  const { t } = useI18n();
  const s = updates.state;
  if (s.kind === "installing") {
    const percent = s.total ? Math.min(100, Math.round((s.done / s.total) * 100)) : null;
    return (
      <div className="update-progress">
        <div className="progress">
          <div className={`progress-bar ${percent === null ? "indeterminate" : ""}`} style={{ width: `${percent ?? 100}%` }} />
        </div>
        <span className="field-hint">
          {percent === null ? t("updateDownloadingSize", { size: megabytes(s.done) }) : t("updateDownloading", { percent })}
        </span>
      </div>
    );
  }
  if (s.kind === "failed") return <div className="form-error">{t("updateFailed", { error: s.error })}</div>;
  return null;
}

/** "Version X is out. Install?" */
export function UpdateModal({ info, updates }: { info: UpdateInfo; updates: Updates }) {
  const { t, lang } = useI18n();
  const installing = updates.state.kind === "installing";
  return (
    <Modal title={t("updateTitle", { version: info.version })} onClose={() => !installing && updates.later()}>
      <Notes items={notesFor(info.notes, lang)} />
      {info.can_install ? (
        <p className="modal-text">{t(IS_ANDROID ? "updateBodyAndroid" : "updateBody")}</p>
      ) : (
        <p className="modal-text">{t("updateManual")}</p>
      )}
      <UpdateProgress updates={updates} />
      <div className="modal-actions">
        <button className="btn ghost" onClick={() => updates.skip(info)} disabled={installing}>
          {t("updateSkip")}
        </button>
        <button className="btn ghost" onClick={updates.later} disabled={installing}>
          {t("updateLater")}
        </button>
        {info.can_install ? (
          <button className="btn primary" onClick={() => updates.install(info)} disabled={installing}>
            {t("updateInstall")}
          </button>
        ) : (
          <button className="btn primary" onClick={() => api.openReleasesPage().catch(() => {})}>
            {t("updateOpenPage")}
          </button>
        )}
      </div>
    </Modal>
  );
}

/** The changes of the version just installed. */
export function WhatsNewModal({ version, onClose }: { version: string; onClose: () => void }) {
  const { t, lang } = useI18n();
  return (
    <Modal title={t("whatsNewTitle", { version })} onClose={onClose}>
      <Notes items={notesFor(changelogSection(version), lang)} />
      <div className="modal-actions">
        <button className="btn primary" onClick={onClose}>
          {t("whatsNewOk")}
        </button>
      </div>
    </Modal>
  );
}

/** The "Updates" card in Settings. */
export function UpdatesCard({ updates, version }: { updates: Updates; version: string }) {
  const { t } = useI18n();
  const s = updates.state;
  const busy = s.kind === "checking" || s.kind === "installing";
  const info = s.kind === "available" || s.kind === "installing" ? s.info : s.kind === "failed" ? s.info : null;
  return (
    <div className="card">
      <span className="field-label">{t("updates")}</span>
      <p className="field-hint">{t("updatesAuto")}</p>
      <div className="update-row">
        <span>
          {t("version")} <span className="mono">{version}</span>
          {s.kind === "checking" && <span className="dim"> · {t("updatesChecking")}</span>}
          {s.kind === "latest" && <span className="dim"> · {t("updatesLatest")}</span>}
          {info && s.kind !== "failed" && <strong> · {t("updatesAvailable", { version: info.version })}</strong>}
        </span>
        <div className="update-buttons">
          <button className="btn ghost small" onClick={() => updates.showWhatsNew(version)} disabled={!changelogSection(version)}>
            {t("updatesWhatsNew")}
          </button>
          <button className="btn ghost small" onClick={() => api.openReleasesPage().catch(() => {})}>
            {t("updatesAll")}
          </button>
          {info && info.can_install ? (
            <button className="btn primary small" onClick={() => updates.install(info)} disabled={busy}>
              {t("updateInstall")}
            </button>
          ) : (
            <button className="btn ghost small" onClick={updates.check} disabled={busy}>
              {t("updatesCheck")}
            </button>
          )}
        </div>
      </div>
      <UpdateProgress updates={updates} />
    </div>
  );
}
