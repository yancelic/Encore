import { test, expect } from '@playwright/test';
import { mockDesktop } from './fixtures';

test('custom crop can be moved, resized, reset and exported with trim/audio', async ({
  page,
}, testInfo) => {
  const errors = await mockDesktop(page);
  await page
    .getByRole('button', { name: 'Open Overtime winner', exact: true })
    .click();
  await page.getByRole('button', { name: 'Custom', exact: true }).click();
  const box = page.locator('.custom-crop-box');
  await expect(box).toBeVisible();
  const before = await box.boundingBox();
  await page.mouse.move(before!.x + 30, before!.y + 30);
  await page.mouse.down();
  await page.mouse.move(before!.x + 45, before!.y + 40);
  await page.mouse.up();
  expect((await box.boundingBox())!.x).toBeGreaterThan(before!.x);
  const handle = await page.locator('.crop-resize').boundingBox();
  await page.mouse.move(handle!.x + 8, handle!.y + 8);
  await page.mouse.down();
  await page.mouse.move(handle!.x - 25, handle!.y - 25);
  await page.mouse.up();
  expect((await box.boundingBox())!.width).toBeLessThan(before!.width);
  await page.getByRole('button', { name: 'Reset crop' }).click();
  await expect(page.getByLabel('Crop width percent')).toHaveValue('80');
  await page.getByLabel('Crop width percent').fill('50');
  await page.getByLabel('Crop height percent').fill('50');
  await page.getByLabel('Crop x percent').fill('25');
  await page.getByLabel('Crop y percent').fill('20');
  await page
    .getByRole('button', { name: 'Mute Game audio', exact: true })
    .click();
  await page.screenshot({ path: testInfo.outputPath('custom-crop.png') });
  await page.getByRole('button', { name: 'Export cropped MP4' }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__uiCalls.filter(
            (call) => call.command === 'export_vertical_cmd',
          ).length,
      ),
    )
    .toBe(1);
  const args = await page.evaluate(
    () =>
      window.__uiCalls.find((call) => call.command === 'export_vertical_cmd')!
        .args,
  );
  expect(args.crop).toEqual([0.25, 0.2, 0.5, 0.5]);
  expect(args.mutedTracks).toEqual([true, false, false]);
  expect(args.blurBg).toBe(false);
  expect(errors).toEqual([]);
});
