# How to test Desktop-TUI

Use this guide from the repository root. Start with [AGENTS.md](AGENTS.md) for
the shared-worktree and Git rules, [HOW_TO_RUN.md](HOW_TO_RUN.md) for setup and
controlled shortcuts, and [CONTRIBUTE.md](CONTRIBUTE.md) for internal handoffs and
requested project reviews.
The module map and implementation limits are in [CODEBASE.md](CODEBASE.md).

## Choose checks that exercise the change

The current package is binary-only, with desktop input regression tests in
`src/ui/desktop/view.rs`. It has no integration tests, CI workflow, or configured Markdown
linter. A filtered run with zero tests proves that the test target compiles, not
that the desktop works. Add focused regression tests for changed logic and perform
the relevant terminal checks for rendering, input, dialogs, resizing, or process lifecycle.

For documentation-only changes, read the complete text and new files, verify
relative links and referenced paths, and check whitespace. Compilation is needed
only when code/configuration changed or a documented command needs validation.
Do not launch the desktop just to verify prose.

## Automated Rust checks

For Rust changes, run these commands:

```sh
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets
cargo fmt --all -- --check
git diff --check
```

The locally patched AppCUI renderer also has output-level regressions. Run them
from this root (the application's default test command does not select dependency
tests):

```sh
cargo test --locked -p appcui --lib backend::crossterm::implementation::regression_tests
```

See [vendor/README.md](vendor/README.md) for the patch and its provenance.

Use `cargo build --locked --release` as well for dependency or release-profile
changes. `cargo run --locked -- --help` checks CLI startup without initializing
the terminal; it does not exercise menus, shortcut loading, or child execution.

Keep `--locked` so checks use the tracked dependency resolution. If the network
is unavailable, add `--offline` only when all dependencies are cached. A cache
miss or inaccessible registry blocks the check; record the command and reason
instead of reporting a pass or changing dependency versions to get past it.

[rustfmt.toml](rustfmt.toml) deliberately sets `disable_all_formatting = true`.
A successful format check does not enforce Rust style; match nearby code and
follow [CODING_CONVENTION.md](CODING_CONVENTION.md) manually. Clippy has an
observed baseline of 14 warnings with Rust 1.97.1 during onboarding verification;
that count may vary with toolchain changes. Compare diagnostics with the base
revision and identify new warnings. Do not silently suppress diagnostics or treat `-D warnings` as an
existing project gate.

## Add and run a regression test

Put unit tests in a `#[cfg(test)] mod tests` at the end of the owning source file.
The child module can access private helpers through `super`. There is no library
target for a `tests/` integration test to import as `desktop_tui`; do not add a
library or restructure the app just to test a private helper.

For example, a test added to [src/ui/terminal/keyboard.rs](src/ui/terminal/keyboard.rs) could assert exact
protocol bytes from the existing `csi_mod` helper:

```rust
#[cfg(test)]
mod tests {
    use super::csi_mod;

    #[test]
    fn ctrl_arrow_encodes_modifier() {
        assert_eq!(csi_mod(b"A", 5), b"\x1b[1;5A".to_vec());
    }
}
```

This is an example, not a test already present in the repository. After adding
it, a focused command would be:

```sh
cargo test --locked keyboard::tests::ctrl_arrow_encodes_modifier
```

Cargo test filters can succeed with zero matching tests. Check that the output
reports the intended test running; use `cargo test --locked -- --list` to inspect
names. Run the full `cargo test --locked` after focused checks.

For a fix, write the desired assertion first and confirm that it fails for the
expected reason. Assert observable behavior: exact key bytes, parsed defaults
and ordering, error results, rendered cells, or cursor bounds. Cover relevant
edges such as omitted fields, duplicate names, split escape sequences, non-ASCII
input, small surfaces, padding, and channel closure. Keep tests deterministic
and independent of installed TUI apps, real dialogs, timing, and process-wide
working-directory or environment changes. See
[HOW_TO_IMPLEMENT_A_FUNCTION.md](HOW_TO_IMPLEMENT_A_FUNCTION.md) for test seams.

## Manual checks in a real terminal

Do not run the interactive desktop as an unattended check: it owns the terminal
and its shortcuts can launch commands. Use a real terminal and an explicit,
controlled directory as shown in [HOW_TO_RUN.md](HOW_TO_RUN.md). Install any sample
programs you choose to use; they are not bundled. Select a shortcut's **Start**
menu item to launch it. Check only the matrix rows relevant to the patch.

