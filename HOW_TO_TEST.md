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

For ALSA eventfd wakeup/cleanup changes, also run the hardware-free C regression
and fortified compiler check in [vendor/miniaudio/README.md](vendor/miniaudio/README.md).
These checks do not select or open a real audio device.

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
| Keyboard and focus | On an empty desktop, press Alt combinations and Escape; confirm the app stays open and Desktop > Exit still works. Type into the focused child; check affected arrows, navigation, modifiers, or function keys. Move focus and confirm input goes to the intended control. Check that Ctrl+C interrupts the child normally and explicit Close still shuts it down. |
| Command palette and binding | With a terminal/native window focused, open Ctrl+Space; search Launch/Focus/Hide/Close, cancel, and use New terminal twice. Confirm ordinary launchers restore instead. Use Desktop menu fallback, rebind and restart, then press the configured prefix twice and verify literal bytes reach the originating child. Menus, modal dialogs, Terminal Lock and Ctrl+C must retain their own behavior. |
| History, selection and clipboard | Generate bounded multilingual/soft-wrapped output; Shift+PageUp/Down, new output while paused, Return to live, keyboard/mouse Copy mode, complete combining/CJK/emoji selection, hard versus soft breaks, and literal Search with next/previous/wrap. Resize, alternate screen, eviction and limits must clear/clamp modes safely. In a display-free PTY verify internal copying between native controls and terminals, title status, oversized paste refusal, client paste and child mouse/paste outside Copy/Search. Optional graphical integration checks Wayland/X11, missing/failing providers, close during clipboard I/O and no helper survivors. |
| File operations and state | In isolated folders, create/rename/copy/move/trash via menus and keys, cancel dialogs/work, confirm/refuse replacement and changed destinations, and check source/backup paths after errors. Cover unreadable files, symlinks/dangling links, special-file refusal, destination directories, same/cross-filesystem moves and cross-filesystem trash refusal. Check progress, narrow layouts, favorites/destination favorites, filename filter, remembered state after restart, corrupt state preservation, hide/lock during work and Exit while saving state. Never use valuable files as fixtures. |
| Launcher editing and diagnostics | New/Edit/Save/Reload custom and starter launchers, invalid/stale drafts, duplicate names/IDs, explicit IDs across rename, source-relative cwd and file-context precedence. Existing visible/hidden windows remain alive and selectable after change/removal. Check Diagnostics scrolling/cancel and --doctor exit codes/no config creation, missing Terminal versus optional helpers, bundled Carboxyl and no configured-command execution. |
| Workspaces and restoration | Switch among four workspaces via menu/palette, move the originating window, focus across workspaces and preserve deliberate Hide state. Tile visible current-workspace windows, hide/lock/resize with terminals/native/docs, then Exit and check cleanup. Restart with an offered Restore; Cancel must launch nothing. Restore available entries with geometry/cwd/hidden state, report missing apps/files individually, and verify saved child output/unsaved buffers are not claimed as restored. |
| ANSI and rendering | Use controlled text/color/cursor output. Check the affected sequences, chunk boundaries, non-ASCII text, and surface edges against the intended result. Record existing emulator limits separately. |
| ASCII desktop background | Open Preference > Background (Alt+5), Browse to backgrounds/cat.asc, inspect the preview, and Save. Restart to check persistence, then move/close windows over the art, resize the outer terminal down to tiny sizes and back, and change the palette. Check spaces/blank lines, CRLF and tab expansion, filenames with spaces/Unicode, invalid/non-ASCII/ANSI/oversized files, picker cancellation, Clear art with Save versus Escape, and cancellation during loading. Failed imports must preserve the previous art; presets/reset must preserve the draft. |
| Window movement and colors | Open Help and a terminal with bold/plain colored text. Drag each window repeatedly over the desktop and other windows, then close or hide it. Check that exposed cells and text retain their colors with active and quiet terminal output. |
| Resize | Resize both the child window and outer terminal, including small sizes. Check canvas bounds, cursor, colors, and child dimensions with active output and while quiet. Resize synchronization runs on the window timer, including while output is quiet. |
| Placeholder dialogs | Exercise file and folder selection, cancellation, and paths containing spaces using explicit argument arrays. Confirm arguments reach the child intact and cancellation prevents launch. |
| Application menu | At 80×24 and a larger size, check Desktop / Application / Tilling order, all four standard categories, disabled empty entries, additional categories, starter overrides, and Apps overflow with the panel enabled and disabled. Only visible open windows belong after Tilling |; use overflow to reach Hide/Close, then restore through Application > Background windows. Include native apps, independent documents, Help, and repeated close/reopen. Confirm focus restoration and released taskbar/menu handles. |
| Native image viewer | Open each supported format; check EXIF orientation, transparency across a palette change, first frame of animation, fit/actual/zoom bounds, arrows, dragging, and outer/window resize. Test invalid/missing/unsupported images and both size limits. Close during decode and reopen another file. |
| Native PDF reader | With Poppler installed, open multi-page text and scanned PDFs from Utility and File Browser. Check Ctrl+O and cancellation, Prev/Next, PgUp/PgDn, P/N, Home/End, fit, zoom/wheel, pan/drag, tiny viewports, and resize. Test missing helpers, missing/invalid/oversized/password-protected input, spaces/Unicode/opaque paths, hide/restore, theme, tiling, lock, close while loading, reopen, and Exit; no helper processes should remain. |
| Native file browser | At 80×24 and larger, navigate home, relative/absolute paths, Up, Refresh, hidden files, unreadable folders, and large directories. Check default Grid, View/Alt+V and menu Details/Grid, arrows, Home/End, PageUp/PageDown, wheel, click selection, Enter/double-click, selected-entry reveal after mode switches/resizing, long/Unicode/opaque names, and tiny viewports. Right-click files/folders/special entries/blank space in both views; test Shift+F10, Escape, menus at screen edges, Refresh/Hidden, and navigation/close with a menu pending. Open with must show compatible native viewers and every shortcut, distinguish identical names, scroll through many choices, cancel cleanly, and reuse native viewers. Verify external choices bind file/parent templates or append once, preserve exact filename bytes, and create independent windows. Open in terminal must use the configured Terminal in the folder, file's parent, or current folder; exercise spaces/opaque paths, missing executables/directories, normal exit, Ctrl+C, and close. Hide/restore, lock/unlock, tile, resize, change themes, and Exit with multiple launches; existing apps stay open and no child remains after closure. |
| Native ASCII Paint | Launch from Utility and confirm singleton restore. Create default/minimum/maximum canvases; reject invalid sizes. Click the bottom-left size or use Resize/Ctrl+R to crop/expand; undo/redo dimensions and cropped RGB cells. Check bottom-right cursor coordinates, footer alignment, and footer clicks never painting. Convert images with Image to ASCII/Ctrl+I in color/monochrome, including alpha, aspect correction, output limits, unsaved prompting, cancellation, failed decode, and saving/exporting the converted art. Select printable brushes, exact RGB colors, and left/right palette swatches. Paint/erase clicks and continuous drags, toggle Eraser, use arrows/Space/Enter/Backspace/Delete, wheel in both axes, and PgUp/PgDn. Check 64-action grouped undo/redo and undoable Clear. Save/reopen colored projects, import LF/CRLF/tab text, export padded colorless LF text without clearing dirty state, and route `.asciipaint` from File Browser while `.txt`/`.asc` still use Text editor. Reject invalid/version/size/dimension/non-ASCII files and failed destinations without losing art or replacing existing files. Cancel pickers and Save/Discard/Cancel for New/Open/Close/Exit/Shutdown/Restart; a failed or canceled save must stop continuation and leave other apps running. Hide/restore and lock/unlock during I/O or pending prompts, resize to tiny/large viewports, tile, and change theme while checking cell RGB and canvas preservation. Test actual power actions only in a disposable VM. |
| Native music player | Open MP3, WAV, FLAC, and Ogg Vorbis files from the picker and File Browser, including mixed-case extensions and spaces/Unicode/opaque filenames. Confirm browser opens reuse and restore the player, replace the track, and never open an editor. Check click/keyboard Open, Play/Pause, Stop, ten-second seeking, volume bounds, progress, invalid/missing/oversized input, and unavailable output devices. Verify audible playback alongside the live ASCII spectrum, including volume/mute, falling peaks, silence, and clearing on pause, stop, seek, replacement, completion, errors, and close. Hide/restore and lock/unlock during playback must show current levels. Tile, resize to narrow/tiny surfaces, and change theme while playing; controls/help/errors retain priority and columns stay thin, the themed rectangle encloses bars/peaks and frequency labels (1k, 2k, 4k, 8k, 16k) stay aligned below its bottom border without overlapping, and bars merge or disappear as space runs out. Check an 8 kHz file ends at 4k. Close during loading, reopen, then Exit; playback must stop. Use a working real audio session for these checks. |
| Native games | Open Snake, 2048, and Minesweeper from Application > Games. Check arrows/WASD, R restart, scores, 2048 merges, and Minesweeper Space/Enter/F plus left/right clicks. Snake must pause on lost focus, hiding, locking, and a small window; Space/P resumes. Hide/restore each board, close/reopen, tile, resize, change theme, then Exit. Verify readable instructions and the resize prompt at small sizes. |
| Mixed window lifecycle | Tile native and terminal/document windows together; change theme and Terminal Lock with some hidden. Restore the original visibility. Close and reopen native windows, close individual documents, then Exit and check for leftover child processes. |
| Child lifecycle | Exercise normal exit, a missing executable, menu Close, Ctrl+C, and Desktop Exit. Observe errors and responsiveness, and use the host's process tools to check for leftover children. |

