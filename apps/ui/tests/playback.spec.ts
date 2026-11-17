import { expect, test, type Page } from '@playwright/test';
import { mockDesktop } from './fixtures';

// A controlled video clock and a fake Web Audio graph reproduce WebKit's
// gesture gate (an AudioContext stays suspended unless resumed inside a click
// or key handler) and slow video startup. These tests exercise transport
// behavior, not codec support.
async function editorWithMedia(page: Page, options: { rejectFirstPlay?: boolean; failDecode?: boolean } = {}) {
  await page.addInitScript(({ rejectFirstPlay, failDecode }) => {
    type MediaState = { paused: boolean; time: number; since: number; pending: boolean;
      reject?: (error: DOMException) => void };
    const states = new WeakMap<HTMLMediaElement, MediaState>();
    const state = (media: HTMLMediaElement) => {
      if (!states.has(media)) states.set(media, { paused: true, time: 0, since: performance.now(), pending: false });
      return states.get(media)!;
    };
    let gesture = false;
    let rejectPlay = rejectFirstPlay;
    for (const event of ['click', 'keydown']) document.addEventListener(event, () => {
      gesture = true;
      setTimeout(() => { gesture = false; }, 0);
    }, true);
    // Network media decoding is replaced by the controlled clock below.
    document.addEventListener('error', (event) => {
      if (event.target instanceof HTMLMediaElement) event.stopImmediatePropagation();
    }, true);
    const proto = HTMLMediaElement.prototype;
    Object.defineProperties(proto, {
      paused: { configurable: true, get() { return state(this).paused; } },
      ended: { configurable: true, get() { return false; } },
      seeking: { configurable: true, get() { return false; } },
      readyState: { configurable: true, get() { return 4; } },
      duration: { configurable: true, get() { return 30; } },
      currentTime: { configurable: true,
        get() {
          const s = state(this);
          return s.time + (!s.paused && !s.pending ? (performance.now() - s.since) / 1000 : 0);
        },
        set(value: number) {
          const s = state(this);
          s.time = value;
          s.since = performance.now();
          this.dispatchEvent(new Event('seeking'));
          setTimeout(() => this.dispatchEvent(new Event('seeked')), 0);
        },
      },
    });
    proto.play = function () {
      const s = state(this);
      if (rejectPlay) {
        rejectPlay = false;
        return Promise.reject(new DOMException('Playback was not allowed', 'NotAllowedError'));
      }
      s.paused = false;
      s.pending = true;
      setTimeout(() => this.dispatchEvent(new Event('play')), 0);
      return new Promise<void>((resolve, reject) => {
        s.reject = reject;
        setTimeout(() => {
          if (s.paused) return;
          s.pending = false;
          s.since = performance.now();
          this.dispatchEvent(new Event('playing'));
          resolve();
        }, 120);
      });
    };
    proto.pause = function () {
      const s = state(this);
      if (s.paused) return;
      s.time = this.currentTime;
      s.paused = true;
      s.pending = false;
      s.reject?.(new DOMException('Paused', 'AbortError'));
      setTimeout(() => this.dispatchEvent(new Event('pause')), 0);
    };

    // Fake Web Audio: records every buffer source start/stop and gain target.
    const log = { starts: [] as { track: number; offset: number; videoTime: number }[], stops: 0, gains: [] as number[],
      resumedInGesture: false, state: 'suspended', closed: false, decodes: 0 };
    (window as any).__audio = log;
    class FakeParam {
      constructor(readonly track: number) {}
      cancelScheduledValues() {}
      setTargetAtTime(value: number) { if (this.track >= 0) log.gains[this.track] = value; }
    }
    let gainCount = 0;
    class FakeContext {
      readonly destination = {};
      private readonly t0 = performance.now();
      get currentTime() { return (performance.now() - this.t0) / 1000; }
      get state() { return log.state; }
      createGain() {
        // The first gain node is the master bus; the rest are per-track.
        const track = gainCount++ - 1;
        return { gain: new FakeParam(track), connect() {} };
      }
      createBufferSource() {
        const node: any = { buffer: null, connect(gain: any) { node.track = gain.gain.track; },
          start(_at: number, offset: number) {
            log.starts.push({ track: node.track, offset, videoTime: (window as any).__videoTime() });
          },
          stop() { log.stops++; }, disconnect() {} };
        return node;
      }
      async decodeAudioData() {
        log.decodes++;
        if (failDecode) throw new DOMException('Unsupported audio', 'EncodingError');
        return { duration: 30 };
      }
      resume() {
        if (gesture) { log.state = 'running'; log.resumedInGesture = true; }
        return Promise.resolve();
      }
      close() { log.closed = true; return Promise.resolve(); }
    }
    (window as any).AudioContext = FakeContext;
    (window as any).__videoTime = () => document.querySelector<HTMLVideoElement>('.editor-shell video')!.currentTime;
  }, { rejectFirstPlay: !!options.rejectFirstPlay, failDecode: !!options.failDecode });
  const errors = await mockDesktop(page, { platform: 'macos', nativeRecording: true,
    audioTracks: ['/fixtures/game.m4a', '/fixtures/chat.m4a', '/fixtures/mic.m4a'] });
  await page.getByRole('button', { name: 'Open Overtime winner', exact: true }).click();
  await expect(page.locator('.play-btn')).toBeEnabled();
  return errors;
}

