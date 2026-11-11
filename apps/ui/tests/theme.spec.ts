import { test, expect, type Page } from '@playwright/test';
import { mockDesktop } from './fixtures';

const surface = (page: Page, selector: string) =>
  page
    .locator(selector)
    .evaluate((element) => getComputedStyle(element).backgroundColor);

for (const mode of ['light', 'dark'] as const) {
  test(`${mode} system theme covers library, editor, settings and montage`, async ({
    page,
  }, testInfo) => {
    await page.emulateMedia({ colorScheme: mode });
    const errors = await mockDesktop(page);
    await expect(page.locator('html')).toHaveAttribute('data-theme', mode);
    await expect(page.locator('html')).toHaveAttribute(
      'data-theme-preference',
      'system',
    );
    await expect(page.locator('.clip-card')).toHaveCount(6);
    await page.screenshot({ path: testInfo.outputPath(`library-${mode}.png`) });
    const background = await surface(page, 'body');
    await page
      .getByRole('button', { name: 'Open Overtime winner', exact: true })
      .click();
    expect(await surface(page, '.editor-shell')).toBe(background);
    expect(
      await page
        .locator('.editor-shell')
        .evaluate((element) => getComputedStyle(element).colorScheme),
    ).toBe(mode);
    await expect(page.locator('.waveform-canvas')).toHaveCount(3);
    await page.screenshot({ path: testInfo.outputPath(`editor-${mode}.png`) });
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(
      page.getByRole('radio', { name: 'System', exact: true }),
    ).toBeChecked();
    expect(await surface(page, '.modal-card')).toBe(background);
    await page.screenshot({
      path: testInfo.outputPath(`settings-${mode}.png`),
    });
    await page.keyboard.press('Escape');
    await page
      .getByRole('button', { name: 'All replays', exact: true })
      .click();
    await page
      .getByRole('button', { name: 'Create montage', exact: true })
      .click();
    await page
      .getByRole('button', { name: 'Select Overtime winner', exact: true })
      .click();
    await page
      .getByRole('button', { name: 'Select Off the backboard', exact: true })
      .click();
    await page.getByRole('button', { name: 'Continue with 2 clips' }).click();
    expect(await surface(page, '.modal-card')).toBe(background);
    await page.screenshot({ path: testInfo.outputPath(`montage-${mode}.png`) });
    expect(errors).toEqual([]);
  });
}

test('system preference responds to OS changes, explicit choice persists across launches', async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  const errors = await mockDesktop(page);
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page
    .getByRole('button', { name: 'Open Overtime winner', exact: true })
    .click();
  await expect(page.locator('.waveform-canvas')).toHaveCount(3);
  const darkWaveform = await page
    .locator('.waveform-canvas')
    .first()
    .evaluate((canvas) => (canvas as HTMLCanvasElement).toDataURL());
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('radio', { name: 'Light', exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await expect
    .poll(() => surface(page, '.editor-shell'))
    .toBe('rgb(236, 235, 243)');
  await expect
    .poll(() =>
      page
        .locator('.waveform-canvas')
        .first()
        .evaluate((canvas) => (canvas as HTMLCanvasElement).toDataURL()),
    )
    .not.toBe(darkWaveform);
  expect(await page.evaluate(() => localStorage.getItem('encore.theme'))).toBe(
    'light',
  );
  expect(
    await page.evaluate(() =>
      window.__uiCalls.filter(
        (call) =>
          call.command === 'save_config' ||
          call.command === 'start_recorder' ||
          call.command === 'stop_recorder',
      ),
    ),
  ).toHaveLength(0);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__uiCalls
            .filter((call) => call.command === 'plugin:window|set_theme')
            .at(-1)?.args.value,
      ),
    )
    .toBe('light');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute(
    'data-theme-preference',
    'light',
  );
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await expect(
    page.getByRole('radio', { name: 'Light', exact: true }),
  ).toBeChecked();
  await page.getByRole('radio', { name: 'Light', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(
    page.getByRole('radio', { name: 'Dark', exact: true }),
  ).toBeChecked();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.getByRole('radio', { name: 'System', exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__uiCalls
            .filter((call) => call.command === 'plugin:window|set_theme')
            .at(-1)?.args.value,
      ),
    )
    .toBe(null);
  expect(errors).toEqual([]);
});

test('invalid stored theme falls back to system', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.addInitScript(() =>
    localStorage.setItem('encore.theme', 'invalid'),
  );
  const errors = await mockDesktop(page);
  await expect(page.locator('html')).toHaveAttribute(
    'data-theme-preference',
    'system',
  );
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect(errors).toEqual([]);
});

test('themes remain usable when preference storage is unavailable', async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.addInitScript(() => {
    Storage.prototype.getItem = () => {
      throw new Error('Storage unavailable');
    };
    Storage.prototype.setItem = () => {
      throw new Error('Storage unavailable');
    };
  });
  const errors = await mockDesktop(page);
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('radio', { name: 'Light', exact: true }).check();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  expect(errors).toEqual([]);
});
