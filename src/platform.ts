// Which platform the page runs on, from the WebView's user agent.

export const IS_ANDROID = /Android/i.test(navigator.userAgent);
/** One window, touch, no tray: the phone layout and flows. */
export const IS_MOBILE = IS_ANDROID;
/** Desktop Linux (WebKitGTK). */
export const IS_LINUX = !IS_ANDROID && /Linux/i.test(navigator.userAgent);
/** macOS (WKWebView). */
export const IS_MAC = /Macintosh|Mac OS X/i.test(navigator.userAgent) && !/iPhone|iPad/i.test(navigator.userAgent);
/** Not Windows: system wording instead of Windows wording, no Mica. */
export const NOT_WINDOWS = IS_MOBILE || IS_LINUX || IS_MAC;
