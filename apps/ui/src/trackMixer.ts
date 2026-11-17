/**
 * Sample-accurate multi-track preview for the clip editor.
 *
 * Each extracted track is decoded once into an AudioBuffer and played through
 * its own GainNode on a single AudioContext clock. This replaces one
 * <audio> element per track: WebKit (macOS) cannot keep several media
 * elements in sync, and correcting them with seeks or playbackRate changes is
 * audible. The editor's <video> stays the transport master; the mixer only
 * follows it via start()/stop().
 */
export class TrackMixer {
  readonly context: AudioContext;
  private readonly master: GainNode;
  private readonly gains: GainNode[] = [];
  private buffers: AudioBuffer[] = [];
  private sources: AudioBufferSourceNode[] = [];
  private startedAt = 0;
  private startOffset = 0;
  private disposed = false;

  constructor() {
    this.context = new AudioContext({ latencyHint: 'playback' });
    this.master = this.context.createGain();
    this.master.connect(this.context.destination);
  }

  /** Fetches and decodes every track. Rejects if any track cannot be decoded. */
  async load(urls: string[], signal?: AbortSignal): Promise<void> {
    const buffers = await Promise.all(urls.map(async (url) => {
      const response = await fetch(url, { signal });
      if (!response.ok) throw new Error(`Audio track request failed (${response.status})`);
      return this.context.decodeAudioData(await response.arrayBuffer());
    }));
    if (this.disposed) return;
    this.buffers = buffers;
    for (let i = this.gains.length; i < buffers.length; i++) {
      const gain = this.context.createGain();
      gain.connect(this.master);
      this.gains.push(gain);
    }
  }

  get trackCount(): number {
    return this.buffers.length;
  }

  get playing(): boolean {
    return this.sources.length > 0;
  }

  /** Current mixer position in clip seconds. */
  get position(): number {
    return this.playing
      ? this.startOffset + (this.context.currentTime - this.startedAt)
      : this.startOffset;
  }

  /**
   * WebKit only lets an AudioContext produce sound after it is resumed inside a
   * user gesture. Call this synchronously from the Play click/key handler.
   */
  unlock(): void {
    if (this.context.state === 'suspended') void this.context.resume().catch(() => {});
  }

  /** Starts (or restarts) every track at `offset` seconds. */
  start(offset: number): void {
    this.stopSources();
    if (this.disposed || this.buffers.length === 0) return;
    this.unlock();
    const at = this.context.currentTime;
    const from = Math.max(0, offset);
    this.buffers.forEach((buffer, i) => {
      if (from >= buffer.duration) return;
      const source = this.context.createBufferSource();
      source.buffer = buffer;
      source.connect(this.gains[i]);
      source.start(at, from);
      this.sources.push(source);
    });
    this.startedAt = at;
    this.startOffset = from;
  }

  /** Stops playback, remembering where it stopped. */
  stop(): void {
    this.startOffset = this.position;
    this.stopSources();
  }

  /** Sets the effective gain of each track; changes are smoothed to avoid clicks. */
  setGains(trackGains: number[]): void {
    const now = this.context.currentTime;
    this.gains.forEach((gain, i) => {
      const target = Math.max(0, trackGains[i] ?? 0);
      gain.gain.cancelScheduledValues(now);
      gain.gain.setTargetAtTime(target, now, 0.015);
    });
  }

  dispose(): void {
    this.disposed = true;
    this.stopSources();
    this.buffers = [];
    void this.context.close().catch(() => {});
  }

  private stopSources(): void {
    for (const source of this.sources) {
      try {
        source.stop();
      } catch {
        // Already stopped.
      }
      source.disconnect();
    }
    this.sources = [];
  }
}
