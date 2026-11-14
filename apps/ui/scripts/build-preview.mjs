import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ui = fileURLToPath(new URL('../', import.meta.url));
if (!['darwin', 'linux'].includes(process.platform)) {
  throw new Error('Use build:installer for Windows. build:preview builds native macOS/Linux development packages.');
}
if (!existsSync(fileURLToPath(new URL('../../../tools/obs-runtime/lib/', import.meta.url)))) {
  throw new Error('Prepare the capture runtime first: python3 native/obs/prepare-macos.py or python3 native/obs/prepare-linux.py');
}
const helper = spawnSync('python3', [fileURLToPath(new URL('./prepare-native-recorder.py', import.meta.url))], { cwd: ui, stdio: 'inherit' });
if (helper.error) throw helper.error;
if (helper.status !== 0) process.exit(helper.status ?? 1);
if (process.platform === 'darwin') {
  if (process.arch !== 'arm64') throw new Error('The macOS preview currently targets Apple Silicon only.');
  if (!existsSync(fileURLToPath(new URL('../../../tools/ffmpeg', import.meta.url)))) {
    throw new Error('Install FFmpeg with Homebrew, then run python3 apps/ui/scripts/prepare-macos-ffmpeg.py from the repo root.');
  }
}
const cli = fileURLToPath(new URL('../node_modules/@tauri-apps/cli/tauri.js', import.meta.url));
const result = spawnSync(process.execPath, [cli, 'build', '--ci', '--config', `src-tauri/tauri.capture.${process.platform}.json`, '--', '--locked'], {
  cwd: ui, stdio: 'inherit', env: { ...process.env, CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS ?? '2' },
});
if (result.error) throw result.error;
if (result.status === 0 && process.platform === 'darwin') {
  const finish = spawnSync('python3', [fileURLToPath(new URL('./finish-macos-bundle.py', import.meta.url))], { cwd: ui, stdio: 'inherit' });
  if (finish.error) throw finish.error;
  process.exit(finish.status ?? 1);
}
process.exit(result.status ?? 1);
