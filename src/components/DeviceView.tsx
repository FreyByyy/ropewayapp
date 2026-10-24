import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, type PeerView, type TransferView } from "../api";
import { useI18n } from "../i18n";
import { IS_MOBILE } from "../platform";
import {
  IconAlert,
  IconCast,
  IconChat,
  IconFiles,
  IconFolder,
  IconPointer,
  IconShield,
  IconShieldCheck,
  IconStar,
} from "../icons";
import { duplicateNames, isDuplicate } from "../identity";
import { Avatar } from "./Avatar";
import { BackButton } from "./MobileNav";
import { TransferCard } from "./TransferCard";
import { pickAndSend } from "./DevicesView";

interface Props {
  peer: PeerView;
  peers: PeerView[];
  transfers: TransferView[];
  unread: number;
  onChat: () => void;
  onTrust: () => void;
  onShareScreen: () => void;
}

/** One device: what can be done with it, and the files exchanged with it. */
export function DeviceView({ peer, peers, transfers, unread, onChat, onTrust, onShareScreen }: Props) {
  const { t } = useI18n();
  const [asked, setAsked] = useState<"view" | "control" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const duplicate = isDuplicate(duplicateNames(peers), peer.name);
  const sorted = [...transfers].sort((a, b) => b.created - a.created);

  const ask = (control: boolean) => {
    setError(null);
    api
      .askScreen(peer.id, control)
      .then(() => setAsked(control ? "control" : "view"))
      .catch((e) => setError(String(e)));
  };

  return (
    <section className="page device-page">
      <header className="settings-head page-head">
        <BackButton />
        <Avatar name={peer.name} id={peer.id} online={peer.online} size={44} />
        <div className="head-text">
          <h1>{peer.name}</h1>
          <p>
            <span className={peer.online ? "status-on" : "status-off"}>{peer.online ? t("online") : t("offline")}</span>
            {peer.address && <span className="mono"> · {peer.address}</span>}
            <span className="mono dim"> · {peer.code}</span>
          </p>
        </div>
        <div className="head-actions">
          <button
            className={`tool star ${peer.favorite ? "on" : ""}`}
            onClick={() => api.setFavorite(peer.id, !peer.favorite).catch(console.error)}
            title={peer.favorite ? t("unfavorite") : t("favorite")}
            aria-pressed={peer.favorite}
          >
            <IconStar size={17} />
            <span>{peer.favorite ? t("inFavorites") : t("favorite")}</span>
          </button>
          <button className={`tool trust ${peer.trusted ? "on" : ""}`} onClick={onTrust} title={peer.trusted ? t("untrust") : t("trust")}>
            {peer.trusted ? <IconShieldCheck size={17} /> : <IconShield size={17} />}
            <span>{peer.trusted ? t("trusted") : t("trust")}</span>
          </button>
        </div>
      </header>

      <div className="page-body">
        {duplicate && (
          <p className="same-name-note">
            <IconAlert size={13} /> {t("sameNameHint")}
          </p>
        )}
        <div className="action-grid">
          <ActionTile
            icon={<IconFiles size={26} />}
            title={t("sendFilesTitle")}
            hint={peer.online ? t("sendFilesHint") : t("sendFilesOffline")}
            onClick={() => pickAndSend(peer).catch((e) => setError(String(e)))}
            extra={
              !IS_MOBILE && (
                <button className="tile-link" onClick={() => pickAndSend(peer, true).catch((e) => setError(String(e)))}>
                  <IconFolder size={14} /> {t("sendFolderLink")}
                </button>
              )
            }
          />
          <ScreenTile peer={peer} asked={asked === "view"} onShare={onShareScreen} onAsk={() => ask(false)} />
          <ActionTile
            icon={<IconPointer size={26} />}
            title={t("controlComputer")}
            hint={asked === "control" ? t("askSent") : peer.online ? t("controlTileHint") : t("offline")}
            disabled={!peer.online}
            onClick={() => ask(true)}
          />
          <ActionTile
            icon={<IconChat size={26} />}
            title={t("chat")}
            hint={unread ? t("unreadCount", { n: unread }) : t("chatTileHint")}
            badge={unread}
            onClick={onChat}
          />
        </div>
        {error && <div className="form-error">{error}</div>}

        <h2 className="settings-section">{t("transfersWith")}</h2>
        {sorted.length === 0 ? (
          <p className="field-hint">{IS_MOBILE ? t("noTransfersMobile") : t("noTransfers")}</p>
        ) : (
          <div className="device-transfers">
            {sorted.map((tr) => (
              <TransferCard key={tr.id} t={tr} />
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function ActionTile({
  icon,
  title,
  hint,
  onClick,
  disabled,
  badge,
  extra,
}: {
  icon: ReactNode;
  title: string;
  hint: string;
  onClick: () => void;
  disabled?: boolean;
  badge?: number;
  extra?: ReactNode;
}) {
  return (
    <div className={`action-tile ${disabled ? "disabled" : ""}`}>
      <button className="action-main" onClick={onClick} disabled={disabled}>
        <span className="action-icon">
          {icon}
          {!!badge && <span className="badge">{badge > 99 ? "99+" : badge}</span>}
        </span>
        <span className="action-title">{title}</span>
        <span className="action-hint">{hint}</span>
      </button>
      {extra}
    </div>
  );
}

/** Screen: show mine or ask to see theirs. */
function ScreenTile({ peer, asked, onShare, onAsk }: { peer: PeerView; asked: boolean; onShare: () => void; onAsk: () => void }) {
  const { t } = useI18n();
  const [menu, setMenu] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setMenu(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [menu]);

  return (
    <div className="menu-anchor action-anchor" ref={ref}>
      <ActionTile
        icon={<IconCast size={26} />}
        title={t("screenShare")}
        hint={asked ? t("askSent") : peer.online ? t("screenTileHint") : t("offline")}
        disabled={!peer.online}
        onClick={() => setMenu((v) => !v)}
      />
      {menu && (
        <div className="menu">
          <button
            onClick={() => {
              setMenu(false);
              onShare();
            }}
          >
            {t("screenMenuShow")}
          </button>
          <button
            onClick={() => {
              setMenu(false);
              onAsk();
            }}
          >
            {t("screenMenuAsk")}
          </button>
        </div>
      )}
    </div>
  );
}
