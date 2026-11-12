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
        framework = app / 'Contents/Resources/obs-runtime/Contents/Frameworks/libobs.framework'
        assert (framework / 'Versions/Current').is_symlink(), 'The macOS framework layout was flattened during packaging'
        # FFmpeg must come from the bundle, even with Homebrew absent from PATH.
        environment['PATH'] = '/usr/bin:/bin:/usr/sbin:/sbin'
        prefix = []
    else:
        package = next((root / 'target/release/bundle/deb').glob('*.deb'))
        subprocess.check_call(['dpkg-deb', '-x', str(package), str(directory)])
        helper = next(path for path in directory.rglob('encore-recorder') if path.is_file())
        prefix = ['xvfb-run', '-a']
        environment['LIBGL_ALWAYS_SOFTWARE'] = '1'
    arguments = [*prefix, str(helper), '--self-test', str(directory / 'self-test')]
    try:
        subprocess.check_call(arguments, cwd=directory, env=environment)
    except subprocess.CalledProcessError as error:
        if sys.platform == 'darwin' and error.returncode < 0:
            subprocess.run(['lldb', '--batch', '-o', 'run', '-o', 'thread backtrace all', '--', *arguments],
                cwd=directory, env=environment, timeout=30, check=False)
        raise
    subprocess.check_call([sys.executable, str(root / 'native/obs/verify-self-test.py'),
        str(directory / 'self-test/encore-self-test.mp4')])
    query = subprocess.check_output([*prefix, str(helper), '--query', 'diagnostics'],
        cwd=directory, env=environment, text=True)
    diagnostics = json.loads(next(line.removeprefix('ENCORE_QUERY ') for line in query.splitlines()
        if line.startswith('ENCORE_QUERY ')))
    registered = {source['id'] for source in diagnostics['sources']}
    assert diagnostics['video_source'] in registered, diagnostics
    assert diagnostics['h264_encoders'], diagnostics
    print(json.dumps({'ok': True, 'relocated_package': sys.platform, 'obs': diagnostics['obs_version']}))
