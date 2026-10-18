import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, type PeerView } from "../api";
import { useI18n } from "../i18n";
import { IconX } from "../icons";

export function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" role="dialog" aria-modal="true" aria-label={title}>
        <div className="modal-head">
          <h2>{title}</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <IconX size={16} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function TrustModal({ peer, myCode, onClose }: { peer: PeerView; myCode: string; onClose: () => void }) {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const trusting = !peer.trusted;

  const confirm = async () => {
    setBusy(true);
    try {
      await api.setTrusted(peer.id, trusting);
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  return (
    <Modal title={trusting ? t("trustTitle", { name: peer.name }) : t("untrustTitle", { name: peer.name })} onClose={onClose}>
      <p className="modal-text">{trusting ? t("trustBody") : t("untrustBody")}</p>
      {trusting && (
        <>
          <p className="modal-text dim">{t("trustCheck")}</p>
          <div className="code-compare">
            <span className="mono big-code">{peer.code}</span>
          </div>
          <p className="modal-text tiny dim">
            {t("deviceCode")} ({t("thisDevice").toLowerCase()}): <span className="mono">{myCode}</span>
          </p>
        </>
      )}
      {error && <div className="form-error">{error}</div>}
      <div className="modal-actions">
        <button className="btn ghost" onClick={onClose}>
          {t("cancel")}
        </button>
        <button className={`btn ${trusting ? "primary" : "danger"}`} onClick={confirm} disabled={busy}>
          {trusting ? t("trust") : t("untrust")}
        </button>
      </div>
    </Modal>
  );
}

export function AddIpModal({ onClose, onConnected }: { onClose: () => void; onConnected: (peer: PeerView) => void }) {
  const { t } = useI18n();
  const [address, setAddress] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => input.current?.focus(), []);

  const connect = async () => {
    if (!address.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const peer = await api.connectAddress(address.trim());
      onConnected(peer);
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  return (
    <Modal title={t("ipTitle")} onClose={onClose}>
      <p className="modal-text">{t("ipBody")}</p>
      <input
        ref={input}
        className="wide-input mono"
        value={address}
        placeholder={t("ipPlaceholder")}
        onChange={(e) => setAddress(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && connect()}
      />
      {error && <div className="form-error">{error}</div>}
      <div className="modal-actions">
        <button className="btn ghost" onClick={onClose}>
          {t("cancel")}
        </button>
        <button className="btn primary" onClick={connect} disabled={busy || !address.trim()}>
          {busy ? t("connecting") : t("connect")}
        </button>
      </div>
    </Modal>
  );
}
