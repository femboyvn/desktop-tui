# How to run Desktop-TUI

Run commands from the repository root. For development workflow, read
[AGENTS.md](AGENTS.md) and [CONTRIBUTE.md](CONTRIBUTE.md); for architecture and
current runtime limits, see [CODEBASE.md](CODEBASE.md).

## Prerequisites

- A Unix-like environment: the child-terminal dependency uses Unix PTYs.
- Rust and Cargo supporting edition 2024, plus a native compiler/linker. There
  is no pinned toolchain or established minimum supported Rust version.
- A C compiler for the compiled native Music Player output backend.
  Playback uses the host's sound device and audio session; no separate music
  player is required.
- Registry access for the first build, unless the required dependencies are
  already cached. Keep the tracked [Cargo.lock](Cargo.lock).
- A real terminal for the desktop. The configured backend is CrossTerm with
  true-color support; a noninteractive command runner cannot verify the UI.

Native dependencies may need system libraries or build tools; use the actual
build diagnostic to identify what is missing. The optional [shell.nix](shell.nix)
provides assorted native tools and sample apps through `nix-shell`, but imports
unpinned `<nixpkgs>` and does not install Rust itself.

## Build and check the CLI

```sh
cargo build --locked
cargo build --locked --release
cargo run --locked -- --help
```

The executables are `target/debug/desktop-tui` and `target/release/desktop-tui`.
Release settings in [.cargo/config.toml](.cargo/config.toml) favor binary size.
`--help` exits before initializing the terminal and is safe to check without
starting the desktop. `--doctor` is also noninteractive and checks configuration
and tools without launching commands or creating configuration directories.
Add `--offline` only when dependencies are cached; a
dependency-fetch failure is not fixed by dropping `--locked`.

## Select a shortcut directory

In a real terminal, start with an explicit directory:

```sh
cargo run --locked -- ./shortcuts
```

The built binary accepts the same optional positional directory:

```sh
./target/release/desktop-tui ./shortcuts
```

Relative paths resolve from the process's working directory. An explicit
directory is created if necessary and does not change the settings location. Shortcut files are read recursively;
choose a directory whose commands you intend to launch. The repository samples
refer to separately installed programs such as `hx`, `btop`, and `atac`; they are
not bundled. [shortcuts/crash.toml](shortcuts/crash.toml) intentionally names a
missing executable, `something`.

With no directory argument, [src/config/preferences/storage.rs](src/config/preferences/storage.rs) obtains `config_dir()` from
`ProjectDirs::from("com", "Julien-cpsn", "desktop-tui")` and creates that directory
if it does not exist. Settings live in `preferences.toml` there, including when
an explicit shortcut directory is used. Typical locations are:

| Platform | Configuration directory |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/desktop-tui` when the variable is absolute; otherwise `$HOME/.config/desktop-tui`. |
| macOS | `$HOME/Library/Application Support/com.Julien-cpsn.desktop-tui`. |

No sample shortcuts are copied into the default directory. Platform-specific
configuration paths do not establish runtime support beyond the Unix PTY
requirement. An empty directory still provides the thirteen starter apps through Application.
External starters (`sh`, `btm`, and `nano`) require separately installed tools.
Carboxyl is included in the portable Linux package; source/Cargo installations
can use a separately installed copy.
Music Player, ASCII Paint, and Kanban are native and compiled into the desktop binary.
Snake, 2048, and Minesweeper are built in under Application > Games.

## Run over SSH or another PTY

Use a terminal allocation when starting a remote desktop:

```sh
ssh -t user@host 'desktop-tui /path/to/shortcuts'
```

Desktop-TUI, its commands, file paths and saved state belong to the remote host.
No X11 forwarding or graphical session is required. A display-free local launch
can use:

```sh
env -u DISPLAY -u WAYLAND_DISPLAY ./target/release/desktop-tui ./shortcuts
```

The internal clipboard works between terminal windows and native text controls.
Use terminal Copy mode and the Paste palette action for a child shell; native
text controls use Ctrl+C/Ctrl+V. Your terminal client's normal paste also reaches
the focused control. Copying to the client operating system's clipboard depends
on the client terminal; the internal clipboard does not claim to update it.
Optional X11/Wayland helpers are only used when the server advertises a graphical
session. Missing display variables and clipboard helpers are normal over SSH.

The input thread checks the actual PTY size at most 100 ms apart, including while
output is quiet. This also handles relays that do not deliver a usable resize
notification. The selected tiling layout follows outer resizing; ordinary
windows are kept within the available desktop when it shrinks. If the client
intercepts Ctrl+Space, use the Desktop menu and save another shortcut.

## Use desktop commands and workspaces

**Ctrl+Space** opens the searchable command palette while an app has focus.
Use **Desktop > Command palette...** if the host terminal or input method takes
that key. Type words, move with arrows/PageUp/PageDown, and press Enter; Escape
cancels. **Desktop > Desktop shortcut...** saves another modified key/function
key. Press the configured prefix again in the open palette to forward it to the
originating terminal; the default Ctrl+Space twice sends literal Ctrl+Space.
Menus, modal dialogs and Terminal Lock keep their own input.

**Desktop > New terminal**, also a palette action, creates an independent
configured Terminal. Normal application launchers restore their existing window.
Ctrl+C sends a normal child interrupt; a window's Close action requests explicit
shutdown. Shift+PageUp/PageDown scroll terminal history. With the terminal
focused, use palette actions **Terminal: Copy mode**, **Search history**, **Paste**
and **Return to live**. Copy mode uses arrows, Space to anchor, Enter to copy or
mouse dragging; Search uses literal text, Enter/Shift+Enter and Escape. Title
status reports mode and clipboard results. Optional `wl-copy`/`wl-paste`, `xclip`
or `xsel` provide optional graphical copying/pasting for terminal windows.
Native controls share the terminal worker's internal clipboard; graphical
provider failures leave that clipboard usable.
See [README](README.md#keyboard-control-and-terminal-history) for limits.

Choose **Desktop > Workspaces > Workspace 1–4** or search **Switch to workspace N**
in the palette. **Move current window to workspace N** moves the window focused
before opening the palette. Switching keeps apps alive and preserves hidden
state; focusing a window on another workspace switches there. Tiling affects the
current workspace's visible windows.

Desktop Exit saves a session and then cleans up managed processes. A later start
offers **Restore** before relaunching saved apps; Cancel leaves them closed.
**Desktop > Restore saved workspace...** offers it again. Missing launchers/files
are reported individually. The snapshot records layout, workspace/hidden state,
launch identities, working directories and saved paths; it does not preserve
terminal output, process memory, unsaved buffers or game boards.

Configuration and state are separate. `preferences.toml` stores settings,
palette rebinding and saved Open with defaults in the configuration directory.
`session.toml` and `state/browser.toml` live under the application state directory,
normally `$XDG_STATE_HOME/desktop-tui` or `~/.local/state/desktop-tui` on Linux.
A custom shortcut directory redirects none of these. Browser state remembers
location, view, filter, hidden toggle and favorites. Use **Open with > Always use
for .extension** to save a default; use Open with again to choose another.

## Use Kanban boards

Choose Application > Utility > Kanban for a new board; each launch makes a
separate window. Use File > Open (Ctrl+O) for an existing `.kanban` file or
open it in File Browser. File > Save (Ctrl+S) and Save As (Ctrl+Shift+S) write
explicitly. Task (Alt+T) and Columns (Alt+C) provide editing actions; arrows,
Enter, Insert, Delete, and Shift+arrows provide keyboard navigation and movement.
See [README](README.md#applications-and-files) for board limits and controls.

Close and desktop power/exit actions ask about unsaved work across Paint and
all boards. Cancel keeps the desktop running. Saved paths restore through the
normal workspace mechanism; unsaved content remains outside session storage.

## Use the bundled Carboxyl browser

The portable Linux x86-64 archive contains both `desktop-tui` and
`libexec/carboxyl` 0.2.0. Extract the complete archive, keep that layout, and run
`./desktop-tui` from the extracted directory. Web Browser finds its companion
relative to the desktop executable, including when launched from another working
directory. No separate Carboxyl installation or PATH change is needed.

The upstream browser binary requires **glibc 2.43 or newer**, libstdc++ providing
`GLIBCXX_3.4.30`, zlib and Fontconfig with their dependencies. The package was
checked on Ubuntu 26.04 x86-64. It contains the applications, sample shortcuts,
license files and browser provenance; it does not replace system libraries.
Older distributions need Carboxyl built from source for their environment.

To build the archive from this checkout (Python 3.11+, Rust and network access):

```sh
python3 scripts/package-linux.py
```

The script builds Desktop-TUI with `cargo build --locked --release`, downloads
and verifies the pinned browser release, and writes the archive and SHA-256
checksum under `target/dist/`. To reuse a previously downloaded browser archive:

```sh
python3 scripts/package-linux.py --carboxyl-archive /path/to/carboxyl-v0.2.0-x86_64-linux.tar.gz
```

The package includes `shortcuts/`; pass its path explicitly to use all samples.
Their other external programs still require installation. A bare `carboxyl`
command prefers an executable `libexec/carboxyl` beside Desktop-TUI. An explicit
command path such as `/opt/carboxyl` or `./carboxyl` is preserved, as are custom
arguments and same-name shortcut overrides. If no executable companion exists,
normal PATH lookup remains available.

## Install the Carboxyl browser separately

For source builds or `cargo install --path . --locked`, install the binary from the pinned
[0.2.0 release](https://github.com/carboxyl-rs/carboxyl/releases/tag/v0.2.0)
and put it on PATH, or build from its Git tag:

```sh
cargo install --git https://github.com/carboxyl-rs/carboxyl --tag v0.2.0 --locked carboxyl
carboxyl --version
```

Follow the pinned project's [build guidance](https://github.com/carboxyl-rs/carboxyl/tree/v0.2.0#build)
for Servo's native build requirements. This is a separate browser installation;
its toolchain and native dependencies need not match Desktop-TUI's.
The crates.io package called `carboxyl` is a different project: do not use
`cargo install carboxyl` for this browser.

Launch **Application > Utility > Web Browser**. The built-in command is
`carboxyl --graphics quadrant --fps 20` with no URL, retaining native text and
starting on the blank page. Click the address field on Carboxyl's top row to edit it. To change the path,
arguments, category, or window options, use a same-name Web Browser shortcut;
[shortcuts/carboxyl.toml](shortcuts/carboxyl.toml) is a complete example.
A missing executable produces the normal managed launch-error dialog.

## Try a controlled command

Create an isolated directory and a complete, small shortcut:

```sh
desktop_tui_shortcuts_dir=$(mktemp -d)
cat > "$desktop_tui_shortcuts_dir/smoke.toml" <<'TOML'
name = "Smoke test"
command = "sh"
args = ["-c", "printf 'Desktop-TUI smoke test\n'; sleep 3"]

