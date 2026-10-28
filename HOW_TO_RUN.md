# How to run Desktop-TUI

Run commands from the repository root. For development workflow, read
[AGENTS.md](AGENTS.md) and [CONTRIBUTE.md](CONTRIBUTE.md); for architecture and
current runtime limits, see [CODEBASE.md](CODEBASE.md).

## Prerequisites

- A Unix-like environment: the child-terminal dependency uses Unix PTYs.
- Rust and Cargo supporting edition 2024, plus a native compiler/linker. There
  is no pinned toolchain or established minimum supported Rust version.
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
starting the desktop. Add `--offline` only when dependencies are cached; a
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
requirement. An empty directory still provides the five starter apps through Application.
External starters (`sh`, `btm`, and `nano`) require separately installed tools.

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

Select **Smoke test > Start**. This explicitly runs Unix `sh` with a finite
`printf` command, waits briefly using `sleep`, then exits; its window closes on
child exit. In TOML, `\n` becomes a newline within the shell's quoted string.
The desktop itself remains open. For an interactive shell instead, edit the
shortcut to `args = []`, restart the desktop, and select **Start** again.
Remove the temporary directory after finishing.

Shortcuts are menu entries, not startup jobs. Use their **Hide/Show** and
**Close** commands, the **Tilling** menu for arrangement, and **Desktop > Exit**
to leave. With the child control focused, Ctrl+C requests child termination and
closes its window; it does not follow ordinary shell interrupt semantics.
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

| Symptom | Check |
| --- | --- |
| Custom launcher missing | Check Application > User or the configured category. Confirm the folder and TOML files exist; traversal errors can be skipped. Names are deduplicated. |
| Startup reports a TOML error | Compare the file with the complete example: `name`, `command`, all three tables, and the three window booleans are required. |
| A command fails to start | Check that its executable is installed and available on `PATH`, or give its path. `--help` does not validate shortcut executables. |
| Output or resizing looks wrong | Reproduce with a controlled command and record terminal size. The emulator supports only part of ANSI; resize synchronization runs on the window timer, including while output is quiet. |
| Build cannot fetch dependencies | Restore registry access, or use `--offline` with a populated cache. Record the actual error. |

Use [HOW_TO_TEST.md](HOW_TO_TEST.md) for checks and reporting. Follow
[CODING_CONVENTION.md](CODING_CONVENTION.md) and
[HOW_TO_IMPLEMENT_A_FUNCTION.md](HOW_TO_IMPLEMENT_A_FUNCTION.md) when changing
the behavior rather than working around it in a shortcut.

Native apps are available under **Application > System > File Browser** and
**Application > Utility > Image Viewer**. See [README.md](README.md#applications-and-files)
for supported image formats, limits, controls, and Text editor document handling.
