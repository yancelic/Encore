"""Code-signing identity shared by the macOS packaging scripts.

macOS privacy permissions (TCC) remember an app by its designated requirement.
With an ad-hoc signature that requirement is the binary hash, so every rebuild is
treated as a new, unknown app and previously granted Screen Recording, Microphone
and Input Monitoring access no longer applies. A certificate-backed signature makes
the requirement "bundle identifier + certificate", which survives rebuilds.

Resolution order:
1. ENCORE_CODESIGN_IDENTITY, or Tauri's APPLE_SIGNING_IDENTITY (release builds).
2. The local development certificate created by setup-macos-dev-signing.sh.
3. Ad-hoc ("-"), with a warning that permissions reset on every build.
"""
import os
import re
import subprocess

DEV_IDENTITY_NAME = 'Encore Local Development'
_cached = None


def identity():
    global _cached
    if _cached is not None:
        return _cached
    explicit = os.environ.get('ENCORE_CODESIGN_IDENTITY') or os.environ.get('APPLE_SIGNING_IDENTITY')
    if explicit:
        _cached = explicit
        return _cached
    listing = subprocess.run(['security', 'find-identity', '-p', 'codesigning'],
                             capture_output=True, text=True).stdout
    for line in listing.splitlines():
        match = re.search(r'\)\s+([0-9A-F]{40})\s+"([^"]+)"', line)
        if match and match.group(2) == DEV_IDENTITY_NAME:
            _cached = match.group(1)
            print(f'Signing with "{DEV_IDENTITY_NAME}" ({_cached[:8]}…); macOS permissions persist across builds.')
            return _cached
    print('warning: no stable signing identity found; using an ad-hoc signature. macOS will ask for\n'
          '         Screen Recording, Microphone and Input Monitoring again after every build.\n'
          '         Run apps/ui/scripts/setup-macos-dev-signing.sh once to keep permissions.')
    _cached = '-'
    return _cached


def sign(path, entitlements=None, preserve_entitlements=False):
    command = ['codesign', '--force', '--sign', identity()]
    if identity() != '-':
        # Self-signed and Developer ID builds made offline must not wait on Apple's timestamp service.
        command.append('--timestamp=none')
    if entitlements:
        command += ['--entitlements', str(entitlements)]
    elif preserve_entitlements:
        command.append('--preserve-metadata=entitlements')
    subprocess.check_call([*command, str(path)])
