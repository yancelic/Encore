import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'win32')
  throw new Error('The Encore NSIS installer must be built on Windows.');
const root = fileURLToPath(new URL('../../../', import.meta.url));
const ui = fileURLToPath(new URL('../', import.meta.url));
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
