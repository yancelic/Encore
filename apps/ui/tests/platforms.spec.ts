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
    if (platform === 'linux') await expect(page.getByRole('button', { name: 'Share Clip', exact: true })).toBeDisabled();
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
