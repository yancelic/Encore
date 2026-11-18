"""Explicitly publish verified development previews without changing the stable feed."""
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
CORE = r'(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)'
BETA = re.compile(rf'({CORE})-beta\.([1-9]\d*)')
VERSION = re.compile(rf'({CORE})(?:-beta\.(?:0|[1-9]\d*))?')


def publication_context(event, ref, artifact_run_id=''):
    if event != 'workflow_dispatch' or ref != 'refs/heads/dev':
        raise ValueError('Beta publication requires an explicit manual run from the dev branch')
    if artifact_run_id:
        raise ValueError('Clear linux_artifact_run_id to publish: all tests and packages must run in this workflow')


def release_plan(base, event, ref, run_number, *, publish_requested=False,
                 target_version='', stable_version='', artifact_run_id=''):
    match = VERSION.fullmatch(base)
    if not match:
        raise ValueError(f'Invalid source version: {base}')
    if not publish_requested:
        return base, ''
    publication_context(event, ref, artifact_run_id)
    if not re.fullmatch(CORE, target_version):
        raise ValueError('Set beta_target to a numeric X.Y.Z target, for example 0.6.2 (without a beta suffix)')
    stable = stable_version.removeprefix('v')
    if not re.fullmatch(CORE, stable):
        raise ValueError('Could not determine a valid latest stable release; publication is blocked')
    target_parts = tuple(map(int, target_version.split('.')))
    if target_parts <= tuple(map(int, stable.split('.'))):
        raise ValueError(f'Beta target {target_version} must be newer than stable {stable}; choose a higher X.Y.Z target')
    if target_parts < tuple(map(int, match[1].split('.'))):
        raise ValueError(f'Beta target {target_version} cannot be older than source version {match[1]}')
    if not re.fullmatch(r'[1-9]\d*', str(run_number)):
        raise ValueError('A positive workflow run number is required')
    return f'{target_version}-beta.{run_number}', 'beta'


def latest_stable_version(repo):
    value = json.loads(gh('api', f'repos/{repo}/releases/latest', capture=True))
    if value.get('draft') is not False or value.get('prerelease') is not False:
        raise ValueError('Latest release is not a published stable release; publication is blocked')
    tag = value.get('tag_name', '')
    if not re.fullmatch('v?' + CORE, tag):
        raise ValueError('Latest stable tag must use X.Y.Z or vX.Y.Z')
    return tag.removeprefix('v')


def environment_plan(root, environment):
    base = json.loads((root / 'apps/ui/src-tauri/tauri.conf.json').read_text(encoding='utf-8'))['version']
    flag = environment.get('ENCORE_PUBLISH_BETA', 'false')
    if flag not in ('', 'true', 'false'):
        raise ValueError('ENCORE_PUBLISH_BETA must be true or false')
    requested = flag == 'true'
    event, ref = environment.get('GITHUB_EVENT_NAME', ''), environment.get('GITHUB_REF', '')
    reuse = environment.get('ENCORE_LINUX_ARTIFACT_RUN_ID', '')
    stable = ''
    if requested:
        publication_context(event, ref, reuse)
        stable = latest_stable_version(environment['GH_REPO'])
    return release_plan(base, event, ref, environment.get('GITHUB_RUN_NUMBER', ''),
                        publish_requested=requested, target_version=environment.get('ENCORE_BETA_TARGET', ''),
                        stable_version=stable, artifact_run_id=reuse)


def apply_version(root, version):
    if not VERSION.fullmatch(version):
        raise ValueError(f'Invalid package version: {version}')
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


