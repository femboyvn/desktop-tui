# Codebase guide

Desktop-TUI is an experimental terminal desktop that launches command-line apps
inside movable AppCUI windows. This repository is one Rust 2024 package with a
single binary entry point, `src/main.rs`. It has no library target, workspace
members beyond itself, or CI workflow. Desktop input regression tests live in
`src/ui/desktop/view.rs`.

This is the owner's personal version, developed independently of the original
parent project. Repository visibility may be public. Keep work local or in the
owner-approved `femboyvn/desktop-tui` repository; do not send contributions,
issues, or pull requests to the parent or other forks, or sync development with
them. Follow [AGENTS.md](AGENTS.md) to verify a destination's identity and fork
status before any authorized push. `publish = false` in
[Cargo.toml](Cargo.toml) disables Cargo package publishing; it does not control Git
hosting visibility. Preserve the inherited MIT license and attribution.

Read [AGENTS.md](AGENTS.md) for the working and Git workflow,
[CODING_CONVENTION.md](CODING_CONVENTION.md) for editing conventions, and
[HOW_TO_IMPLEMENT_A_FUNCTION.md](HOW_TO_IMPLEMENT_A_FUNCTION.md) for implementation
and verification steps.
The focused command guides are [HOW_TO_RUN.md](HOW_TO_RUN.md) and
[HOW_TO_TEST.md](HOW_TO_TEST.md); personal development and handoff expectations are in
[CONTRIBUTE.md](CONTRIBUTE.md).

## Setup and commands

Use a Rust toolchain that supports edition 2024, Cargo, and a native compiler/linker.
The native Music Player uses a C compiler to build its miniaudio device bridge;
Symphonia handles decoding and seeking in Rust. Its codecs and backend are compiled into the desktop;
runtime playback uses the host's working audio drivers and session.
Install the rustfmt and Clippy components if needed:

```sh
rustup component add rustfmt clippy
cargo build --locked
cargo run --locked -- --help
```

There is no `rust-toolchain.toml` or declared `rust-version`; an exact minimum
supported Rust version has not been established. `Cargo.lock` is tracked and
should be used for reproducible dependency selection. A first build needs access
to the package registry unless dependencies are cached.

The selected UI backend is CrossTerm, with true-color support enabled on AppCUI.
The `virtual-terminal` dependency uses a Unix PTY; use a Unix-like environment for
development and manual verification. Linux is the supported application target; platform-specific configuration
paths do not establish support for other operating systems. Native build
requirements can also come from dependency default features; use build diagnostics
to identify missing system libraries rather than disabling backends incidentally.

An optional `nix-shell` uses [shell.nix](shell.nix) for assorted native tools,
libraries, and sample TUI programs. It imports unpinned `<nixpkgs>` and does not
install the Rust toolchain itself. It is not required for the standard Cargo workflow.

| Command | Purpose |
| --- | --- |
| `cargo check --locked` | Type-check the binary without producing the executable. |
| `cargo build --locked` | Build the debug executable in `target/debug/`. |
| `cargo build --locked --release` | Build with the size-focused release profile. |
| `cargo test --locked` | Compile and run the unit tests. |
| `cargo clippy --locked --all-targets` | Check Rust code, including test targets, for lint diagnostics. |
| `cargo fmt --all -- --check` | Invoke rustfmt; formatting is deliberately disabled by configuration. |
| `cargo run --locked -- ./shortcuts` | Start the interactive desktop with the repository examples. |
| `git diff --check` | Check tracked patch whitespace; inspect new files separately. |

Add `--offline` to Cargo build/check/test/lint commands only if all needed
dependencies are cached. The sample shortcuts refer to external executables;
install the relevant program or use a small custom shortcut directory for manual
checks. The full desktop requires a real terminal. `--help` exits through Clap
before terminal initialization and is suitable for a noninteractive CLI check.

## Repository map

