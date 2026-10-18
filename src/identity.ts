import type { PeerView } from "./api";

/** Same as the core's `short_code`: the first 16 hex digits in groups of 4. */
export function shortCode(id: string): string {
  return (id.toUpperCase().slice(0, 16).match(/.{1,4}/g) ?? []).join("-");
}

const nameKey = (name: string) => name.trim().toLowerCase();

/** Names worn by more than one device: anyone can pick any name. */
export function duplicateNames(peers: PeerView[]): Set<string> {
  const seen = new Set<string>();
  const dup = new Set<string>();
  for (const p of peers) {
    const k = nameKey(p.name);
    if (seen.has(k)) dup.add(k);
    seen.add(k);
  }
  return dup;
}

export const isDuplicate = (dup: Set<string>, name: string) => dup.has(nameKey(name));
