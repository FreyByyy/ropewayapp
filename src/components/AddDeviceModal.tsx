import { useEffect, useRef, useState } from "react";
import { Format, requestPermissions, scan } from "@tauri-apps/plugin-barcode-scanner";
import { api, type PairingCode, type PeerView, type QrMatrix } from "../api";
import { useI18n } from "../i18n";
import { IS_MOBILE } from "../platform";
import { IconScan } from "../icons";
import { BrandQr } from "./BrandQr";
import { explainError } from "../errors";
import { Modal } from "./Modals";

type Tab = "qr" | "code" | "ip";

/**
 * Adding a device: this device's QR code, a temporary code, an address, or
 * (on a phone) scanning another device's QR code. Ends with the device found
 * and an unticked "trust it" box.
 */
export function AddDeviceModal({
  peers,
  myCode,
  onClose,
  onDone,
}: {
  peers: PeerView[];
  myCode: string;
  onClose: () => void;
  onDone: (peer: PeerView) => void;
}) {
  const { t } = useI18n();
  const [tab, setTab] = useState<Tab>(IS_MOBILE ? "code" : "qr");
  const [found, setFound] = useState<PeerView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // A device never seen before that connects while the QR or code is shown
  // is the one being added.
  const known = useRef(new Set(peers.map((p) => p.id)));
  useEffect(() => {
    if (found) return;
    const newcomer = peers.find((p) => p.online && !known.current.has(p.id));
    if (newcomer) setFound(newcomer);
  }, [peers, found]);

  const attempt = async (connect: () => Promise<PeerView>) => {
    setBusy(true);
    setError(null);
    try {
      const peer = await connect();
      known.current.add(peer.id);
      setFound(peer);
    } catch (e) {
      setError(explainError(e, t));
    } finally {
      setBusy(false);
    }
  };

  const scanQr = async () => {
    setError(null);
    try {
      const allowed = await requestPermissions();
      if (allowed !== "granted") {
        setError(t("cameraDenied"));
        return;
      }
      const result = await scan({ windowed: false, formats: [Format.QRCode] });
      if (result?.content) await attempt(() => api.connectInvite(result.content));
    } catch (e) {
      setError(String(e));
    }
  };

  if (found) {
    return <Found peer={found} myCode={myCode} onClose={onClose} onDone={onDone} />;
  }

  return (
    <Modal title={t("addDevice")} onClose={onClose}>
      {IS_MOBILE && (
        <button className="btn primary scan-btn" onClick={scanQr} disabled={busy}>
          <IconScan size={18} />
          <span>{t("scanQr")}</span>
        </button>
      )}
      <div className="segmented add-tabs">
        {(["qr", "code", "ip"] as Tab[]).map((id) => (
          <button key={id} className={tab === id ? "on" : ""} onClick={() => setTab(id)}>
            {t(id === "qr" ? "tabMyQr" : id === "code" ? "tabCode" : "tabIp")}
          </button>
        ))}
      </div>
      {tab === "qr" && <QrTab />}
      {tab === "code" && <CodeTab busy={busy} onConnect={(code) => attempt(() => api.connectCode(code))} />}
      {tab === "ip" && <IpTab busy={busy} onConnect={(address) => attempt(() => api.connectAddress(address))} />}
      {error && <div className="form-error">{error}</div>}
    </Modal>
  );
}

function QrTab() {
  const { t } = useI18n();
  const [qr, setQr] = useState<QrMatrix | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.inviteQr().then(setQr).catch((e) => setError(String(e)));
  }, []);
  return (
    <div className="add-pane qr-pane">
      {qr ? <BrandQr qr={qr} size={232} label={t("myQrLabel")} /> : <div className="qr-placeholder" />}
      <p className="modal-text dim">{t(IS_MOBILE ? "myQrHintMobile" : "myQrHint")}</p>
      {error && <div className="form-error">{error}</div>}
    </div>
  );
}