type AudioLog = { starts: { track: number; offset: number; videoTime: number }[]; stops: number; gains: number[];
  resumedInGesture: boolean; state: string; closed: boolean; decodes: number };
const audioLog = (page: Page): Promise<AudioLog> => page.evaluate(() => (window as any).__audio);
const videoMuted = (page: Page) => page.locator('.editor-shell video').evaluate((v: HTMLVideoElement) => v.muted);

test('decodes every track once and plays them from one Web Audio clock', async ({ page }) => {
  const errors = await editorWithMedia(page);
  expect((await audioLog(page)).decodes).toBe(3);
  await expect(page.locator('.editor-shell audio')).toHaveCount(0);
  await page.locator('.play-btn').click();
  await expect.poll(async () => (await audioLog(page)).starts.length).toBe(3);
  const log = await audioLog(page);
  expect(log.resumedInGesture).toBe(true);
  expect(log.starts.map((s) => s.track).sort()).toEqual([0, 1, 2]);
  expect(await videoMuted(page)).toBe(true);
  await page.getByRole('slider', { name: 'Chat audio volume', exact: true }).fill('0.5');
  await expect.poll(async () => (await audioLog(page)).gains[1]).toBeCloseTo(0.425);
  await page.getByRole('button', { name: 'Mute Game audio', exact: true }).click();
  await expect.poll(async () => (await audioLog(page)).gains[0]).toBe(0);
  // Gain changes must not restart the tracks.
  expect((await audioLog(page)).starts.length).toBe(3);
  await expect(page.getByRole('alert')).toHaveCount(0);
  expect(errors).toEqual([]);
});

test('shows rejected playback and lets Play retry the complete clip', async ({ page }) => {
  await editorWithMedia(page, { rejectFirstPlay: true });
  await page.locator('.play-btn').click();
  await expect(page.getByRole('alert')).toContainText('Press Play');
  expect((await audioLog(page)).starts).toEqual([]);
  await page.locator('.play-btn').click();
  await expect.poll(async () => (await audioLog(page)).starts.length).toBe(3);
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('holds audio while video buffers and resumes at the video position', async ({ page }) => {
  await editorWithMedia(page);
  await page.keyboard.press('Space');
  await expect.poll(async () => (await audioLog(page)).starts.length).toBe(3);
  await page.locator('.editor-shell video').dispatchEvent('waiting');
  await expect.poll(async () => (await audioLog(page)).stops).toBe(3);
  await page.waitForTimeout(150);
  await page.locator('.editor-shell video').dispatchEvent('playing');
  await expect.poll(async () => (await audioLog(page)).starts.length).toBe(6);
  // Compare with the video position captured when each track was started.
  const resumed = (await audioLog(page)).starts.slice(3);
  expect(resumed.every((s) => s.offset > 0.1 && Math.abs(s.offset - s.videoTime) < 0.02)).toBe(true);
  await page.keyboard.press('Space');
  await expect.poll(async () => (await audioLog(page)).stops).toBe(6);
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('seeking while paused stays silent and Play starts from the playhead', async ({ page }) => {
  await editorWithMedia(page);
  const lane = await page.locator('.video-lane .track-lane').boundingBox();
  await page.mouse.click(lane!.x + lane!.width / 2, lane!.y + lane!.height / 2);
  await page.waitForTimeout(50);
  expect((await audioLog(page)).starts).toEqual([]);
  await page.locator('.play-btn').click();
  await expect.poll(async () => (await audioLog(page)).starts.length).toBe(3);
  expect((await audioLog(page)).starts.every((s) => s.offset > 10)).toBe(true);
});

test('falls back to the video audio when tracks cannot be decoded', async ({ page }) => {
  await editorWithMedia(page, { failDecode: true });
  await expect(page.getByRole('alert')).toContainText('first track only');
  await page.locator('.play-btn').click();
  await expect.poll(() => page.locator('.editor-shell video').evaluate((v: HTMLVideoElement) => v.paused)).toBe(false);
  expect(await videoMuted(page)).toBe(false);
  expect((await audioLog(page)).starts).toEqual([]);
});
