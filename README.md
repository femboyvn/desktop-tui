Desktop-TUI 🖥️
===

This checkout is maintained for private, independent development in
`femboyvn/desktop-tui`. Changes and reviews stay in that repository; development
does not include contributions to the original parent project or public forks.
Verify that the remote is private and detached before pushing, as described in
[AGENTS.md](AGENTS.md). Cargo package publishing is disabled.

> [!WARNING]
> This is an experimental terminal desktop. Review the current limitations in
> [CODEBASE.md](CODEBASE.md) before relying on it.

A desktop environment without graphics (tmux-like).

Features:
- [x] Parse shortcut files containing apps
  - [x] Custom additional commands
  - [x] Custom window options
  - [x] Custom terminal options
- [x] Display any application or command that uses stdout
  - [x] Move and resize windows
  - [x] Handle and display application error
- [x] Change tilling options
- [x] Can let the user select a file or a folder to use its path as a command argument
- [x] Clock

![demo](./demo.gif)

## How to use

### Install from this checkout

Use the private checkout as the installation source:

```shell
cargo install --path . --locked
```

### Compile

```shell
cargo build --locked
```

```shell
cargo build --locked --release
```

### Run

You can replace `cargo run --locked --` with the locally installed `desktop-tui`.

```shell
cargo run --locked -- ./shortcuts
```

If no path is provided, desktop-tui will try using theses paths:
- Linux: `/home/my_username/.config/desktop-tui`
- macOS: `/Users/MyUsername/Library/Application Support/com.Julien-cpsn.desktop-tui`
- Windows: `C:\Users\MyUsername\AppData\Roaming\Julien-cpsn\desktop-tui\config`

## Shortcut file

Example `helix.toml` shortcut file:

```toml
# Window name
name = "Text editor"

# Command to execute
command = "hx"
# Each command argument
args = []

[taskbar]
# Shortcut position on the action bar
# Optional
position = 3

# Optional
[[taskbar.additional_commands]]
# Command name
name = "Open folder"
# Command to execute
command = "hx"
# <FILE_PATH> or <FOLDER_PATH> will be replaced by a path selected in a dialog
args = ["<FOLDER_PATH>"]

[[taskbar.additional_commands]]
name = "Open file"
command = "hx"
args = ["<FILE_PATH>"]

[window]
resizable = true
close_button = true
fixed_position = false
# Optional
size = { width = 10, height = 5 }

[terminal]
# Pad inner window
padding = [0, 0]
# Optional
background_color = { r = 30, g = 30, b = 30 }
```

## Development

Read [CONTRIBUTE.md](CONTRIBUTE.md) for private development and internal review guidance.
Use [HOW_TO_RUN.md](HOW_TO_RUN.md) to build and launch the app and
[HOW_TO_TEST.md](HOW_TO_TEST.md) to verify a change.
Start with [AGENTS.md](AGENTS.md) for the development and Git workflow. See
[CODEBASE.md](CODEBASE.md) for setup and architecture,
[CODING_CONVENTION.md](CODING_CONVENTION.md) for coding conventions, and
[HOW_TO_IMPLEMENT_A_FUNCTION.md](HOW_TO_IMPLEMENT_A_FUNCTION.md) for implementation
and verification steps.

## License

The MIT license for this project can be seen [here](./LICENSE)
