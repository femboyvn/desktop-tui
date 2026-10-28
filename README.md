Desktop-TUI 🖥️
===

This checkout is the owner's personal, independently maintained version in
`femboyvn/desktop-tui`. Personal use does not require a private GitHub repository.
Changes and reviews stay in this repository; development does not include
contributions to the original parent project or other forks.
Verify the destination and its independence before pushing, as described in
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
- [x] Offline Help and saved desktop/terminal palettes
- [x] Linux screen lock, shutdown, and restart controls

![demo](./demo.gif)

## How to use

### Install from this checkout

Use this personal checkout as the installation source:

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

If no path is provided, Desktop-TUI reads shortcuts from `~/.config/desktop-tui` on Linux (or `$XDG_CONFIG_HOME/desktop-tui` when set).

## Source layout

The binary uses technical layers, with functional groups inside each layer:

- `src/ui/`: desktop and panel views, preferences dialogs, terminal windows and keyboard input, locking screens, and theme construction.
- `src/config/`: preferences models, storage, palette presets and editing drafts, plus shortcut definitions and discovery.
- `src/runtime/`: terminal emulation and password credentials.
- `src/platform/linux/`: status readings, resource usage, and system actions.
- `vendor/appcui/`: local AppCUI dependency with a terminal rendering fix; see [patch notes](vendor/README.md).

Groups use a named entry file alongside their child folder (for example, `ui/desktop.rs` and `ui/desktop/`). Entry files declare private implementation modules and expose shared types and entry points within the crate. `main.rs`, `args.rs`, and test-only `test_support.rs` stay at the source root; `utils.rs` remains outside the module tree.

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

Read [CONTRIBUTE.md](CONTRIBUTE.md) for personal development and project review guidance.
Use [HOW_TO_RUN.md](HOW_TO_RUN.md) to build and launch the app and
[HOW_TO_TEST.md](HOW_TO_TEST.md) to verify a change.
Start with [AGENTS.md](AGENTS.md) for the development and Git workflow. See
[CODEBASE.md](CODEBASE.md) for setup and architecture,
[CODING_CONVENTION.md](CODING_CONVENTION.md) for coding conventions, and
[HOW_TO_IMPLEMENT_A_FUNCTION.md](HOW_TO_IMPLEMENT_A_FUNCTION.md) for implementation
and verification steps.

## License

The MIT license for this project can be seen [here](./LICENSE)

## Desktop menu and preferences

Linux is the supported target. Open **Desktop** for **Help**, **Preference**, **Terminal Lock**, **Screen Lock**, **Shutdown**, **Restart**, and **Exit**. Help opens a resizable offline guide with categories on the left and scrollable details on the right; its contents are also available in [HELP.md](HELP.md).

**Preference** has **Desktop**, **Terminal**, **Security**, and **Panel** tabs. Choose from **Femboy**, **Transgender**, **Asexual**, **Nonbinary**, and **Monochrome** presets, then click **Apply** to update the color draft across the Desktop and Terminal tabs. Browsing presets keeps existing edits until Apply is clicked. Monochrome uses neutral grays. Custom identifies individually edited palettes. Select a color role, enter a `#RRGGBB` color, and inspect the swatch. Desktop roles include the background, surfaces, text, muted text, borders, accents, and selection. Terminal roles include default foreground/background and all 16 normal and bright ANSI colors.

The default Femboy palette uses pale pink `#F6D9EE`, white `#FFF7FC`, plum `#3F2A4A`, pink accents `#E8A4C9`, and lavender selection `#C8B5EA`. **Reset to Femboy** changes the color draft and preserves password settings. **Save** validates and persists all edits, then immediately recolors the desktop and running terminal windows, including hidden windows. **Cancel** or Escape discards edits. Failed saves leave the active palette and saved password unchanged.

Settings live in `preferences.toml` in the application configuration folder, normally `~/.config/desktop-tui` on Linux (or `$XDG_CONFIG_HOME/desktop-tui`). A custom shortcut folder does not change this location. Only that exact settings file is excluded from shortcut discovery. Missing settings use defaults. Invalid settings produce one startup warning and use default colors without overwriting the original file. Terminal Lock refuses to start when password settings are malformed or unreadable.

The **Security** tab lets you set, change, or remove a persistent Terminal Lock password. Choose **Set / change saved password**, enter and confirm the new password, then **Save**. Changing or removing an existing password requires the current password. **Keep current setting** preserves it while editing colors. Cancel or Escape discards every tab's edits. Saved passwords are reused for Terminal Lock after restarting Desktop-TUI; when none is saved, locking asks for a temporary password each time. This setting does not change your Linux account password.

Only a salted **Argon2id** password verifier is saved; plaintext passwords are never written to disk. The verifier is stored with the palette in `preferences.toml`, using owner-only permissions: `0600` on the file and `0700` on the application configuration folder. Saves replace the file atomically, so password and color edits succeed together. Symlinked settings and unsafe password-storage permissions are rejected. If permissions were changed externally, restore them with `chmod 700 ~/.config/desktop-tui` and `chmod 600 ~/.config/desktop-tui/preferences.toml` (adjust for `$XDG_CONFIG_HOME`). Passwords cannot be recovered. Anyone with access to your account or permission to terminate Desktop-TUI can bypass its TUI lock; **Screen Lock** locks the graphical Linux session.

**Preference > Panel** configures the top-right status display. Enable or hide the panel, clock, date, Wi-Fi/network status, battery, CPU usage, RAM usage, disk usage, and network download/upload rates. Choose a 12- or 24-hour clock, seconds, a short/ISO/day-month date, and optional network interface names. **Save** applies these options immediately and persists them with the palette and password; Cancel or Escape discards them. Color presets and Reset to Femboy preserve panel settings. Ctrl+Tab cycles all four tabs; Alt+4 opens Panel and Alt+G focuses its date format.

