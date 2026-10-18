import { createContext, useContext } from "react";
import { useI18n } from "../i18n";

/** On a phone: goes from a chat or page back to the list. `null` elsewhere. */
export const MobileBackContext = createContext<(() => void) | null>(null);

export function BackButton() {
  const back = useContext(MobileBackContext);
  const { t } = useI18n();
  if (!back) return null;
  return (
    <button className="mobile-back" onClick={back} aria-label={t("back")} title={t("back")}>
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M15 5l-7 7 7 7" />
      </svg>
    </button>
  );
}
