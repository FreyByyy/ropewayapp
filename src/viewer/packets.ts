// Screen packets as sent by the core (see crates/ropeway-core/src/screen.rs):
// kind u8 | flags u8 | timestamp µs u64 BE | length u32 BE | payload

export const PKT_INFO = 0;
export const PKT_VIDEO = 1;
export const PKT_AUDIO = 2;
const FLAG_KEYFRAME = 1;
const HEADER = 14;

export interface Packet {
  kind: number;
  keyframe: boolean;
  ts: number;
  payload: Uint8Array;
}

export function parsePacket(bytes: Uint8Array): Packet | null {
  if (bytes.length < HEADER) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const len = view.getUint32(10);
  if (HEADER + len > bytes.length) return null;
  return {
    kind: bytes[0],
    keyframe: (bytes[1] & FLAG_KEYFRAME) !== 0,
    ts: Number(view.getBigUint64(2)),
    payload: bytes.subarray(HEADER, HEADER + len),
  };
}

/**
 * WebCodecs codec string ("avc1.PPCCLL") from the SPS at the start of an
 * Annex B keyframe.
 */
export function codecFromKeyframe(data: Uint8Array): string | null {
  for (let i = 0; i + 4 < data.length; i++) {
    const startCode3 = data[i] === 0 && data[i + 1] === 0 && data[i + 2] === 1;
    if (!startCode3) continue;
    const nal = i + 3;
    if ((data[nal] & 0x1f) === 7 && nal + 3 < data.length) {
      const hex = (b: number) => b.toString(16).padStart(2, "0");
      return `avc1.${hex(data[nal + 1])}${hex(data[nal + 2])}${hex(data[nal + 3])}`;
    }
  }
  return null;
}
