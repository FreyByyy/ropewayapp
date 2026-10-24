import type { ReactNode } from "react";
import { useI18n } from "../i18n";
import { IconChat, IconDevices, IconFolder, IconPlus, IconSettings } from "../icons";
import type { Section } from "./Sidebar";

export type RailSection = Section;

/** Material navigation rail: destinations with a tonal pill on the active one. */
export function NavRail({
  section,
  unread,
  offers,
  requests,
  onSelect,
  onAdd,
}: {
  section: RailSection;
  unread: number;
  /** Incoming file offers waiting for a decision. */
  offers: number;
  requests: number;
  onSelect: (s: RailSection) => void;
  onAdd: () => void;
}) {
  const { t } = useI18n();
  const items: { id: RailSection; label: string; icon: ReactNode; badge: number }[] = [
    { id: "devices", label: t("devices"), icon: <IconDevices size={22} />, badge: offers },
    { id: "chats", label: t("chats"), icon: <IconChat size={22} />, badge: unread },
    { id: "shared", label: t("sharedFolder"), icon: <IconFolder size={22} />, badge: requests },
    { id: "settings", label: t("settings"), icon: <IconSettings size={22} />, badge: 0 },
  ];
  return (
    <nav className="rail" aria-label="Ropeway">
      <button className="rail-fab" onClick={onAdd} title={t("addDevice")} aria-label={t("addDevice")}>
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
