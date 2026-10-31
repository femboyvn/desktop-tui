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
The Carboxyl integration and pinned terminal core were checked with Rust 1.97.1.

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
| [src/runtime/music/spectrum.rs](src/runtime/music/spectrum.rs) | Preallocated bounded capture, per-channel Hann/FFT analysis, 16 logarithmic bands, and bar/peak decay. |
| [src/runtime/ascii_paint.rs](src/runtime/ascii_paint.rs) | Bounded RGB cell document, grouped undo/redo, project/text validation, atomic writes, and one-operation file worker. |
| [src/ui/native/ascii_paint.rs](src/ui/native/ascii_paint.rs) | Paint canvas input/rendering, brush/dimensions dialogs, file actions, and unsaved-action guard. |
| [src/runtime/pdf.rs](src/runtime/pdf.rs) | Bounded anonymous PDF snapshots, Poppler metadata/page rendering, cancellation, timeouts, and helper reaping after the event loop. |
| [src/ui/native/](src/ui/native/) | Native Image Viewer, PDF Reader, File Browser, Music Player, ASCII Paint, and game windows and controls. |
| [src/ui/native/games.rs](src/ui/native/games.rs), [src/ui/native/games/](src/ui/native/games/) | Shared game window/input/rendering and bounded Snake, 2048, and Minesweeper rules. |
| [src/config/shortcuts.rs](src/config/shortcuts.rs) | Serde types generated through `nestify::nest!`; `parse_shortcut_dir` loads, deduplicates, and sorts shortcuts. |
| [src/ui/desktop/view.rs](src/ui/desktop/view.rs) | `MyDesktop`, app-bar menus, shortcut-to-window handles, launch/show/hide/close commands, arrangement, preferences/locking integration, and panel updates. |
| [src/ui/terminal/window.rs](src/ui/terminal/window.rs) | `TuiWindow`, child command/channels, canvas updates, resize synchronization, shutdown, file/folder selection. |
| [src/ui/terminal/keyboard.rs](src/ui/terminal/keyboard.rs) | `CustomKeyboardControl`; UTF-8/application-cursor keys, paste, mouse translation, input queue errors, and focused child cursor painting. |
| [src/runtime/terminal.rs](src/runtime/terminal.rs) | `TerminalParser` facade over pinned vendored WezTerm core; incremental byte feeds, grapheme projection, modes/cursor, mouse/paste encoding, bounded query replies, and palette resolution. |
| [src/ui/terminal/input_queue.rs](src/ui/terminal/input_queue.rs) | Ordered nonblocking 256 KiB pending input; adjacent motion/resize coalescing, small PTY writes, and explicit exhaustion errors. |
| [src/config/preferences.rs](src/config/preferences.rs), [src/config/preferences/](src/config/preferences/) | Settings models/defaults, atomic storage, palette presets, palette drafts, and password-editing drafts. |
| [src/config/preferences/ascii_background.rs](src/config/preferences/ascii_background.rs), [src/runtime/ascii_background.rs](src/runtime/ascii_background.rs) | Validated ASCII background text and bounded regular-file loading for Preference > Background. |
| [src/ui/desktop/](src/ui/desktop/) | Desktop status panel, offline Help, and system-action confirmation alongside the main view. |
| [src/ui/preferences/](src/ui/preferences/) | Preferences dialog and its custom controls. |
| [src/ui/locking/](src/ui/locking/) | Secret input, temporary-password setup, and the lock screen with private retry/input boundaries. |
| [src/ui/theme.rs](src/ui/theme.rs) | AppCUI theme construction. |
| [src/runtime/locking.rs](src/runtime/locking.rs) | Password credential creation, verification, and bounded serialization. |
| [src/platform/linux/](src/platform/linux/) | Linux status readings, resource sampling, and system actions. |
| [src/platform/bundled.rs](src/platform/bundled.rs) | Resolve a bare `carboxyl` command to an executable sibling `libexec/carboxyl`, preserving explicit command paths and opaque OS path bytes. |
| [scripts/package-linux.py](scripts/package-linux.py), [packaging/](packaging/) | Build the Linux x86-64 archive with pinned checksum-verified Carboxyl, notices, provenance and sample shortcuts. |
| [src/test_support.rs](src/test_support.rs) | Test-only temporary directories and the shared UI lock. |
| [src/utils.rs](src/utils.rs) | Unregistered helper file, outside the module tree. |
| [shortcuts/](shortcuts/) | Example TOML launchers; the Linux package includes Carboxyl, while other external commands require installation. |
| [backgrounds/](backgrounds/) | Plain ASCII artwork examples for desktop backgrounds. |
| [Cargo.toml](Cargo.toml), [Cargo.lock](Cargo.lock) | Package/dependency declarations and tracked dependency resolution. |
| [vendor/](vendor/README.md) | Local AppCUI 0.4.6 Unicode/input/rendering changes and pinned `tattoy-wezterm-term` 0.1.0-fork.5 core with licenses/provenance. |
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
   preserving OS path bytes. A bare `carboxyl` then prefers the executable-relative
   `libexec/carboxyl` companion; explicit executable paths are preserved and a
   missing companion keeps PATH lookup. Selected editor document paths are appended as opaque
   arguments after that resolution.