For the File Browser, also check keyboard and mouse multi-select in both
views. Copy or move marked files to another folder, decline a replacement
mid-batch, and verify the completed count and remaining sources. Use Find
with nested names, hidden folders, and a directory symlink. Return with
Back and check that the saved filter applies. With a temporary
`XDG_DATA_HOME`, trash a batch of fixtures and restore it in the Trash view.
Check the original paths, a conflicting destination, and a missing parent.

Ctrl+C with the child control focused sends byte 0x03 as a normal interrupt.
A shell may interrupt its foreground job and remain open; a child that exits
closes its window. Test explicit Close separately for managed process cleanup.
For resize and input checks, use an interactive or longer-running controlled child rather than relying on a short-lived print.
Unicode preservation and cell width are implemented; the outer terminal still controls fonts and shaping. Browser site support belongs to Carboxyl.

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

ASCII Paint document, history, normalization, serialization, bounded file I/O,
atomic-write failure, and worker tests live in `src/runtime/ascii_paint.rs`.
Canvas keyboard/mouse, footer placement, canvas resizing, image conversion,
viewport, rendering, dialogs, and save continuation tests live in `src/ui/native/ascii_paint.rs`; catalog and desktop lifecycle/routing
coverage lives in `src/config/catalog.rs` and `src/ui/desktop/view.rs`. Run focused
checks before the full suite, and verify each filter actually selects tests:

```sh
cargo test --locked ascii_paint
```

These headless tests do not replace the ASCII Paint real-terminal matrix above.

Music Player spectrum tests live in `src/runtime/music/spectrum.rs`; they use
synthetic bass/mid/treble tones, silence, fixed volume scaling, opposite-phase
stereo, eight-channel input, sample rates from 8 to 192 kHz, bounded capture, and
elapsed-time decay. `src/runtime/music.rs` exercises callback contention and
post-volume capture without audio hardware, plus transport/history resets.
`src/ui/native/music_player.rs` checks exact ASCII bars/peaks, thin columns on wide
surfaces, rectangle corners/edges with labels below the frame, frequency-label
placement and crowding, theme attributes, wrapped errors,
narrow/tiny surfaces, spectrum-only refreshes, and restoration. The runtime tests
also preserve an 8 kHz track's 4k axis endpoint across pause, stop, and completion.
Run the focused checks before the full suite:

```sh
cargo test --locked music
```

These checks do not open an output device or verify audible/animated playback;
record the Music Player real-terminal cases above separately.

File Browser grid/hit testing/menu targeting and invalidation tests live in
`src/ui/native/browser.rs`. Chooser filtering, identity, scrolling, and cancellation
are in `src/ui/desktop/open_with.rs`; independent-window argument/directory and
cleanup coverage is in `src/ui/desktop/view.rs`. Terminal context/template and
launch-error tests are in `src/ui/terminal/window.rs`. Run focused checks first:

```sh
cargo test --locked browser
cargo test --locked open_with
cargo test --locked ui::terminal::window::tests
```

These use AppCUI's debug backend and controlled Unix child processes. They do not
replace the File Browser real-terminal matrix above.

File-information coverage in `src/ui/native/browser.rs` checks context-menu and
Alt+Enter targeting in Grid/Details, displayed metadata for files/folders/special
entries, read-only text, original opaque paths, long names, and Close/Escape.
In a real terminal, open File information from each view and check all fields,
scroll full paths, Tab between text and Close, resize the dialog, close with both
the button and Escape, and confirm the browser selection and files are unchanged.
Change a file externally, Refresh, and reopen the dialog to check updated metadata.

## Workflow regression coverage

Run the focused owning-module tests, then the full Rust gates above:

