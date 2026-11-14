"""Prepare the background helper before bundling the native desktop application."""
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parents[3]
subprocess.check_call(['cargo', 'build', '--locked', '--release', '-p', 'encore-recorder'], cwd=root)
if sys.platform == 'darwin':
    helper = root / 'tools/Encore Recorder.app/Contents'
    (helper / 'MacOS').mkdir(parents=True, exist_ok=True)
    shutil.copy2(root / 'target/release/encore-recorder', helper / 'MacOS/encore-recorder')
    with (helper / 'Info.plist').open('wb') as file:
        plistlib.dump({
            'CFBundleIdentifier': 'com.encore.recorder', 'CFBundleName': 'Encore Recorder',
            'CFBundleDisplayName': 'Encore Recorder', 'CFBundleExecutable': 'encore-recorder',
            'CFBundlePackageType': 'APPL', 'CFBundleVersion': '0.5.0',
            'CFBundleShortVersionString': '0.5.0', 'LSMinimumSystemVersion': '15.0',
            'LSUIElement': True, 'NSHighResolutionCapable': True,
            'NSMicrophoneUsageDescription': 'Encore records your microphone as a separate track in saved game clips.',
            'NSScreenCaptureUsageDescription': 'Encore captures the selected game window to keep your replay buffer ready.',
            'NSAudioCaptureUsageDescription': 'Encore records game and chat audio in separate tracks for your clips.',
            'NSSupportsAutomaticTermination': False, 'NSSupportsSuddenTermination': False,
        }, file)
    subprocess.check_call(['codesign', '--force', '--sign', '-', '--entitlements',
        str(root / 'apps/ui/src-tauri/entitlements.macos.plist'), str(helper.parent)])
