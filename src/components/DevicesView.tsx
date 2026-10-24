import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type PeerView } from "../api";
import { useI18n } from "../i18n";
import { IS_LINUX, IS_MAC, IS_MOBILE } from "../platform";
import { IconAlert, IconCast, IconChat, IconFiles, IconPlus, IconPointer, IconRadar, IconShieldCheck, IconStar } from "../icons";
import { duplicateNames, isDuplicate } from "../identity";
import { Avatar } from "./Avatar";
import { BackButton } from "./MobileNav";

interface Props {
  peers: PeerView[];
  /** Unread messages and waiting file offers, per device. */
  badges: Record<string, number>;
  onOpen: (peer: PeerView) => void;
  onChat: (peer: PeerView) => void;
  onShareScreen: (peer: PeerView) => void;
  onAdd: () => void;
}

/** Sends files to a device, picked in the system dialog. */
export async function pickAndSend(peer: PeerView, directory = false) {
  const selected = IS_MOBILE ? await api.pickFiles() : await open({ multiple: true, directory });
  if (!selected) return;
  const paths = Array.isArray(selected) ? selected : [selected];
  if (paths.length) await api.sendPaths(peer.id, paths);
}

/** Start page: favourite devices first, then everything else on the network. */
export function DevicesView({ peers, badges, onOpen, onChat, onShareScreen, onAdd }: Props) {
  const { t } = useI18n();
  const dup = duplicateNames(peers);
  const favorites = peers.filter((p) => p.favorite);
  const online = peers.filter((p) => !p.favorite && p.online);
  const offline = peers.filter((p) => !p.favorite && !p.online);

  const card = (p: PeerView, big: boolean) => (
    <DeviceCard
      key={p.id}
      peer={p}
      big={big}
      duplicate={isDuplicate(dup, p.name)}
      badge={badges[p.id] ?? 0}
      onOpen={() => onOpen(p)}
      onChat={() => onChat(p)}
      onShareScreen={() => onShareScreen(p)}
    />
  );

  return (
    <section className="page devices-page">
      <header className="settings-head page-head">
        <BackButton />
        <h1>{t("myDevices")}</h1>
        <button className="btn primary small" onClick={onAdd}>
          <IconPlus size={15} />
          <span>{t("addDevice")}</span>
        </button>
      </header>
      <div className="page-body">
        {peers.length === 0 ? (
          <div className="devices-empty">
            <IconRadar className="pulse" size={34} />
            <p className="empty-title">{t("searching")}</p>
            <p className="empty-hint">
              {t(IS_MOBILE ? "searchingHintMobile" : IS_LINUX ? "searchingHintLinux" : IS_MAC ? "searchingHintMac" : "searchingHint")}
            </p>
            <button className="btn primary" onClick={onAdd}>
              {t("addDevice")}
            </button>
          </div>
        ) : (
          <>
            {favorites.length > 0 ? (
              <>
                <h2 className="settings-section">{t("favorites")}</h2>
                <div className="device-grid big">{favorites.map((p) => card(p, true))}</div>
              </>
            ) : (
              <p className="field-hint favorites-hint">
                <IconStar size={14} /> {t("favoritesHint")}
              </p>
            )}
            {online.length > 0 && (
              <>
                <h2 className="settings-section">{t("onlineDevices")}</h2>
                <div className="device-grid">{online.map((p) => card(p, false))}</div>
              </>
            )}
            {offline.length > 0 && (
              <>
                <h2 className="settings-section">{t("offlineDevices")}</h2>
                <div className="device-grid">{offline.map((p) => card(p, false))}</div>
              </>
            )}
          </>
        )}
      </div>
    </section>
  );
}

function DeviceCard({
  peer,
  big,
  duplicate,
  badge,
  onOpen,
  onChat,
  onShareScreen,
}: {
  peer: PeerView;
  big: boolean;
  duplicate: boolean;
  badge: number;
  onOpen: () => void;
  onChat: () => void;
  onShareScreen: () => void;
}) {
  const { t } = useI18n();
  const [sent, setSent] = useState(false);
  return (
    <div className={`device-tile ${big ? "big" : ""} ${peer.online ? "" : "is-offline"}`}>
      <button className="device-tile-main" onClick={onOpen}>
        <Avatar name={peer.name} id={peer.id} online={peer.online} size={big ? 46 : 38} />
        <span className="device-tile-text">
          <span className="device-tile-name">
            <span className="truncate">{peer.name}</span>
            {peer.trusted && <IconShieldCheck size={14} className="trusted-mark" />}
            {duplicate && (
              <span className="same-name" title={t("sameNameHint")}>
                <IconAlert size={13} />
              </span>
            )}
          </span>
          <span className="device-tile-sub">
            {duplicate ? <span className="mono">{peer.code}</span> : peer.online ? (peer.address ?? t("online")) : t("offline")}
          </span>
        </span>
        {badge > 0 && <span className="badge">{badge > 99 ? "99+" : badge}</span>}
      </button>
      <button
        className={`star ${peer.favorite ? "on" : ""}`}
        onClick={() => api.setFavorite(peer.id, !peer.favorite).catch(console.error)}
        title={peer.favorite ? t("unfavorite") : t("favorite")}
        aria-pressed={peer.favorite}
      >
        <IconStar size={17} />
      </button>
      {big && (
        <div className="device-tile-actions">
          <button className="tool" onClick={() => pickAndSend(peer).catch(console.error)} title={t("sendFiles")}>
            <IconFiles size={17} />
            <span>{t("files")}</span>
          </button>
          <button className="tool" disabled={!peer.online} onClick={onShareScreen} title={t("screenMenuShow")}>
            <IconCast size={17} />
            <span>{t("screen")}</span>
          </button>
          <button
            className="tool"
            disabled={!peer.online}
            onClick={() => api.askScreen(peer.id, true).then(() => setSent(true)).catch(console.error)}
            title={t("controlHint")}
          >
            <IconPointer size={17} />
            <span>{sent ? t("asked") : t("control")}</span>
          </button>
          <button className="tool" onClick={onChat} title={t("chat")}>
            <IconChat size={17} />
            <span>{t("chat")}</span>
          </button>
        </div>
      )}
    </div>
  );
}
