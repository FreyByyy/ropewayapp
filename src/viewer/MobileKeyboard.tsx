import { useRef, useState } from "react";
import { useI18n } from "../i18n";
import { sendText, tapKey } from "./control";

/** Kept in the field so that Backspace on an "empty" field still registers. */
const PAD = " ";

/**
 * A phone's on-screen keyboard for the remote computer. Phone keyboards
 * report no key codes and compose words in place (suggestions, autocorrect),
 * so the field's text is compared with what it was and the difference is
 * typed or erased remotely.
 */
export function MobileKeyboard() {
  const { t } = useI18n();
  const field = useRef<HTMLInputElement>(null);
  const prev = useRef(PAD);
  const composing = useRef(false);
  const [open, setOpen] = useState(false);

  const reset = () => {
    prev.current = PAD;
    if (field.current) field.current.value = PAD;
  };

  const onInput = () => {
    const el = field.current;
    if (!el) return;
    const now = el.value;
    const old = prev.current;
    let common = 0;
    while (common < now.length && common < old.length && now[common] === old[common]) common++;
    for (let i = common; i < old.length; i++) tapKey("Backspace");
    sendText(now.slice(common));
    prev.current = now;
    if (!now.startsWith(PAD) || now.length > 64) {
      // Lost the pad, or grew long: start over (the remote side is unchanged).
      requestAnimationFrame(() => {
        if (document.activeElement === el && !composing.current) reset();
      });
    }
  };

  return (
    <>
      <input
        ref={field}
        className="mobile-keyboard-field"
        defaultValue={PAD}
        autoCapitalize="off"
        autoComplete="off"
        spellCheck={false}
        onInput={onInput}
        onCompositionStart={() => (composing.current = true)}
        onCompositionEnd={() => (composing.current = false)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            tapKey("Enter");
          }
        }}
        onBlur={() => setOpen(false)}
        onFocus={() => {
          reset();
          setOpen(true);
        }}
      />
      <button
        className={`viewer-fab keyboard ${open ? "on" : ""}`}
        aria-label={t("keyboard")}
        title={t("keyboard")}
        onPointerDown={(e) => e.preventDefault()}
        onClick={() => (open ? field.current?.blur() : field.current?.focus())}
      >
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
          <rect x="2.5" y="6" width="19" height="12" rx="2" />
          <path d="M6 10h.01M9 10h.01M12 10h.01M15 10h.01M18 10h.01M6 14h.01M18 14h.01M9 14h6" />
        </svg>
      </button>
    </>
  );
}
