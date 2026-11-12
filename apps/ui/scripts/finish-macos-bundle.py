"""Copy signed native bundles with their framework symlinks intact, then create the DMG."""
from pathlib import Path
import os
import shutil
import subprocess
import sys

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
subprocess.check_call(['codesign', '--force', '--sign', '-', '--preserve-metadata=entitlements', str(app)])
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
