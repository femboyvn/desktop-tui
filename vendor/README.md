# Local dependencies

Desktop-TUI uses two local Rust dependencies: AppCUI below, and the pinned
[`tattoy-wezterm-term` 0.1.0-fork.5 core](wezterm-term/PATCHES.md). The core's
patch record includes archive hash, VCS provenance, retained MIT license, and
changes to graphemes, replies, capabilities and host-driven history retention.
The root Cargo.lock controls both.

The core additionally exposes `TerminalState::enforce_scrollback_limits` backed
by `Screen` retention pruning. Desktop-TUI supplies its row and text budgets
without simulating child output or moving its cursor. The helper removes the
oldest complete rows, keeps live cells/cursor and surviving stable row IDs,
and applies primary-history bounds even while the alternate screen is active.
The facade invokes it after resize so quiet reflow cannot leave oversized history.
This patch retains the upstream license and attribution; details belong in the
[core patch record](wezterm-term/PATCHES.md).

Application regressions for pruning/reflow are included in:

```sh
cargo test --locked runtime::terminal::tests
```

## AppCUI

`appcui/` contains AppCUI 0.4.6 from the cached crates.io package, retaining
its original source, manifest metadata, authorship, and README. The package
declares the MIT license in `appcui/Cargo.toml`; Desktop-TUI's own license is
unchanged.

Provenance:

- Package: `appcui` 0.4.6.
- Crates.io archive SHA-256: `b176effec313893928db922fd976c08b39f081f9da88dfa02c959cda99ced645`.
- Packaged VCS revision: `3e78f52da9cd2ff0d85716c13ac57d9cba874203`, directory `appcui`.

Cargo uses this local copy through the path dependency in the root manifest.
Modified packaged sources:

- `src/system/menu_handle_manager.rs`, `src/system/runtime_manager.rs`,
  `src/ui/common/control_base.rs`, `src/ui/menu/menu.rs`, and
  `src/ui/appbar/app_bar.rs`: mutable owned menus, recursive release/clear,
  popup closure, and app-bar item removal for dynamic window taskbar entries.

- `src/ui/menu/menu.rs` and `src/system/runtime_manager.rs`: open submenus on
  hover and route pointer motion through the visible parent chain, so moving
  between Application categories replaces the opened submenu.

- `src/backend/crossterm/input.rs`: recognize completed left-click pairs within
  500 ms and emit double-click actions for lists and window controls. Movement,
  dragging, keyboard input, resize, and scrolling cancel the pair.
- `src/system/runtime_manager.rs`: keep double clicks in open menus from reaching
  controls underneath.

- `src/ui/desktop/events.rs`, `src/ui/window/events.rs`, and
  `src/system/runtime_manager.rs`: default preview-key hooks before focused-child
  dispatch. Desktop commands can open the palette while a PTY owns focus;
  open menus and modals retain their input. A modal preview hook lets the palette
  consume its second prefix and return it for terminal forwarding. Default hooks
  ignore input, preserving existing controls; Terminal Lock excludes desktop
  shortcut handling at the application layer.

- `src/system/runtime_manager.rs`: connect timers allocated before a modal is
  registered to its final control handle and schedule their threads, matching
  ordinary child registration. Launcher Manager and Diagnostics need this to
  display background results. The regression
  `modal_registration_connects_a_timer_started_before_showing` covers routing.

- `src/backend/crossterm/input.rs`: periodically compare the kernel PTY size
  while waiting for input, so missed resize notifications over a PTY/SSH relay
  cannot leave the desktop at stale dimensions. Duplicate or invalid sizes are
  ignored; keyboard/modifier notifications are retained.
- `src/system/clipboard.rs` and `src/backend/crossterm/implementation.rs`:
  a bounded, thread-safe internal text cache replaces Unix CrossTerm's
  display-dependent clipboard calls in native controls. The application's
  asynchronous terminal clipboard service shares this cache. Native copying
  therefore works without DISPLAY/Wayland and cannot panic on X11 connection
  failure. Graphical helper processes remain optional at the application layer.

- `src/dialogs.rs`, `src/dialogs/open_save_dialog.rs`, `src/ui/common/control_base.rs`,
  and `src/system/runtime_manager.rs`: opt-in file-picker background preservation;
  Image Viewer uses it so other windows retain their original colors.

- `src/backend/crossterm/implementation.rs`: the rendering fix described below.
- `src/ui/window/toolbar/toolbar.rs`: remove a final position adjustment whose
  result is never used.
- `src/backend/utils/ansi_formatter.rs`: compile the mouse-enable helper only
  for tests, its only active callers.
- `src/utils/log.rs`: compile logging imports and storage only in debug builds,
  matching their callers.
