import { flushSync } from "react-dom";

type ViewTransitionDocument = Document & {
  startViewTransition?: (update: () => void) => { ready: Promise<void> };
};

/**
 * Applies a change of look with the new look spreading as a circle from the
 * pointer, so a switch between light and dark does not flash all at once.
 */
export function revealChange(from: { clientX: number; clientY: number } | null, change: () => void) {
  const doc = document as ViewTransitionDocument;
  const motion = document.documentElement.dataset.motion;
  if (!doc.startViewTransition || motion === "off") {
    change();
    return;
  }
  const x = from?.clientX ?? window.innerWidth / 2;
  const y = from?.clientY ?? window.innerHeight / 2;
  const radius = Math.hypot(Math.max(x, window.innerWidth - x), Math.max(y, window.innerHeight - y));
  const transition = doc.startViewTransition(() => flushSync(change));
  transition.ready
    .then(() => {
      document.documentElement.animate(
        { clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${radius}px at ${x}px ${y}px)`] },
        {
          duration: motion === "reduced" ? 320 : 650,
          easing: "cubic-bezier(0.4, 0, 0.2, 1)",
          pseudoElement: "::view-transition-new(root)",
        },
      );
    })
    .catch(() => {});
}
