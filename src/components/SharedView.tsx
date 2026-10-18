import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { BackButton } from "./MobileNav";
import {
  api,
  joinLocal,
  type ChangeRequest,
  type EditView,
  type Folders,
  type MeView,
  type PeerView,
  type SharedEntry,
  type SharedListing,
  type TransferView,
} from "../api";
import { fmtDate, fmtSize } from "../format";
import type { Dispatch } from "../store";
import { useI18n, type MessageKey } from "../i18n";
import {
  IconChevronRight,
  IconDownload,
  IconExternal,
  IconFiles,
  IconFolder,
  IconPencil,
  IconRefresh,
  IconTrash,
  IconUpload,
} from "../icons";
import { Avatar } from "./Avatar";
import { Modal } from "./Modals";
import { TransferCard } from "./TransferCard";

const ME = "me";
const POLL_MS = 5000;
const RECENT_MS = 90_000;

interface Props {
  me: MeView;
  peers: PeerView[];
  changes: ChangeRequest[];
  edits: EditView[];
  transfers: Record<string, TransferView>;
  folders: Folders;
  dispatch: Dispatch;
  onOpenSettings: () => void;
}

export function SharedView({ me, peers, changes, edits, transfers, folders, dispatch, onOpenSettings }: Props) {
  const { t } = useI18n();
  const [source, setSource] = useState(ME);
  const [dir, setDir] = useState("");
  const [listing, setListing] = useState<SharedListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [toDelete, setToDelete] = useState<SharedEntry | null>(null);
  const requestSeq = useRef(0);

  const online = peers.filter((p) => p.online);
  const peer = source === ME ? undefined : peers.find((p) => p.id === source);

  // A device that went offline cannot be browsed.
  useEffect(() => {
    if (source !== ME && !peer?.online) setSource(ME);
  }, [source, peer?.online]);

  const load = useCallback(async () => {
    const seq = ++requestSeq.current;
    // Copies are edited in other programs; re-check whether they changed.
    api.edits().then((list) => dispatch({ type: "edits", edits: list })).catch(() => {});
    try {
      const result = source === ME ? await api.myShared() : await api.peerShared(source);
      if (seq !== requestSeq.current) return;
      setListing(result);
      setError(null);
    } catch (e) {
      if (seq !== requestSeq.current) return;
      setError(String(e));
    }
  }, [source, dispatch]);

  useEffect(() => {
    setDir("");
    setListing(null);
    setError(null);
    load();
    const timer = setInterval(load, POLL_MS);
    return () => clearInterval(timer);
  }, [load]);

  // Approved changes alter listings; refresh right away.
  const appliedCount = changes.filter((c) => c.status === "applied").length;
  useEffect(() => {
    load();
  }, [appliedCount, folders.shared_dir, folders.sharing_enabled, load]);

  const rows = useMemo(() => {
    if (!listing) return [];
    return listing.entries
      .filter((e) => parentOf(e.path) === dir)
      .sort((a, b) => Number(b.is_dir) - Number(a.is_dir) || nameOf(a.path).localeCompare(nameOf(b.path)));
  }, [listing, dir]);

  const incoming = changes.filter((c) => c.direction === "incoming" && (c.status === "pending" || c.status === "applying"));
  const mine = changes.filter((c) => c.direction === "outgoing").slice(0, 6);
  const now = Date.now();
  const copies = Object.values(transfers)
    .filter((tr) => tr.kind === "download" && (!tr.finished || now - tr.finished < RECENT_MS))
    .sort((a, b) => b.created - a.created)
    .slice(0, 4);

  const act = (f: () => Promise<unknown>) => {
    setActionError(null);
    f().catch((e) => setActionError(String(e)));
  };

  const crumbs = dir ? dir.split("/") : [];
  const sourceName = peer ? peer.name : t("myFiles");

  return (
    <section className="shared">
      <header className="chat-head">
        <BackButton />
        <span className="head-icon">
          <IconFolder size={20} />
        </span>
        <div className="head-text">
          <h1>{t("sharedFolder")}</h1>
          <p>{t("sharedHint")}</p>
        </div>
        <div className="head-actions">
          {source === ME && listing?.root && (
            <button className="tool" onClick={() => act(() => api.openPath(listing.root!))}>
              <IconExternal size={16} />
              <span>{t("openFolder")}</span>
            </button>
          )}
          <button className="tool" onClick={load} title={t("refresh")}>
            <IconRefresh size={16} />
            <span>{t("refresh")}</span>
          </button>
        </div>
      </header>

      <div className="shared-body">
        {incoming.length > 0 && (
          <div className="requests">
            <div className="block-title">{t("requestsTitle")}</div>
            {incoming.map((c) => (
              <IncomingRequest key={c.id} change={c} onError={setActionError} />
            ))}
          </div>
        )}

        <div className="sources">
          <button className={`source ${source === ME ? "on" : ""}`} onClick={() => setSource(ME)}>
            <Avatar name={me.name} id={me.id} size={22} />
            {t("myFiles")}
          </button>
          {online.map((p) => (
            <button key={p.id} className={`source ${source === p.id ? "on" : ""}`} onClick={() => setSource(p.id)}>
              <Avatar name={p.name} id={p.id} size={22} />
              {p.name}
            </button>
          ))}
          {online.length === 0 && <span className="sources-hint">{t("noDevicesOnline")}</span>}
        </div>

        {copies.length > 0 && (
          <div className="copies">
            {copies.map((tr) => (
              <TransferCard key={tr.id} t={tr} />
            ))}
          </div>
        )}

        {actionError && <div className="form-error">{actionError}</div>}

        <div className="browser">
          <div className="crumbs">
            <button onClick={() => setDir("")} className={dir ? "" : "current"}>
              {sourceName}
            </button>
            {crumbs.map((seg, i) => {
              const path = crumbs.slice(0, i + 1).join("/");
              return (
                <span key={path} className="crumb">
                  <IconChevronRight size={14} />
                  <button onClick={() => setDir(path)} className={path === dir ? "current" : ""}>
                    {seg}
                  </button>
                </span>
              );
            })}
          </div>

          {error ? (
            <div className="browser-empty">
              {error.includes("trusted devices only") ? t("sharedTrustedOnlyError") : t("loadFailed", { error })}
            </div>
          ) : !listing ? (
            <div className="browser-empty">{t("loading")}</div>
          ) : !listing.enabled ? (
            <div className="browser-empty">
              <p>{source === ME ? t("sharingOff") : t("peerSharingOff")}</p>
              {source === ME && (
                <>
                  <p className="dim">{t("sharingOffHint")}</p>
                  <button className="btn primary small" onClick={onOpenSettings}>
                    {t("settings")}
                  </button>
                </>
              )}
            </div>
          ) : rows.length === 0 ? (
            <div className="browser-empty">{source === ME && !dir ? t("emptyMyShared") : t("emptyFolder")}</div>
          ) : (
            <table className="files">
              <thead>
                <tr>
                  <th>{t("colName")}</th>
                  <th className="num">{t("colSize")}</th>
                  <th className="num">{t("colModified")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {rows.map((e) => (
                  <EntryRow
                    key={e.path}
                    entry={e}
                    peer={peer}
                    root={listing.root}
                    edit={peer ? edits.find((x) => x.peer === peer.id && x.path === e.path) : undefined}
                    pending={
                      peer
                        ? changes.find(
                            (c) =>
                              c.direction === "outgoing" &&
                              c.peer === peer.id &&
                              c.path === e.path &&
                              c.status === "pending",
                          )
                        : undefined
                    }
                    onEnter={() => setDir(e.path)}
                    onDelete={() => setToDelete(e)}
                    act={act}
                  />
                ))}
              </tbody>
            </table>
          )}
        </div>

        {mine.length > 0 && (
          <div className="my-requests">
            <div className="block-title">{t("myRequests")}</div>
            <ul>
              {mine.map((c) => (
                <li key={c.id}>
                  <span className={`chip ch-${c.status}`}>{t(CHANGE_STATUS[c.status])}</span>
                  <span className="dim">{c.kind === "replace" ? t("chReplace") : t("chDelete")}</span>
                  <span className="truncate mono-ish">{c.path}</span>
                  <span className="dim">· {c.peer_name}</span>
                  {c.error && (
                    <span className="req-error truncate" title={c.error}>
                      {c.error}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>

      {toDelete && peer && (
        <Modal title={t("confirmDeleteTitle", { name: nameOf(toDelete.path) })} onClose={() => setToDelete(null)}>
          <p className="modal-text">{t("confirmDeleteBody")}</p>
          <div className="modal-actions">
            <button className="btn ghost" onClick={() => setToDelete(null)}>
              {t("cancel")}
            </button>
            <button
              className="btn danger"
              onClick={() => {
                const path = toDelete.path;
                setToDelete(null);
                act(() => api.proposeDelete(peer.id, path));
              }}
            >
              {t("sendRequest")}
            </button>
          </div>
        </Modal>
      )}
    </section>
  );
}

const CHANGE_STATUS: Record<ChangeRequest["status"], MessageKey> = {
  pending: "chPending",
  applying: "chApplying",
  applied: "chApplied",
  rejected: "chRejected",
  failed: "chFailed",
};

function EntryRow({
  entry,
  peer,
  root,
  edit,
  pending,
  onEnter,
  onDelete,
  act,
}: {
  entry: SharedEntry;
  peer?: PeerView;
  root: string | null;
  edit?: EditView;
  pending?: ChangeRequest;
  onEnter: () => void;
  onDelete: () => void;
  act: (f: () => Promise<unknown>) => void;
}) {
  const { t, lang } = useI18n();
  const name = nameOf(entry.path);
  const local = !peer && root ? joinLocal(root, entry.path) : null;

  const openEntry = () => {
    if (entry.is_dir) onEnter();
    else if (local) act(() => api.openPath(local));
  };

  return (
    <tr className={entry.is_dir ? "dir" : ""}>
      <td className="name-cell">
        <button className="entry-name" onClick={openEntry} title={entry.path}>
          <span className={`entry-icon ${entry.is_dir ? "folder" : ""}`}>
            {entry.is_dir ? <IconFolder size={18} /> : <IconFiles size={18} />}
          </span>
          <span className="truncate">{name}</span>
        </button>
        {edit && (
          <span className={`chip ${edit.modified ? "ch-applied" : "ch-neutral"}`}>
            {!edit.ready ? t("copyLoading") : edit.modified ? t("copyModified") : t("copyHere")}
          </span>
        )}
        {pending && <span className="chip ch-pending">{t("awaitingApproval")}</span>}
      </td>
      <td className="num dim">{entry.is_dir ? "" : fmtSize(entry.size, lang)}</td>
      <td className="num dim">{fmtDate(entry.modified, lang)}</td>
      <td className="row-actions">
        {peer ? (
          <>
            {edit?.ready && (
              <>
                <IconButton label={t("openCopy")} onClick={() => act(() => api.openPath(edit.local))}>
                  <IconExternal size={16} />
                </IconButton>
                <button
                  className="btn primary small"
                  disabled={!edit.modified || !!pending}
                  title={edit.modified ? t("sendChanges") : t("editHint")}
                  onClick={() => act(() => api.proposeReplace(peer.id, entry.path))}
                >
                  <IconUpload size={14} /> {t("sendChanges")}
                </button>
                <button className="btn ghost small" onClick={() => act(() => api.discardEdit(peer.id, entry.path))}>
                  {t("discardCopy")}
                </button>
              </>
            )}
            {!edit && !entry.is_dir && (
              <IconButton label={`${t("edit")} — ${t("editHint")}`} onClick={() => act(() => api.editShared(peer.id, entry.path))}>
                <IconPencil size={16} />
              </IconButton>
            )}
            <IconButton label={t("download")} onClick={() => act(() => api.downloadShared(peer.id, entry.path))}>
              <IconDownload size={16} />
            </IconButton>
            <IconButton label={t("deleteReq")} danger disabled={!!pending} onClick={onDelete}>
              <IconTrash size={16} />
            </IconButton>
          </>
        ) : (
          local && (
            <IconButton label={t("showInFolder")} onClick={() => act(() => api.revealPath(local))}>
              <IconExternal size={16} />
            </IconButton>
          )
        )}
      </td>
    </tr>
  );
}

function IncomingRequest({ change, onError }: { change: ChangeRequest; onError: (e: string) => void }) {
  const { t, lang } = useI18n();
  const [busy, setBusy] = useState(false);
  const applying = change.status === "applying";
  const run = (f: () => Promise<unknown>) => {
    setBusy(true);
    f().catch((e) => {
      onError(String(e));
      setBusy(false);
    });
  };
  return (
    <div className="request">
      <span className={`request-icon ${change.kind}`}>
        {change.kind === "replace" ? <IconUpload size={18} /> : <IconTrash size={18} />}
      </span>
      <div className="request-text">
        <div>
          <b>{change.peer_name}</b> {change.kind === "replace" ? t("replaceReq") : t("deleteReqText")}{" "}
          <span className="mono-ish">«{change.path}»</span>
          {change.size != null && <span className="dim"> · {fmtSize(change.size, lang)}</span>}
        </div>
        {change.conflict && <div className="warn">{t("conflictWarn")}</div>}
        <div className="dim small">{change.kind === "replace" ? t("oldInBin") : t("deletedToBin")}</div>
      </div>
      <div className="request-actions">
        {applying ? (
          <span className="dim">{t("chApplying")}</span>
        ) : (
          <>
            <button className="btn primary small" disabled={busy} onClick={() => run(() => api.approveChange(change.id))}>
              {t("approve")}
            </button>
            <button className="btn ghost small" disabled={busy} onClick={() => run(() => api.rejectChange(change.id))}>
              {t("decline")}
            </button>
          </>
        )}
      </div>
    </div>
  );
}

function IconButton({
  label,
  onClick,
  danger,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button className={`icon-btn ${danger ? "danger" : ""}`} title={label} aria-label={label} onClick={onClick} disabled={disabled}>
      {children}
    </button>
  );
}

function nameOf(path: string) {
  const i = path.lastIndexOf("/");
  return i === -1 ? path : path.slice(i + 1);
}

function parentOf(path: string) {
  const i = path.lastIndexOf("/");
  return i === -1 ? "" : path.slice(0, i);
}
