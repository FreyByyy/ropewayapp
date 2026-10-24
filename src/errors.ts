import type { Translate } from "./i18n";
import { IS_MAC } from "./platform";

/**
 * Turns a known error from the app's core into advice the user can act on;
 * anything else is shown as it is. Matched on the English text the core
 * produces, which does not change with the UI language.
 */
export function explainError(raw: unknown, t: Translate): string {
  const text = String(raw);
  if (/screen recording is not allowed/i.test(text)) return t(IS_MAC ? "errScreenPermissionMac" : "errScreenPermission");
  if (/needs Windows 10 version 1903/i.test(text)) return t("errWindowCaptureOld");
  if (/screen capture failed to start/i.test(text)) return t("errCaptureStart", { error: text.replace(/^.*?failed to start:?\s*/i, "") });
  if (/screen sharing is not available on this device/i.test(text)) return t("errScreenUnavailable");
  if (/no device on this network shows this code/i.test(text)) return t("errCodeNotFound");
  if (/the code has six digits/i.test(text)) return t("errCodeDigits");
  if (/this is not a Ropeway code/i.test(text)) return t("errNotInvite");
  if (/this is the code of this device/i.test(text)) return t("errOwnInvite");
  if (/did not answer|timed out|device fingerprint mismatch/i.test(text)) return t("errNoAnswer");
  return text;
}
