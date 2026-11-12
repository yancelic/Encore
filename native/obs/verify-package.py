"""Relocate a development package and exercise its bundled recorder and runtime."""
from pathlib import Path
import json
import os
import shutil
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix='encore-package-') as temporary:
    directory = Path(temporary)
    environment = dict(os.environ)
    environment.pop('ENCORE_NATIVE_RUNTIME', None)
    if sys.platform == 'darwin':
        app = directory / 'Encore.app'
        subprocess.check_call(['ditto', str(root / 'target/release/bundle/macos/Encore.app'), str(app)])
        subprocess.check_call(['codesign', '--verify', '--deep', '--strict', str(app)])
        helper = app / 'Contents/Resources/Encore Recorder.app/Contents/MacOS/encore-recorder'
        # FFmpeg must come from the bundle, even with Homebrew absent from PATH.
        environment['PATH'] = '/usr/bin:/bin:/usr/sbin:/sbin'
        prefix = []
    else:
        package = next((root / 'target/release/bundle/deb').glob('*.deb'))
        subprocess.check_call(['dpkg-deb', '-x', str(package), str(directory)])
        helper = next(path for path in directory.rglob('encore-recorder') if path.is_file())
        prefix = ['xvfb-run', '-a']
        environment['LIBGL_ALWAYS_SOFTWARE'] = '1'
    subprocess.check_call([*prefix, str(helper), '--self-test', str(directory / 'self-test')],
        cwd=directory, env=environment)
    subprocess.check_call([sys.executable, str(root / 'native/obs/verify-self-test.py'),
        str(directory / 'self-test/encore-self-test.mp4')])
    diagnostics = json.loads(subprocess.check_output([*prefix, str(helper), '--query', 'diagnostics'],
        cwd=directory, env=environment))
    registered = {source['id'] for source in diagnostics['sources']}
    assert diagnostics['video_source'] in registered, diagnostics
    assert diagnostics['h264_encoders'], diagnostics
    print(json.dumps({'ok': True, 'relocated_package': sys.platform, 'obs': diagnostics['obs_version']}))
