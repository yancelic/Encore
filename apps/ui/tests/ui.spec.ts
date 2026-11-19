import { test, expect, type Page } from '@playwright/test';
import { mockDesktop } from './fixtures';

const calls = (page: Page, command: string) =>
  page.evaluate(
    (command) => window.__uiCalls.filter((call) => call.command === command),
    command,
  );

test('library message stays dismissed after reopening the library and reloading', async ({ page }) => {
  await mockDesktop(page);
  const message = page.getByText(/Good games/);
  await expect(message).toBeVisible();
  await page.getByRole('button', { name: 'Dismiss library message' }).click();
  await expect(message).toHaveCount(0);
  await expect(page.getByText('Saved on this device')).toBeVisible();
  await page.getByRole('button', { name: 'Open Overtime winner', exact: true }).click();
  await page.getByRole('button', { name: 'All replays', exact: true }).click();
  await expect(message).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Your replays.' })).toBeVisible();
  await expect(message).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Dismiss library message' })).toHaveCount(0);
});

test('interface text and images cannot be selected or dragged while fields remain editable', async ({ page }) => {
  await mockDesktop(page);
  const heading = page.getByRole('heading', { name: 'Your replays.' });
  const box = (await heading.boundingBox())!;
  await page.mouse.move(box.x + 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width - 2, box.y + box.height / 2, { steps: 12 });
  await page.mouse.up();
  expect(await page.evaluate(() => window.getSelection()?.toString())).toBe('');
  for (const selector of ['.brand-mark-img', '.clip-card img']) {
    expect(await page.locator(selector).first().evaluate(element =>
      element.dispatchEvent(new DragEvent('dragstart', { bubbles: true, cancelable: true })),
    )).toBe(false);
  }
  const search = page.getByRole('searchbox');
  await search.fill('Overtime');
  await search.selectText();
  expect(await search.evaluate(element => {
    const input = element as HTMLInputElement;
    return input.value.slice(input.selectionStart!, input.selectionEnd!);
  })).toBe('Overtime');
  await page.keyboard.type('Off the backboard');
  await expect(search).toHaveValue('Off the backboard');
  await expect(page.locator('.clip-card')).toHaveCount(1);
});

