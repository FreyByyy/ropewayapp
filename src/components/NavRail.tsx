import type { ReactNode } from "react";
import { useI18n } from "../i18n";
import { IconChat, IconFolder, IconPlus, IconSettings } from "../icons";

export type RailSection = "chats" | "shared" | "settings";

/** Material navigation rail: destinations with a tonal pill on the active one. */
export function NavRail({
  section,
  unread,
  requests,
  onSelect,
  onAddIp,
}: {
  section: RailSection;
  unread: number;
  requests: number;
  onSelect: (s: RailSection) => void;
  onAddIp: () => void;
}) {
  const { t } = useI18n();
  const items: { id: RailSection; label: string; icon: ReactNode; badge: number }[] = [
    { id: "chats", label: t("chats"), icon: <IconChat size={22} />, badge: unread },
    { id: "shared", label: t("sharedFolder"), icon: <IconFolder size={22} />, badge: requests },
    { id: "settings", label: t("settings"), icon: <IconSettings size={22} />, badge: 0 },
  ];
  return (
    <nav className="rail" aria-label="Ropeway">
      <button className="rail-fab" onClick={onAddIp} title={t("addByIp")} aria-label={t("addByIp")}>
        <IconPlus size={22} />
      </button>
      <div className="rail-items">
        {items.map((it) => (
          <button
            key={it.id}
            className={`rail-item ${section === it.id ? "active" : ""}`}
            onClick={() => onSelect(it.id)}
            aria-current={section === it.id ? "page" : undefined}
          >
            <span className="rail-pill">
              {it.icon}
              {it.badge > 0 && <span className="rail-badge">{it.badge > 99 ? "99+" : it.badge}</span>}
            </span>
            <span className="rail-label">{it.label}</span>
          </button>
        ))}
      </div>
    </nav>
  );
}