- `src/utils/fs/root.rs`: compile Windows-only drive kinds and icon arms for
  Windows backends and tests.
- `src/backend/termios/api/sizing.rs`: explicitly cast the resize signal handler
  through a function pointer before converting to libc's handler type.

`src/backend/crossterm/regression_tests.rs` is new. The warning cleanups above
retain the existing runtime behavior and do not add lint suppressions. Registry
cache markers and the upstream lockfile are omitted; the root lockfile controls
dependency resolution.

The local fix resets text styles before choosing colors and invalidates the
color cache after every style reset. It also establishes styles for the first
changed cell of each frame, because the terminal retains styles between frames.
The output routine accepts a writer so tests can inspect real ANSI output
without initializing or taking over a terminal.

Run the renderer regression tests from the repository root:

```sh
cargo test --locked -p appcui --lib backend::crossterm::implementation::regression_tests
```

The existing tests for the warning cleanup are:

```sh
cargo test --locked -p appcui --lib backend::utils::tests::check_ansi_methods
cargo test --locked -p appcui --lib utils::fs::tests::check_roottype
cargo test --locked -p appcui --lib ui::window::tests:: -- --test-threads=1
```

Run AppCUI UI tests sequentially because its application singleton is shared
across the process.

Keep future changes focused and document them here. This is a local dependency
patch; it does not require synchronizing with or publishing to another project.

The Carboxyl integration adds complete glyph metadata while keeping
`Character: Copy`. `graphics/surface.rs` stores sparse lead text, column width,
and continuation offsets. `write_grapheme`, `cell_text`, and `cell_width` expose
that data. All writers, clears, recoloring, resize and composition preserve or
atomically erase affected glyphs. A partially clipped wide glyph becomes spaces
in visible cells, so it cannot overwrite an adjacent window. CrossTerm and ANSI
renderers print each grapheme once, skip continuations, and include metadata in
repaint caches. Debug output, SurfaceTester and event-recorder hashes include
complete text and width.

Metadata-free surfaces retain the exact SRF version 1 format. Metadata-bearing
surfaces write version 2: the existing cell payload followed by a `u32` lead
count, then each lead's `u32` index, `u32` width, `u64` UTF-8 byte length, and text.
Readers reconstruct continuations and validate row bounds and overlapping spans.

CrossTerm input delivers Press and Repeat with their actual Unicode character,
preserves modified keys and BackTab, and forwards Paste. CrossTerm enables outer
bracketed-paste capture on entry and disables it on close. `OnKeyPressed::on_paste`
opts controls into whole-payload handling. Otherwise the runtime re-resolves
focus and modal/menu boundaries while delivering typed-character fallback.
Mouse events carry local modifiers and use the control's origin when clipped.
`WheelAt` and `OverAt` offer coordinates/modifiers before legacy fallback;
native controls explicitly decline these metadata variants. DoubleClick retains
the native second-press callback and captures its matching release. Legacy
backend event constructors were updated for compatibility; Linux CrossTerm is
the supported, verified build target.

Desktop workflow coverage lives in the application debug-backend tests:

```sh
cargo test --locked ui::desktop::palette::tests
cargo test --locked ui::desktop::view::tests
```

These exercise palette filtering/cancellation/second-prefix forwarding and desktop
integration without initializing a real terminal. Manual focus, modal, lock,
rebind and outer-terminal interception checks remain necessary.

Additional regressions and the complete dependency suite:

```sh
cargo test --locked -p appcui --lib grapheme_ -- --test-threads=1
cargo test --locked -p appcui --lib backend::crossterm::input -- --test-threads=1
cargo test --locked -p appcui --lib system::runtime_manager::regression_tests -- --test-threads=1
cargo test --locked -p appcui --lib -- --test-threads=1
```

At the integration baseline, the full suite has one existing failure:
`ui::menu::tests::check_submenus_open` expects hash `0x60340250EC2EF1C2` but gets
`0xF51C6661294D459A`. It reproduces on untouched revision `15fcb77` with
`CROSSTERM,TRUE_COLORS`; no fixture was changed to hide this mismatch.

The file-picker backdrop regression is in
`src/system/runtime_manager/regression_tests.rs`:

```sh
cargo test --locked -p appcui --lib file_picker_preserves_background_colors_when_requested -- --test-threads=1
```

CrossTerm double-click regressions:

```sh
cargo test --locked -p appcui --lib double_click_tests -- --test-threads=1
```

Submenu hover regression:

```sh
cargo test --locked -p appcui --lib hovering_parent_categories_switches_open_submenus -- --test-threads=1
```

Taskbar popup ownership regression:

```sh
cargo test --locked -p appcui --lib removing_window_menu_releases_handles_and_closes_its_popup -- --test-threads=1
```
