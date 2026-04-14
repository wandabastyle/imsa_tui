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

## Verification Matrix

- Rust-only changes must pass:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features`
  - `cargo test`
- Web-only changes must pass (in `web/`):
  - `pnpm run verify`
- Cross-cutting changes (Rust + Web) must pass both Rust and Web checks.

## Commits

- Use conventional-style commit subjects (`fix(...)`, `feat(...)`, `chore(...)`, `docs(...)`, `refactor(...)`, `test(...)`).
- Keep commits scoped: do not mix unrelated changes in the same commit.
- Do not amend commits unless explicitly requested.
- Never commit secrets (`.env`, tokens, credential files).

## Branch and Merge

- Never work directly on `main` for feature/fix/doc changes.
- Before making changes, create or switch to a dedicated branch (for example `feat/...`, `fix/...`, `chore/...`).
- When work is ready, open a pull request and merge through the PR flow only (no direct pushes to `main`).
- Do feature/fix work on a dedicated branch; merge to `main` only after checks pass.
- Before merge, ensure working tree is clean and all required checks are green.
- Do not force-push protected branches.

## Shell Command Compatibility

- When suggesting terminal commands to operators, prefer fish-friendly syntax.
- Avoid bash-only constructs in examples (for example `$(...)`, HEREDOCs, and `&&` chains).
- Prefer simple one-command-per-line sequences that run in both fish and bash when possible.

## Docs and Tests Sync

- If behavior, runtime flags, storage paths, or operational flow changes, update `README.md` in the same branch.
- Keep `README.md` concise for quick-start usage and high-level project context.
- For operator runbooks, deployment variants, reverse-proxy setup, and deeper troubleshooting, update the GitHub Wiki in the same workstream.
- When wiki content changes, keep the local mirror in `docs/wiki/` in sync so wiki updates are reproducible from the repository.
- Decide README vs Wiki by scope: put essential, frequently-needed commands in `README.md`; put detailed procedures and extended references in Wiki pages.
- Wiki publish workflow (after updating `docs/wiki/`):
  1. Ensure wiki git repo exists (create/edit one page in GitHub UI once if needed).
  2. Clone wiki repo locally (for example to `/tmp/imsa_tui.wiki`).
  3. Copy `docs/wiki/*.md` into the cloned wiki repo.
  4. Commit with a docs-scoped message.
  5. Push to `wandabastyle/imsa_tui.wiki.git`.
  6. Verify navigation links/pages render on the GitHub wiki.
- If a tracked phase/plan item changes status, update `docs/TODO.md` in the same branch.
- For behavior changes, add or update regression/unit tests in the same branch.

## Version Bumps

- Keep feature/fix commits separate from version bump commits.
- Bump crate version only when preparing a release.
- Ensure release tag points to the version bump commit.