[taskbar]

[window]
resizable = true
close_button = true
fixed_position = false

[terminal]
TOML
cargo run --locked -- "$desktop_tui_shortcuts_dir"
```

Select **Application > User > Smoke test**. This explicitly runs Unix `sh` with a finite
`printf` command, waits briefly using `sleep`, then exits; its window closes on
child exit. In TOML, `\n` becomes a newline within the shell's quoted string.
The desktop itself remains open. For an interactive shell instead, edit the
shortcut to `args = []`, use **Desktop > Launchers... > Reload**, and select its
Application launcher again.
Remove the temporary directory after finishing.

Shortcuts are menu entries, not startup jobs. Open windows appear after **Tilling |**.
Use a window button’s **Hide (run in background)** or **Close** command. Restore
hidden instances through **Application > Background windows**. Use **Tilling**
for arrangement and **Desktop > Exit** to save the workspace snapshot and leave.
With the child control focused, Ctrl+C is a normal interrupt delivered to the
child; Close is the explicit shutdown action. A child that exits closes its window.
Escape on the desktop keeps the application open, including when a terminal
delivers it separately as the prefix of an Alt key sequence.

## Configure commands and diagnose problems

`command` is one executable and `args` is an array of separate arguments. There
is no implicit shell, expansion, pipeline, or redirection. Use an explicit
`sh` and `-c` only when shell syntax is intended. An argument containing spaces
stays one argument. See [README.md](README.md) for the fuller shortcut example.

`<FILE_PATH>` and `<FOLDER_PATH>` in the executable or individual arguments open
selection dialogs before launch. Cancellation prevents that launch. Additional
commands in a shortcut's menu use their own executable and argument array.

Use **Desktop > Launchers...** to select/Edit or New, Save, then Reload. Existing
running windows stay open across reload. Optional `id` preserves identity across
renames; without it, name is the identity. Duplicate names or IDs are errors.
Optional `working_directory` is absolute or relative to its TOML file's parent;
File Browser's explicit folder/file context takes precedence. Save keeps an
existing launcher's source file, refuses stale edits and validates drafts before
atomic replacement. Exact lowercase `.toml` files are discovered recursively;
traversal/read/parse errors report the path instead of being silently skipped.

From the repository root, run the read-only report:

```sh
cargo run --locked -- --doctor ./shortcuts
```

An installed binary accepts the same flag and optional directory from any
working directory; pass an absolute shortcut path when needed. Without a
directory it inspects the default configuration location. Unlike normal startup,
doctor does not create a missing directory; that is a configuration error. It
checks preferences, launchers/cwd, executable resolution (including bundled
Carboxyl), Poppler, graphical-session variables and clipboard tools. Configuration
or Terminal executable errors return nonzero; missing optional tools warn.
Configured commands are never executed. **Desktop > Diagnostics...** displays the
same report asynchronously. Availability checks do not prove a graphical
clipboard connection or browser site compatibility.

| Symptom | Check |
| --- | --- |
| Custom launcher missing | Check Application > User or the configured category. Confirm the selected folder and exact .toml files exist; check reported traversal/parse errors and duplicate names/IDs. Reload saved edits through Desktop > Launchers. |
| Startup reports a TOML error | Compare the file with the complete example: `name`, `command`, all three tables, and the three window booleans are required. |
| A command fails to start | Check that its executable is installed and available on `PATH`, or give its path. Use --doctor for read-only executable/cwd checks; --help only validates CLI syntax. |
| Saved app default unavailable | Open with lets you replace a missing saved extension association. Save the choice with Always use for .extension. |
| Clipboard uses internal fallback | This is normal over SSH and needs no repair. Optional graphical integration uses DISPLAY/WAYLAND_DISPLAY and wl-copy/wl-paste, xclip or xsel; --doctor checks availability only. |
| Restore skips a window | Check the stable launcher ID and saved file/cwd path. Restoring launches a new process; missing entries are reported without stopping other restores. |
| Output or resizing looks wrong | Reproduce with a controlled command and record terminal size. Record the exact UTF-8/escape stream. The vendored emulator handles incremental UTF-8, graphemes, alternate screens and terminal modes; image protocols are disabled. Resize synchronization also runs while output is quiet. |
| Build cannot fetch dependencies | Restore registry access, or use `--offline` with a populated cache. Record the actual error. |

Use [HOW_TO_TEST.md](HOW_TO_TEST.md) for checks and reporting. Follow
[CODING_CONVENTION.md](CODING_CONVENTION.md) and
[HOW_TO_IMPLEMENT_A_FUNCTION.md](HOW_TO_IMPLEMENT_A_FUNCTION.md) when changing
the behavior rather than working around it in a shortcut.

Native apps are available under **Application > System > File Browser** and
**Application > Utility > Image Viewer / PDF Reader / Music Player / ASCII Paint**, with Snake, 2048,
and Minesweeper under **Application > Games**. See
[README.md](README.md#applications-and-files) for image/audio formats, limits,
controls, ASCII Paint projects/text export, and Text editor document handling. PDF Reader needs Poppler's
`pdfinfo` and `pdftoppm` on PATH (`poppler-utils` on Debian/Ubuntu or `poppler`
on Arch); rendering runs outside the UI and needs no graphical session.