| Path | Responsibility / main symbols |
| --- | --- |
| [src/main.rs](src/main.rs) | Module declarations, Tokio runtime, configuration directory selection, shortcut parsing, AppCUI setup and event loop. |
| [src/args.rs](src/args.rs) | Clap-derived `Args`; optional positional `shortcut_dir: Option<PathBuf>`. |
| [src/config/catalog.rs](src/config/catalog.rs) | Starter merge, application IDs, categories, and native/external catalog entries. |
| [src/runtime/latest.rs](src/runtime/latest.rs), [src/runtime/images.rs](src/runtime/images.rs), [src/runtime/files.rs](src/runtime/files.rs) | Bounded latest-request workers, image decode/geometry/render preparation, and directory enumeration. |
| [src/runtime/processes.rs](src/runtime/processes.rs) | Managed PTY runner, independent PID observation, asynchronous process-tree cleanup, and shutdown task draining. |
| [src/runtime/music.rs](src/runtime/music.rs) | Bounded audio input, retained encoded streams, coalesced playback worker/state snapshots, and device cleanup after the event loop. |
| [src/ui/native/](src/ui/native/) | Native Image Viewer, File Browser, and Music Player windows and controls. |
| [src/config/shortcuts.rs](src/config/shortcuts.rs) | Serde types generated through `nestify::nest!`; `parse_shortcut_dir` loads, deduplicates, and sorts shortcuts. |
| [src/ui/desktop/view.rs](src/ui/desktop/view.rs) | `MyDesktop`, app-bar menus, shortcut-to-window handles, launch/show/hide/close commands, arrangement, preferences/locking integration, and panel updates. |
| [src/ui/terminal/window.rs](src/ui/terminal/window.rs) | `TuiWindow`, child command/channels, canvas updates, resize synchronization, shutdown, file/folder selection. |
| [src/ui/terminal/keyboard.rs](src/ui/terminal/keyboard.rs) | `CustomKeyboardControl` and its `OnKeyPressed` implementation; `to_escape_sequence_vec` and helpers translate AppCUI keys into terminal bytes. |
| [src/runtime/terminal.rs](src/runtime/terminal.rs) | Stateful `TerminalParser`; text/control characters, supported ANSI commands, cursor state, styles, and color conversion into an AppCUI `Surface`. |
| [src/config/preferences.rs](src/config/preferences.rs), [src/config/preferences/](src/config/preferences/) | Settings models/defaults, atomic storage, palette presets, palette drafts, and password-editing drafts. |
| [src/ui/desktop/](src/ui/desktop/) | Desktop status panel, offline Help, and system-action confirmation alongside the main view. |
| [src/ui/preferences/](src/ui/preferences/) | Preferences dialog and its custom controls. |
| [src/ui/locking/](src/ui/locking/) | Secret input, temporary-password setup, and the lock screen with private retry/input boundaries. |
| [src/ui/theme.rs](src/ui/theme.rs) | AppCUI theme construction. |
| [src/runtime/locking.rs](src/runtime/locking.rs) | Password credential creation, verification, and bounded serialization. |
| [src/platform/linux/](src/platform/linux/) | Linux status readings, resource sampling, and system actions. |
| [src/test_support.rs](src/test_support.rs) | Test-only temporary directories and the shared UI lock. |
| [src/utils.rs](src/utils.rs) | Unregistered helper file, outside the module tree. |
| [shortcuts/](shortcuts/) | Example TOML launchers; executable names are external tools, not bundled binaries. |
| [Cargo.toml](Cargo.toml), [Cargo.lock](Cargo.lock) | Package/dependency declarations and tracked dependency resolution. |
| [vendor/](vendor/README.md) | Local AppCUI 0.4.6 dependency with a CrossTerm style/color rendering fix and ANSI output regression tests. |
| [.cargo/config.toml](.cargo/config.toml) | Development/release profiles; release uses `opt-level = "z"`, LTO, one codegen unit, and symbol stripping. |
| [rustfmt.toml](rustfmt.toml) | `disable_all_formatting = true`; preserve manual source style. |
| [.agents/skills/](.agents/skills/) | Repository-local brainstorming, testing, and OS integration workflows; see `AGENTS.md` for selection and invocation. |
| [.gitignore](.gitignore) | Ignores Cargo output, IDE directories, and `.DS_STORE`. |
| [README.md](README.md), [LICENSE](LICENSE) | User instructions, shortcut example, and MIT license. |

