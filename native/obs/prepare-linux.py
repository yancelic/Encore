"""Stage the Ubuntu capture runtime and pinned PipeWire application-audio plugin."""
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
AUDIO_COMMIT = 'cc5a661e2f1037341cf92d6c990b6088876810b0'


def run(*args):
    subprocess.check_call(args)


def prepare():
    runtime = ROOT / 'tools/obs-runtime'
    cache = ROOT / 'target/native-obs-linux'
    source = cache / 'pipewire-audio'
    source.mkdir(parents=True, exist_ok=True)
    if not (source / '.git').exists():
        run('git', 'init', str(source))
    run('git', '-C', str(source), 'fetch', '--depth=1', 'https://github.com/dimtpap/obs-pipewire-audio-capture.git', AUDIO_COMMIT)
    run('git', '-C', str(source), 'checkout', '--detach', 'FETCH_HEAD')
    build = cache / 'audio-build'
    run('cmake', '-S', str(source), '-B', str(build), '-DCMAKE_BUILD_TYPE=Release', '-DLEGACY=ON',
        '-DCMAKE_INSTALL_LIBDIR=lib', f'-DCMAKE_INSTALL_PREFIX={runtime}')
    run('cmake', '--build', str(build), '--parallel', '2')
    run('cmake', '--install', str(build))
    build = cache / 'bridge-build'
    run('cmake', '-S', str(ROOT / 'native/obs'), '-B', str(build), '-DCMAKE_BUILD_TYPE=RelWithDebInfo',
        f'-DCMAKE_INSTALL_PREFIX={runtime}')
    run('cmake', '--build', str(build), '--parallel', '2')
    run('cmake', '--install', str(build))
    library_dir = Path(subprocess.check_output(['pkg-config', '--variable=libdir', 'libobs'], text=True).strip())
    data_dir = Path('/usr/share/obs')
    shutil.copytree(data_dir / 'libobs', runtime / 'share/obs/libobs', dirs_exist_ok=True)
    plugins = ['linux-capture', 'linux-pipewire', 'linux-pulseaudio', 'obs-ffmpeg', 'obs-x264']
    for name in plugins:
        plugin = library_dir / 'obs-plugins' / f'{name}.so'
        if not plugin.is_file():
            raise RuntimeError(f'Required capture plugin missing: {plugin}')
        shutil.copy2(plugin, runtime / 'lib/obs-plugins' / plugin.name)
        data = data_dir / 'obs-plugins' / name
        if data.is_dir():
            shutil.copytree(data, runtime / 'share/obs/obs-plugins' / name, dirs_exist_ok=True)
    renderer = library_dir / 'libobs-opengl.so'
    if not renderer.exists():
        renderer = next(library_dir.glob('libobs-opengl.so.*'))
    shutil.copy2(renderer.resolve(), runtime / 'lib/libobs-opengl.so')
    shutil.copy2(source / 'LICENSE', runtime / 'COPYING-PIPEWIRE-AUDIO')
    shutil.copy2('/usr/share/doc/obs-studio/copyright', runtime / 'COPYING-OBS')
    (runtime / 'SOURCES.txt').write_text(
        'OBS Studio: Ubuntu libobs/obs-studio packages, https://launchpad.net/ubuntu/+source/obs-studio\n'
        f'PipeWire application audio: https://github.com/dimtpap/obs-pipewire-audio-capture/tree/{AUDIO_COMMIT}\n', encoding='utf-8')


if __name__ == '__main__':
    prepare()
