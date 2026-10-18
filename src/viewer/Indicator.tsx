import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type CoreEvent, type Language, type ScreenView } from "../api";
import { makeT } from "../i18n";

/**
 * Small always-on-top bar shown while someone controls this computer, so
 * the owner always sees it and can cut the connection in one click.
 */
export function Indicator() {
  const [screen, setScreen] = useState<ScreenView | null>(null);
  const [lang, setLang] = useState<Language>("ru");
  const t = useMemo(() => makeT(lang), [lang]);

  useEffect(() => {
    document.documentElement.classList.add("indicator-root");
    api.settings().then((s) => setLang(s.language)).catch(() => {});
    api.screen().then(setScreen).catch(() => {});
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen<CoreEvent>("ropeway", ({ payload }) => {
      if (payload.kind === "screen") setScreen(payload.data);
    }).then((off) => {
      if (cancelled) off();
      else unlisten = off;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const controlled = screen?.sharing.filter((s) => s.controllers.length > 0) ?? [];
  const names = [...new Set(controlled.flatMap((s) => s.controllers.map((c) => c.name)))].join(", ");

  return (
    <div className="indicator" data-tauri-drag-region>
      <span className="live-dot" data-tauri-drag-region />
      <span className="indicator-text" data-tauri-drag-region>
        {names ? t("controlledBy", { names }) : "Ropeway"}
      </span>
      <button
        className="indicator-stop"
        onClick={() => controlled.forEach((s) => api.stopShare(s.id).catch(console.error))}
      >
        {t("disconnect")}
      </button>
    </div>
  );
}