The clock uses the computer's local time. Device and resource readings refresh in the background every three seconds from Linux `/sys` and `/proc`, without extra commands or privileges. Wi-Fi shows an approximate signal percentage when the driver provides it, or `WiFi up`; wired and other routed connections show `Net up`. `Net offline` means no usable local link was detected, and `Net N/A` means status could not be read. Link status does not verify Internet access. The battery is omitted when unavailable, and `+` indicates charging; multiple system batteries use their mean percentage. Device/peripheral batteries are excluded.

CPU shows average busy time across all cores, including system and interrupt work, between samples. RAM usage subtracts `MemAvailable` from total RAM, with a cache-aware fallback on older kernels. Disk shows space usage on `/`, accounting for blocks reserved from ordinary users. Network rates use the active interface shown by the connection indicator, with separate received (Rx) and transmitted (Tx) bytes per second; they are not cumulative totals. CPU and network rates need two valid samples before displaying a value. Missing readings and reset/switched interface counters show N/A instead of fabricated zeroes.

The status display sits at the top right, with Desktop, Tilling, and application menus on the left. On narrow screens it uses compact labels: `W` Wi-Fi, `B` battery, `C` CPU, `M` RAM, `D` disk, and `v`/`^` network download/upload. Percentages are marked with `%`; compact traffic units B/K/M/G mean bytes/KiB/MiB/GiB per second. The compact date is day/month. Hover over the display for full multiline readings. If the compact display still cannot fit, whole indicators are omitted, keeping the clock and connection first. When individual application menus would not fit, **Apps** lists every application: select one to open its usual Start, Hide/Show, Close, and additional-command menu. All status indicators and desktop menus are hidden during Terminal Lock.

Existing default and ANSI colors, including indexed colors 0–15, follow palette changes. Explicit RGB colors and indexed colors 16–255 stay fixed. A shortcut's `terminal.background_color` remains the terminal's default background, including after ANSI resets.

Click Desktop, Tilling, or an application name in the top bar to open its menu. Navigate menus with the arrow keys and Enter. Tab and Shift+Tab move between dialog controls; Ctrl+Tab switches Preference tabs. Alt+S saves, Alt+C cancels, and Alt+R resets the draft. Alt+P focuses the preset selector and Alt+A applies its selection. Alt+U opens Security, Alt+L focuses the password action, and Alt+O / Alt+N / Alt+F focus current / new / confirmation password fields. Ctrl+U clears a password field. Hide/Show keeps an application running; Start opens it, and Close stops it. The Tilling menu offers Cascade, Vertical, Horizontal, and Grid arrangements, or No arrangement for manual positioning.

## Terminal Lock

**Desktop > Terminal Lock** hides and locks the TUI desktop while applications keep running. It reuses the password saved in **Preference > Security**. When none is saved, set and confirm a temporary password, then click **Lock**; Cancel or Escape dismisses setup. Once locked, enter the same password and press Enter or click **Unlock**. Escape cannot dismiss the lock screen. Incorrect attempts have a one-second retry delay, and Ctrl+U clears password input.

Unlocking restores previously visible windows and keeps hidden windows hidden, preserving terminal content and processes. Input buffers are cleared after use. Saved passwords persist as salted Argon2id verifiers; temporary passwords last for one lock session and are never written to disk.

This locks the current Desktop-TUI environment. It does not lock Linux, other terminals, or prevent someone with permission from terminating the process. Use **Screen Lock** for graphical desktop locking. Temporary lock passwords cannot be recovered if forgotten.

## Linux system actions

**Screen Lock** requests your graphical desktop's authentication screen using [`loginctl lock-session`](https://www.freedesktop.org/software/systemd/man/257/loginctl.html), passing `XDG_SESSION_ID` when present. If it fails or is unavailable, Desktop-TUI tries [`xdg-screensaver lock`](https://portland.freedesktop.org/doc/xdg-screensaver.html). Launch within a graphical Linux session with `DISPLAY` or `WAYLAND_DISPLAY` set and a desktop locker enabled. Screen Lock does not ask for a password inside Desktop-TUI or verify that the graphical desktop actually locked.

**Shutdown** and **Restart** control the computer and require confirmation. Cancel is initially focused; Escape cancels. Requests use these exact commands:

```shell
systemctl --no-ask-password --no-wall --check-inhibitors=yes poweroff
systemctl --no-ask-password --no-wall --check-inhibitors=yes reboot
```

The explicit [inhibitor check](https://www.freedesktop.org/software/systemd/man/255/systemctl.html#--check-inhibitors=) respects blocking application inhibitors during noninteractive requests. Compatible systemd tooling and session permissions are required; an unavailable command, denied request, or unsupported environment produces an error with guidance.

Only one system request runs at a time, off the UI thread. Helpers have a 15-second timeout and are terminated and reaped on timeout; the reported outcome is uncertain, so check the system before trying again. Power requests are never retried automatically. Desktop-TUI and its applications keep running while requests are pending or fail. **Exit** retains its separate behavior of closing Desktop-TUI and managed applications.

## Validation

```shell
cargo test --locked --offline
```

Tests use fake system runners to cover command selection, cancellation, duplicate requests, permission errors, unavailable commands, and timeouts. Harmless helper processes check timeout cleanup, and terminal processes check live recoloring of visible and hidden windows. UI interaction checks run at 80×24. Verify real screen locking in a graphical desktop and actual power actions only in a disposable VM.