| Area | Exercise and record |
| --- | --- |
| Startup and menus | Launch with the explicit directory, start a known command, hide/show, close, and reopen it. Confirm responsiveness and arrangement if affected. |
| Keyboard and focus | On an empty desktop, press Alt combinations and Escape; confirm the app stays open and Desktop > Exit still works. Type into the focused child; check affected arrows, navigation, modifiers, or function keys. Move focus and confirm input goes to the intended control. Check Ctrl+C termination separately. |
| ANSI and rendering | Use controlled text/color/cursor output. Check the affected sequences, chunk boundaries, non-ASCII text, and surface edges against the intended result. Record existing emulator limits separately. |
| Window movement and colors | Open Help and a terminal with bold/plain colored text. Drag each window repeatedly over the desktop and other windows, then close or hide it. Check that exposed cells and text retain their colors with active and quiet terminal output. |
| Resize | Resize both the child window and outer terminal, including small sizes. Check canvas bounds, cursor, colors, and child dimensions with active output and while quiet. Resize synchronization runs on the window timer, including while output is quiet. |
| Placeholder dialogs | Exercise file and folder selection, cancellation, and paths containing spaces using explicit argument arrays. Confirm arguments reach the child intact and cancellation prevents launch. |
| Application menu | At 80×24 and a larger size, check Desktop / Application / Tilling order, all four standard categories, disabled empty entries, additional categories, starter overrides, and Apps overflow with the panel enabled and disabled. Only visible open windows belong after Tilling |; use overflow to reach Hide/Close, then restore through Application > Background windows. Include native apps, independent documents, Help, and repeated close/reopen. Confirm focus restoration and released taskbar/menu handles. |
| Native image viewer | Open each supported format; check EXIF orientation, transparency across a palette change, first frame of animation, fit/actual/zoom bounds, arrows, dragging, and outer/window resize. Test invalid/missing/unsupported images and both size limits. Close during decode and reopen another file. |
| Native PDF reader | With Poppler installed, open multi-page text and scanned PDFs from Utility and File Browser. Check Ctrl+O and cancellation, Prev/Next, PgUp/PgDn, P/N, Home/End, fit, zoom/wheel, pan/drag, tiny viewports, and resize. Test missing helpers, missing/invalid/oversized/password-protected input, spaces/Unicode/opaque paths, hide/restore, theme, tiling, lock, close while loading, reopen, and Exit; no helper processes should remain. |
| Native file browser | Navigate home, relative/absolute paths, Up, Refresh, hidden files, unreadable folders, and large directories. Open images and multiple editor documents. Use spaces, Unicode, non-UTF-8 names, and literal placeholder text; confirm each selected path reaches the editor as one argument and a running editor remains open. |
| Native music player | Open MP3, WAV, FLAC, and Ogg Vorbis files. Check click/keyboard Open, Play/Pause, Stop, ten-second seeking, volume bounds, progress, invalid/missing/oversized input, and unavailable output devices. Hide, lock, restore, tile, resize, and change theme while playing. Close during loading, reopen, then Exit; playback must stop. Use a working real audio session to verify audible output. |
| Native games | Open Snake, 2048, and Minesweeper from Application > Games. Check arrows/WASD, R restart, scores, 2048 merges, and Minesweeper Space/Enter/F plus left/right clicks. Snake must pause on lost focus, hiding, locking, and a small window; Space/P resumes. Hide/restore each board, close/reopen, tile, resize, change theme, then Exit. Verify readable instructions and the resize prompt at small sizes. |
| Mixed window lifecycle | Tile native and terminal/document windows together; change theme and Terminal Lock with some hidden. Restore the original visibility. Close and reopen native windows, close individual documents, then Exit and check for leftover child processes. |
| Child lifecycle | Exercise normal exit, a missing executable, menu Close, Ctrl+C, and Desktop Exit. Observe errors and responsiveness, and use the host's process tools to check for leftover children. |

Ctrl+C with the child control focused requests termination and closes its window;
it is not forwarded as an ordinary shell interrupt. A finite command's window
closes when the child exits. For resize and input checks, use an interactive or
longer-running controlled child rather than relying on a short-lived print.
The emulator is partial; distinguish a regression from an existing limitation.

## Review and report evidence

Inspect the complete tracked patch and new files, then `git status --short`.
`git diff --check` covers tracked diffs, so inspect untracked files separately.
Verify local Markdown link targets and confirm commands and examples match the
source. Keep unrelated changes out of the patch; follow [AGENTS.md](AGENTS.md)
before staging or committing.

Report commands actually run and their outcomes, tests that ran, new versus
existing warnings, manual scenarios exercised, and any blocked or remaining
checks with reasons. Include the OS, terminal, toolchain, relevant shortcut, and
revision for a reproducible UI failure. State whether the work is committed.
Keep evidence local or within the owner-approved project destination; do not send
reports, issues, or pull requests to the original parent project or other forks.
A headless build cannot establish interactive correctness, and no CI result is
available in this repository to substitute for these checks.

For this feature, also run `cargo build --locked --release`. Automated coverage
for catalogs is in `src/config/catalog.rs`; image, filesystem, worker, and process
coverage is in `src/runtime/`; native navigation tests are in `src/ui/native/`.
The AppCUI debug backend exercises UI integration without starting the interactive
desktop. Real-terminal smoke checks above remain necessary for visual/input approval.

PDF parsing, bounds, timeout/cancellation, navigation, zoom, and desktop lifecycle
tests run in the normal suite. Run the opt-in two-page rendering regression on
a host with Poppler's `pdfinfo` and `pdftoppm` installed:

```sh
cargo test --locked runtime::pdf::tests::poppler_renders -- --ignored
```
