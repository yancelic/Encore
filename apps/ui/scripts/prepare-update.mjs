import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export function prepareUpdate({ root = fileURLToPath(new URL('../../../', import.meta.url)), notes = process.env.ENCORE_RELEASE_NOTES ?? '' } = {}) {
  const config = JSON.parse(readFileSync(join(root, 'apps/ui/src-tauri/tauri.conf.json'), 'utf8'));
  const pkg = JSON.parse(readFileSync(join(root, 'apps/ui/package.json'), 'utf8'));
  const workspace = readFileSync(join(root, 'Cargo.toml'), 'utf8');
  const version = config.version;
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) throw new Error('Invalid release version');
  if (pkg.version !== version || !workspace.includes(`version = "${version}"`)) {
    throw new Error('Keep Cargo.toml, package.json and tauri.conf.json versions in sync before releasing.');
  }
  const directory = join(root, 'target/release/bundle/nsis');
  const installer = join(directory, `${config.productName}_${version}_x64-setup.exe`);
  if (!existsSync(installer) || !existsSync(`${installer}.sig`)) {
    throw new Error('Build a signed x64 NSIS installer before preparing update metadata.');
  }
  const signature = readFileSync(`${installer}.sig`, 'utf8').trim();
  if (!signature) throw new Error('The installer signature is empty.');
  const manifest = {
    version,
    notes,
    pub_date: new Date().toISOString(),
    platforms: {
      'windows-x86_64': {
        signature,
        url: `https://github.com/yancelic/Encore/releases/download/v${version}/${encodeURIComponent(basename(installer))}`,
      },
    },
  };
  writeFileSync(join(directory, 'latest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`Update metadata ready for tag v${version}: target/release/bundle/nsis/latest.json`);
  return manifest;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) prepareUpdate();