```sh
cargo test --locked terminal:: -- --test-threads=1
cargo test --locked runtime::clipboard::tests -- --test-threads=1
cargo test --locked runtime::files::
cargo test --locked browser
cargo test --locked config::shortcuts::tests
cargo test --locked config::workflow::tests
cargo test --locked config::session::tests
cargo test --locked ui::desktop::launchers::tests
cargo test --locked ui::desktop::palette::tests
cargo test --locked ui::desktop::view::tests
cargo test --locked platform::diagnostics::tests
cargo test --locked args::tests
cargo run --locked -- --doctor ./shortcuts
```

History tests exercise grapheme-preserving selection, soft-wrap literal search,
work budgets, eviction, large wrapped history and resize anchors. Clipboard tests
use controlled helper processes for failures, bounds, timeout, ownership and
cleanup; actual graphical-session interoperability remains a manual check.
Filesystem tests use isolated trees/state and cover cancellation, publication,
replacement snapshots, symlinks, permissions, special files and supported
cross-device behavior. Launcher tests cover backwards compatibility, deterministic
loading, validation, relative cwd and atomic/stale-save preservation. Debug-backend
tests cover modal cancellation, palette routing and session identities/visibility.
No headless success establishes terminal appearance or physical keyboard routing.

For CLI diagnostics, additionally use an isolated XDG_CONFIG_HOME and missing
selected directory: expect nonzero and verify no directories were created. Test
invalid TOML, an unavailable configured Terminal, optional-tool warnings, and a
spaced executable with source-relative cwd from outside the repository. A harmless
fixture executable that would create a marker must not run under --doctor.

