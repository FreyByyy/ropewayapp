import type { QrMatrix } from "../api";

/** Modules of light margin around the code; scanners need some. */
const QUIET = 3;
/** Share of the code's width under the logo. Error correction is at its
 *  highest (30 %), and this covers about 6 % of the modules. */
const LOGO_SHARE = 0.24;
const INK = "#0B1A2E";
const EYE = "#0E6B5E";

/**
 * A QR code in Ropeway's style: round dots, rounded corner markers and the
 * logo in the middle. Always dark on white, whatever the theme, so every
 * camera reads it.
 */
export function BrandQr({ qr, size = 240, label }: { qr: QrMatrix; size?: number; label: string }) {
  const n = qr.width;
  const total = n + QUIET * 2;
  const logo = Math.ceil(n * LOGO_SHARE) | 1;
  const logoStart = (n - logo) / 2;
  const inFinder = (x: number, y: number) => (x < 7 && y < 7) || (x >= n - 7 && y < 7) || (x < 7 && y >= n - 7);
  const inLogo = (x: number, y: number) =>
    x >= logoStart - 0.5 && x < logoStart + logo + 0.5 && y >= logoStart - 0.5 && y < logoStart + logo + 0.5;

  const dots: string[] = [];
  for (let y = 0; y < n; y++) {
    for (let x = 0; x < n; x++) {
      if (qr.cells[y * n + x] !== "1" || inFinder(x, y) || inLogo(x, y)) continue;
      const cx = x + QUIET + 0.5;
      const cy = y + QUIET + 0.5;
      // A circle as two arcs, all in one path.
      dots.push(`M${cx - 0.43} ${cy}a0.43 0.43 0 1 0 0.86 0a0.43 0.43 0 1 0 -0.86 0`);
    }
  }
  const eyes: [number, number][] = [
    [0, 0],
    [n - 7, 0],
    [0, n - 7],
  ];
  const plate = logo + 1;
  const plateAt = QUIET + logoStart - 0.5;

  return (
    <svg
      className="brand-qr"
      width={size}
      height={size}
      viewBox={`0 0 ${total} ${total}`}
      role="img"
      aria-label={label}
      shapeRendering="geometricPrecision"
    >
      <rect width={total} height={total} rx={total * 0.06} fill="#fff" />
      <path d={dots.join("")} fill={INK} />
      {eyes.map(([x, y]) => (
        <g key={`${x}-${y}`} transform={`translate(${x + QUIET} ${y + QUIET})`}>
          <rect x={0.5} y={0.5} width={6} height={6} rx={1.6} fill="none" stroke={INK} strokeWidth={1} />
          <rect x={2} y={2} width={3} height={3} rx={0.8} fill={EYE} />
        </g>
      ))}
      <rect x={plateAt} y={plateAt} width={plate} height={plate} rx={plate * 0.22} fill="#fff" />
      <g transform={`translate(${plateAt + plate * 0.12} ${plateAt + plate * 0.1}) scale(${(plate * 0.76) / 100})`}>
        <path d="M50 22 L22 74 H78 Z" fill="none" stroke={INK} strokeWidth="8" strokeLinejoin="round" />
        <circle cx="50" cy="22" r="13" fill="#6CC8B5" />
        <circle cx="22" cy="74" r="13" fill={INK} />
        <circle cx="78" cy="74" r="13" fill={INK} />
      </g>
    </svg>
  );
}
