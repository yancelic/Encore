# Contributing

Use a topic branch, open a pull request to `dev`, then promote the reviewed
changes through a pull request from `dev` to `main`. Small documentation or
maintenance corrections may go directly to `main` when explicitly authorized.

Use a GitHub noreply address for commit metadata when keeping a personal email
private. Configure it locally for this clone with `git config --local user.email`.

## Publication checks

Enable repository hooks once per clone (Python 3.10 or newer is required):

```sh
git config --local core.hooksPath .githooks
```

The commit hook checks the staged snapshot. The push hook checks outgoing commits,
including content added and removed within the same push. GitHub Actions also
checks the committed tree and pull request description.

```sh
python scripts/check_public_content.py --staged
python scripts/check_public_content.py --text-file description.txt
python -m unittest discover -s scripts/tests -v
```

Keep local planning, session notes and unpublished proposals in `.local-notes/`.
Use generic test identities and paths. Publish current behavior, build and test
instructions, supported platforms, known limitations and reproducible diagnostics.
Keep credentials in secure storage or CI secrets; public verification keys and
update signatures belong in the release tooling.

Review release descriptions and uploaded assets before publishing. Pattern checks
supplement human review and do not detect every possible secret. Hooks must be
enabled in each clone; GitHub checks run after content has reached the server.
Ignoring or deleting a file does not remove it from earlier commits.
