"""Version and publish tested macOS/Linux previews without changing the stable feed."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
BETA = re.compile(r'(\d+\.\d+\.\d+)-beta\.(\d+)')


def release_plan(base, event, ref, run_number):
    match = BETA.fullmatch(base)
    if not match:
        raise ValueError(f'Expected a beta source version, got {base}')
    if event == 'push' and ref == 'refs/heads/dev':
        if not str(run_number).isdigit() or int(run_number) < 1:
            raise ValueError('A positive workflow run number is required')
        return f'{match[1]}-beta.{int(run_number)}', 'beta'
    if event == 'push' and ref.startswith('refs/tags/'):
        tag = ref.removeprefix('refs/tags/')
        if tag != f'v{base}':
            raise ValueError(f'Tag {tag} does not match source version {base}')
        return base, tag
    return base, ''


def apply_version(root, version):
    if not BETA.fullmatch(version):
        raise ValueError(f'Invalid beta version: {version}')
    cargo = root / 'Cargo.toml'
    cargo_text = cargo.read_text(encoding='utf-8')
    workspace = tomllib.loads(cargo_text)['workspace']
    old = workspace['package']['version']
    names = {tomllib.loads((root / member / 'Cargo.toml').read_text(encoding='utf-8'))['package']['name']
             for member in workspace['members']}
    changes = {}
    for relative in ['apps/ui/package.json', 'apps/ui/package-lock.json', 'apps/ui/src-tauri/tauri.conf.json']:
        path = root / relative
        value = json.loads(path.read_text(encoding='utf-8'))
        if value['version'] != old:
            raise ValueError(f'Out-of-sync version in {relative}')
        value['version'] = version
        if relative.endswith('package-lock.json'):
            if value['packages']['']['version'] != old:
                raise ValueError('Out-of-sync npm root package version')
            value['packages']['']['version'] = version
        changes[path] = json.dumps(value, indent=2, ensure_ascii=False) + '\n'
    section = re.search(r'(?ms)^\[workspace\.package\]\n(.*?)(?=^\[|\Z)', cargo_text)
    if not section:
        raise ValueError('Workspace package metadata missing')
    changed_section, count = re.subn(r'(?m)^version = "[^"]+"$', f'version = "{version}"', section[1])
    if count != 1:
        raise ValueError('Expected one workspace version')
    changes[cargo] = cargo_text[:section.start(1)] + changed_section + cargo_text[section.end(1):]
    lock = root / 'Cargo.lock'
    blocks = lock.read_text(encoding='utf-8').split('[[package]]')
    seen = set()
    for index, block in enumerate(blocks[1:], 1):
        package = tomllib.loads(block)
        if package['name'] not in names:
            continue
        if package['name'] in seen or package['version'] != old:
            raise ValueError('Workspace lockfile versions do not match')
        seen.add(package['name'])
        blocks[index] = re.sub(r'(?m)^version = "[^"]+"$', f'version = "{version}"', block, count=1)
    if seen != names:
        raise ValueError('Workspace packages missing from Cargo.lock')
    changes[lock] = '[[package]]'.join(blocks)
    # Validate all files before writing any of them.
    for path, content in changes.items():
        path.write_text(content, encoding='utf-8', newline='\n')


def one_file(directory, pattern):
    files = list(directory.rglob(pattern))
    if len(files) != 1:
        raise ValueError(f'Expected exactly one {pattern} in {directory}, got {len(files)}')
    return files[0]


def prepare_assets(root, version, sha, run_id):
    if not BETA.fullmatch(version) or not re.fullmatch(r'[0-9a-f]{40}', sha):
        raise ValueError('Invalid release version or source commit')
    artifacts = root / 'target/beta-artifact'
    deb = one_file(artifacts / 'ubuntu', '*.deb')
    dmg = one_file(artifacts / 'macos', '*.dmg')
    flatpak = one_file(artifacts / 'flatpak', '*.flatpak')
    deb_version = subprocess.check_output(['dpkg-deb', '-f', str(deb), 'Version'], text=True).strip()
    architecture = subprocess.check_output(['dpkg-deb', '-f', str(deb), 'Architecture'], text=True).strip()
    if deb_version != version or architecture != 'amd64':
        raise ValueError(f'Unexpected Ubuntu package: {deb_version}, {architecture}')
    if dmg.name != f'Encore_{version}_aarch64.dmg':
        raise ValueError(f'Unexpected macOS package: {dmg.name}')
    output = root / 'target/beta-release'
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError('Release staging directory must be empty')
    for source, name in [(deb, 'Encore-Ubuntu-amd64.deb'), (dmg, 'Encore-macOS-Apple-Silicon.dmg'),
                         (flatpak, 'Encore-Linux-x86_64.flatpak')]:
        shutil.copyfile(source, output / name)
    shutil.copyfile(root / 'docs/linux-beta-testing.md', output / 'LINUX-TESTING.md')
    (output / 'BUILD.json').write_text(json.dumps({
        'version': version, 'source_commit': sha, 'workflow_run_id': run_id,
        'workflow_url': f'https://github.com/yancelic/Encore/actions/runs/{run_id}',
    }, indent=2) + '\n', encoding='utf-8')
    checksums = ''.join(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n'
                        for path in sorted(output.iterdir()))
    (output / 'SHA256SUMS.txt').write_text(checksums, encoding='utf-8')
    notes = (root / 'docs/releases/macos-linux-beta.md').read_text(encoding='utf-8')
    notes += f'\nPackage version: `{version}`\n\nSource commit: `{sha}`\n'
    (root / 'target/beta-release-notes.md').write_text(notes, encoding='utf-8')


def gh(*args, capture=False):
    return subprocess.run(['gh', *args], check=True, text=True,
                          stdout=subprocess.PIPE if capture else None).stdout


def publish(root, tag, version, sha):
    if tag != 'beta' and tag != f'v{version}':
        raise ValueError('Unexpected beta release tag')
    if not BETA.fullmatch(version) or not re.fullmatch(r'[0-9a-f]{40}', sha):
        raise ValueError('Invalid release metadata')
    repo = os.environ['GH_REPO']
    title = f'Encore {version} — macOS & Linux Beta'
    notes = str(root / 'target/beta-release-notes.md')
    assets = [str(path) for path in sorted((root / 'target/beta-release').iterdir())]
    existing = subprocess.run(['gh', 'release', 'view', tag, '--json', 'isPrerelease'],
                              text=True, capture_output=True)
    if existing.returncode == 0:
        if tag != 'beta' or not json.loads(existing.stdout)['isPrerelease']:
            raise ValueError('Only the rolling beta prerelease may be updated')
        gh('release', 'edit', tag, '--draft=true', '--prerelease', '--latest=false')
        gh('release', 'upload', tag, *assets, '--clobber')
        # beta is an explicitly rolling channel; numbered release tags stay fixed.
        gh('api', '--method', 'PATCH', f'repos/{repo}/git/refs/tags/beta',
           '-f', f'sha={sha}', '-F', 'force=true')
        gh('release', 'edit', tag, '--title', title, '--notes-file', notes, '--target', sha)
    else:
        if 'release not found' not in existing.stderr.lower() and '404' not in existing.stderr:
            raise RuntimeError(existing.stderr)
        gh('release', 'create', tag, *assets, '--target', sha, '--draft', '--prerelease',
           '--latest=false', '--title', title, '--notes-file', notes)
    gh('release', 'edit', tag, '--draft=false', '--prerelease', '--latest=false')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['plan', 'apply', 'prepare', 'publish'])
    args = parser.parse_args()
    base = json.loads((ROOT / 'apps/ui/src-tauri/tauri.conf.json').read_text(encoding='utf-8'))['version']
    if args.command == 'plan':
        version, tag = release_plan(base, os.environ['GITHUB_EVENT_NAME'], os.environ['GITHUB_REF'],
                                    os.environ['GITHUB_RUN_NUMBER'])
        print(f'version={version}\ntag={tag}')
    elif args.command == 'apply':
        apply_version(ROOT, os.environ['ENCORE_BETA_VERSION'])
    elif args.command == 'prepare':
        prepare_assets(ROOT, os.environ['ENCORE_BETA_VERSION'], os.environ['GITHUB_SHA'], os.environ['GITHUB_RUN_ID'])
    else:
        publish(ROOT, os.environ['ENCORE_BETA_TAG'], os.environ['ENCORE_BETA_VERSION'], os.environ['GITHUB_SHA'])


if __name__ == '__main__':
    main()
