const RATE = 48000;
/** Delay kept in front of playback to absorb network jitter. */
const TARGET_DELAY = 0.08;
const MAX_DELAY = 0.4;

/** Plays 48 kHz stereo s16le chunks with a small jitter buffer. */
export class AudioPlayer {
  private ctx: AudioContext;
  private gain: GainNode;
  private next = 0;

  constructor() {
    this.ctx = new AudioContext({ sampleRate: RATE, latencyHint: "interactive" });
    this.gain = this.ctx.createGain();
    this.gain.connect(this.ctx.destination);
  }

  /** The webview may refuse to start audio until the user clicks. */
  get blocked() {
    return this.ctx.state === "suspended";
  }

  resume() {
    this.ctx.resume().catch(() => {});
  }

  setVolume(v: number) {
    this.gain.gain.value = v;
  }

  play(pcm: Uint8Array) {
    if (this.ctx.state === "suspended") this.ctx.resume().catch(() => {});
    const frames = Math.floor(pcm.byteLength / 4);
    if (frames === 0) return;
    const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength);
    const buffer = this.ctx.createBuffer(2, frames, RATE);
    const left = buffer.getChannelData(0);
    const right = buffer.getChannelData(1);
    for (let i = 0; i < frames; i++) {
      left[i] = view.getInt16(i * 4, true) / 32768;
      right[i] = view.getInt16(i * 4 + 2, true) / 32768;
    }
    const now = this.ctx.currentTime;
    if (this.next < now + 0.01 || this.next > now + MAX_DELAY) this.next = now + TARGET_DELAY;
    const source = this.ctx.createBufferSource();
    source.buffer = buffer;
    source.connect(this.gain);
    source.start(this.next);
    this.next += frames / RATE;
  }

  close() {
    this.ctx.close().catch(() => {});
  }
}
