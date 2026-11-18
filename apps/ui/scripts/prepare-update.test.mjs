import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import test from 'node:test';
import { prepareUpdate } from './prepare-update.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'encore-update-'));
  t.after(() => {
    assert.equal(dirname(resolve(root)), resolve(tmpdir()));
    assert.ok(basename(root).startsWith('encore-update-'));
    rmSync(root, { recursive: true, force: true });
  });
  mkdirSync(join(root, 'apps/ui/src-tauri'), { recursive: true });
  mkdirSync(join(root, 'target/release/bundle/nsis'), { recursive: true });
  writeFileSync(join(root, 'apps/ui/src-tauri/tauri.conf.json'), JSON.stringify({ productName: 'Encore', version: '1.2.3' }));
  writeFileSync(join(root, 'apps/ui/package.json'), JSON.stringify({ version: '1.2.3' }));
  writeFileSync(join(root, 'Cargo.toml'), '[workspace.package]\nversion = "1.2.3"\n');
  const installer = join(root, 'target/release/bundle/nsis/Encore_1.2.3_x64-setup.exe');
  writeFileSync(installer, 'test installer');
  writeFileSync(`${installer}.sig`, 'signed-update-fixture\n');
  return { root, installer };
}

test('metadata names the exact versioned installer and includes signature contents', (t) => {
  const { root } = fixture(t);
  const manifest = prepareUpdate({ root, notes: 'A new release\nSecond line' });
  assert.equal(manifest.platforms['windows-x86_64'].url, 'https://github.com/yancelic/Encore/releases/download/v1.2.3/Encore_1.2.3_x64-setup.exe');
  assert.equal(manifest.platforms['windows-x86_64'].signature, 'signed-update-fixture');
  assert.equal(manifest.notes, 'A new release\nSecond line');
  assert.deepEqual(JSON.parse(readFileSync(join(root, 'target/release/bundle/nsis/latest.json'), 'utf8')), manifest);
});

test('mismatched versions and unsigned installers cannot produce metadata', (t) => {
  const { root, installer } = fixture(t);
  rmSync(`${installer}.sig`);
  assert.throws(() => prepareUpdate({ root }), /signed x64 NSIS/);
  writeFileSync(`${installer}.sig`, '');
  assert.throws(() => prepareUpdate({ root }), /signature is empty/);
  writeFileSync(join(root, 'apps/ui/package.json'), JSON.stringify({ version: '1.2.4' }));
  assert.throws(() => prepareUpdate({ root }), /versions in sync/);
});

test('a release without a macOS directory publishes only Windows', (t) => {
  const { root } = fixture(t);
  const manifest = prepareUpdate({ root, macosDir: undefined });
  assert.deepEqual(Object.keys(manifest.platforms), ['windows-x86_64']);
});

test('the signed macOS archive from the matching stable run is added for Apple Silicon', (t) => {
  const { root } = fixture(t);
  const macosDir = join(root, 'macos-artifact');
  mkdirSync(macosDir);
  const archive = join(macosDir, 'Encore_1.2.3_aarch64.app.tar.gz');
  writeFileSync(archive, 'test archive');
  assert.throws(() => prepareUpdate({ root, macosDir }), /signed Encore_1.2.3_aarch64.app.tar.gz/);
  writeFileSync(`${archive}.sig`, '');
  assert.throws(() => prepareUpdate({ root, macosDir }), /macOS update archive signature is empty/);
  writeFileSync(`${archive}.sig`, 'signed-macos-fixture\n');
  const manifest = prepareUpdate({ root, macosDir });
  assert.deepEqual(manifest.platforms['darwin-aarch64'], {
    signature: 'signed-macos-fixture',
    url: 'https://github.com/yancelic/Encore/releases/download/v1.2.3/Encore_1.2.3_aarch64.app.tar.gz',
  });
  assert.equal(manifest.platforms['windows-x86_64'].signature, 'signed-update-fixture');
});
