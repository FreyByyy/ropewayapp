import type { ReactNode } from "react";
import { GENERAL, type ChatMessage, type MeView, type PeerView } from "../api";
import { IS_LINUX, IS_MAC, IS_MOBILE } from "../platform";
import { useI18n } from "../i18n";
import {
  IconAlert,
  IconChat,
  IconDevices,
  IconFolder,
  IconHash,
  IconMenu,
  IconPlus,
  IconRadar,
  IconSettings,
  IconShieldCheck,
  IconStar,
  Logo,
} from "../icons";
import { Avatar } from "./Avatar";
import { duplicateNames, isDuplicate } from "../identity";

export type Section = "devices" | "chats" | "shared" | "settings";

interface Props {
  me: MeView | null;
  peers: PeerView[];
  last: Record<string, ChatMessage>;
  unread: Record<string, number>;
  /** Incoming file offers waiting for a decision, per device. */
  offers: Record<string, number>;
  /** Change requests to this device's shared folder waiting for a decision. */
  requests: number;
  section: Section;
  /** The device or conversation open next to the list. */
  selected: string | null;
  onSection: (s: Section) => void;
  onDevice: (id: string) => void;
  onChat: (conversation: string) => void;
  onAdd: () => void;
  /** "full": brand, navigation and list; "list": the list only (next to a rail). */
  variant?: "full" | "list";
  /** Icons-only pane (Fluent navigation view). */
  compact?: boolean;
  onToggleCompact?: () => void;
}

export function Sidebar({
  me,
  peers,
  last,
  unread,
  offers,
  requests,
  section,
  selected,
  onSection,
  onDevice,
  onChat,
  onAdd,
  variant = "full",
  compact = false,
  onToggleCompact,
}: Props) {
  const { t } = useI18n();
  const dupNames = duplicateNames(peers);
  const chatsUnread = Object.values(unread).reduce((a, b) => a + b, 0);
  const showChats = section === "chats";

  const navItem = (id: Section, icon: ReactNode, label: string, badge: number) => (
    <button
      className={`nav-item general ${section === id ? "active" : ""}`}
      onClick={() => onSection(id)}
      title={compact ? label : undefined}
      aria-current={section === id ? "page" : undefined}
    >
      <span className={`nav-icon ${id === "shared" ? "folder" : ""}`}>{icon}</span>
      <span className="nav-text">
        <span className="nav-title">{label}</span>
      </span>
      <Badge n={badge} />
    </button>
  );

  return (
    <aside className={`sidebar ${variant} ${compact ? "compact" : ""}`}>
      {variant === "full" ? (
        <>
          <div className="brand">
            {onToggleCompact && (
              <button className="pane-toggle" onClick={onToggleCompact} title={t("menuToggle")} aria-label={t("menuToggle")}>
                <IconMenu size={18} />
              </button>
            )}
            <Logo size={26} />
            <span className="brand-name">Ropeway</span>
          </div>
          <nav className="side-nav">
            {navItem("devices", <IconDevices />, t("devices"), Object.values(offers).reduce((a, b) => a + b, 0))}
            {navItem("chats", <IconChat />, t("chats"), chatsUnread)}
            {navItem("shared", <IconFolder />, t("sharedFolder"), requests)}
          </nav>
        </>
      ) : (
        <div className="list-head">{showChats ? t("chats") : t("devices")}</div>
      )}

      {showChats ? (
        <ChatList me={me} peers={peers} last={last} unread={unread} selected={selected} compact={compact} onChat={onChat} />
      ) : (
        <>
          <div className="section-head">
            <span>{t("devices")}</span>
            <span className="section-count">{t("onlineCount", { n: peers.filter((p) => p.online).length })}</span>
          </div>
          <div className="peer-list">
            {peers.length === 0 && (
              <div className="searching">
                <IconRadar className="pulse" size={22} />
                <p className="searching-title">{t("searching")}</p>
                <p className="searching-hint">
                  {t(IS_MOBILE ? "searchingHintMobile" : IS_LINUX ? "searchingHintLinux" : IS_MAC ? "searchingHintMac" : "searchingHint")}
                </p>
              </div>
            )}
            {[...peers]
              .sort((a, b) => Number(b.favorite) - Number(a.favorite))
              .map((p) => {
                const dup = isDuplicate(dupNames, p.name);
                return (
                  <button
                    key={p.id}
                    className={`nav-item peer ${selected === p.id ? "active" : ""} ${p.online ? "" : "is-offline"}`}
                    onClick={() => onDevice(p.id)}
                    title={compact ? p.name : undefined}
                  >
                    <Avatar name={p.name} id={p.id} online={p.online} />
                    <span className="nav-text">
                      <span className="nav-title">
                        <span className="truncate">{p.name}</span>
                        {p.favorite && <IconStar size={13} className="favorite-mark" />}
                        {p.trusted && <IconShieldCheck size={14} className="trusted-mark" />}
                        {dup && (
                          <span className="same-name" title={t("sameNameHint")}>
                            <IconAlert size={13} />
                          </span>
                        )}
                      </span>
                      <span className="nav-sub">
                        {dup ? <span className="mono">{p.code}</span> : p.online ? (p.address ?? t("online")) : t("offline")}
                      </span>
                    </span>
                    <Badge n={offers[p.id] ?? 0} />
                  </button>
                );
              })}
          </div>
        </>
      )}

      {variant === "full" && (
        <div className="sidebar-foot">
          <button className="foot-btn add" onClick={onAdd} title={compact ? t("addDevice") : undefined}>
            <IconPlus size={16} />
            <span className="foot-label">{t("addDevice")}</span>
          </button>
          <button
            className={`foot-btn foot-settings ${section === "settings" ? "active" : ""}`}
            onClick={() => onSection("settings")}
            title={t("settings")}
            aria-label={t("settings")}
          >
            <IconSettings size={17} />
            <span className="foot-label">{t("settings")}</span>
          </button>
        </div>
      )}
    </aside>
  );
}