Attended CrossTerm PTY checks at 80×24 covered the palette, rebinding, independent
shells, interrupts, history/search, exact Unicode copy/paste through the internal
clipboard, file create/rename/copy/trash/filter/path navigation, launcher save and
reload, Diagnostics, workspace membership/hidden state, explicit restoration,
tiling and temporary Terminal Lock/unlock. A separate stable 120×36 PTY session
confirmed tiling to a 118×33 child terminal, hide/focus preserving its PID, and
Close/New replacing it while retaining the correct size. See the dated
[milestone verification record](docs/milestones/2026-11-01.md#implementation-and-verification-record).
PTY/SSH is the primary acceptance target. Run the relevant matrix without
DISPLAY/WAYLAND_DISPLAY, resize a quiet terminal through 80×24, 120×36, 40×12
and back, and verify child dimensions without reselecting the tiling option.
Check native Ctrl+C/Ctrl+V and terminal Copy/Paste in both directions. These
display-free relay checks were performed; see the milestone record. An actual
network SSH server was not exercised. X11/Wayland clipboard exchange is an
optional additional check and does not block PTY acceptance.

## Carboxyl and Unicode terminal regressions

Run the focused application and dependency checks before the full commands above:

```sh
cargo test --locked runtime::terminal::tests
cargo test --locked ui::terminal::
cargo test --locked config::catalog::tests
cargo test --locked -p appcui --lib grapheme_ -- --test-threads=1
cargo test --locked -p appcui --lib backend::crossterm::input:: -- --test-threads=1
cargo test --locked -p appcui --lib system::runtime_manager::regression_tests -- --test-threads=1
cargo test --locked -p appcui --lib -- --test-threads=1
cargo build --locked --release
```

The core tests feed every byte split, separate scalar feeds, and immediate cell
contents/widths for decomposed Vietnamese, long combining sequences, CJK,
variation selectors, skin tones, flags, ZWJ sequences and adjacent emoji. Surface
and renderer tests cover clipping either half, overlap, wide/narrow replacement,
recoloring, metadata serialization, hashes and unchanged-frame caches. Input tests
cover exact key/mouse bytes, repeated modified keys, paste focus/modal boundaries,
queue recovery, output budgets, geometry and child cleanup.

For real-terminal checks, use the bundled Carboxyl 0.2.0 or install it separately, and serve the controlled
[local fixture](tests/fixtures/browser.html) from the repository root:

```sh
python3 -m http.server 8765 --bind 127.0.0.1 --directory tests/fixtures
```

In a separate real terminal, run Desktop-TUI with explicit sample shortcuts,
open Utility > Web Browser, click Carboxyl's top-row address field and enter
`http://127.0.0.1:8765/browser.html`. At 80×24 and larger, verify links, JavaScript,
form input, Unicode URL/form paste, images, multilingual text, history and
scrolling. Check modified navigation, mouse/wheel coordinates and modifiers,
double clicks and drag/release outside the window. Resize both windows and the
outer terminal, including quiet output. Overlap windows across wide glyphs;
check cursor focus, hiding/restoring, tiling, palette changes, lock/unlock,
close/reopen, Ctrl+C and Desktop Exit. Verify no child/descendant remains and
missing Carboxyl uses the normal launch-error dialog. Stop the local HTTP server
when finished. These are manual checks, not results established by headless tests.

## Portable Linux package

Run the executable-resolution and package-builder regressions:

```sh
cargo test --locked platform::bundled::tests
python3 -m unittest discover -s scripts -p 'test_package_linux.py'
python3 scripts/package-linux.py
```

The bundle pins and verifies the Carboxyl archive SHA-256 before extraction;
check tests reject changed downloads, unsafe archive members and wrong binary
architectures. Extract the resulting archive into a path containing spaces,
change to another directory, and run the extracted desktop with `--help` and
`libexec/carboxyl --version`. Confirm 0.2.0 and inspect included license files,
provenance and shortcut samples. These noninteractive checks verify loading and
packaging; the real-terminal Web Browser matrix above still applies.

On a compatible Linux x86-64 host (glibc 2.43+ for the pinned upstream binary),
launch the extracted desktop without Carboxyl on PATH, then open Web Browser.
Check same-name overrides with custom arguments and explicit executable paths,
launching from another working directory, hiding/restoring, locking, closing,
and no remaining child processes. Test a source/Cargo build without a companion
to confirm normal PATH fallback, and a non-executable companion. Do not launch
the interactive browser or desktop as an unattended packaging check.

## Kanban verification

Run focused model, file, input/rendering, dialog and desktop regressions:

```sh
cargo test --locked kanban
```

Then run all required Rust checks above. In a real terminal with a controlled
shortcut directory, open several boards via Application > Utility > Kanban and
File > New/Open. Save/reopen distinct `.kanban` files and Unicode paths; aliases
must focus the existing board and Save As must refuse another open board's file.
Verify custom columns and details survive reopening, manual task/column ordering,
required titles, multiline notes, priorities, and leap-year dates. Cancel all
editors and deletion destinations; keep at least one column.

Check mouse selection, double-click editing, same-column reordering and moves,
invalid release, Escape/focus loss, hide/show, Terminal Lock/unlock and resize
mid-drag. Scroll columns horizontally and tasks vertically, navigate to offscreen
cards, paint long combining/CJK/emoji titles and empty columns, shrink to tiny
windows, tile, switch workspaces and change theme. Close clean/dirty/saving boards
via titlebar and taskbar. Test Save/Discard/Cancel across Paint and multiple boards
before Exit; a later Cancel or failed save must keep all apps and dirty contents.
Use the mock system runner for Shutdown/Restart, never host power commands.
Restore saved board paths in another session; exclude untitled and unsaved content.
Headless tests do not replace this real-terminal matrix.

## AI Chat regressions

Run focused offline tests before the full Rust gates:

```sh
cargo test --locked ai
cargo test --locked config::preferences
```

Fixtures cover provider authentication/request schemas, mixed-provider history,
SSE byte splits/Unicode/errors/refusals/EOF, size limits, immutable attachment
copies, private atomic storage, corruption, symlinks, concurrent instances,
interrupted recovery, cancellation/late results, flush acknowledgements, retries,
secret redaction, independent settings saves, migration and settings draft preservation. AppCUI debug tests exercise
composer snapshots, model edits, transcript wrapping/scroll anchoring and guards.
Automated tests use no live provider credentials or paid API calls.

In an attended terminal with explicit sample shortcuts, check AI Chat at 80x24,
120x36 and 40x12: menu/palette launch, singleton restore, the separate AI Chat Settings dialog,
key paste/Keep/Replace/Remove/Save/Cancel, multiline compose, keyboard/mouse,
streaming scroll, provider/model switching, Attach/Files/Copy, and readable errors.
Exercise supported and unsupported files, exclusion of a historical attachment,
Stop/Retry, New/Delete, hide/show, workspaces, tiling, lock/unlock, resize, close,
restart and Exit. Test save failure with Retry/Discard/Cancel and verify other
windows stay open after cancellation. Use separate authorized test keys for any
live provider smoke checks; headless fixtures do not establish real API or terminal
appearance. No API request should occur merely on opening settings or history. Verify system
Preferences has its original five tabs and no AI controls. AI Save/Cancel must
leave system preferences unchanged; system Save must preserve ai.toml even when
it is invalid. Test one-time migration, existing standalone-file precedence,
missing keys, stale AI saves, public/symlinked AI files, and shortcut scanning.
