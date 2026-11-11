import { test, expect } from '@playwright/test';
import { mockDesktop } from './fixtures';

test('checks once on launch, never downloads without confirmation, and supports Later', async ({ page }) => {
  const errors = await mockDesktop(page, { update: '0.5.0' });
  const notice = page.getByRole('complementary', { name: 'App update available' });
  await expect(notice).toContainText('0.5.0');
  await page.getByRole('button', { name: 'Review update', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Encore 0.5.0' });
  await expect(dialog).toContainText('current replay buffer and unsaved edits will be cleared');
  await expect(dialog).toContainText('Better capture');
  await dialog.getByRole('button', { name: 'Later', exact: true }).click();
  await notice.getByRole('button', { name: 'Later', exact: true }).click();
  await expect(notice).toHaveCount(0);
  await page.waitForTimeout(1800);
  const calls = await page.evaluate(() => window.__uiCalls.map((c) => c.command));
  expect(calls.filter((c) => c === 'check_for_updates')).toHaveLength(1);
  expect(calls).not.toContain('install_update');
  expect(calls).not.toContain('stop_recorder');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: 'Review Encore 0.5.0' }).click();
  await expect(dialog).toBeVisible();
  expect(errors).toEqual([]);
});

test('manual check reports up to date and does not start recording or installation', async ({ page }) => {
  await mockDesktop(page);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByText('You’re up to date.')).toBeVisible();
  await page.getByRole('button', { name: 'Check for updates', exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__uiCalls.filter((c) => c.command === 'check_for_updates').length)).toBe(2);
  await expect(page.getByText('You’re up to date.')).toBeVisible();
  expect(await page.evaluate(() => window.__uiCalls.some((c) => c.command === 'install_update' || c.command === 'start_recorder'))).toBe(false);
});

test('offline startup leaves the library usable and manual retry can find a release', async ({ page }) => {
  const errors = await mockDesktop(page, { updateCheckError: true });
  await expect(page.getByRole('button', { name: 'Open Overtime winner' })).toBeVisible();
  await expect(page.getByRole('complementary', { name: 'App update available' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Network unavailable');
  await page.evaluate(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === 'check_for_updates') return { version: '0.5.0', notes: null };
      return original(command, args);
    };
  });
  await page.getByRole('button', { name: 'Check for updates', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Review Encore 0.5.0' })).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(0);
  expect(errors).toEqual([]);
});

test('confirmation shows progress and locks dismissal while capture is prepared', async ({ page }, testInfo) => {
  const errors = await mockDesktop(page, { update: '0.5.0' });
  await page.getByRole('button', { name: 'Review update', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Encore 0.5.0' });
  await page.screenshot({ path: testInfo.outputPath('update-confirmation.png') });
  await dialog.getByRole('button', { name: 'Update and restart', exact: true }).click();
  await expect(dialog.getByRole('progressbar')).toHaveAttribute('value', '50');
  await expect(dialog.getByRole('button', { name: 'Later', exact: true })).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('Finishing saves and exports');
  await page.screenshot({ path: testInfo.outputPath('update-preparing.png') });
  expect(await page.evaluate(() => window.__uiCalls.filter((c) => c.command === 'install_update').length)).toBe(1);
  expect(errors).toEqual([]);
});

test('failed download or signature verification unlocks the UI and allows a fresh check', async ({ page }) => {
  const errors = await mockDesktop(page, { update: '0.5.0', updateInstallError: true });
  await page.getByRole('button', { name: 'Review update', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Encore 0.5.0' });
  await dialog.getByRole('button', { name: 'Update and restart', exact: true }).click();
  await expect(dialog.getByRole('alert')).toContainText('signature verification failed');
  await expect(dialog.getByRole('button', { name: 'Later', exact: true })).toBeEnabled();
  await dialog.getByRole('button', { name: 'Check again', exact: true }).click();
  await expect(dialog.getByRole('button', { name: 'Update and restart', exact: true })).toBeEnabled();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Open Overtime winner' })).toBeVisible();
  expect(errors).toEqual([]);
});
