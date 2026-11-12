import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ui = fileURLToPath(new URL('../', import.meta.url));
if (!['darwin', 'linux'].includes(process.platform)) {
  throw new Error('Use build:installer for Windows. build:preview builds the macOS/Linux library and editor preview.');
}
if (process.platform === 'darwin') {
  if (process.arch !== 'arm64') throw new Error('The macOS preview currently targets Apple Silicon only.');
  if (!existsSync(fileURLToPath(new URL('../../../tools/ffmpeg', import.meta.url)))) {
    throw new Error('Install FFmpeg with Homebrew, then run python3 apps/ui/scripts/prepare-macos-ffmpeg.py from the repo root.');
  }
}
const cli = fileURLToPath(new URL('../node_modules/@tauri-apps/cli/tauri.js', import.meta.url));
const result = spawnSync(process.execPath, [cli, 'build', '--ci', '--', '--locked'], {
  cwd: ui, stdio: 'inherit', env: { ...process.env, CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS ?? '2' },
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
