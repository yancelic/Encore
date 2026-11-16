import { expect, test, type Page } from '@playwright/test';
import { mockDesktop } from './fixtures';

// Controlled media clocks reproduce WebKit's per-element gesture gate and slow
// audio startup. These tests exercise transport behavior, not codec support.
async function editorWithMedia(page: Page, rejectFirstAudio = false) {
  await page.addInitScript(({ rejectFirstAudio }) => {
    type MediaState = { paused: boolean; time: number; since: number; pending: boolean;
      allowed: boolean; seeks: number; plays: number; reject?: (error: DOMException) => void };
    const states = new WeakMap<HTMLMediaElement, MediaState>();
    const state = (media: HTMLMediaElement) => {
      if (!states.has(media)) states.set(media, { paused: true, time: 0, since: performance.now(),
        pending: false, allowed: false, seeks: 0, plays: 0 });
      return states.get(media)!;
    };
    let gesture = false;
    let rejectAudio = rejectFirstAudio;
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
          s.seeks++;
          if (s.pending) {
            s.reject?.(new DOMException('Seek interrupted playback', 'AbortError'));
            s.pending = false;
            s.paused = true;
          }
        },
      },
    });
    proto.play = function () {
      const s = state(this);
      s.plays++;
      if (this instanceof HTMLAudioElement && ((!gesture && !s.allowed) || rejectAudio)) {
        rejectAudio = false;
        return Promise.reject(new DOMException('Audio requires a user gesture', 'NotAllowedError'));
      }
      s.allowed = true;
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
        }, this instanceof HTMLAudioElement ? 220 : 0);
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
    (window as any).__mediaState = (media: HTMLMediaElement) => ({ ...state(media), time: media.currentTime });
  }, { rejectFirstAudio });
  const errors = await mockDesktop(page, { platform: 'macos', nativeRecording: true,
    audioTracks: ['/fixtures/game.m4a', '/fixtures/chat.m4a', '/fixtures/mic.m4a'] });
  await page.getByRole('button', { name: 'Open Overtime winner', exact: true }).click();
  await expect(page.locator('.editor-shell audio')).toHaveCount(3);
  await expect(page.locator('.play-btn')).toBeEnabled();
  return errors;
}

async function audioState(page: Page) {
  return page.locator('.editor-shell audio').evaluateAll((elements) => elements.map((element) => ({
    ...(window as any).__mediaState(element), muted: (element as HTMLAudioElement).muted,
    volume: (element as HTMLAudioElement).volume,
  })));
}

test('starts all audio in the Play gesture without seeking through slow startup', async ({ page }) => {
  const errors = await editorWithMedia(page);
  await page.locator('.play-btn').click();
  await expect.poll(async () => (await audioState(page)).every((s) => !s.pending && !s.paused)).toBe(true);
  const tracks = await audioState(page);
  expect(tracks.every((s) => s.plays === 1 && s.seeks === 0 && !s.muted && s.volume > 0)).toBe(true);
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.getByRole('slider', { name: 'Chat audio volume', exact: true }).fill('0.5');
  expect((await audioState(page))[1].volume).toBeCloseTo(0.425);
  expect((await audioState(page)).every((s) => s.seeks === 0)).toBe(true);
  expect(errors).toEqual([]);
});

test('shows rejected audio playback and lets Play retry the complete clip', async ({ page }) => {
  await editorWithMedia(page, true);
  await page.locator('.play-btn').click();
  await expect(page.getByRole('alert')).toContainText('Press Play');
  await expect.poll(() => page.locator('.editor-shell video').evaluate((v: HTMLVideoElement) => v.paused)).toBe(true);
  await expect.poll(async () => (await audioState(page)).every((s) => s.paused)).toBe(true);
  await page.locator('.play-btn').click();
  await expect.poll(async () => (await audioState(page)).every((s) => !s.pending && !s.paused)).toBe(true);
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('holds audio while video buffers and resumes previously started tracks', async ({ page }) => {
  await editorWithMedia(page);
  await page.keyboard.press('Space');
  await expect.poll(async () => (await audioState(page)).every((s) => !s.pending && !s.paused)).toBe(true);
  await page.locator('.editor-shell video').dispatchEvent('waiting');
  await expect.poll(async () => (await audioState(page)).every((s) => s.paused)).toBe(true);
  await page.locator('.editor-shell video').dispatchEvent('playing');
  await expect.poll(async () => (await audioState(page)).every((s) => !s.pending && !s.paused)).toBe(true);
  await page.keyboard.press('Space');
  await expect.poll(async () => (await audioState(page)).every((s) => s.paused)).toBe(true);
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('seeks paused tracks with the playhead and corrects small drift without more seeks', async ({ page }) => {
  await editorWithMedia(page);
  const lane = await page.locator('.video-lane .track-lane').boundingBox();
  await page.mouse.click(lane!.x + lane!.width / 2, lane!.y + lane!.height / 2);
  expect((await audioState(page)).every((s) => s.time > 10 && s.seeks > 0 && s.paused)).toBe(true);
  const seeks = (await audioState(page)).map((s) => s.seeks);
  await page.locator('.play-btn').click();
  await expect.poll(async () => (await audioState(page)).every((s) => !s.pending && !s.paused)).toBe(true);
  expect((await audioState(page)).map((s) => s.seeks)).toEqual(seeks);
});
