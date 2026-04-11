# Agent Rules

## Release Tags

- Always create release tags as annotated tags (`git tag -a`), never lightweight tags.
- Every release tag (`vX.Y.Z`) must include a short description in the tag message.
- Tag message format:
  - first line: version (`vX.Y.Z`)
  - then 2-5 bullets with key changes and any operator-impacting behavior.
- If a lightweight or incomplete release tag is created locally by mistake and has not been pushed,
  delete and recreate it as a fully annotated tag.

## Linting

- After changing any file, run the relevant lint/check commands for affected areas.
- Continue fixing and re-running lint/check commands until all reported errors are resolved.
