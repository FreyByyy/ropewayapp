const HUES = [168, 200, 228, 262, 300, 340, 20, 40, 95, 135];

function hueFor(id: string) {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return HUES[h % HUES.length];
}

function initials(name: string) {
  const parts = name.trim().split(/[\s\-_.]+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[1][0]).toUpperCase();
}

export function Avatar({ name, id, online, size = 36 }: { name: string; id: string; online?: boolean; size?: number }) {
  return (
    <span
      className="avatar"
      style={{ width: size, height: size, fontSize: size * 0.38, ["--hue" as string]: hueFor(id) }}
    >
      {initials(name)}
      {online !== undefined && <span className={`presence ${online ? "on" : "off"}`} />}
    </span>
  );
}
