# Agent guidance

This file applies to the whole repository. Follow the user's task and any more
specific `AGENTS.md` in the directory you change. Keep these documents aligned
with the code when architecture, commands, or conventions change.

## Personal project policy

Develop this project as the owner's personal, independently maintained version.
Personal use does not require private GitHub repository visibility. Do not
contribute changes, open issues or pull requests, push branches, or synchronize
work with the original parent project or other forks. Preserve the existing
license and attribution.

The intended repository is `femboyvn/desktop-tui`. Use only its owner-approved
remote for any authorized push or review. Before pushing, verify the destination
owner/repository and that it is independent of the original parent's fork network.
Remote names and local configuration do not establish those server-side properties.
Keep work local if the destination cannot be verified. Do not change repository
visibility as part of routine development. Create an issue or pull request only
when the user requests one.

## Read before working

1. Read [CODEBASE.md](CODEBASE.md) for setup, module ownership, and runtime flow.
2. Read [CODING_CONVENTION.md](CODING_CONVENTION.md) before editing Rust or TOML.
3. Follow [HOW_TO_IMPLEMENT_A_FUNCTION.md](HOW_TO_IMPLEMENT_A_FUNCTION.md) for a
   function, feature, or bug fix.
4. Read [README.md](README.md) and the relevant source and sample shortcuts for
   user-facing behavior. The source and Cargo configuration are authoritative
   when prose has drifted; update the affected documentation with the change.
5. Read [CONTRIBUTE.md](CONTRIBUTE.md) for personal development setup, project
   handoffs, and review expectations.
6. Use [HOW_TO_RUN.md](HOW_TO_RUN.md) for build/run instructions and
   [HOW_TO_TEST.md](HOW_TO_TEST.md) for automated and manual verification.

## Repository skills

Skills live in `.agents/skills/`, the repository location described in the
[official OpenAI skills documentation](https://learn.chatgpt.com/docs/build-skills).
Load only the skill relevant to the task; these skills complement the guides
above and do not grant permission to change the host or publish work.

| Skill | When to use |
| --- | --- |
| [desktop-tui-brainstorming](.agents/skills/desktop-tui-brainstorming/SKILL.md) | Explore a feature, compatibility choice, or substantial behavior change before implementation. |
| [desktop-tui-testing](.agents/skills/desktop-tui-testing/SKILL.md) | Select tests, reproduce a regression, or verify a Rust/UI change. |
| [desktop-tui-os-integration](.agents/skills/desktop-tui-os-integration/SKILL.md) | Work on Unix PTYs, process lifecycle, config/path behavior, installation, or OS launch integration. |

In Codex, invoke a skill explicitly with `$desktop-tui-brainstorming`,
`$desktop-tui-testing`, or `$desktop-tui-os-integration`; matching tasks can also
select them automatically. If they do not appear, restart the session. In an
agent host without local skill discovery, open the relevant `SKILL.md` directly.

## Working rules

- This is an experimental Rust 2024 binary using AppCUI and a virtual terminal.
  Extend the existing modules and event handlers; keep changes focused on the
  requested behavior.
- State the intended behavior and verification before a substantial change.
  Resolve routine implementation choices from the existing code. Ask for missing
  requirements only when they materially affect the result.
- Preserve shortcut compatibility unless the task calls for a schema change.
  Update the README example and affected `shortcuts/*.toml` when the schema changes.
- `rustfmt.toml` deliberately disables formatting. Match nearby style manually;
  avoid automatic format/import rewrites and unrelated cleanup.
- Keep UI callbacks responsive. Review child-process shutdown, channel handling,
  surface bounds, and terminal size together when touching window behavior.
- Do not run the interactive desktop as an unattended check. It owns the terminal
  and can launch commands defined in shortcuts. Use a real terminal and explicit
  sample configuration for manual checks.
- If delegating, give agents distinct file ownership or read-only research tasks.
  Coordinate shared interfaces first. One agent manages branch, index, and commits;
  review the combined diff before finishing.

## Git workflow

1. Inspect `git status --short --branch`, `git diff`, `git diff --cached`, and
   `git log -5 --oneline` before editing. Record existing changes and keep them
   out of your patch. Never discard, stash, or overwrite another developer's
   work to obtain a clean tree.
2. Use a short-lived branch from the agreed base, such as `docs/agent-onboarding`,
   `feat/terminal-search`, or `fix/resize-bounds`. Inspect the current branch first;
   reuse the task branch or a platform-managed worktree when one already exists.
   Use separate worktrees for concurrent edits that need isolation. If Git writes
   are restricted, prepare the patch in place and report the limitation.
3. Keep each commit about one logical change. Do not mix behavior changes with
   broad formatting, dependency upgrades, or generated files. `Cargo.lock` is
   tracked: include intentional dependency resolution changes alongside the
   manifest; use `--locked` for routine builds and checks.
4. Review new files as well as tracked diffs. When a commit is requested, stage
   explicit paths or selected hunks, then inspect `git diff --cached` and
   `git diff --cached --check`. Avoid blanket staging in a shared worktree.
5. Use a concise imperative commit subject. New work should use a type prefix
   such as `docs: add agent onboarding guides`, `fix: preserve terminal bounds`,
   or `feat: add shortcut option`. Historical subjects vary; these prefixes are
   the convention for new work. Explain a non-obvious reason in the body.
6. Avoid rewriting published history. Do not force-push, reset destructively,
   clean untracked files, amend another person's commit, or rebase shared work
   without explicit authorization. Integrate only against the agreed local or
   approved project base; resolve conflicts deliberately and repeat affected
   checks. Do not fetch or merge from the original parent as a routine sync step.
7. Leave a reviewable patch by default. Commit, push, merge, publish, or tag only
   when requested or covered by the agreed project workflow. Apply the personal
   project policy before any push or external write. A handoff or a
   user-requested project review should state the problem, resulting behavior,
   validation, and any remaining limitations; a pull request is optional.

## Verification

Run commands from the repository root. For Rust changes, use:

```sh
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets
cargo fmt --all -- --check
git diff --check
```

Run targeted unit tests during development, then the full test command. Desktop
input regression coverage lives in `src/ui/desktop/view.rs`; a zero-test filtered run
proves compilation of the test target only. Add focused regression tests for
changed logic. See the implementation guide for test placement and manual UI checks.

Clippy may report pre-existing warnings; distinguish those from warnings introduced
by the patch. Do not silently suppress diagnostics or impose `-D warnings` as an
existing project gate. The formatting command checks the configured tool but does
not enforce style while `disable_all_formatting = true` is set.

For documentation-only changes, inspect the full text and new-file contents,
verify local links and referenced paths, and check whitespace. Compilation is
needed only if code/configuration changed or a documented command needs validating.
There is currently no CI workflow or Markdown linter configured.

If dependencies cannot be fetched, use `--offline` only when they are cached.
Report a blocked check with its command and reason; do not count it as passing.
Changes to rendering, keyboard input, window/process lifecycle, or dialogs also
need the relevant real-terminal smoke checks from the implementation guide.

## Handoff

Before finishing, review `git status --short` and the complete patch, including
untracked files. Report what changed, commands actually run and their outcomes,
manual checks performed or still needed, and any known limitation. State whether
the work is committed. Keep the guidance files current when their facts change.
