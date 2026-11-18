import { test, expect } from '@playwright/test';
import { mockDesktop } from './fixtures';

test('Windows screen mode persists and explains full display and audio capture', async ({ page }) => {
  const errors = await mockDesktop(page);
  const modes = page.getByRole('group', { name: 'Capture Mode' });
  await expect(modes.getByRole('button', { name: 'Window', exact: true })).toHaveCount(0);
  await modes.getByRole('button', { name: 'Screen', exact: true }).click();
  await expect(modes.getByRole('button', { name: 'Screen', exact: true })).toHaveAttribute('aria-pressed', 'true');
  expect(await page.evaluate(() => window.__uiCalls.find(call => call.command === 'save_config')?.args.cfg.mode)).toBe('ScreenCapture');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByLabel('Capture mode', { exact: true })).toHaveValue('ScreenCapture');
  await expect(page.getByText('Records everything visible on your primary display', { exact: false })).toBeVisible();
  await page.getByLabel('Separate audio by', { exact: true }).selectOption('Applications');
  await expect(page.getByText('The Game track records all audio', { exact: false })).toBeVisible();
  await expect(page.getByRole('option', { name: 'System audio (Default Playback Device)', exact: true })).toHaveCount(1);
  expect(errors).toEqual([]);
});

for (const trigger of ['button', 'hotkey'] as const) {
  test(`first ${trigger} clip appears after saving finishes beyond the early refresh`, async ({ page }) => {
    const errors = await mockDesktop(page, { empty: true, captureDelay: 2500 });
    await expect(page.getByRole('button', { name: 'Save Clip', exact: true })).toBeVisible();
    // Establish a status poll with no saved clip before the first completion.
    await expect.poll(() => page.evaluate(() => window.__uiCalls.filter(call => call.command === 'get_recorder_status').length)).toBeGreaterThan(0);
    if (trigger === 'button') await page.getByRole('button', { name: 'Save Clip', exact: true }).click();
    else await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('trigger_save_clip'));
    await expect(page.getByText('Saving clip...', { exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open New captured replay', exact: true })).toBeVisible({ timeout: 8000 });
    await expect(page.getByText('New clip captured!', { exact: true })).toBeVisible();
    expect(errors).toEqual([]);
  });
}