Layers use named module files alongside child folders, such as
`ui/desktop.rs` and `ui/desktop/`. Group entry files declare private implementation
modules and expose shared types and entry points through crate-scoped re-exports.

## Runtime flow

1. `main` loads `preferences.toml` through `config::preferences`. With no shortcut
   argument, shortcuts use the settings directory derived from
   `ProjectDirs::from("com", "Julien-cpsn", "desktop-tui")`. An explicit shortcut
   directory does not change the settings location. The shortcut directory is
   created if necessary.
2. `parse_shortcut_dir` recursively walks the selected directory, deserializes
   matching files, excludes the exact settings file, drops duplicate shortcut names,
   and sorts by taskbar position.
3. `MyDesktop::new` merges external starters without replacing user shortcuts.
   `on_start` creates Desktop/Application/Tilling menus and registers external
   shortcut command menus. Visible open windows receive taskbar buttons in opening
   order; hidden windows are listed under Application > Background windows, then starts a 100 ms desktop timer for clock/panel updates and pending system
   actions. Linux readings refresh in the background. Apps start on menu commands; native IDs and immutable external indices dispatch
   independently of categories or labels. The application requests 255 AppCUI timer
   slots for native windows and independently opened documents.
4. `MyDesktop::create_window` passes the selected command, args, and window/terminal
   options to `TuiWindow::new`. Placeholder dialogs resolve `<FILE_PATH>` and
   `<FOLDER_PATH>` in both the executable and individual arguments before launch,
   preserving OS path bytes. Selected editor document paths are appended as opaque
   arguments after that resolution.
5. `TuiWindow::new` configures a `virtual_terminal::Command`, creates a canvas and
   keyboard control, starts a 25 ms UI timer, and starts a managed runner on Tokio. Its supervisor observes PID messages
   independently of UI polling and forwards output through a bounded channel.
   Program and arguments are passed separately; the repository does not implicitly
   invoke a shell. A shell is used only when the shortcut explicitly names one.
6. Keyboard events on the focused control become `Input::Data` messages. Ctrl+C
   is handled specially: it requests child termination and sets `should_exit`.
7. The window timer uses `try_recv` to consume one output message per update.
   PTY output arrives as `Output::Stdout`, including merged stdout/stderr. The
   parser updates the canvas surface. The timer synchronizes size changes to the
   child, canvas, and parser even while output is quiet, retrying a queued resize
   when the input channel is full. Errors and child termination close the window.
8. Titlebar closure, Ctrl+C, menu Close, and Desktop Exit share idempotent
   cleanup. A supervisor uses bmart (also used by virtual-terminal) to stop the
   child process tree asynchronously and retains the runner until child wait
   completes. Desktop Exit includes native windows and document handles; `main`
   drains managed runner tasks and joins audio workers after the UI loop releases
   its controls. Music workers coalesce desired controls, stream from bounded
   encoded input. A bounded PCM buffer feeds a nonblocking audio callback;
   closure silences it immediately, and device teardown joins its callback thread.
   The desktop denies AppCUI's Escape close request so an Escape prefix from an
   Alt sequence cannot exit the application.

## Configuration contract

The complete user example lives in [README.md](README.md). Source definitions in
`src/config/shortcuts.rs` govern deserialization:

- `name` and `command` are required; top-level `args` defaults to an empty vector.
- Optional top-level `category` defaults to User when absent or blank. System,
  Utility, User, and Games precede alphabetically ordered additional categories.
  Native File Browser/Image Viewer/Music Player entries remain separate from custom
  shortcuts; names matching Terminal, System Performance, or Text editor override
  external starters.