/** The general chat, then conversations with devices, newest first. */
function ChatList({
  me,
  peers,
  last,
  unread,
  selected,
  compact,
  onChat,
}: {
  me: MeView | null;
  peers: PeerView[];
  last: Record<string, ChatMessage>;
  unread: Record<string, number>;
  selected: string | null;
  compact: boolean;
  onChat: (conversation: string) => void;
}) {
  const { t } = useI18n();
  const general = last[GENERAL];
  const withChats = peers
    .filter((p) => last[p.id] || unread[p.id])
    .sort((a, b) => (last[b.id]?.ts ?? 0) - (last[a.id]?.ts ?? 0));
  return (
    <div className="peer-list chat-list">
      <button
        className={`nav-item general ${selected === GENERAL ? "active" : ""}`}
        onClick={() => onChat(GENERAL)}
        title={compact ? t("generalChat") : undefined}
      >
        <span className="nav-icon">
          <IconHash />
        </span>
        <span className="nav-text">
          <span className="nav-title">{t("generalChat")}</span>
          <span className="nav-sub">{general ? preview(general, me, t("you")) : t("generalChatHint")}</span>
        </span>
        <Badge n={unread[GENERAL]} />
      </button>
      {withChats.map((p) => (
        <button
          key={p.id}
          className={`nav-item peer ${selected === p.id ? "active" : ""} ${p.online ? "" : "is-offline"}`}
          onClick={() => onChat(p.id)}
          title={compact ? p.name : undefined}
        >
          <Avatar name={p.name} id={p.id} online={p.online} />
          <span className="nav-text">
            <span className="nav-title">
              <span className="truncate">{p.name}</span>
            </span>
            <span className="nav-sub">{last[p.id] ? preview(last[p.id], me, t("you")) : ""}</span>
          </span>
          <Badge n={unread[p.id]} />
        </button>
      ))}
      {withChats.length === 0 && <p className="chat-list-hint">{t("chatsHint")}</p>}
    </div>
  );
}

function Badge({ n }: { n?: number }) {
  if (!n) return null;
  return <span className="badge">{n > 99 ? "99+" : n}</span>;
}

function preview(msg: ChatMessage, me: MeView | null, youLabel: string) {
  const text = msg.text.replace(/\s+/g, " ");
  if (msg.conversation !== GENERAL) return msg.outgoing ? `${youLabel}: ${text}` : text;
  const who = msg.outgoing || msg.from_id === me?.id ? youLabel : msg.from_name;
  return `${who}: ${text}`;
}
