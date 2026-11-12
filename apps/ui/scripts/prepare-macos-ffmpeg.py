"""Prepare an ARM64 FFmpeg and its Homebrew dylibs for a relocatable preview bundle."""
from pathlib import Path
import os
import platform
import shutil
import subprocess
import sys


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def prepare():
    if sys.platform != 'darwin' or platform.machine() != 'arm64':
        raise SystemExit('Run this script on an Apple Silicon Mac.')
    source = shutil.which('ffmpeg')
    if not source:
        raise SystemExit('Install FFmpeg first: brew install ffmpeg')
    root = Path(__file__).resolve().parents[3]
    tools = root / 'tools'
    libraries = tools / 'ffmpeg-libs'
    libraries.mkdir(parents=True, exist_ok=True)
    binary = tools / 'ffmpeg'
    shutil.copy2(source, binary)
    if 'arm64' not in run('lipo', '-archs', str(binary)).split():
        raise SystemExit('FFmpeg does not contain an ARM64 binary.')
    pending = [(Path(source).resolve(), binary)]
    copied = {}
    completed = []
    while pending:
        original, target = pending.pop()
        # Every dependency is inspected before its load commands are rewritten.
        dependencies = [line.strip().split(' (', 1)[0]
                        for line in run('otool', '-L', str(original)).splitlines()[1:]]
        own_id = run('otool', '-D', str(original)).splitlines()
        own_id = own_id[1].strip() if len(own_id) > 1 else None
        os.chmod(target, 0o755)
        # Removing a signature is optional: some Homebrew libraries are unsigned.
        subprocess.run(['codesign', '--remove-signature', str(target)],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        for dependency in dependencies:
            if dependency == own_id or dependency.startswith(('/usr/lib/', '/System/Library/')):
                continue
            if not dependency.startswith(('/opt/homebrew/', '/usr/local/')):
                raise SystemExit(f'Unresolved dependency {dependency} in {original}')
            dep = Path(dependency).resolve()
            name = dep.name
            if name in copied and copied[name] != dep:
                raise SystemExit(f'Conflicting dylib filenames: {name}')
            if name not in copied:
                copied[name] = dep
                shutil.copy2(dep, libraries / name)
                pending.append((dep, libraries / name))
            relative = f'@loader_path/ffmpeg-libs/{name}' if target == binary else f'@loader_path/{name}'
            subprocess.check_call(['install_name_tool', '-change', dependency, relative, str(target)])
        if target != binary:
            subprocess.check_call(['install_name_tool', '-id', f'@loader_path/{target.name}', str(target)])
        completed.append(target)
    for target in completed:
        subprocess.check_call(['codesign', '--force', '--sign', '-', str(target)])
    subprocess.check_call([str(binary), '-version'])
    print(f'Prepared FFmpeg and {len(copied)} libraries. This preview uses ad-hoc signing, not Apple notarization.')


if __name__ == '__main__':
    prepare()
