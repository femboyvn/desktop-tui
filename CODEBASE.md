# Codebase guide

Desktop-TUI is an experimental terminal desktop that launches command-line apps
inside movable AppCUI windows. This repository is one Rust 2024 package with a
single binary entry point, `src/main.rs`. It has no library target, workspace
members beyond itself, CI workflow, or automated tests at present.

Development is intended to remain private and independent of the original parent
project. Keep work local or in an owner-approved private repository; do not send
contributions, issues, or pull requests to the parent or public forks, or sync
development with them. Follow [AGENTS.md](AGENTS.md) to verify a destination's
visibility and fork status before any authorized push. This policy does not
establish the current server-side repository settings. `publish = false` in
[Cargo.toml](Cargo.toml) disables Cargo package publishing; it does not control Git
hosting visibility. Preserve the inherited MIT license and attribution.

Read [AGENTS.md](AGENTS.md) for the working and Git workflow,
[CODING_CONVENTION.md](CODING_CONVENTION.md) for editing conventions, and
[HOW_TO_IMPLEMENT_A_FUNCTION.md](HOW_TO_IMPLEMENT_A_FUNCTION.md) for implementation
and verification steps.
The focused command guides are [HOW_TO_RUN.md](HOW_TO_RUN.md) and
[HOW_TO_TEST.md](HOW_TO_TEST.md); private development and handoff expectations are in
[CONTRIBUTE.md](CONTRIBUTE.md).

## Setup and commands

Use a Rust toolchain that supports edition 2024, Cargo, and a native compiler/linker.
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
development and manual verification. The README lists platform-specific config
paths, but those paths do not establish Windows runtime support. Native build
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
| `cargo test --locked` | Compile and run tests; the current tree contains none. |
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
| [src/shortcut.rs](src/shortcut.rs) | Serde types generated through `nestify::nest!`; `parse_shortcut_dir` loads, deduplicates, and sorts shortcuts. |
| [src/desktop.rs](src/desktop.rs) | `MyDesktop`, app-bar menus, shortcut-to-window handles, launch/show/hide/close commands, arrangement, and clock timer. |
| [src/tui_window.rs](src/tui_window.rs) | `TuiWindow`, `CustomKeyboardControl`, child command/channels, canvas updates, resize synchronization, shutdown, file/folder selection. |
| [src/keyboard.rs](src/keyboard.rs) | `OnKeyPressed` for `CustomKeyboardControl`; `to_escape_sequence_vec` and helpers translate AppCUI keys into terminal bytes. |
| [src/terminal_emulation.rs](src/terminal_emulation.rs) | Stateful `TerminalParser`; text/control characters, supported ANSI commands, cursor state, styles, and color conversion into an AppCUI `Surface`. |
| [src/utils.rs](src/utils.rs) | `time_to_string`, which formats local time as `%H:%M ` for the app bar. |
| [shortcuts/](shortcuts/) | Example TOML launchers; executable names are external tools, not bundled binaries. |
| [Cargo.toml](Cargo.toml), [Cargo.lock](Cargo.lock) | Package/dependency declarations and tracked dependency resolution. |
| [.cargo/config.toml](.cargo/config.toml) | Development/release profiles; release uses `opt-level = "z"`, LTO, one codegen unit, and symbol stripping. |
| [rustfmt.toml](rustfmt.toml) | `disable_all_formatting = true`; preserve manual source style. |
| [.agents/skills/](.agents/skills/) | Repository-local brainstorming, testing, and OS integration workflows; see `AGENTS.md` for selection and invocation. |
| [.gitignore](.gitignore) | Ignores Cargo output, IDE directories, and `.DS_STORE`. |
| [README.md](README.md), [LICENSE](LICENSE) | User instructions, shortcut example, and MIT license. |

## Runtime flow

1. `main` parses the optional shortcut directory. With no argument it uses
   `ProjectDirs::from("com", "Julien-cpsn", "desktop-tui")` and creates the config
   directory if necessary. An explicit argument bypasses that creation step.
2. `parse_shortcut_dir` recursively walks the selected directory, deserializes
   matching files, drops duplicate shortcut names, and sorts by taskbar position.
3. `MyDesktop::on_start` creates Desktop/Tilling menus and one app menu per
   shortcut, then starts the two-second clock timer. Apps start on menu commands.
4. `MyDesktop::create_window` passes the selected command, args, and window/terminal
   options to `TuiWindow::new`. Placeholder dialogs resolve `<FILE_PATH>` and
   `<FOLDER_PATH>` in both the executable and individual arguments before launch.
5. `TuiWindow::new` configures a `virtual_terminal::Command`, creates a canvas and
   keyboard control, starts a 25 ms UI timer, and spawns `cmd.run()` on Tokio.
   Program and arguments are passed separately; the repository does not implicitly
   invoke a shell. A shell is used only when the shortcut explicitly names one.
6. Keyboard events on the focused control become `Input::Data` messages. Ctrl+C
   is handled specially: it requests child termination and sets `should_exit`.
7. The window timer uses `try_recv` to consume one output message per update.
   PTY output arrives as `Output::Stdout`, including merged stdout/stderr. The
   parser updates a copied surface, then the window replaces the canvas surface.
   Size changes are synchronized to the child, canvas, and parser in this output
   path. Errors and child termination close the window.
8. `close_command` requests termination and closes both channels before closing
   the window. The desktop Exit command invokes it on reachable window handles,
   closes the desktop, and `main` exits the process after the UI loop returns.

## Configuration contract

The complete user example lives in [README.md](README.md). Source definitions in
`src/shortcut.rs` govern deserialization:

- `name` and `command` are required; top-level `args` defaults to an empty vector.
- The `taskbar`, `window`, and `terminal` tables are required. Taskbar position is
  optional; `additional_commands` defaults to empty. Each additional command has
  a required name, executable, and argument list.
- Window booleans (`resizable`, `close_button`, `fixed_position`) are required.
  Window size is optional; `TuiWindow::new` uses 100 by 25 when it is absent.
- Terminal padding and RGB background are optional. The window defaults to a
  black background. Padding affects both canvas placement and inner dimensions.
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
- Shortcut indices connect menu vectors and `app_windows`. Closed windows can
  leave stale handles; callers check whether a handle still resolves. Keep these
  relationships intact when changing launch or close behavior.
- Window geometry mixes signed padding, unsigned sizes, and narrower canvas size
  conversions. Saturating subtraction alone does not validate padding or guarantee
  nonzero dimensions. Resize handling currently depends on receiving stdout.
- The terminal emulator is partial: it decodes each chunk with lossy UTF-8 and
  has no pending buffer for split UTF-8/ANSI sequences. It clamps at the bottom
  instead of implementing full terminal scrollback. SGR reset also resets cursor
  state. Keyboard fallback casts a character to one byte. Test desired behavior
  explicitly when extending ANSI or Unicode handling.
- Several error paths currently use `unwrap`, `expect`, or `.ok()`. Their presence
  is not a rule to copy into new fallible code. See the coding conventions.