5. `TuiWindow::new` configures a `virtual_terminal::Command`, creates a canvas and
   keyboard control, starts a 25 ms UI timer, and starts a managed runner on Tokio. Its supervisor observes PID messages
   independently of UI polling and forwards output through a bounded channel.
   Program and arguments are passed separately; the repository does not implicitly
   invoke a shell. A shell is used only when the shortcut explicitly names one.
6. The terminal control and window share `Rc<RefCell<TerminalParser>>` on the UI thread. Focused UTF-8 keys, mode-aware paste, mouse events and query replies enter the ordered nonblocking input queue. Ctrl+C
   is handled specially: it requests child termination and sets `should_exit`.
7. Each 25 ms window tick drains at most 64 output messages or 256 KiB, preserving
   a partial oversized message, then projects once to the canvas. PTY stdout and
   stderr are merged. Resize synchronizes PTY, emulator, canvas, input bounds and
   cursor even while output is quiet. Adjacent queued motion and resize updates
   coalesce; text and button transitions retain order. Pending input is capped at
   256 KiB; small writes avoid filling the PTY dependency's message buffer with
   large pastes. Exhaustion reports an error and closes the child through normal
   cleanup. Query replies have a separate 64 KiB buffer with the same error path.
   The child sees `TERM=xterm-256color` and `COLORTERM=truecolor`; image protocols
   are disabled and omitted from capability replies.
   Input closure waits for the runner's final error/exit and remaining output;
   a broken writer with a live child gets a nonblocking one-second grace before
   error cleanup. Only the timer's observed result marks a terminal window closed.
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
  Native File Browser/Image Viewer/PDF Reader/Music Player/ASCII Paint and game entries remain separate from custom
  shortcuts; names matching Terminal, System Performance, Text editor, or Web Browser override
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
- Terminal geometry clamps signed padding to the available client area and inner
  dimensions to 1..=10,000 before canvas/PTY casts. The window timer applies the
  same size and origin to the emulator, canvas and input control, and queues the
  ordered PTY resize while output is quiet or backpressured.
- The pinned terminal core supports incremental UTF-8/escape parsing, alternate
  screens, scrolling, wrapping, cursor state, erase operations, and modes. The
  local grapheme correction joins clusters split across feeds without delaying
  display. AppCUI keeps `Character: Copy` and sparse Surface metadata for full
  graphemes and continuation cells. Composition clips whole glyphs; CrossTerm
  prints each grapheme once and includes metadata in its repaint cache.
  Metadata-free surfaces retain SRF version 1; Unicode metadata uses version 2.
  Outer terminal fonts/shaping determine appearance. No image protocol passthrough
  or browser navigation layer is provided. Surface/emulator/PTY dimensions share
  the existing 10,000-cell-per-axis limit.
