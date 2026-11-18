import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('beta_release', Path(__file__).parents[1] / 'beta_release.py')
beta = importlib.util.module_from_spec(spec)
spec.loader.exec_module(beta)


class BetaReleaseTests(unittest.TestCase):
    def test_push_pr_tag_and_ordinary_manual_checks_keep_source_version_and_never_publish(self):
        for base in ['0.6.1', '0.7.0-beta.1']:
            for event, ref in [('push', 'refs/heads/dev'), ('push', 'refs/heads/main'),
                               ('pull_request', 'refs/pull/5/merge'), ('workflow_dispatch', 'refs/heads/dev'),
                               ('push', 'refs/tags/v0.7.0-beta.1')]:
                with self.subTest(base=base,event=event,ref=ref):
                    self.assertEqual(beta.release_plan(base,event,ref,'87'),(base,''))

    def plan(self, **overrides):
        values = dict(base='0.6.1', event='workflow_dispatch', ref='refs/heads/dev',run_number='87',
                      publish_requested=True,target_version='0.6.2',stable_version='v0.6.1')
        values.update(overrides)
        return beta.release_plan(**values)

    def test_explicit_manual_run_versions_packages_without_bumping_source(self):
        self.assertEqual(self.plan(),('0.6.2-beta.87','beta'))
        self.assertEqual(self.plan(target_version='0.7.0',run_number='88'),('0.7.0-beta.88','beta'))
        self.assertEqual(self.plan(target_version='0.10.0',stable_version='0.9.0'),('0.10.0-beta.87','beta'))

    def test_publication_requires_manual_dev_even_when_flag_is_set(self):
        for event,ref in [('push','refs/heads/dev'),('pull_request','refs/heads/dev'),
                          ('workflow_dispatch','refs/heads/main'),('workflow_dispatch','refs/tags/beta')]:
            with self.subTest(event=event,ref=ref),self.assertRaisesRegex(ValueError,'manual.*dev'):
                self.plan(event=event,ref=ref)

    def test_equal_older_and_source_regressing_targets_are_rejected(self):
        for target in ['0.6.1','0.6.0','0.5.9']:
            with self.subTest(target=target),self.assertRaisesRegex(ValueError,'must be newer than stable 0.6.1'):
                self.plan(target_version=target)
        with self.assertRaisesRegex(ValueError,'older than source'):
            self.plan(base='0.7.0-beta.1')

    def test_invalid_versions_targets_and_run_numbers_fail_closed(self):
        for overrides in [{'base':'bad'},{'base':'0.6.0-rc.1'},{'target_version':''},
                          {'target_version':'0.06.2'},{'target_version':'0.6.2-beta.1'},
                          {'target_version':'0.6.2\ntag=beta'},{'stable_version':'0.6.1-beta.1'},
                          {'stable_version':''},{'run_number':'0'},{'run_number':'../x'}]:
            with self.subTest(overrides=overrides),self.assertRaises(ValueError):
                self.plan(**overrides)

    def test_reused_artifacts_cannot_bypass_full_publication_checks(self):
        with self.assertRaisesRegex(ValueError,'Clear linux_artifact_run_id'):
            self.plan(artifact_run_id='12345')

    def test_ordinary_checks_do_not_query_release_api(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            self.fixture(root)
            for env in [dict(GITHUB_EVENT_NAME='push',GITHUB_REF='refs/heads/dev'),
                        dict(GITHUB_EVENT_NAME='workflow_dispatch',GITHUB_REF='refs/heads/dev',
                             ENCORE_PUBLISH_BETA='false',ENCORE_LINUX_ARTIFACT_RUN_ID='12345')]:
                with patch.object(beta,'latest_stable_version') as latest:
                    self.assertEqual(beta.environment_plan(root,env),('0.6.0-beta.1',''))
                    latest.assert_not_called()

    def test_live_stable_lookup_rejects_unpublished_or_nonstable_tags(self):
        with patch.object(beta,'gh',return_value=json.dumps({'tag_name':'v0.6.1','draft':False,'prerelease':False})):
            self.assertEqual(beta.latest_stable_version('example/project'),'0.6.1')
        for data in [{'tag_name':'v0.7.0-beta.1','draft':False,'prerelease':False},
                     {'tag_name':'v0.6.1','draft':True,'prerelease':False},
                     {'tag_name':'v0.6.1','draft':False,'prerelease':True},{}]:
            with patch.object(beta,'gh',return_value=json.dumps(data)),self.assertRaises(ValueError):
                beta.latest_stable_version('example/project')

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

    def stage(self,root):
        artifacts=root/'target/beta-artifact'
        for relative in ['ubuntu/Encore_0.6.2-beta.87_amd64.deb','macos/Encore_0.6.2-beta.87_aarch64.dmg',
                         'flatpak/Encore-linux-x86_64-dev.flatpak']:
            path=artifacts/relative
            path.parent.mkdir(parents=True,exist_ok=True)
            path.write_bytes(b'verified fixture package')
        (root/'docs/releases').mkdir(parents=True)
        (root/'docs/linux-beta-testing.md').write_text('Testing guide')
        (root/'docs/releases/macos-linux-beta.md').write_text('Development preview')
        with patch.object(beta.subprocess,'check_output',side_effect=['0.6.2-beta.87\n','amd64\n']):
            beta.prepare_assets(root,'0.6.2-beta.87','a'*40,'1000')

    def publication_env(self):
        return dict(GITHUB_EVENT_NAME='workflow_dispatch',GITHUB_REF='refs/heads/dev',GITHUB_RUN_NUMBER='87',
                    GITHUB_SHA='a'*40,GITHUB_RUN_ID='1000',GH_REPO='example/project',
                    ENCORE_PUBLISH_BETA='true',ENCORE_BETA_TARGET='0.6.2',ENCORE_LINUX_ARTIFACT_RUN_ID='')

    def test_prepared_assets_have_verified_provenance_and_checksums(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            self.stage(root)
            beta.verify_staged_assets(root,'0.6.2-beta.87','a'*40,'1000')
            with self.assertRaisesRegex(ValueError,'do not match'):
                beta.verify_staged_assets(root,'0.6.2-beta.87','b'*40,'1000')
            (root/'target/beta-release/Encore-Linux-x86_64.flatpak').write_bytes(b'tampered')
            with self.assertRaisesRegex(ValueError,'checksum mismatch'):
                beta.verify_staged_assets(root,'0.6.2-beta.87','a'*40,'1000')

    def test_publisher_requires_manual_authorization_before_any_remote_command(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            self.fixture(root)
            for event,ref,flag in [('push','refs/heads/dev','false'),('push','refs/heads/dev','true'),
                                   ('workflow_dispatch','refs/heads/dev','false'),
                                   ('workflow_dispatch','refs/heads/main','true')]:
                env=self.publication_env()|dict(GITHUB_EVENT_NAME=event,GITHUB_REF=ref,ENCORE_PUBLISH_BETA=flag)
                with patch.dict(os.environ,env),patch.object(beta,'gh') as remote,\
                     patch.object(beta.subprocess,'run') as process,self.assertRaises(ValueError):
                    beta.publish(root,'beta','0.6.2-beta.87','a'*40)
                remote.assert_not_called()
                process.assert_not_called()

    def test_valid_manual_publication_reaches_prerelease_creation_without_stable_feed(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            self.fixture(root)
            self.stage(root)
            with patch.dict(os.environ,self.publication_env()),\
                 patch.object(beta,'latest_stable_version',return_value='0.6.1'),\
                 patch.object(beta,'gh') as remote,\
                 patch.object(beta.subprocess,'run',return_value=subprocess.CompletedProcess([],1,'','release not found')):
                beta.publish(root,'beta','0.6.2-beta.87','a'*40)
            calls=[call.args for call in remote.call_args_list]
            self.assertEqual(calls[0][:3],('release','create','beta'))
            self.assertIn('--prerelease',calls[0])
            self.assertIn('--latest=false',calls[0])
            self.assertEqual(calls[-2],('api','--method','PATCH','repos/example/project/git/refs/tags/beta',
                                       '-f','sha='+'a'*40,'-F','force=true'))
            self.assertFalse(any('latest.json' in str(value) for call in calls for value in call))
            self.assertEqual(calls[-1],('release','edit','beta','--draft=false','--prerelease','--latest=false'))

    def test_stable_promotion_during_build_blocks_publication(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            self.fixture(root)
            self.stage(root)
            with patch.dict(os.environ,self.publication_env()),\
                 patch.object(beta,'latest_stable_version',return_value='0.6.2'),\
                 patch.object(beta,'gh') as remote,self.assertRaisesRegex(ValueError,'must be newer'):
                beta.publish(root,'beta','0.6.2-beta.87','a'*40)
            remote.assert_not_called()

    def test_existing_rolling_release_updates_but_stable_release_is_protected(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            self.fixture(root)
            self.stage(root)
            for prerelease in [True,False]:
                result=subprocess.CompletedProcess([],0,json.dumps({'isPrerelease':prerelease}),'')
                with patch.dict(os.environ,self.publication_env()),\
                     patch.object(beta,'latest_stable_version',return_value='0.6.1'),\
                     patch.object(beta,'gh') as remote,\
                     patch.object(beta.subprocess,'run',return_value=result):
                    if not prerelease:
                        with self.assertRaisesRegex(ValueError,'Only the rolling beta'):
                            beta.publish(root,'beta','0.6.2-beta.87','a'*40)
                        remote.assert_not_called()
                    else:
                        beta.publish(root,'beta','0.6.2-beta.87','a'*40)
                        calls=[call.args for call in remote.call_args_list]
                        self.assertEqual(calls[0],('release','edit','beta','--draft=true','--prerelease','--latest=false'))
                        self.assertEqual(calls[1][:3],('release','upload','beta'))
                        self.assertEqual(calls[-2][3],'repos/example/project/git/refs/tags/beta')
                        self.assertEqual(calls[-1],('release','edit','beta','--draft=false','--prerelease','--latest=false'))

    def test_incomplete_or_tampered_staging_never_mutates_a_release(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            self.fixture(root)
            self.stage(root)
            (root/'target/beta-release/BUILD.json').unlink()
            with patch.dict(os.environ,self.publication_env()),\
                 patch.object(beta,'latest_stable_version',return_value='0.6.1'),\
                 patch.object(beta,'gh') as remote,self.assertRaisesRegex(ValueError,'missing or incomplete'):
                beta.publish(root,'beta','0.6.2-beta.87','a'*40)
            remote.assert_not_called()


if __name__ == '__main__':
    unittest.main()
