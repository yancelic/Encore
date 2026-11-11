import { test, expect, type Page } from '@playwright/test';
import { mockDesktop } from './fixtures';

const calls = (page: Page, command: string) => page.evaluate(
  (name) => window.__uiCalls.filter((call) => call.command === name), command,
);

test('adds a running game by its window title, prevents duplicates and only scans on request', async ({ page }, testInfo) => {
  const errors = await mockDesktop(page);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  expect(await calls(page, 'list_capture_applications')).toHaveLength(0);
  await page.getByRole('button', { name: 'Choose running application', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Rocket League already added' })).toBeDisabled();
  await page.getByRole('searchbox', { name: 'Search running applications' }).fill('new game');
  await expect(page.getByRole('button', { name: 'Rocket League already added' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Add My New Game', exact: true }).click();
  await expect(page.getByRole('button', { name: 'My New Game already added' })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Remove NewGame-Win64-Shipping.exe', exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('running-game-picker.png') });
  await page.waitForTimeout(1700);
  expect(await calls(page, 'list_capture_applications')).toHaveLength(1);
  expect(await calls(page, 'save_config')).toHaveLength(0);
  await page.getByRole('button', { name: 'Save settings', exact: true }).click();
  const saved = (await calls(page, 'save_config'))[0].args.cfg;
  expect(saved.games).toEqual(['RocketLeague.exe', 'cs2.exe', 'NewGame-Win64-Shipping.exe']);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Remove NewGame-Win64-Shipping.exe', exact: true })).toBeVisible();
  expect(errors).toEqual([]);
});

test('browses for a game that is not running, handles cancellation and adds only the selected filename', async ({ page }) => {
  await mockDesktop(page, { pickedGame: null });
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: 'Browse game .exe', exact: true }).click();
  await expect.poll(async () => (await calls(page, 'pick_game_executable')).length).toBe(1);
  await expect(page.getByText('Games to capture (2)')).toBeVisible();
  await page.evaluate(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => command === 'pick_game_executable'
      ? Promise.resolve('OfflineGame.exe') : original(command, args);
  });
  await page.getByRole('button', { name: 'Browse game .exe', exact: true }).click();
  await expect(page.getByText('Games to capture (3)')).toBeVisible();
  await page.getByRole('button', { name: 'Browse game .exe', exact: true }).click();
  await expect(page.getByText('OfflineGame.exe is already in your games.')).toBeVisible();
  await page.getByRole('button', { name: 'Save settings', exact: true }).click();
  expect((await calls(page, 'save_config'))[0].args.cfg.games).toEqual(['RocketLeague.exe', 'cs2.exe', 'OfflineGame.exe']);
  expect(await calls(page, 'list_capture_applications')).toHaveLength(0);
});

test('empty and failed lists offer refresh and file selection, and Cancel discards additions', async ({ page }) => {
  const errors = await mockDesktop(page, { captureApplicationsError: true });
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: 'Choose running application', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Could not load running applications');
  await page.evaluate(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => command === 'list_capture_applications'
      ? Promise.resolve([]) : original(command, args);
  });
  await page.locator('#running-game-picker').getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(page.getByText('No application windows found.', { exact: false })).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.getByRole('button', { name: 'Browse game .exe', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Remove OfflineGame.exe', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Remove OfflineGame.exe', exact: true })).toHaveCount(0);
  expect(await calls(page, 'save_config')).toHaveLength(0);
  expect(errors).toEqual([]);
});

test('manual entry remains optional and rejects paths', async ({ page }) => {
  await mockDesktop(page);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByText('Enter executable name manually', { exact: true }).click();
  await page.getByRole('textbox', { name: 'Add game executable', exact: true }).fill('C:\\Games\\Game.exe');
  await page.getByRole('button', { name: 'Add Game', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Enter an executable filename');
  await page.getByRole('textbox', { name: 'Add game executable', exact: true }).fill('Emulator');
  await page.getByRole('button', { name: 'Add Game', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Remove Emulator.exe', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Save settings', exact: true }).click();
  expect((await calls(page, 'save_config'))[0].args.cfg.games).toContain('Emulator.exe');
});
