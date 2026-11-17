import importlib.util
import json
from pathlib import Path
import tempfile
import tomllib
import unittest

spec = importlib.util.spec_from_file_location('beta_release', Path(__file__).parents[1] / 'beta_release.py')
beta = importlib.util.module_from_spec(spec)
spec.loader.exec_module(beta)


class BetaReleaseTests(unittest.TestCase):
    def test_dev_builds_get_increasing_package_versions_and_a_stable_download_channel(self):
        self.assertEqual(beta.release_plan('0.6.0-beta.1', 'push', 'refs/heads/dev', '87'), ('0.6.0-beta.87', 'beta'))
        self.assertEqual(beta.release_plan('0.6.0-beta.1', 'push', 'refs/heads/dev', '88'), ('0.6.0-beta.88', 'beta'))

    def test_pull_requests_and_manual_sandbox_tests_do_not_publish(self):
        for event, ref in [('pull_request', 'refs/pull/5/merge'), ('workflow_dispatch', 'refs/heads/dev'), ('push', 'refs/heads/main')]:
            self.assertEqual(beta.release_plan('0.6.0-beta.1', event, ref, '87'), ('0.6.0-beta.1', ''))

    def test_numbered_tags_must_match_the_source(self):
        self.assertEqual(beta.release_plan('0.6.0-beta.2', 'push', 'refs/tags/v0.6.0-beta.2', '87'),
                         ('0.6.0-beta.2', 'v0.6.0-beta.2'))
        with self.assertRaises(ValueError):
            beta.release_plan('0.6.0-beta.2', 'push', 'refs/tags/v0.6.0-beta.3', '87')

    def test_invalid_versions_and_run_numbers_are_rejected(self):
        for version, number in [('0.6.0-rc.1', '87'), ('bad', '87'), ('0.6.0-beta.1', '0'), ('0.6.0-beta.1', '../x')]:
            with self.assertRaises(ValueError):
                beta.release_plan(version, 'push', 'refs/heads/dev', number)

    def test_stable_sources_can_be_checked_without_publishing_a_beta_release(self):
        self.assertEqual(beta.release_plan('0.6.0', 'pull_request', 'refs/pull/8/merge', '90'), ('0.6.0', ''))
        self.assertEqual(beta.release_plan('0.6.0', 'push', 'refs/tags/v0.6.0', '90'), ('0.6.0', ''))
        self.assertEqual(beta.release_plan('0.6.0', 'push', 'refs/heads/dev', '90'), ('0.6.0-beta.90', 'beta'))

    def test_promoting_to_stable_updates_only_workspace_versions(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            self.fixture(root)
            beta.apply_version(root, '0.6.0')
            self.assertEqual([p['version'] for p in tomllib.loads((root / 'Cargo.lock').read_text())['package']],
                             ['0.6.0', '0.6.0-beta.1'])
            self.assertEqual(json.loads((root / 'apps/ui/src-tauri/tauri.conf.json').read_text())['version'], '0.6.0')

    def fixture(self, root):
        (root / 'member').mkdir()
        (root / 'member/Cargo.toml').write_text('[package]\nname = "encore-test"\n', encoding='utf-8')
        (root / 'Cargo.toml').write_text('[workspace]\nmembers = ["member"]\n[workspace.package]\nversion = "0.6.0-beta.1"\n', encoding='utf-8')
        (root / 'Cargo.lock').write_text('version = 4\n[[package]]\nname = "encore-test"\nversion = "0.6.0-beta.1"\n\n[[package]]\nname = "third-party"\nversion = "0.6.0-beta.1"\n', encoding='utf-8')
        for relative in ['apps/ui/package.json', 'apps/ui/src-tauri/tauri.conf.json', 'apps/ui/package-lock.json']:
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            value = {'version': '0.6.0-beta.1'}
            if relative.endswith('package-lock.json'):
                value['packages'] = {'': {'version': '0.6.0-beta.1'}, 'node_modules/dependency': {'version': '0.6.0-beta.1'}}
            path.write_text(json.dumps(value), encoding='utf-8')

    def test_package_versions_stay_in_sync_without_changing_third_party_dependencies(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            self.fixture(root)
            beta.apply_version(root, '0.6.0-beta.87')
            self.assertEqual(tomllib.loads((root / 'Cargo.toml').read_text())['workspace']['package']['version'], '0.6.0-beta.87')
            lock = tomllib.loads((root / 'Cargo.lock').read_text())
            self.assertEqual([p['version'] for p in lock['package']], ['0.6.0-beta.87', '0.6.0-beta.1'])
            npm = json.loads((root / 'apps/ui/package-lock.json').read_text())
            self.assertEqual(npm['version'], '0.6.0-beta.87')
            self.assertEqual(npm['packages']['']['version'], '0.6.0-beta.87')
            self.assertEqual(npm['packages']['node_modules/dependency']['version'], '0.6.0-beta.1')

    def test_mismatched_manifest_does_not_partially_rewrite_files(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            self.fixture(root)
            (root / 'apps/ui/package.json').write_text('{"version":"0.5.0"}', encoding='utf-8')
            before = {p: p.read_bytes() for p in root.rglob('*') if p.is_file()}
            with self.assertRaises(ValueError):
                beta.apply_version(root, '0.6.0-beta.87')
            self.assertEqual(before, {p: p.read_bytes() for p in root.rglob('*') if p.is_file()})


if __name__ == '__main__':
    unittest.main()
