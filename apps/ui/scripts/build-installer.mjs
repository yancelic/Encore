import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { prepareUpdate } from './prepare-update.mjs';

if (process.platform !== 'win32')
  throw new Error('The Encore NSIS installer must be built on Windows.');
const root = fileURLToPath(new URL('../../../', import.meta.url));
const ui = fileURLToPath(new URL('../', import.meta.url));
// Use an explicit environment key in CI, or the ignored local signing key.
const localKey = join(root, '.local-backups', 'updater', 'encore.key');
if (!process.env.TAURI_SIGNING_PRIVATE_KEY && existsSync(localKey)) {
  process.env.TAURI_SIGNING_PRIVATE_KEY = localKey;
}
if (!process.env.TAURI_SIGNING_PRIVATE_KEY) {
  throw new Error('Set TAURI_SIGNING_PRIVATE_KEY to the original Encore updater key before building. See docs/updates.md.');
}
process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ??= '';
if (
  !existsSync(
    fileURLToPath(new URL('../../../tools/ffmpeg.exe', import.meta.url)),
  )
) {
  throw new Error('Place ffmpeg.exe in tools/ before building the installer.');
}

function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    stdio: 'inherit',
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run(
  'cargo',
  ['build', '--locked', '--release', '--bin', 'encore-recorder'],
  root,
);
run(
  process.execPath,
  [
    fileURLToPath(
      new URL('../node_modules/@tauri-apps/cli/tauri.js', import.meta.url),
    ),
    'build',
    '--ci',
    '--bundles',
    'nsis',
    '--',
    '--locked',
  ],
  ui,
);
console.log('Installer ready in target/release/bundle/nsis/');
prepareUpdate();
run('cargo', [
  'test', '--locked', '--release', '-p', 'encore-ui',
  '--test', 'update_signature', '--', '--ignored',
], root);
console.log('Update signature verified against the public key shipped in Encore.');