def verify_staged_assets(root, version, sha, run_id):
    output = root / 'target/beta-release'
    expected = {'Encore-Ubuntu-amd64.deb', 'Encore-macOS-Apple-Silicon.dmg',
                'Encore-Linux-x86_64.flatpak', 'LINUX-TESTING.md', 'BUILD.json', 'SHA256SUMS.txt'}
    if not output.is_dir() or {p.name for p in output.iterdir()} != expected:
        raise ValueError('Verified release staging is missing or incomplete; run package preparation first')
    metadata = json.loads((output / 'BUILD.json').read_text(encoding='utf-8'))
    if (metadata.get('version'), metadata.get('source_commit'), str(metadata.get('workflow_run_id'))) != (version, sha, str(run_id)):
        raise ValueError('Staged packages do not match this manual run, source commit and target version')
    entries = {}
    for line in (output / 'SHA256SUMS.txt').read_text(encoding='utf-8').splitlines():
        digest, separator, name = line.partition('  ')
        if not separator or not re.fullmatch(r'[0-9a-f]{64}', digest) or name in entries:
            raise ValueError('Invalid staged checksum manifest')
        entries[name] = digest
    if set(entries) != expected - {'SHA256SUMS.txt'}:
        raise ValueError('Staged checksum manifest does not cover the verified packages')
    for name, digest in entries.items():
        if hashlib.sha256((output / name).read_bytes()).hexdigest() != digest:
            raise ValueError(f'Staged asset checksum mismatch: {name}')
    if not (root / 'target/beta-release-notes.md').is_file():
        raise ValueError('Prepared release notes are missing')


def publish(root, tag, version, sha):
    # Recheck authorization and the live stable release before any remote mutation.
    planned_version, planned_tag = environment_plan(root, os.environ)
    if tag != 'beta' or (version, tag) != (planned_version, planned_tag):
        raise ValueError('Publication requires the version from an explicitly selected manual beta run')
    if not BETA.fullmatch(version) or not re.fullmatch(r'[0-9a-f]{40}', sha):
        raise ValueError('Invalid release metadata')
    if sha != os.environ.get('GITHUB_SHA'):
        raise ValueError('Publication source does not match this workflow run')
    verify_staged_assets(root, version, sha, os.environ['GITHUB_RUN_ID'])
    repo = os.environ['GH_REPO']
    title = f'Encore {version} — Development previews'
    notes = str(root / 'target/beta-release-notes.md')
    assets = [str(path) for path in sorted((root / 'target/beta-release').iterdir())]
    existing = subprocess.run(['gh', 'release', 'view', tag, '--json', 'isPrerelease'],
                              text=True, capture_output=True)
    if existing.returncode == 0:
        if tag != 'beta' or not json.loads(existing.stdout)['isPrerelease']:
            raise ValueError('Only the rolling beta prerelease may be updated')
        gh('release', 'edit', tag, '--draft=true', '--prerelease', '--latest=false')
        gh('release', 'upload', tag, *assets, '--clobber')
        gh('release', 'edit', tag, '--title', title, '--notes-file', notes, '--target', sha)
    else:
        if 'release not found' not in existing.stderr.lower() and '404' not in existing.stderr:
            raise RuntimeError(existing.stderr)
        gh('release', 'create', tag, *assets, '--target', sha, '--draft', '--prerelease',
           '--latest=false', '--title', title, '--notes-file', notes)
    # Also move a retained tag when recreating a previously removed release.
    # beta is an explicitly rolling channel; numbered release tags stay fixed.
    gh('api', '--method', 'PATCH', f'repos/{repo}/git/refs/tags/beta',
       '-f', f'sha={sha}', '-F', 'force=true')
    gh('release', 'edit', tag, '--draft=false', '--prerelease', '--latest=false')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['plan', 'apply', 'prepare', 'publish'])
    args = parser.parse_args()
    if args.command == 'plan':
        version, tag = environment_plan(ROOT, os.environ)
        print(f'version={version}\ntag={tag}')
    elif args.command == 'apply':
        apply_version(ROOT, os.environ['ENCORE_BETA_VERSION'])
    elif args.command == 'prepare':
        prepare_assets(ROOT, os.environ['ENCORE_BETA_VERSION'], os.environ['GITHUB_SHA'], os.environ['GITHUB_RUN_ID'])
    else:
        publish(ROOT, os.environ['ENCORE_BETA_TAG'], os.environ['ENCORE_BETA_VERSION'], os.environ['GITHUB_SHA'])


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, subprocess.CalledProcessError) as error:
        raise SystemExit(f'Beta publication error: {error}')