test('library search, game navigation, favorites and list actions remain usable', async ({
  page,
}, testInfo) => {
  const errors = await mockDesktop(page);
  await expect(
    page.getByRole('heading', { name: 'Your replays.' }),
  ).toBeVisible();
  await expect(page.locator('.clip-card')).toHaveCount(6);
  await page.screenshot({ path: testInfo.outputPath('library.png') });
  await page
    .getByRole('button', { name: 'Assetto Corsa', exact: true })
    .click();
  await expect(page.locator('.clip-card')).toHaveCount(3);
  await expect(
    page.getByRole('button', { name: 'Rocket League', exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'All replays', exact: true }).click();
  await page.getByRole('button', { name: 'Favorites', exact: true }).click();
  await expect(page.locator('.clip-card')).toHaveCount(1);
  await page.getByRole('button', { name: 'All replays', exact: true }).click();
  await page.getByRole('searchbox').fill('nothing-matches');
  await expect(
    page.getByRole('heading', { name: 'No matching replays.' }),
  ).toBeVisible();
  await page
    .getByRole('button', { name: 'Clear filters', exact: true })
    .first()
    .click();
  await expect(page.locator('.clip-card')).toHaveCount(6);
  await page.getByRole('button', { name: 'List view', exact: true }).click();
  await expect(page.locator('.clip-row')).toHaveCount(6);
  await page
    .getByRole('button', { name: 'Favorite Off the backboard', exact: true })
    .click();
  await expect.poll(() => calls(page, 'toggle_favorite')).toHaveLength(1);
  await page
    .locator('.clip-row')
    .first()
    .getByRole('button', { name: 'Copy', exact: true })
    .click();
  await expect
    .poll(() => calls(page, 'copy_clip_to_clipboard'))
    .toHaveLength(1);
  expect((await calls(page, 'copy_clip_to_clipboard'))[0].args.id).toBe('1');
  await page
    .getByRole('button', { name: 'Delete Last second save', exact: true })
    .click();
  await expect(
    page.getByRole('button', { name: 'Delete?', exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Cancel deletion' }).click();
  expect(await calls(page, 'delete_clip')).toHaveLength(0);
  await page.screenshot({ path: testInfo.outputPath('library-list.png') });
  expect(errors).toEqual([]);
});

test('montage preserves chosen order and submits the reordered clips', async ({
  page,
}, testInfo) => {
  await mockDesktop(page);
  await page
    .getByRole('button', { name: 'Create montage', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Select Overtime winner', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Assetto Corsa', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Select The perfect line', exact: true })
    .click();
  await page.getByRole('button', { name: 'Continue with 2 clips' }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog
    .getByLabel('Montage title', { exact: true })
    .fill('Overtime highlights');
  await dialog
    .getByRole('button', { name: 'Move down', exact: true })
    .first()
    .click();
  await expect(dialog.locator('.montage-item-row').first()).toContainText(
    'The perfect line',
  );
  await page.screenshot({ path: testInfo.outputPath('montage.png') });
  await dialog
    .getByRole('button', { name: 'Create montage', exact: true })
    .click();
  await expect.poll(() => calls(page, 'build_montage_cmd')).toHaveLength(1);
  expect((await calls(page, 'build_montage_cmd'))[0].args).toEqual({
    ids: ['4', '1'],
    title: 'Overtime highlights',
  });
});

test('editor mixes audio, scrubs, trims and keeps export arguments intact', async ({
  page,
}, testInfo) => {
  const errors = await mockDesktop(page);
  await page
    .getByRole('button', { name: 'Open Overtime winner', exact: true })
    .click();
  await expect(page.locator('.editor-shell')).toBeVisible();
  await page
    .getByRole('button', { name: 'Mute Game audio', exact: true })
    .click();
  await page
    .getByRole('slider', { name: 'Chat audio volume', exact: true })
    .fill('0.5');
  await page.screenshot({ path: testInfo.outputPath('editor.png') });
  const lane = await page.locator('.video-lane .track-lane').boundingBox();
  await page.mouse.click(
    lane!.x + lane!.width * 0.25,
    lane!.y + lane!.height / 2,
  );
  await page.keyboard.press('i');
  await page.getByRole('button', { name: 'Save Trim', exact: true }).click();
  await expect.poll(() => calls(page, 'trim_clip_cmd')).toHaveLength(1);
  const trim = (await calls(page, 'trim_clip_cmd'))[0].args;
  expect(trim.id).toBe('1');
  expect(trim.startSecs).toBeGreaterThan(0);
  expect(trim.endSecs).toBeGreaterThan(trim.startSecs);
  expect(trim.mutedTracks).toEqual([true, false, false]);
  expect(trim.trackVolumes).toEqual([1, 0.5, 1]);
  await page
    .getByRole('button', { name: '9:16 vertical', exact: true })
    .click();
  await expect(page.locator('.vertical-crop-window')).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('editor-vertical.png') });
  await page.getByRole('button', { name: 'Center Crop', exact: true }).click();
  await page
    .getByRole('button', { name: 'Export 9:16 MP4', exact: true })
    .click();
  await expect.poll(() => calls(page, 'export_vertical_cmd')).toHaveLength(1);
  expect((await calls(page, 'export_vertical_cmd'))[0].args.blurBg).toBe(false);
  expect(errors).toEqual([]);
});

test('share uses unsaved trim and audio mix and waits for export', async ({ page }) => {
  const errors = await mockDesktop(page, { shareDelay: 800 });
  await page.getByRole('button', { name: 'Open Overtime winner', exact: true }).click();
  await page.getByRole('button', { name: 'Mute Game audio', exact: true }).click();
  await page.getByRole('slider', { name: 'Chat audio volume', exact: true }).fill('0.5');
  const lane = await page.locator('.video-lane .track-lane').boundingBox();
  await page.mouse.click(lane!.x + lane!.width * 0.25, lane!.y + lane!.height / 2);
  await page.keyboard.press('i');
  await page.mouse.click(lane!.x + lane!.width * 0.75, lane!.y + lane!.height / 2);
  await page.keyboard.press('o');
  await page.getByRole('button', { name: 'Share Clip', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Preparing share...', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Save Trim', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Copied to Clipboard!', exact: true })).toBeVisible();
  const shares = await calls(page, 'copy_clip_to_clipboard');
  expect(shares).toHaveLength(1);
  expect(shares[0].args).toMatchObject({
    id: '1', mutedTracks: [true, false, false], trackVolumes: [1, 0.5, 1],
  });
  expect(shares[0].args.startSecs).toBeCloseTo(7.5, 0);
  expect(shares[0].args.endSecs).toBeCloseTo(22.5, 0);
  expect(await calls(page, 'trim_clip_cmd')).toHaveLength(0);
  expect(errors).toEqual([]);
});

test('failed share displays an error without claiming clipboard success', async ({ page }) => {
  await mockDesktop(page, { shareError: true });
  await page.getByRole('button', { name: 'Open Overtime winner', exact: true }).click();
  await page.getByRole('button', { name: 'Share Clip', exact: true }).click();
  await expect(page.locator('.toast.error')).toContainText('Share export failed');
  await expect(page.getByRole('button', { name: 'Share Clip', exact: true })).toBeEnabled();
  await expect(page.getByRole('button', { name: 'Copied to Clipboard!', exact: true })).toHaveCount(0);
});

test('settings trap focus, record shortcuts, save changes and restore focus', async ({
  page,
}, testInfo) => {
  await mockDesktop(page);
  const opener = page.getByRole('button', { name: 'Settings', exact: true });
  await opener.click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('settings.png') });
  await dialog.getByRole('radio', { name: '120 FPS', exact: true }).check();
  await dialog.getByLabel('Save shortcut', { exact: true }).click();
  await page.keyboard.press('Escape');
  await expect(dialog).toBeVisible();
  await dialog.getByLabel('Save shortcut', { exact: true }).click();
  await page.keyboard.press('Control+Shift+K');
  await expect(
    dialog.getByLabel('Save shortcut', { exact: true }),
  ).toContainText('Ctrl+Shift+K');
  await dialog
    .getByRole('button', { name: 'Save settings', exact: true })
    .focus();
  await page.keyboard.press('Tab');
  expect(
    await page.evaluate(() =>
      Boolean(document.activeElement?.closest('dialog')),
    ),
  ).toBe(true);
  await dialog
    .getByRole('button', { name: 'Save settings', exact: true })
    .click();
  await expect(dialog).not.toBeVisible();
  const saved = (await calls(page, 'save_config'))[0].args.cfg;
  expect(saved.fps).toBe(120);
  expect(saved.hotkey).toBe('Ctrl+Shift+K');
  await expect(opener).toBeFocused();
  await opener.click();
  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
});

test('audio routing keeps device choices and accepts a generic application name', async ({ page }) => {
  await mockDesktop(page);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('tab', { name: 'Audio', exact: true }).click();
  await expect(dialog.getByText('Game and chat use the same playback device.', { exact: false })).toBeVisible();
  await dialog.getByLabel('Game audio', { exact: true }).selectOption('Headphones');
  await dialog.getByRole('radio', { name: 'Applications', exact: true }).check();
  await expect(dialog.getByLabel('Chat application')).toBeVisible();
  await dialog.getByLabel('Chat application').fill('Some Chat App.exe');
  await expect(dialog.getByText('Some Chat App.exe is not running.', { exact: false })).toBeVisible();
  await dialog.getByLabel('Chat application').fill('Chat - voice channel');
  await expect(dialog.getByRole('button', { name: 'Save settings', exact: true })).toBeDisabled();
  await dialog.getByRole('tab', { name: 'Recording', exact: true }).click();
  await dialog.getByRole('button', { name: 'Review audio settings', exact: true }).click();
  await expect(dialog.getByRole('tab', { name: 'Audio', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.getByRole('tab', { name: 'Audio', exact: true })).toBeFocused();
  await dialog.getByLabel('Chat application').fill('TeamSpeak.exe');
  await dialog.getByRole('radio', { name: 'Audio devices', exact: true }).check();
  await expect(dialog.getByLabel('Game audio', { exact: true })).toHaveValue('Headphones');
  await dialog.getByRole('radio', { name: 'Applications', exact: true }).check();
  await expect(dialog.getByLabel('Chat application')).toHaveValue('TeamSpeak.exe');
  await dialog.getByRole('button', { name: 'Save settings', exact: true }).click();
  await expect(dialog).not.toBeVisible();
  const saved = (await calls(page, 'save_config'))[0].args.cfg;
  expect(saved.audio_mode).toBe('Applications');
  expect(saved.chat_application).toBe('TeamSpeak.exe');
  expect(saved.audio_device).toBe('Headphones');
});

test('offline and first-run states still expose recorder controls', async ({
  page,
}, testInfo) => {
  await mockDesktop(page, { empty: true, offline: true, recorderDelay: 700 });
  await expect(
    page.getByRole('heading', { name: 'Your next great play starts here.' }),
  ).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('empty.png') });
  const toggle = page.locator('.recorder-toggle');
  const size = async () => {
    const box = (await toggle.boundingBox())!;
    return { width: box.width, height: box.height };
  };
  const initialSize = await size();
  await page
    .getByRole('button', { name: 'Start Recorder', exact: true })
    .click();
  await expect(toggle).toHaveText('Starting...');
  expect(await size()).toEqual(initialSize);
  await expect.poll(() => calls(page, 'start_recorder')).toHaveLength(1);
  await expect(
    page.getByText('Ready to capture', { exact: true }),
  ).toBeVisible();
  expect(await size()).toEqual(initialSize);
  for (const theme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: theme });
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    const colors = await page.locator('.status-pill.ready').evaluate(element => ({
      label: getComputedStyle(element).color,
      dot: getComputedStyle(element.querySelector('.status-dot')!).backgroundColor,
      offlineDot: 'rgb(157, 167, 174)',
    }));
    expect(colors.dot).not.toBe(colors.offlineDot);
    expect(colors.label).toBe(theme === 'dark' ? 'rgb(127, 216, 166)' : 'rgb(31, 110, 68)');
    await page.screenshot({ path: testInfo.outputPath(`recorder-ready-${theme}.png`) });
  }
  await page
    .getByRole('button', { name: 'Stop background recorder', exact: true })
    .click();
  await expect(toggle).toHaveText('Stopping...');
  expect(await size()).toEqual(initialSize);
  await expect(
    page.getByRole('button', { name: 'Start Recorder', exact: true }),
  ).toBeVisible();
  expect(await size()).toEqual(initialSize);
});

for (const size of [
  { width: 1020, height: 660 },
  { width: 480, height: 800 },
]) {
  test(`layout remains usable at ${size.width}x${size.height}`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize(size);
    await mockDesktop(page);
    await expect(
      page.getByRole('heading', { name: 'Your replays.' }),
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({ path: testInfo.outputPath('library-size.png') });
    await page
      .getByRole('button', { name: 'Open Overtime winner', exact: true })
      .click();
    await expect(
      page.getByRole('button', { name: 'Share Clip', exact: true }),
    ).toBeInViewport();
    await page.screenshot({ path: testInfo.outputPath('editor-size.png') });
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await expect(
      page.getByRole('button', { name: 'Save settings', exact: true }),
    ).toBeInViewport();
    await page.screenshot({ path: testInfo.outputPath('settings-size.png') });
  });
}