- The `taskbar`, `window`, and `terminal` tables are required. Taskbar position is
  optional; `additional_commands` defaults to empty. Each additional command has
  a required name, executable, and argument list.
- Window booleans (`resizable`, `close_button`, `fixed_position`) are required.
  Window size is optional; `TuiWindow::new` uses 100 by 25 when it is absent.
- Terminal padding and RGB background are optional. The saved terminal palette
  supplies default colors; an explicit shortcut background overrides its background.
  Padding affects both canvas placement and inner dimensions.
- Names are deduplicated by first encountered entry. Walk order is not explicitly
  sorted, so do not rely on a particular duplicate winning. Sorting uses
  `position.unwrap_or(99)`; an omitted position is not guaranteed to sort last.

## Current boundaries to understand before changing code

These describe the current implementation, not promises to preserve defects:

- Directory traversal errors are filtered out; readable matching files with I/O
  or TOML errors propagate a failure. The extension check uses `ends_with("toml")`,
  not strict equality. No separate validation layer currently checks all options.
- AppCUI attribute macros generate base fields, command enums, and event wiring.
  Imports and trait implementations that look unused can participate in generated
  code; inspect macro usage before removing them.
- Immutable shortcut indices connect external menu vectors and `app_windows`;
  categorized launch commands map menu handles to application IDs. Taskbar entries
  have typed window targets and separate popup/selector handles, and are removed
  along with their owned menus on closure. Native windows
  use typed handles; editor documents and extra commands have separate terminal
  handles. Closed windows can
  leave stale handles; callers check whether a handle still resolves. Keep these
  relationships intact when changing launch or close behavior.
- Window geometry mixes signed padding, unsigned sizes, and narrower canvas size
  conversions. Saturating subtraction alone does not validate padding or guarantee
  all padding and cast bounds. Inner dimensions are clamped to at least one cell;
  resize synchronization runs on the window timer.
- The terminal emulator is partial: it decodes each chunk with lossy UTF-8 and
  has no pending buffer for split UTF-8/ANSI sequences. It clamps at the bottom
  instead of implementing full terminal scrollback. SGR reset also resets cursor
  state. Keyboard fallback casts a character to one byte. Test desired behavior
  explicitly when extending ANSI or Unicode handling.
- Several error paths currently use `unwrap`, `expect`, or `.ok()`. Their presence
  is not a rule to copy into new fallible code. See the coding conventions.

## Native application workers

Each native window owns a latest-request worker with one active job, one pending
request, and one result. New requests replace pending work and invalidate older
results. Closing cancels pending work without joining from a callback; worker
state is separate for each window instance. Image workers cache decoded pixels
and prepare only the visible region, compositing alpha before AppCUI's true-color
SmallBlocks renderer prepares a surface. PNG/JPEG/WebP/GIF/BMP decoders are the
only enabled image crate format features; image input is capped at 32 MiB,
dimensions at 16 million pixels, and decoder max_alloc configured to 128 MiB where supported. WebP chunk extents
and embedded frame dimensions are checked separately before decoder construction
because that decoder does not enforce the allocation option. Fit uses
half-cell pixel geometry; explicit zoom is 5–400%, while Fit may be smaller.
Viewport preparation is bounded to 2048 × 1024 character cells.

File Browser enumerates and sorts directories outside callbacks, then inserts up
to 256 rows per tick. Original PathBuf values drive navigation and document
arguments; lossy text is used only for labels and the editable path display.
Native windows and all tracked terminal instances participate in arrangement,
palette updates, Terminal Lock visibility snapshots, and desktop shutdown.

Native timer callbacks defer result polling while hidden, including during Terminal
Lock. Failed terminal launch dialogs are also deferred until the window is visible.

The taskbar projects tracked terminal, native, document, and Help windows. It
shows visible instances and groups them under Apps when necessary. Hiding a
window retains it for Application > Background windows; closing unregisters its
popup and removes its app-bar item. Overflow lists paginate beyond100 entries
to stay within AppCUI keyboard navigation bounds.
