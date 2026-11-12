"""Stage the pinned OBS core/plugins and matching dependency headers for Apple Silicon.

No OBS frontend, Qt, CEF browser, updater, or camera plugin is bundled.
"""
from pathlib import Path
import hashlib
import platform
import shutil
import subprocess
import sys
import tarfile
import urllib.request

OBS_COMMIT = 'dcdbd2e9048ae66237fdab32dbba72b745b1b9c5'
DMG_URL = 'https://github.com/obsproject/obs-studio/releases/download/32.0.4/OBS-Studio-32.0.4-macOS-Apple.dmg'
DMG_SHA = '7c056644636a794cb18281af98d54b5795ad8b0985e950d93b8c510b645b6298'
DEPS_URL = 'https://github.com/obsproject/obs-deps/releases/download/2025-08-23/macos-deps-2025-08-23-universal.tar.xz'
DEPS_SHA = '9403bb43fb0a9bb215739a5659ca274fe884dbbbcd22bd9ca781c961fb041c42'


def download(url, path, sha=None):
    if not path.exists():
        urllib.request.urlretrieve(url, path)
    if sha and hashlib.sha256(path.read_bytes()).hexdigest() != sha:
        raise RuntimeError(f'Checksum mismatch: {path}')


def copy(path, destination):
    if path.is_symlink():
        if destination.is_symlink() and destination.readlink() == path.readlink():
            return
        if destination.exists() or destination.is_symlink():
            destination.unlink()
        destination.symlink_to(path.readlink())
    elif path.is_dir():
        shutil.copytree(path, destination, symlinks=True, dirs_exist_ok=True)
    else:
        shutil.copy2(path, destination)


def prepare():
    if sys.platform != 'darwin' or platform.machine() != 'arm64':
        raise SystemExit('Run this preparation on an Apple Silicon Mac.')
    root = Path(__file__).resolve().parents[2]
    cache = root / 'target' / 'native-obs-macos'
    cache.mkdir(parents=True, exist_ok=True)
    dmg = cache / 'OBS-32.0.4-Apple.dmg'
    deps_archive = cache / 'obs-deps.tar.xz'
    source_archive = cache / 'obs-source.tar.gz'
    download(DMG_URL, dmg, DMG_SHA)
    download(DEPS_URL, deps_archive, DEPS_SHA)
    download(f'https://github.com/obsproject/obs-studio/archive/{OBS_COMMIT}.tar.gz', source_archive)
    deps = cache / 'deps'
    source_container = cache / 'source'
    for archive, destination in [(deps_archive, deps), (source_archive, source_container)]:
        if not destination.exists():
            destination.mkdir()
            with tarfile.open(archive) as contents:
                contents.extractall(destination, filter='data')
    source = source_container / f'obs-studio-{OBS_COMMIT}'
    # Release tarballs have occasionally contained an extra top-level directory.
    if not (deps / 'lib').is_dir():
        candidates = list(deps.glob('*/lib'))
        if len(candidates) != 1:
            raise RuntimeError('Cannot locate OBS dependency libraries')
        deps = candidates[0].parent
    runtime = root / 'tools' / 'obs-runtime'
    frameworks = runtime / 'Contents' / 'Frameworks'
    plugins = runtime / 'Contents' / 'PlugIns'
    frameworks.mkdir(parents=True, exist_ok=True)
    plugins.mkdir(parents=True, exist_ok=True)
    for library in (deps / 'lib').iterdir():
        if '.dylib' in library.name or library.suffix == '.framework':
            copy(library, frameworks / library.name)
    mount = cache / 'mounted-obs'
    mount.mkdir(exist_ok=True)
    subprocess.check_call(['hdiutil', 'attach', '-nobrowse', '-readonly', '-mountpoint', str(mount), str(dmg)])
    try:
        app = mount / 'OBS.app' / 'Contents'
        if not app.is_dir():
            raise RuntimeError('OBS.app was not found in the verified disk image')
        copy(app / 'Frameworks' / 'libobs.framework', frameworks / 'libobs.framework')
        for renderer in (app / 'Frameworks').glob('libobs-*'):
            copy(renderer, frameworks / renderer.name)
        for name in ['mac-capture', 'mac-videotoolbox', 'obs-ffmpeg', 'obs-x264']:
            plugin = app / 'PlugIns' / f'{name}.plugin'
            if not plugin.exists():
                raise RuntimeError(f'Required OBS capture plugin missing: {name}')
            copy(plugin, plugins / plugin.name)
    finally:
        subprocess.check_call(['hdiutil', 'detach', str(mount)])
    (runtime / 'COPYING-OBS').write_bytes((source / 'COPYING').read_bytes())
    (runtime / 'SOURCES.txt').write_text(f'OBS Studio 32.0.4: https://github.com/obsproject/obs-studio/tree/{OBS_COMMIT}\nOBS dependencies: {DEPS_URL}\n', encoding='utf-8')
    build = cache / 'build'
    subprocess.check_call(['cmake', '-S', str(root / 'native' / 'obs'), '-B', str(build),
        '-DCMAKE_BUILD_TYPE=RelWithDebInfo', '-DCMAKE_OSX_DEPLOYMENT_TARGET=15.0',
        f'-DENCORE_OBS_SOURCE={source}', f'-DENCORE_OBS_DEPS={deps}', f'-DENCORE_OBS_RUNTIME={runtime}',
        f'-DCMAKE_INSTALL_PREFIX={runtime}'])
    subprocess.check_call(['cmake', '--build', str(build), '--parallel', '2'])
    subprocess.check_call(['cmake', '--install', str(build)])
    subprocess.check_call(['codesign', '--force', '--sign', '-', str(runtime / 'lib' / 'libencore_capture.dylib')])
    print(f'Native capture runtime staged at {runtime}')


if __name__ == '__main__':
    prepare()
