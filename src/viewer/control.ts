import { api, type InputEvent } from "../api";

const send = (event: InputEvent) => {
  api.sendInput(event).catch(() => {});
};

/** Position of the picture inside the canvas element (object-fit: contain). */
function pictureRect(canvas: HTMLCanvasElement) {
  const box = canvas.getBoundingClientRect();
  const vw = canvas.width || 1;
  const vh = canvas.height || 1;
  const scale = Math.min(box.width / vw, box.height / vh);
  const w = vw * scale;
  const h = vh * scale;
  return { left: box.left + (box.width - w) / 2, top: box.top + (box.height - h) / 2, width: w, height: h };
}

function wheelNotches(delta: number, mode: number) {
  if (mode === 1) return delta / 3; // lines
  if (mode === 2) return delta; // pages
  return delta / 100; // pixels
}

/**
 * Sends mouse, wheel and keyboard input from the viewer to the controlled
 * computer. Returns a function that detaches everything.
 */
export function attachControl(canvas: HTMLCanvasElement, isEnabled: () => boolean, textKeys = false) {
  let pending: { x: number; y: number } | null = null;
  let timer = 0;

  const position = (e: MouseEvent) => {
    const r = pictureRect(canvas);
    return {
      x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
      y: Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
    };
  };

  // Mouse moves are coalesced to at most one per 16 ms. A timer, not
  // requestAnimationFrame: the webview pauses animation frames while the
  // window is occluded, which would freeze the remote pointer.
  const onMove = (e: MouseEvent) => {
    if (!isEnabled()) return;
    pending = position(e);
    if (!timer) {
      timer = window.setTimeout(() => {
        timer = 0;
        if (pending) send({ t: "mouse_move", ...pending });
        pending = null;
      }, 16);
    }
  };
  const onDown = (e: MouseEvent) => {
    if (!isEnabled()) return;
    e.preventDefault();
    canvas.focus();
    send({ t: "mouse_move", ...position(e) });
    send({ t: "mouse_button", button: e.button, down: true });
  };
  const onUp = (e: MouseEvent) => {
    if (!isEnabled()) return;
    e.preventDefault();
    send({ t: "mouse_button", button: e.button, down: false });
  };
  const onWheel = (e: WheelEvent) => {
    if (!isEnabled()) return;
    e.preventDefault();
    send({ t: "wheel", dx: wheelNotches(e.deltaX, e.deltaMode), dy: wheelNotches(e.deltaY, e.deltaMode) });
  };
  const onContext = (e: Event) => {
    if (isEnabled()) e.preventDefault();
  };
  const onKey = (e: KeyboardEvent) => {
    if (!isEnabled() || !e.code) return;
    // Fields of the viewer's own panel (and the phone keyboard) keep their keys.
    if ((e.target as HTMLElement | null)?.tagName === "INPUT") return;
    // Everything goes to the remote computer, including shortcuts that would
    // otherwise act on this window (Ctrl+R, Ctrl+W, F5 …).
    e.preventDefault();
    e.stopPropagation();
    // Phones: characters are typed as text (any keyboard layout works);
    // Backspace, Enter and other named keys still go as keys.
    if (textKeys && e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
      if (e.type === "keydown") send({ t: "text", text: e.key });
      return;
    }
    send({ t: "key", code: e.code, down: e.type === "keydown" });
  };
  const onBlur = () => send({ t: "release_all" });

  // Touch: tap clicks, a long press right-clicks, dragging moves with the
  // button held, two fingers scroll.
  const LONG_PRESS_MS = 550;
  const DRAG_START_PX = 10;
  let touch: { x: number; y: number; dragging: boolean; long: boolean; timer: number } | null = null;
  let scrollY: number | null = null;
  const touchPos = (t: Touch) => position(t as unknown as MouseEvent);
  const onTouchStart = (e: TouchEvent) => {
    if (!isEnabled()) return;
    e.preventDefault();
    if (e.touches.length >= 2) {
      if (touch) clearTimeout(touch.timer);
      if (touch?.dragging) send({ t: "mouse_button", button: 0, down: false });
      touch = null;
      scrollY = (e.touches[0].clientY + e.touches[1].clientY) / 2;
      return;
    }
    const t = e.touches[0];
    const start = { x: t.clientX, y: t.clientY, dragging: false, long: false, timer: 0 };
    start.timer = window.setTimeout(() => {
      if (!touch || touch.dragging) return;
      touch.long = true;
      send({ t: "mouse_move", ...touchPos(t) });
      send({ t: "mouse_button", button: 2, down: true });
      send({ t: "mouse_button", button: 2, down: false });
      navigator.vibrate?.(15);
    }, LONG_PRESS_MS);
    touch = start;
  };
  const onTouchMove = (e: TouchEvent) => {
    if (!isEnabled()) return;
    e.preventDefault();
    if (scrollY !== null && e.touches.length >= 2) {
      const y = (e.touches[0].clientY + e.touches[1].clientY) / 2;
      const notches = (scrollY - y) / 40;
      if (Math.abs(notches) >= 1) {
        send({ t: "wheel", dx: 0, dy: Math.trunc(notches) });
        scrollY = y;
      }
      return;
    }
    if (!touch || touch.long) return;
    const t = e.touches[0];
    if (!touch.dragging && Math.hypot(t.clientX - touch.x, t.clientY - touch.y) > DRAG_START_PX) {
      clearTimeout(touch.timer);
      touch.dragging = true;
      send({ t: "mouse_move", ...position({ clientX: touch.x, clientY: touch.y } as MouseEvent) });
      send({ t: "mouse_button", button: 0, down: true });
    }
    if (touch.dragging) send({ t: "mouse_move", ...touchPos(t) });
  };
  const onTouchEnd = (e: TouchEvent) => {
    if (!isEnabled()) return;
    e.preventDefault();
    if (e.touches.length === 0) scrollY = null;
    if (!touch) return;
    clearTimeout(touch.timer);
    if (touch.dragging) {
      send({ t: "mouse_button", button: 0, down: false });
    } else if (!touch.long) {
      const p = position({ clientX: touch.x, clientY: touch.y } as MouseEvent);
      send({ t: "mouse_move", ...p });
      send({ t: "mouse_button", button: 0, down: true });
      send({ t: "mouse_button", button: 0, down: false });
    }
    touch = null;
  };

  canvas.addEventListener("mousemove", onMove);
  canvas.addEventListener("mousedown", onDown);
  window.addEventListener("mouseup", onUp);
  canvas.addEventListener("wheel", onWheel, { passive: false });
  canvas.addEventListener("contextmenu", onContext);
  window.addEventListener("keydown", onKey, true);
  window.addEventListener("keyup", onKey, true);
  window.addEventListener("blur", onBlur);
  canvas.addEventListener("touchstart", onTouchStart, { passive: false });
  canvas.addEventListener("touchmove", onTouchMove, { passive: false });
  canvas.addEventListener("touchend", onTouchEnd, { passive: false });
  canvas.addEventListener("touchcancel", onTouchEnd, { passive: false });

  return () => {
    canvas.removeEventListener("touchstart", onTouchStart);
    canvas.removeEventListener("touchmove", onTouchMove);
    canvas.removeEventListener("touchend", onTouchEnd);
    canvas.removeEventListener("touchcancel", onTouchEnd);
    clearTimeout(timer);
    canvas.removeEventListener("mousemove", onMove);
    canvas.removeEventListener("mousedown", onDown);
    window.removeEventListener("mouseup", onUp);
    canvas.removeEventListener("wheel", onWheel);
    canvas.removeEventListener("contextmenu", onContext);
    window.removeEventListener("keydown", onKey, true);
    window.removeEventListener("keyup", onKey, true);
    window.removeEventListener("blur", onBlur);
    send({ t: "release_all" });
  };
}

export function pressWinKey() {
  send({ t: "key", code: "MetaLeft", down: true });
  send({ t: "key", code: "MetaLeft", down: false });
}

export function switchMonitor(id: string) {
  send({ t: "switch_monitor", id });
}

/** Text from a phone keyboard, which has no physical key codes. */
export function sendText(text: string) {
  if (text) send({ t: "text", text });
}

/** One key press: down and up. */
export function tapKey(code: string) {
  send({ t: "key", code, down: true });
  send({ t: "key", code, down: false });
}
