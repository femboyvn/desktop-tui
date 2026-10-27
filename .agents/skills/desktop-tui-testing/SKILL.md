---
name: desktop-tui-testing
description: Use when adding Desktop-TUI regression tests, investigating a failing check, or verifying Rust, shortcut, input, rendering, resize, or child-process changes. Prose-only edits need document checks.
---

# Test Desktop-TUI changes

Choose evidence that exercises the changed behavior. Read
[HOW_TO_TEST.md](../../../HOW_TO_TEST.md) for commands and manual cases, and
[HOW_TO_IMPLEMENT_A_FUNCTION.md](../../../HOW_TO_IMPLEMENT_A_FUNCTION.md) for test
seams. Inspect the patch, owning source, and current Git state before selecting
checks; preserve other contributors' changes.

## Select a verification path

| Changed behavior | Evidence to collect |
| --- | --- |
| Rust logic | Focused regression tests, then the full Cargo checks in HOW_TO_TEST.md |
| Shortcut loading/schema | Defaults, invalid input, ordering/deduplication, and affected samples |
| Key encoding | Exact bytes/modifiers, affected non-ASCII cases, focus and Ctrl+C smoke checks |
| ANSI/geometry/resize | Surface/state assertions, relevant chunk boundaries and size bounds; real-terminal checks with active and quiet children |
| Child/dialog lifecycle | Setup cancellation, asynchronous launch error, normal exit, close/terminate paths, and leftover-child checks |
| Documentation only | Full content, referenced paths, local links, shell/TOML examples, whitespace including new files |

The crate is binary-only. Place unit tests in `#[cfg(test)] mod tests` in the
owning module; private helpers are accessible with `super`. Do not invent a
library import or restructure the application just to test a helper.

For a bug, reproduce the desired assertion failing for the intended reason,
then implement and rerun it. Use observable results rather than matching source
text or reproducing the algorithm. Keep filesystem fixtures isolated and avoid
process-wide directory/environment mutations or dependence on installed TUI apps.

During development, a focused filter such as
`cargo test --locked keyboard::tests::ctrl_arrow_encodes_modifier` is useful
only after that test exists. Confirm that the intended test actually ran; a
filter matching zero tests still succeeds. Follow targeted checks with the full
`cargo test --locked` and the other checks in the testing guide.

## Interpret results accurately

A zero-test run proves test-target compilation, not key, rendering, or lifecycle
correctness. Formatting is deliberately disabled in `rustfmt.toml`; a green
format check does not establish style compliance. Compare Clippy diagnostics
with the unchanged base rather than suppressing existing warnings or imposing a
new warning policy incidentally. Keep `--locked`; offline mode needs cached crates.

Use [HOW_TO_RUN.md](../../../HOW_TO_RUN.md) for controlled shortcuts. Run UI
checks only in a real terminal. If interactive verification is unavailable,
report which behavior remains unchecked instead of substituting CLI help or a
headless build. Do not install missing programs or change host settings merely
to make a check runnable unless the task authorizes that setup.

Return the commands actually run, test names/counts, relevant manual outcomes,
new versus existing diagnostics, and blocked checks. For example, after only a
zero-test run and disabled format check, state that input and resize behavior
remain unverified; identify the regression tests and terminal cases still needed.
