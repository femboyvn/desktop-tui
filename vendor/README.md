# Local AppCUI dependency

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

- `src/ui/menu/menu.rs` and `src/system/runtime_manager.rs`: open submenus on
  hover and route pointer motion through the visible parent chain, so moving
  between Application categories replaces the opened submenu.

- `src/backend/crossterm/input.rs`: recognize completed left-click pairs within
  500 ms and emit double-click actions for lists and window controls. Movement,
  dragging, keyboard input, resize, and scrolling cancel the pair.
- `src/system/runtime_manager.rs`: keep double clicks in open menus from reaching
  controls underneath.

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