- Several error paths currently use `unwrap`, `expect`, or `.ok()`. Their presence
  is not a rule to copy into new fallible code. See the coding conventions.

## Native application workers

Music Player's nonblocking audio callback captures recently played, post-volume
interleaved samples into a preallocated ring holding one FFT window. Capture uses
`try_lock`; contention skips visualization capture while audio continues. The
100 ms playback worker copies and drains the ring, releases its lock, then uses
Symphonia's FFT separately for each channel and combines channel power, preserving
opposite-phase stereo. FFT length is the next power of two covering 50 ms, capped
at 16,384 frames per channel. A Hann window and energy normalization feed 16
logarithmic bands from 40 Hz to min(16 kHz, Nyquist) with fixed −60–0 dB scaling.
Snapshots carry default-zero normalized levels and peaks, plus the analysis upper
frequency for accurate axis labels even when paused, stopped, or finished. Attack
is immediate; bars fall one full height in 250 ms and peaks in one second, using elapsed worker
time. Empty capture decays to silence. Transport changes clear captured history
and display state; a visualization revision preserves resets across coalesced
pause/resume. Closure silences the callback immediately and worker teardown clears
capture. Hidden/locked windows keep analysis running and poll current snapshots on
restore. The native music control paints theme-colored ASCII bars beneath wrapped
text, limits columns to two characters, and merges bands to fit narrow surfaces.
A rectangle using the theme's line color encloses the bars and peaks; labels sit
outside its bottom edge on the same logarithmic frequency axis up to the track's
analysis limit. Overlapping labels are omitted. Three bar rows, two border rows,
and a label row must fit after the text. The default player window uses 70% of
desktop height to leave space for the frame.

Preference > Background loads ASCII text through a latest-request worker, then
keeps the validated copy in the dialog draft until Save. Its read-only preview
uses AppCUI TextArea. Save is disabled during loading; Clear and Cancel discard
pending results without waiting for I/O. Regular-file reads and normalized art
are capped at 16 KiB; CRLF becomes LF and tabs expand to four-column stops.
The optional `ascii_background` preferences string defaults to empty for older
settings. Desktop `OnPaint` centers the saved text using the palette's desktop
text/background colors and clips it to the surface. Rendering does no I/O;
source files are not needed after saving. Color presets preserve the draft art.

ASCII Paint owns a bounded `Document` (1–200 columns by 1–80 rows), with
row-major printable ASCII cells and explicit foreground/background RGB values.
Its original blank cell supplies erase/Clear colors. History retains 64 actions,
groups each mouse stroke, and compares dimensions/cells with a saved snapshot for
dirty state, including undo across a save. Explicit canvas resizing preserves the
top-left overlap and blank-fills expansion; history restores cropped cells and
dimensions. Window resizing affects only the viewport. A reserved footer displays
clickable size at the left and one-based cursor coordinates at the right. Palette
changes recolor the surrounding UI while cell RGB stays fixed.

`PaintWorker` accepts one immutable Open/Save/Export/ConvertImage operation at a
time; editing is frozen until completion. Version 1 TOML `.asciipaint` stores dimensions, the
original blank cell, and row-major cells, capped at 2 MiB. `.txt`/`.asc` imports
reuse background validation and bounded 16 KiB reads, CRLF normalization, and
four-column tab expansion, then enforce canvas dimensions. Imports are dirty and
untitled. Colorless exports preserve row padding and use LF; they do not update
the saved snapshot. Saves/exports use a same-directory temporary file and atomic
rename; failures retain the artwork and existing destination.

Image conversion reuses the image decoder and its regular-file, 32 MiB, 16 megapixel,
and orientation checks. It composites alpha onto black before sampling, fits chosen
cell bounds with 2:1 cell-height correction, and maps luminance to printable ASCII
density. Foreground can retain sampled RGB or use white for monochrome. Conversion
returns dirty untitled artwork only on success and uses the same unsaved-action
guard and modal reentry protection as other file actions.

