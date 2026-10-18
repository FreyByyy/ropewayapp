import { GENERAL, type ChatMessage, type MeView, type PeerView } from "../api";
import { IS_LINUX, IS_MAC, IS_MOBILE } from "../platform";
import { useI18n } from "../i18n";
import { IconAlert, IconFolder, IconHash, IconMenu, IconPlus, IconRadar, IconSettings, IconShieldCheck, Logo } from "../icons";
import { Avatar } from "./Avatar";
import { duplicateNames, isDuplicate } from "../identity";

interface Props {
  me: MeView | null;
  peers: PeerView[];
  last: Record<string, ChatMessage>;
  unread: Record<string, number>;
  /** Incoming file offers waiting for a decision, per device. */
  offers: Record<string, number>;
  /** Change requests to this device's shared folder waiting for a decision. */
  requests: number;
  selected: string | null;
  settingsOpen: boolean;
  sharedOpen: boolean;
  onSelect: (conversation: string) => void;
  onShared: () => void;
  onSettings: () => void;
  onAddIp: () => void;
  /** "full": brand, navigation and devices; "list": conversations only (next to a rail). */
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
  selected,
  settingsOpen,
  sharedOpen,
  onSelect,
  onShared,
  onSettings,
  onAddIp,
  variant = "full",
  compact = false,
  onToggleCompact,
}: Props) {
  const { t } = useI18n();
  const onlineCount = peers.filter((p) => p.online).length;
  const dupNames = duplicateNames(peers);
  const general = last[GENERAL];

  return (
    <aside className={`sidebar ${variant} ${compact ? "compact" : ""}`}>
      {variant === "full" ? (
        <div className="brand">
          {onToggleCompact && (
            <button className="pane-toggle" onClick={onToggleCompact} title={t("menuToggle")} aria-label={t("menuToggle")}>
              <IconMenu size={18} />
            </button>
          )}
          <Logo size={26} />
          <span className="brand-name">Ropeway</span>
        </div>
      ) : (
        <div className="list-head">{t("chats")}</div>
      )}

      <button
        className={`nav-item general ${selected === GENERAL ? "active" : ""}`}
        onClick={() => onSelect(GENERAL)}
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

      {variant === "full" && (
        <button
          className={`nav-item general ${sharedOpen ? "active" : ""}`}
          onClick={onShared}
          title={compact ? t("sharedFolder") : undefined}
        >
          <span className="nav-icon folder">
            <IconFolder />
          </span>
          <span className="nav-text">
            <span className="nav-title">{t("sharedFolder")}</span>
            <span className="nav-sub">{t("sharedHint")}</span>
          </span>
          <Badge n={requests} />
        </button>
      )}

      <div className="section-head">
        <span>{t("devices")}</span>
        <span className="section-count">{t("onlineCount", { n: onlineCount })}</span>
      </div>

      <div className="peer-list">
        {peers.length === 0 && (
          <div className="searching">
            <IconRadar className="pulse" size={22} />
            <p className="searching-title">{t("searching")}</p>
            <p className="searching-hint">{t(IS_MOBILE ? "searchingHintMobile" : IS_LINUX ? "searchingHintLinux" : IS_MAC ? "searchingHintMac" : "searchingHint")}</p>
          </div>
        )}
        {peers.map((p) => {
          const msg = last[p.id];
          const dup = isDuplicate(dupNames, p.name);
          return (
            <button
              key={p.id}
              className={`nav-item peer ${selected === p.id ? "active" : ""} ${p.online ? "" : "is-offline"}`}
              onClick={() => onSelect(p.id)}
              title={compact ? p.name : undefined}
            >
              <Avatar name={p.name} id={p.id} online={p.online} />
              <span className="nav-text">
                <span className="nav-title">
                  <span className="truncate">{p.name}</span>
                  {p.trusted && <IconShieldCheck size={14} className="trusted-mark" />}
                  {dup && (
                    <span className="same-name" title={t("sameNameHint")}>
                      <IconAlert size={13} />
                    </span>
                  )}
                </span>
                <span className="nav-sub">
                  {dup ? (
                    <span className="mono">{p.code}</span>
                  ) : msg ? (
                    preview(msg, me, t("you"))
                  ) : p.online ? (
                    (p.address ?? t("online"))
                  ) : (
                    t("offline")
                  )}
                </span>
              </span>
              <Badge n={(unread[p.id] ?? 0) + (offers[p.id] ?? 0)} />
            </button>
          );
        })}
      </div>

      {variant === "full" && (
        <div className="sidebar-foot">
          <button className="foot-btn add" onClick={onAddIp} title={compact ? t("addByIp") : undefined}>
            <IconPlus size={16} />
            <span className="foot-label">{t("addByIp")}</span>
          </button>
          <button
            className={`foot-btn foot-settings ${settingsOpen ? "active" : ""}`}
            onClick={onSettings}
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
