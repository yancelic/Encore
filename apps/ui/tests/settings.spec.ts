import { expect, test } from '@playwright/test';
import { mockDesktop } from './fixtures';

test('toolbar keeps folder, refresh, current clip properties and Settings in order', async ({ page }) => {
  await mockDesktop(page, { config: { buffer_secs: 90, height: 0, fps: 144, hotkey: 'Ctrl+K' } });
  const toolbar = page.locator('.top-right');
  await expect(toolbar.getByRole('button')).toHaveCount(3);
  await expect(toolbar.locator(':scope > *')).toHaveCount(4);
  await expect(toolbar.locator(':scope > *').nth(0)).toHaveAccessibleName('Open clips folder');
  await expect(toolbar.locator(':scope > *').nth(1)).toHaveAccessibleName('Rescan clips folder');
  await expect(toolbar.locator(':scope > *').nth(2)).toHaveText('90s·Native·144fps');
  await expect(toolbar.locator(':scope > *').nth(3)).toHaveAccessibleName('Settings');
  await expect(toolbar.getByText('Ctrl+K', { exact: true })).toHaveCount(0);
});

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} settings show recording essentials first and theme native menu options`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1020, height: 660 });
    await page.emulateMedia({ colorScheme: theme });
    const errors = await mockDesktop(page);
    const optionColors = async (selector: string) => page.locator(selector).first().evaluate((option) => {
      const style = getComputedStyle(option);
      return { background: style.backgroundColor, color: style.color, scheme: style.colorScheme };
    });
    const expected = theme === 'dark'
      ? { background: 'rgb(32, 36, 43)', color: 'rgb(236, 235, 243)', scheme: 'dark' }
      : { background: 'rgb(245, 244, 249)', color: 'rgb(12, 18, 12)', scheme: 'light' };
    expect(await optionColors('select[aria-label="Sort clips"] option')).toEqual(expected);
    // Sorting remains usable after changing the native menu palette.
    await page.getByLabel('Sort clips').selectOption('Oldest');
    await expect(page.getByLabel('Sort clips')).toHaveValue('Oldest');
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(page.getByRole('tab', { name: 'Recording', exact: true })).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByRole('radiogroup', { name: 'Replay length', exact: true })).toBeInViewport({ ratio: 1 });
    await expect(page.getByLabel('Save shortcut', { exact: true })).toBeInViewport({ ratio: 1 });
    await expect(page.getByRole('radio', { name: '1080p', exact: true })).toBeInViewport({ ratio: 1 });
    await expect(page.getByRole('radio', { name: '60 FPS', exact: true })).toBeInViewport({ ratio: 1 });
    await expect(page.getByRole('dialog').getByRole('combobox')).toHaveCount(0);
    await page.screenshot({ path: testInfo.outputPath(`recording-${theme}.png`) });
    await page.getByRole('tab', { name: 'Audio', exact: true }).click();
    expect(await optionColors('#mic-audio option')).toEqual(expected);
    await expect(page.getByLabel('Microphone', { exact: true })).toBeInViewport({ ratio: 1 });
    await page.screenshot({ path: testInfo.outputPath(`audio-${theme}.png`) });
    expect(errors).toEqual([]);
  });
}

test('tab navigation and radio keys preserve drafts across categories and save them together', async ({ page }) => {
  const errors = await mockDesktop(page, { config: { buffer_secs: 75 } });
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('radio', { name: '75s (custom)', exact: true })).toBeChecked();
  await page.getByRole('radio', { name: 'Native', exact: true }).check();
  await page.getByRole('radio', { name: '60 FPS', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('radio', { name: '120 FPS', exact: true })).toBeChecked();
  const recordingTab = page.getByRole('tab', { name: 'Recording', exact: true });
  await recordingTab.focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab', { name: 'Audio', exact: true })).toBeFocused();
  await page.getByLabel('Microphone', { exact: true }).selectOption('off');
  await page.getByRole('tab', { name: 'Audio', exact: true }).focus();
  await page.keyboard.press('End');
  await expect(page.getByRole('tab', { name: 'General', exact: true })).toBeFocused();
  await page.getByRole('button', { name: 'Start recorder with Windows', exact: true }).click();
  await page.getByRole('tab', { name: 'General', exact: true }).focus();
  await page.keyboard.press('Home');
  await expect(recordingTab).toBeFocused();
  await expect(page.getByRole('radio', { name: '75s (custom)', exact: true })).toBeChecked();
  await expect(page.getByRole('radio', { name: 'Native', exact: true })).toBeChecked();
  await page.getByRole('button', { name: 'Save settings', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  const saved = await page.evaluate(() => window.__uiCalls.find(call => call.command === 'save_config')?.args.cfg);
  expect(saved).toMatchObject({ buffer_secs: 75, height: 0, fps: 120, mic_device: 'off', start_with_windows: true });
  await expect(page.locator('.top-right .meta-chip')).toHaveText('75s·Native·120fps');
  expect(errors).toEqual([]);
});

test('switching settings categories cancels shortcut listening and Cancel discards recording changes', async ({ page }) => {
  await mockDesktop(page);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('radio', { name: '120s', exact: true }).check();
  await page.getByLabel('Save shortcut', { exact: true }).click();
  await page.getByRole('tab', { name: 'Audio', exact: true }).click();
  await page.getByRole('tab', { name: 'Recording', exact: true }).click();
  await expect(page.getByLabel('Save shortcut', { exact: true })).toContainText('Alt+S');
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('radio', { name: '30s', exact: true })).toBeChecked();
  expect(await page.evaluate(() => window.__uiCalls.filter(call => call.command === 'save_config'))).toHaveLength(0);
});
