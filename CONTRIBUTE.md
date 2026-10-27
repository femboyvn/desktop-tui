# Contributing

Contributions to Desktop-TUI can include bug fixes, features, shortcut examples,
tests, and documentation. This is an experimental project; keep each change
focused and explain the behavior it improves. Discuss a substantial redesign or
breaking change in an issue before investing in its implementation.

## Find your starting point

- [README.md](README.md): user-facing behavior and shortcut examples.
- [HOW_TO_RUN.md](HOW_TO_RUN.md): build, launch, configuration, and troubleshooting.
- [HOW_TO_TEST.md](HOW_TO_TEST.md): automated checks and manual verification.
- [CODEBASE.md](CODEBASE.md): setup, module ownership, runtime flow, and known limits.
- [CODING_CONVENTION.md](CODING_CONVENTION.md): Rust, TOML, and testing conventions.
- [HOW_TO_IMPLEMENT_A_FUNCTION.md](HOW_TO_IMPLEMENT_A_FUNCTION.md): implementation
  steps, regression tests, and manual terminal checks.
- [AGENTS.md](AGENTS.md): agent instructions, shared-worktree practices, and the
  detailed Git and verification workflow.

## Report a bug or propose a feature

Check existing issues and pull requests for related work first. For a bug, include
the expected and actual behavior, reproduction steps, relevant shortcut TOML,
operating system, terminal, Rust version, and repository revision. Include useful
error output or a screenshot when it clarifies a rendering problem. Remove private
paths, credentials, and unrelated configuration before sharing examples.

For a feature, describe the use case, desired behavior, and any effect on existing
shortcuts, keyboard handling, or window behavior. Explain compatibility changes
explicitly so reviewers can assess them.

## Set up a checkout

Fork and clone the repository if you do not have write access, or use an existing
checkout. Confirm which repository and branch the pull request should target;
do not assume that a remote named `origin` is the upstream project.

Use a Unix-like environment, a Rust toolchain supporting edition 2024, Cargo,
and a native compiler/linker. The child-terminal dependency uses Unix PTYs.
Run these commands from the repository root:

```sh
rustup component add rustfmt clippy
cargo build --locked
cargo run --locked -- --help
```

The first build needs dependency access unless the Cargo cache is populated.
See [CODEBASE.md](CODEBASE.md) for native dependencies and the optional Nix shell.
Example shortcut programs are installed separately. For interactive testing in a
real terminal, use `cargo run --locked -- ./shortcuts` or supply a controlled
shortcut directory containing a program available on your system.

## Make a focused change

1. Inspect `git status --short --branch`, tracked/staged diffs, and recent history.
   Preserve existing work. Start a short-lived branch from the intended base, or
   reuse the branch already dedicated to the task. Names such as `fix/resize-bounds`
   and `docs/shortcut-examples` describe the scope clearly.
2. Read the owning module and follow the linked implementation and coding guides.
   Preserve shortcut compatibility unless changing it is part of the proposal.
   Update user documentation and affected examples alongside behavior changes.
3. Add focused regression tests for changed logic. Keep unrelated refactors,
   dependency upgrades, and formatting changes out of the patch. Preserve the
   tracked `Cargo.lock`; update it with intentional dependency changes only.
4. Review the complete patch, including new files. Stage explicit paths or hunks
   and inspect `git diff --cached` and `git diff --cached --check` before committing.
   Use an imperative subject with a prefix such as `fix:`, `feat:`, or `docs:`.
5. Sync with the target branch before integration and repeat affected checks after
   conflict resolution. Avoid rewriting shared history. Push your task branch and
   open a pull request when the change is ready for review. Agents should follow
   the authorization and handoff rules in `AGENTS.md` before committing or pushing.

## Verify the change

For Rust changes, run:

```sh
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets
cargo fmt --all -- --check
git diff --check
```

There are currently no automated tests or CI workflow. A zero-test success only
confirms that the test target compiles. New unit tests belong in the owning source
file under `#[cfg(test)]`; the package has no library target for integration tests
to import. Test observable behavior and relevant boundaries rather than repeating
the implementation.

`rustfmt.toml` deliberately disables formatting, so a successful format check
does not enforce style. Match nearby formatting manually. Distinguish existing
Clippy warnings from diagnostics introduced by the change; do not suppress warnings
to obtain a clean report. Use `--offline` only when dependencies are cached, and
report unavailable or failed checks explicitly.

Rendering, input, dialogs, resizing, and process lifecycle changes also need the
relevant real-terminal checks in the implementation guide. Use a release build
when changing dependencies or release configuration. For documentation-only edits,
review content, local links, referenced paths, and whitespace, including new files.
No runtime tests are needed for prose-only changes.

## Submit a pull request

Use a descriptive title and explain:

- The problem or use case, with a related issue link when one exists.
- The resulting behavior and any compatibility impact.
- The checks actually run and their outcomes, including relevant manual checks.
- Any remaining limitations or checks you could not perform.

Include a small reproduction or before/after screenshot for a visual change when
useful. Keep the diff focused so it can be reviewed independently. Address review
feedback with the same verification discipline and rerun affected checks after
updates. The repository's existing license is in [LICENSE](LICENSE).
