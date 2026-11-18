"""Copy signed native bundles with their framework symlinks intact, then create the DMG
and the in-app update archive from the same finished, signed application."""
from pathlib import Path
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile

if sys.platform != 'darwin':
    raise SystemExit('This bundling step requires macOS')
root = Path(__file__).resolve().parents[3]
app = root / 'target/release/bundle/macos/Encore.app'
resources = app / 'Contents/Resources'
for name in ['obs-runtime', 'Encore Recorder.app']:
    destination = resources / name
    if not destination.resolve().is_relative_to(app.resolve()):
        raise RuntimeError(f'Unexpected bundle destination: {destination}')
    if destination.exists():
        shutil.rmtree(destination)
    subprocess.check_call(['ditto', str(root / 'tools' / name), str(destination)])
import macos_signing
macos_signing.sign(app, preserve_entitlements=True)
subprocess.check_call(['codesign', '--verify', '--deep', '--strict', str(app)])
staging = root / 'target/native-dmg'
staging.mkdir(exist_ok=True)
subprocess.check_call(['ditto', str(app), str(staging / app.name)])
shortcut = staging / 'Applications'
if not shortcut.is_symlink():
    os.symlink('/Applications', shortcut)
version = __import__('json').loads((root / 'apps/ui/src-tauri/tauri.conf.json').read_text())['version']
output = root / f'target/release/bundle/dmg/Encore_{version}_aarch64.dmg'
output.parent.mkdir(exist_ok=True)
subprocess.check_call(['hdiutil', 'create', '-volname', 'Encore', '-srcfolder', str(staging),
    '-ov', '-format', 'UDZO', str(output)])

# Tauri's own updater artifact would archive the bundle before the resources above are added
# and signed, so build it here. The updater skips the first path component and expects it to
# be the .app; Python's tarfile keeps framework symlinks and adds no AppleDouble entries.
archive = output.parent / f'Encore_{version}_aarch64.app.tar.gz'
with tarfile.open(archive, 'w:gz') as tar:
    tar.add(app, arcname=app.name)
with tempfile.TemporaryDirectory() as extracted:
    with tarfile.open(archive) as tar:
        tar.extractall(extracted, filter='tar')
    subprocess.check_call(['codesign', '--verify', '--deep', '--strict', str(Path(extracted) / app.name)])
print(f'Update archive ready: {archive.relative_to(root)}')

signature = Path(f'{archive}.sig')
signature.unlink(missing_ok=True)
if os.environ.get('TAURI_SIGNING_PRIVATE_KEY'):
    env = {**os.environ, 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD': os.environ.get('TAURI_SIGNING_PRIVATE_KEY_PASSWORD', '')}
    cli = root / 'apps/ui/node_modules/@tauri-apps/cli/tauri.js'
    subprocess.check_call(['node', str(cli), 'signer', 'sign', str(archive)], env=env)
    if not signature.read_text().strip():
        raise RuntimeError('The update archive signature is empty')
    print(f'Update archive signed: {signature.relative_to(root)}')
else:
    print('TAURI_SIGNING_PRIVATE_KEY is not set; the update archive is unsigned and cannot be published.')
