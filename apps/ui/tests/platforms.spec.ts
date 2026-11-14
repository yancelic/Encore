import { expect, test, type Page } from '@playwright/test';
import { mockDesktop } from './fixtures';

const calls = (page: Page, command: string) => page.evaluate(
  name => window.__uiCalls.filter(call => call.command === name), command,
);

for (const platform of ['macos', 'linux'] as const) {
  test(`${platform} preview edits existing clips without invoking Windows services`, async ({ page }) => {
    const errors = await mockDesktop(page, { platform });
    await expect(page.getByText('Library & editor preview').first()).toBeVisible();
    await expect(page.getByRole('button', { name: 'Start Recorder', exact: true })).toHaveCount(0);
    await expect(page.getByRole('group', { name: 'Capture Mode' })).toHaveCount(0);
    await page.getByRole('button', { name: 'Open Overtime winner', exact: true }).click();
    await page.getByRole('button', { name: 'Mute Game audio', exact: true }).click();
    await page.getByRole('button', { name: 'Save Trim', exact: true }).click();
    await expect.poll(() => calls(page, 'trim_clip_cmd')).toHaveLength(1);
    expect((await calls(page, 'trim_clip_cmd'))[0].args.mutedTracks).toEqual([true, false, false]);
    if (platform === 'linux') await expect(page.getByRole('button', { name: 'Share Clip', exact: true })).toBeEnabled();
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(page.getByText('Recording is not available in this preview yet.', { exact: false })).toBeVisible();
    await expect(page.getByLabel('Clips folder', { exact: true })).toBeVisible();
    await expect(page.getByLabel('Capture mode', { exact: true })).toHaveCount(0);
    await page.getByLabel('Clips folder', { exact: true }).fill('/home/test/Videos/Encore');
    await page.getByRole('button', { name: 'Save settings', exact: true }).click();
    await expect.poll(() => calls(page, 'save_config')).toHaveLength(1);
    for (const command of ['check_for_updates', 'get_recorder_status', 'start_recorder', 'list_audio_devices', 'list_audio_applications', 'list_capture_applications']) {
      expect(await calls(page, command), command).toHaveLength(0);
    }
    expect(errors).toEqual([]);
  });
}

test('Linux first run explains how to open existing clips', async ({ page }) => {
  await mockDesktop(page, { platform: 'linux', empty: true });
  await expect(page.getByText('Choose your clips folder in Settings, then add MP4 files to browse and edit them here.')).toBeVisible();
});

test('native macOS settings preserve application identifiers and Command shortcuts', async ({ page }) => {
  const errors = await mockDesktop(page, { platform: 'macos', nativeRecording: true,
    captureApplications: [{ exe: 'com.example.game', title: 'My Mac Game' }],
    audioApplications: ['com.example.game', 'com.hnc.Discord'] });
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: 'Choose running application', exact: true }).click();
  await page.getByRole('button', { name: 'Add My Mac Game', exact: true }).click();
  await page.getByLabel('Separate audio by', { exact: true }).selectOption('Applications');
  await page.getByLabel('Chat application', { exact: true }).fill('com.hnc.Discord');
  await expect(page.getByRole('button', { name: 'Save settings', exact: true })).toBeEnabled();
  await expect(page.getByLabel('Start recorder at login', { exact: true })).toBeVisible();
  await page.getByLabel('Save shortcut', { exact: true }).click();
  await page.keyboard.press('Meta+Shift+S');
  await expect(page.getByLabel('Save shortcut', { exact: true })).toContainText('Shift+Super+S');
  await page.getByRole('button', { name: 'Save settings', exact: true }).click();
  const saved = (await calls(page, 'save_config'))[0].args.cfg;
  expect(saved.games).toContain('com.example.game');
  expect(saved.chat_application).toBe('com.hnc.Discord');
  expect(saved.hotkey).toBe('Shift+Super+S');
  expect(await calls(page, 'check_for_updates')).toHaveLength(0);
  expect(errors).toEqual([]);
});

test('Wayland uses the system window picker and explicit application audio', async ({ page }) => {
  const errors = await mockDesktop(page, { platform: 'linux', nativeRecording: true, wayland: true,
    audioApplications: ['game', 'Discord'] });
  await expect(page.getByRole('group', { name: 'Capture Mode' }).getByRole('button', { name: 'Window', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('option', { name: 'Active Window (Follows Focus)', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: 'Choose recording window…', exact: true }).click();
  await expect.poll(() => calls(page, 'select_capture_source')).toHaveLength(1);
  await page.getByLabel('Separate audio by', { exact: true }).selectOption('Applications');
  await page.getByLabel('Game audio application', { exact: true }).fill('game');
  await page.getByLabel('Chat application', { exact: true }).fill('Discord');
  await expect(page.getByRole('button', { name: 'Choose running application', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'Save settings', exact: true }).click();
  const saved = (await calls(page, 'save_config'))[0].args.cfg;
  expect(saved.game_application).toBe('game');
  expect(saved.chat_application).toBe('Discord');
  expect(saved.mode).toBe('GameDetection');
  expect(errors).toEqual([]);
});
