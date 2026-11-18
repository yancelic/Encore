import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest


CHECKER_PATH = Path(__file__).resolve().parents[1] / "check_public_content.py"
spec = importlib.util.spec_from_file_location("public_content", CHECKER_PATH)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


def personal_path():
    return "/" + "Users/" + "private-person/project"


class ContentTests(unittest.TestCase):
    def test_rejects_local_and_credential_files_even_with_binary_contents(self):
        for path in [".local-notes/ideas.md", "docs/feature-notes.md", "AGENTS.md",
                     "subdir/HANDOFF.md", ".env.local", "release.key", "certificate.pfx",
                     ".vscode/settings.json", "brag-output-next/clip.json"]:
            with self.subTest(path=path):
                self.assertTrue(checker.inspect_file(path, b"\x00\x01\x02"))

    def test_allows_public_keys_signatures_and_source_assets(self):
        for path in ["release.key.pub", "installer.exe.sig", "apps/ui/src/assets/logo.svg",
                     "docs/updates.md", "scripts/check_public_content.py"]:
            self.assertFalse(checker.private_path(path), path)

    def test_detects_secret_material_without_printing_it(self):
        token = "gh" + "p_" + "a" * 36
        findings = checker.inspect_text("description", token)
        self.assertTrue(findings)
        self.assertNotIn(token, "\n".join(findings))
        header = "-" * 5 + "BEGIN " + "PRIVATE KEY" + "-" * 5
        self.assertTrue(checker.inspect_text("file", header))

    def test_detects_personal_home_paths_but_allows_generic_fixtures(self):
        for path in [personal_path(), "C:" + "\\Users\\" + "private-person\\project",
                     "/" + "home/" + "private-person/project"]:
            self.assertTrue(checker.inspect_text("fixture", path))
        for path in ["/Users/test/project", r"C:\Users\example\project", "/home/runner/work"]:
            self.assertFalse(checker.inspect_text("fixture", path))

    def test_reports_line_numbers_without_echoing_private_values(self):
        findings = checker.inspect_text("fixture", "line one\n" + personal_path())
        self.assertEqual(findings, ["fixture:2: personal home path"])

    def test_handles_utf16_and_binary_assets(self):
        self.assertTrue(checker.inspect_file("notes.txt", personal_path().encode("utf-16")))
        self.assertFalse(checker.inspect_file("icon.bgra", b"\0\x01\xff"))

    def test_rejects_session_notes_and_unpublished_proposals(self):
        texts = ["Local " + "backup: disk copy", "pending from " + "the user",
                 "later editor " + "features such as effects", "## " + "Feature backlog\nPlans",
                 "codex/" + "release-1.2.3", "https://" + "chatgpt.com/c/example",
                 ".local-backups/" + "updater/" + "secret.key"]
        for text in texts:
            with self.subTest(text=text):
                self.assertTrue(checker.inspect_text("description", text))

    def test_accepts_useful_user_and_developer_documentation(self):
        text = "Known limits: Linux beta needs real-device verification. Build with Rust. "
        text += "TAURI_SIGNING_PRIVATE_KEY is supplied through CI secrets."
        self.assertFalse(checker.inspect_text("documentation", text))


class GitSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.run_git("init", "-q")
        self.run_git("config", "user.name", "Test")
        self.run_git("config", "user.email", "test@example.invalid")
        self.run_git("config", "core.autocrlf", "false")

    def tearDown(self):
        self.directory.cleanup()

    def run_git(self, *args):
        return checker.git(self.root, *args).decode().strip()

    def stage(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
        self.run_git("add", "-f", "--", path)

    def test_checks_index_instead_of_safe_unstaged_replacement(self):
        self.stage("example.txt", personal_path())
        (self.root / "example.txt").write_text("safe unstaged edit", encoding="utf-8")
        self.assertTrue(checker.inspect_snapshot(self.root, None))

    def test_does_not_publish_unstaged_private_edits(self):
        self.stage("example.txt", "safe staged content")
        (self.root / "example.txt").write_text(personal_path(), encoding="utf-8")
        self.assertFalse(checker.inspect_snapshot(self.root, None))

    def test_rejects_force_added_private_file(self):
        self.stage(".gitignore", ".local-notes/\n")
        self.stage(".local-notes/ideas.md", "not public")
        findings = checker.inspect_snapshot(self.root, None)
        self.assertIn(".local-notes/ideas.md: private or generated file", findings)

    def test_scans_committed_tree_and_deleted_staged_file(self):
        self.stage("example.txt", personal_path())
        self.run_git("commit", "-qm", "test fixture")
        self.assertTrue(checker.inspect_snapshot(self.root, "HEAD"))
        self.run_git("rm", "-q", "example.txt")
        self.assertFalse(checker.inspect_snapshot(self.root, None))

    def test_push_checks_bad_intermediate_commit_even_when_head_is_clean(self):
        self.stage("example.txt", "safe")
        self.run_git("commit", "-qm", "baseline")
        baseline = self.run_git("rev-parse", "HEAD")
        self.run_git("update-ref", "refs/remotes/origin/main", baseline)
        self.stage("example.txt", personal_path())
        self.run_git("commit", "-qm", "intermediate")
        unsafe_commit = self.run_git("rev-parse", "HEAD")
        self.stage("example.txt", "safe again")
        self.run_git("commit", "-qm", "final")
        head = self.run_git("rev-parse", "HEAD")
        lines = [f"refs/heads/topic {head} refs/heads/topic {'0' * 40}"]
        outgoing = checker.pushed_commits(self.root, lines)
        self.assertIn(unsafe_commit, outgoing)
        self.assertFalse(checker.inspect_snapshot(self.root, head))
        self.assertTrue(any(checker.inspect_snapshot(self.root, ref) for ref in outgoing))

    def test_branch_deletion_introduces_no_content(self):
        self.assertEqual(checker.pushed_commits(self.root,
                         [f"refs/heads/topic {'0' * 40} refs/heads/topic {'a' * 40}"]), [])

    def test_cli_checks_pr_metadata_without_echoing_values(self):
        self.stage("example.txt", "safe")
        self.run_git("commit", "-qm", "baseline")
        event = self.root / "event.json"
        import json
        event.write_text(json.dumps({"pull_request": {"title": "Update", "body": personal_path()}}))
        result = subprocess.run(["python", str(CHECKER_PATH), "--ref", "HEAD", "--event", str(event)],
                                cwd=self.root, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn("personal home path", result.stderr)
        self.assertNotIn(personal_path(), result.stderr)


if __name__ == "__main__":
    unittest.main()
