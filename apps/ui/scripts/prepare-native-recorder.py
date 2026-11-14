"""Prepare the background helper before bundling the native desktop application."""
from pathlib import Path
import json
import plistlib
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parents[3]
subprocess.check_call(['cargo', 'build', '--locked', '--release', '-p', 'encore-recorder'], cwd=root)
if sys.platform == 'darwin':
    # Apple bundle versions contain only the numeric core, including for betas.
    version = json.loads((root / 'apps/ui/src-tauri/tauri.conf.json').read_text())['version'].split('-')[0]
    helper = root / 'tools/Encore Recorder.app/Contents'
    (helper / 'MacOS').mkdir(parents=True, exist_ok=True)
    shutil.copy2(root / 'target/release/encore-recorder', helper / 'MacOS/encore-recorder')
    with (helper / 'Info.plist').open('wb') as file:
        plistlib.dump({
            'CFBundleIdentifier': 'com.encore.recorder', 'CFBundleName': 'Encore Recorder',
            'CFBundleDisplayName': 'Encore Recorder', 'CFBundleExecutable': 'encore-recorder',
            'CFBundlePackageType': 'APPL', 'CFBundleVersion': version,
            'CFBundleShortVersionString': version, 'LSMinimumSystemVersion': '15.0',
            'LSUIElement': True, 'NSHighResolutionCapable': True,
            'NSMicrophoneUsageDescription': 'Encore records your microphone as a separate track in saved game clips.',
            'NSScreenCaptureUsageDescription': 'Encore captures the selected game window to keep your replay buffer ready.',
            'NSAudioCaptureUsageDescription': 'Encore records game and chat audio in separate tracks for your clips.',
            'NSSupportsAutomaticTermination': False, 'NSSupportsSuddenTermination': False,
        }, file)
    subprocess.check_call(['codesign', '--force', '--sign', '-', '--entitlements',
        str(root / 'apps/ui/src-tauri/entitlements.macos.plist'), str(helper.parent)])
