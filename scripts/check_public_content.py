"""Check Git snapshots and publication text without printing matched values."""
from __future__ import annotations

import argparse
import io
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys


PRIVATE_NAMES = {
    "agents.md", "claude.md", "handoff.md", "design.md", "backlog.md",
    "roadmap.md", "feature-notes.md",
}
PRIVATE_DIRS = {".local-notes", ".private", ".local-backups", ".codex", ".agents", ".vscode"}
GENERIC_USERS = {"test", "example", "user", "username", "public", "default", "runner", "runneradmin"}
HOME_PATH = re.compile(r"(?:[A-Za-z]:[\\/]+Users[\\/]+|/(?:Users|home)/)([^\\/\s\"'<>]+)")
TEXT_RULES = {
    "private key material": re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH |DSA |ENCRYPTED )?PRIVATE KEY-----"),
    "signing secret material": re.compile(r"untrusted\s+comment:.*(?:encrypted )?secret key", re.I),
    "GitHub token": re.compile(r"\b(?:github_pat_[A-Za-z0-9_]{20,}|gh[pousr]_[A-Za-z0-9]{30,})\b"),
    "cloud access key": re.compile(r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b"),
    "API token": re.compile(r"\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{30,}"),
    "messaging token": re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{20,}"),
    "private chat link": re.compile(r"(?:codex|plugin)://|https://chatgpt\.com/c/", re.I),
    "session note": re.compile(r"\b(?:for\s+this\s+local\s+development\s+session|local\s+backup:|pending\s+from\s+the\s+user)(?=\s|$)", re.I),
    "obsolete task branch": re.compile(r"\bcodex/(?:dev|release)-[\w./-]+"),
    "unpublished feature note": re.compile(r"\b(?:later|future|planned)\s+editor\s+features\b", re.I),
    "private planning heading": re.compile(r"^#{1,6}\s+(?:(?:internal|private)\s+(?:notes|planning)|(?:feature\s+)?backlog|roadmap|future\s+features)\b", re.I | re.M),
    "credential storage detail": re.compile(r"(?:local|signing|updater|private)\s+key\s+(?:has\s+no\s+password|is\s+(?:unencrypted|passwordless))", re.I),
    "local credential location": re.compile(r"\.local-backups[/\\]+updater[/\\]+[\w.-]+\.key\b", re.I),
}


def git(root: Path, *args: str, data: bytes | None = None) -> bytes:
    return subprocess.run(["git", *args], cwd=root, input=data, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, check=True).stdout


def private_path(path: str) -> bool:
    parts = PurePosixPath(path.lower()).parts
    name = parts[-1]
    return (name in PRIVATE_NAMES or any(part in PRIVATE_DIRS for part in parts)
            or name == ".env" or name.startswith(".env.")
            or PurePosixPath(name).suffix in {".key", ".pem", ".p12", ".pfx"}
            or any(part == "brag-output" or part.startswith("brag-output-") for part in parts)
            or (len(parts) == 1 and name.endswith(".mp3")))


def inspect_text(label: str, text: str) -> list[str]:
    findings = []
    for rule, pattern in TEXT_RULES.items():
        for match in pattern.finditer(text):
            findings.append(f"{label}:{text.count(chr(10), 0, match.start()) + 1}: {rule}")
    for match in HOME_PATH.finditer(text):
        if match.group(1).lower() not in GENERIC_USERS:
            findings.append(f"{label}:{text.count(chr(10), 0, match.start()) + 1}: personal home path")
    return findings


def inspect_file(path: str, data: bytes) -> list[str]:
    findings = [f"{path}: private or generated file"] if private_path(path) else []
    if data.startswith((b"\xff\xfe", b"\xfe\xff")):
        text = data.decode("utf-16", errors="replace")
    elif b"\0" in data:
        return findings
    else:
        text = data.decode("utf-8-sig", errors="replace")
    return findings + inspect_text(path, text)


def snapshot_entries(root: Path, ref: str | None) -> list[tuple[str, str]]:
    args = ("ls-tree", "-r", "-z", "--full-tree", ref) if ref else ("ls-files", "--stage", "-z")
    entries = []
    for row in git(root, *args).split(b"\0"):
        if not row:
            continue
        metadata, path = row.split(b"\t", 1)
        mode, oid_or_type, stage_or_oid = metadata.split()
        if mode == b"160000":
            continue
        if ref:
            oid = stage_or_oid
        else:
            if stage_or_oid != b"0":
                raise ValueError("Resolve Git conflicts before checking publication content.")
            oid = oid_or_type
        entries.append((path.decode("utf-8", errors="surrogateescape"), oid.decode()))
    return entries


def inspect_snapshot(root: Path, ref: str | None) -> list[str]:
    entries = snapshot_entries(root, ref)
    if not entries:
        return []
    payload = "".join(oid + "\n" for _, oid in entries).encode()
    stream = io.BytesIO(git(root, "cat-file", "--batch", data=payload))
    findings = []
    for path, _ in entries:
        _, kind, length = stream.readline().split()
        if kind != b"blob":
            raise ValueError("Expected a file blob in the Git snapshot.")
        data = stream.read(int(length))
        stream.read(1)
        findings.extend(inspect_file(path, data))
    return findings


def pushed_commits(root: Path, lines: list[str]) -> list[str]:
    commits = set()
    for line in lines:
        _, local_sha, _, _ = line.split()
        if set(local_sha) == {"0"}:  # A branch deletion introduces no content.
            continue
        # Check every outgoing commit, including files removed again before HEAD.
        outgoing = git(root, "rev-list", local_sha, "--not", "--remotes=origin").decode().splitlines()
        commits.update(outgoing or [local_sha])
    return sorted(commits)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--staged", action="store_true", help="Inspect the complete Git index.")
    mode.add_argument("--ref", help="Inspect a commit or branch tree.")
    mode.add_argument("--pre-push", action="store_true", help="Read Git pre-push refs from stdin.")
    mode.add_argument("--text-file", type=Path, help="Inspect a PR or release description before publishing.")
    parser.add_argument("--event", type=Path, help="Also inspect a GitHub pull request event's title and body.")
    args = parser.parse_args()
    try:
        if args.text_file:
            findings = inspect_text("publication-text", args.text_file.read_text(encoding="utf-8-sig"))
        else:
            root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
            refs = pushed_commits(root, sys.stdin.read().splitlines()) if args.pre_push else [args.ref]
            findings = []
            for ref in refs:
                findings.extend(inspect_snapshot(root, ref))
        if args.event:
            pr = json.loads(args.event.read_text(encoding="utf-8")).get("pull_request", {})
            findings.extend(inspect_text("pull-request", (pr.get("title") or "") + "\n" + (pr.get("body") or "")))
    except (OSError, ValueError, subprocess.CalledProcessError):
        print("Public content check could not complete; publication is blocked.", file=sys.stderr)
        return 2
    for finding in sorted(set(findings)):
        print(finding, file=sys.stderr)
    if findings:
        print("Keep local planning and credentials outside published Git content. See CONTRIBUTING.md.", file=sys.stderr)
        return 1
    print("Public content check passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