The desktop keeps one Paint window and routes only `.asciipaint` files from File
Browser to it; existing text associations stay unchanged. Paint queues New,
Open, Close, and desktop Exit/Shutdown/Restart behind Save/Discard/Cancel.
A chosen save must succeed before continuation. Cancellation preserves other
apps. Pending prompts and results wait while hidden/locked; restoration uses the
launcher or Background windows. Desktop actions wait for Paint's guard before
closing other windows or issuing a system request.

Each Image Viewer and File Browser window owns a latest-request worker with one active job, one pending
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
to 256 entries per tick. A custom BrowserEntries control shares entries and selection
between the default responsive icon grid and Details; its row geometry drives
painting, hit testing, keyboard navigation, scrolling, and selection reveal.
It owns context menus and invalidates targets on navigation/resize/closure.
The File information menu action and Alt+Enter queue an entry snapshot for
BrowserWindow's timer to show in a read-only, scrollable FileInformationDialog.
It uses the name/path/kind/size/modified values already collected by the directory
worker, without new filesystem I/O in callbacks; Refresh obtains updated metadata.
BrowserWindow queues typed BrowserAction requests (Open/OpenWith/OpenTerminal)
for the desktop timer. The scrollable desktop OpenWithDialog uses stable AppId
values and labels compatible native viewers separately from every external shortcut.
Original PathBuf values drive navigation and document
arguments; lossy text is used only for labels and the editable path display.
The desktop routes case-insensitive MP3/WAV/FLAC/OGG extensions to Music Player,
supported images to Image Viewer, PDFs to PDF Reader, `.asciipaint` to ASCII Paint, and other files to the
configured Text editor. Audio opens preserve the original OS path and reuse and
restore the native player; its worker loads and starts the selected track.
Open with external choices and browser terminals use independent tracked terminal
handles rather than the shortcut singleton map. TuiWindow's LaunchContext binds a
selected file and parent, or a selected folder, before byte-preserving template
expansion; the file is appended only when no FILE_PATH template exists. These
contexts set the child directory through virtual-terminal's current_dir API,
without changing the desktop process directory. Default and document constructors
retain their existing picker and append behavior. ProcessGuard owns cleanup for
all terminal instances, including these launches and asynchronous launch failures.
Native windows and all tracked terminal instances participate in arrangement,
palette updates, Terminal Lock visibility snapshots, and desktop shutdown.

Native timer callbacks defer result polling while hidden, including during Terminal
Lock. Failed terminal launch dialogs are also deferred until the window is visible.

PDF Reader uses `PdfWorker` around the latest-request queue, retaining a join
handle for shutdown after AppCUI releases its controls. Poppler's `pdfinfo` and
`pdftoppm` must be installed on PATH. A regular file is copied into an anonymous
Linux memfd capped at 64 MiB; helpers read that snapshot through stdin, so paths
are never interpolated into commands. Each helper has a 15-second timeout,
bounded stdout/stderr pipes, and kill/wait cleanup on failure or cancellation.
Only one page raster (at most 2048 pixels per edge) is cached. The existing
image renderer prepares its viewport; `Zoom::FitScale` applies 25–1600% relative
to Fit without changing Image Viewer's absolute zoom limits. File Browser routes
case-insensitive `.pdf` extensions to the reader. Errors render inside its window.

The three native games share `GameWindow` and `GameControl`, with one instance per
`GameKind` and deterministic rules in private modules. They do no I/O and spawn
no workers or child processes. Snake uses a 140 ms timer, pauses on focus loss,
hiding, locking, or insufficient space, and resumes only on Space/P. Puzzle games
advance only on input. Game windows are bounded to the available desktop on
launch and terminal resize. Fixed board geometry survives resize; small windows show
a prompt and ignore game input. Minesweeper plants ten mines after the first
reveal, excluding that cell and its neighbors. Scores and boards live only for
the window's lifetime.

The taskbar projects tracked terminal, native, document, and Help windows. It
shows visible instances and groups them under Apps when necessary. Hiding a
window retains it for Application > Background windows; closing unregisters its
popup and removes its app-bar item. Overflow lists paginate beyond100 entries
to stay within AppCUI keyboard navigation bounds.
