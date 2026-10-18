import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n, type MessageKey } from "../i18n";
import { ALL_EMOJI, EMOJI_CATEGORIES } from "../emoji/data";

const RECENT_KEY = "ropeway.emoji.recent";
const RECENT_MAX = 24;

const TITLES: Record<string, MessageKey> = {
  recent: "emojiRecent",
  smileys: "emojiSmileys",
  people: "emojiPeople",
  nature: "emojiNature",
  food: "emojiFood",
  activity: "emojiActivity",
  travel: "emojiTravel",
  objects: "emojiObjects",
  symbols: "emojiSymbols",
};

function loadRecent(): string[] {
  try {
    const list = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(list) ? list.filter((e): e is string => typeof e === "string").slice(0, RECENT_MAX) : [];
  } catch {
    return [];
  }
}

function saveRecent(list: string[]) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    // Recent emoji are a convenience; nothing to do without storage.
  }
}

/** Emoji panel above the message box. Stays open for several picks. */
export function EmojiPicker({ onPick, onClose }: { onPick: (emoji: string) => void; onClose: () => void }) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [recent, setRecent] = useState(loadRecent);
  // Recent emoji are fixed while the panel is open, so the grid does not jump under the pointer.
  const [shownRecent] = useState(recent);
  const [active, setActive] = useState(shownRecent.length ? "recent" : EMOJI_CATEGORIES[0].id);
  const root = useRef<HTMLDivElement>(null);
  const grid = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);

  const sections = useMemo(
    () => [
      ...(shownRecent.length ? [{ id: "recent", icon: "🕘", items: shownRecent.map((e) => ({ e, k: "" })) }] : []),
      ...EMOJI_CATEGORIES,
    ],
    [shownRecent],
  );

  const found = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return null;
    return ALL_EMOJI.filter((item) => words.every((w) => item.k.includes(w) || item.e === w));
  }, [query]);

  useEffect(() => {
    search.current?.focus();
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element | null;
      if (root.current?.contains(target) || target?.closest(".emoji-toggle")) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  const pick = (emoji: string) => {
    onPick(emoji);
    const next = [emoji, ...recent.filter((e) => e !== emoji)].slice(0, RECENT_MAX);
    setRecent(next);
    saveRecent(next);
  };

  const jump = (id: string) => {
    setQuery("");
    setActive(id);
    requestAnimationFrame(() => {
      const el = grid.current?.querySelector<HTMLElement>(`[data-section="${id}"]`);
      if (el && grid.current) grid.current.scrollTo({ top: el.offsetTop - grid.current.offsetTop });
    });
  };

  // The tab of the section at the top of the grid is highlighted.
  const onScroll = () => {
    const g = grid.current;
    if (!g || found) return;
    let current = sections[0]?.id;
    for (const el of g.querySelectorAll<HTMLElement>("[data-section]")) {
      if (el.offsetTop - g.offsetTop <= g.scrollTop + 8) current = el.dataset.section!;
    }
    if (current && current !== active) setActive(current);
  };

  const button = (e: string) => (
    <button key={e} className="emoji-cell" onClick={() => pick(e)} title={e}>
      {e}
    </button>
  );

  return (
    <div className="emoji-picker" ref={root} role="dialog" aria-label={t("emoji")}>
      <input
        ref={search}
        className="emoji-search"
        value={query}
        placeholder={t("emojiSearch")}
        onChange={(e) => setQuery(e.target.value)}
      />
      <div className="emoji-tabs" role="tablist">
        {sections.map((s) => (
          <button
            key={s.id}
            role="tab"
            aria-selected={!found && active === s.id}
            className={`emoji-tab ${!found && active === s.id ? "on" : ""}`}
            title={t(TITLES[s.id])}
            onClick={() => jump(s.id)}
          >
            {s.icon}
          </button>
        ))}
      </div>
      <div className="emoji-grid" ref={grid} onScroll={onScroll}>
        {found ? (
          found.length ? (
            <div className="emoji-cells">{found.map((i) => button(i.e))}</div>
          ) : (
            <p className="emoji-none">{t("emojiNone")}</p>
          )
        ) : (
          sections.map((s) => (
            <section key={s.id} data-section={s.id}>
              <h3 className="emoji-head">{t(TITLES[s.id])}</h3>
              <div className="emoji-cells">{s.items.map((i) => button(i.e))}</div>
            </section>
          ))
        )}
      </div>
    </div>
  );
}