function CodeTab({ busy, onConnect }: { busy: boolean; onConnect: (code: string) => void }) {
  const { t } = useI18n();
  const [mine, setMine] = useState<PairingCode | null>(null);
  const [now, setNow] = useState(Date.now());
  const [input, setInput] = useState("");

  // The code stops working when the window closes.
  useEffect(() => () => void api.stopPairingCode().catch(() => {}), []);
  useEffect(() => {
    if (!mine) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [mine]);

  const left = mine ? Math.max(0, Math.round((mine.expires - now) / 1000)) : 0;
  const digits = input.replace(/\D/g, "").slice(0, 6);

  return (
    <div className="add-pane code-pane">
      <div className="code-block">
        <span className="field-label">{t("myCode")}</span>
        {mine && left > 0 ? (
          <>
            <div className="big-code mono">
              {mine.code.slice(0, 3)} {mine.code.slice(3)}
            </div>
            <p className="field-hint">{t("myCodeHint", { time: `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}` })}</p>
          </>
        ) : (
          <>
            <p className="field-hint">{t("myCodeIntro")}</p>
            <button className="btn" onClick={() => api.startPairingCode().then(setMine).catch(console.error)}>
              {t("showCode")}
            </button>
          </>
        )}
      </div>
      <div className="code-block">
        <span className="field-label">{t("enterCode")}</span>
        <div className="code-entry">
          <input
            className="wide-input mono code-input"
            inputMode="numeric"
            autoComplete="off"
            placeholder="000 000"
            value={digits.length > 3 ? `${digits.slice(0, 3)} ${digits.slice(3)}` : digits}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && digits.length === 6 && onConnect(digits)}
          />
          <button className="btn primary" disabled={busy || digits.length !== 6} onClick={() => onConnect(digits)}>
            {busy ? t("searchingShort") : t("connect")}
          </button>
        </div>
        <p className="field-hint">{t("enterCodeHint")}</p>
      </div>
    </div>
  );
}

function IpTab({ busy, onConnect }: { busy: boolean; onConnect: (address: string) => void }) {
  const { t } = useI18n();
  const [address, setAddress] = useState("");
  return (
    <div className="add-pane">
      <p className="modal-text">{t("ipBody")}</p>
      <div className="code-entry">
        <input
          className="wide-input mono"
          value={address}
          placeholder={t("ipPlaceholder")}
          onChange={(e) => setAddress(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && address.trim() && onConnect(address.trim())}
        />
        <button className="btn primary" onClick={() => onConnect(address.trim())} disabled={busy || !address.trim()}>
          {busy ? t("connecting") : t("connect")}
        </button>
      </div>
    </div>
  );
}

/** The device was found: show its code and offer to trust it. */
function Found({ peer, myCode, onClose, onDone }: { peer: PeerView; myCode: string; onClose: () => void; onDone: (p: PeerView) => void }) {
  const { t } = useI18n();
  const [trust, setTrust] = useState(peer.trusted);
  const [error, setError] = useState<string | null>(null);
  const done = async () => {
    try {
      if (trust !== peer.trusted) await api.setTrusted(peer.id, trust);
      onDone({ ...peer, trusted: trust });
    } catch (e) {
      setError(String(e));
    }
  };
  return (
    <Modal title={t("deviceAdded", { name: peer.name })} onClose={onClose}>
      <p className="modal-text">{t("deviceAddedBody")}</p>
      <div className="code-compare">
        <span className="mono big-code">{peer.code}</span>
      </div>
      <p className="modal-text tiny dim">
        {t("deviceCode")} ({t("thisDevice").toLowerCase()}): <span className="mono">{myCode}</span>
      </p>
      <label className="check-row">
        <input type="checkbox" checked={trust} onChange={(e) => setTrust(e.target.checked)} />
        <span>
          {t("trustThisDevice")}
          <span className="field-hint">{t("trustThisDeviceHint")}</span>
        </span>
      </label>
      {error && <div className="form-error">{error}</div>}
      <div className="modal-actions">
        <button className="btn primary" onClick={done}>
          {t("done")}
        </button>
      </div>
    </Modal>
  );
}
