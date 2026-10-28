# Desktop-TUI Help

This guide is bundled with Desktop-TUI and works offline.
Select a category on the left to read its details on the right.
Use Up/Down to choose, Tab to enter details, and Page Up/Page Down
or the mouse wheel to scroll. Drag the divider to resize the panes.

## Launching applications

Open Application in the top bar, choose a category, then an app.
Selecting it again restores and focuses its existing window.
Hover another category to switch an opened submenu without clicking.
System, Utility, User, and Games are always present; extra categories
follow alphabetically. Empty categories show No apps yet.

File Browser, Terminal (sh), System Performance (btm / bottom),
Image Viewer, and Text editor (nano) are available with an empty
shortcut folder. External tools must be installed separately.
System Performance uses bottom’s default-light theme to match the
default terminal background. A custom shortcut can choose another theme.
Shortcuts with the same external starter names replace them.
Native browser/viewer identities remain separate.

External app menus also offer Start, Hide/Show, and Close.
Additional commands appear below Close; they can open a file or
folder picker when a shortcut uses `<FILE_PATH>` or
`<FOLDER_PATH>`. Additional commands open separate windows.
Choose Close to stop the primary application process.

## File Browser

Application > System > File Browser starts in your home directory.
Enter an absolute or relative path, or double-click a folder in the listing.
Double-click a regular file to open it; Enter opens the selected row.
Up (Alt+U) opens the parent, Refresh (Alt+R) reloads, and Hidden
(Alt+H) toggles dot files. Directories sort before files; columns
show name, type, byte size, and local modification time. Failed
background reads leave the previous listing usable.

Open supported images in Image Viewer. Other regular files open in
independent Text editor document windows, preserving any running
editor. The selected absolute path is one OS argument, appended
after configured templates are resolved. Display labels may replace
invalid UTF-8 bytes; navigation and launch arguments retain them.
File Browser does not modify files or open special files.

## Image Viewer

Application > Utility > Image Viewer opens a file picker.
The picker keeps the colors of windows behind it. Ctrl+O
opens another file. F fits, 1 shows actual size, +/- zoom by 1.25,
and the mouse wheel zooms in (up) or out (down).
Arrows or mouse dragging pan. Explicit zoom is 5–400%; Fit can
use less than 5% for large images. At actual size, each character
cell displays one source pixel horizontally and two vertically.
The title shows filename, oriented dimensions, and zoom.

PNG, JPEG, WebP, GIF, and BMP are supported. Animated files show
the first frame. Aspect ratio and image orientation are preserved;
transparency uses the current theme background. Decoding and
viewport preparation happen in a bounded background worker. Files
are limited to 32 MiB and 16 megapixels. Corrupt, unsupported, or
oversized images show an error; Ctrl+O can open another file.
There is no image editing or animation playback.

## Window controls

Drag the title bar to move a window. Drag its bottom-right corner
to resize a resizable window. Use the maximize button to maximize
or restore it. Close buttons stop the window; Ctrl+C in a terminal
stops its running command.

Hide in an application's menu hides its window while the process
keeps running. Choose Show to bring it back. Start does not create
another copy of an already running application.

## Tiling

The Tilling menu offers No arrangement, Cascade, Vertical,
Horizontal, and Grid. The chosen arrangement is reapplied when
windows open or close. Choose No arrangement to position windows
yourself.

## Keyboard shortcuts

Click Desktop, Application, Tilling, or an application name in the top bar to
open its menu. Arrow keys navigate an open menu; Enter selects an
item. Tab and Shift+Tab move between dialog controls. Escape
closes menus and cancels dialogs. Ctrl+Tab switches Preference
tabs. Alt+S saves, Alt+C cancels, and Alt+R resets the preference
draft. Alt+P focuses the preset list; Alt+A applies its selection
to the draft. In Terminal Lock password fields, Ctrl+U clears input.

## Shortcut files

Pass a shortcut folder as the positional command-line argument.
Without one, shortcuts are read from the application configuration
folder (normally `~/.config/desktop-tui` on Linux, or
`$XDG_CONFIG_HOME/desktop-tui`). Shortcut TOML files describe
commands, arguments, taskbar order, window controls, size,
padding, and an optional terminal background override. An optional
top-level category (for example `category = "Utility"`) controls
the Application menu grouping; omitted categories use User. See the
README for a complete example.

## Preferences

Desktop > Preference opens Desktop, Terminal, Security, and Panel tabs.
Choose a preset and click Apply to update the color draft across
the Desktop and Terminal tabs. Presets
include Femboy, Transgender, Asexual, Nonbinary, and Monochrome.
Monochrome uses neutral grays. Browsing the list keeps your edits
until you click Apply. Custom identifies individually edited colors.

Select a color role and edit its #RRGGBB value; the swatch previews it.
Desktop roles cover the background, surfaces, text, muted text,
borders, accents, and selection. Terminal roles include default
foreground/background and all 16 normal and bright ANSI colors.

Reset to Femboy replaces only the colors with light pink, white,
plum, and lavender defaults; password settings are preserved. Save validates every role, writes
preferences.toml in the application configuration folder, and
immediately recolors the desktop, native windows, and terminal windows,
including hidden windows. Cancel or Escape discards the draft. A
failed save keeps the active palette and saved password unchanged.

A custom shortcut folder does not move preferences.toml. Missing
settings use defaults. Invalid settings show one startup warning
and use default colors without replacing the original file. Broken
password settings prevent Terminal Lock until the file is repaired.

Default colors and ANSI colors (including indexed colors 0–15)
follow saved palette changes. Explicit RGB colors and indexed
colors 16–255 stay fixed. A shortcut's explicit terminal
background overrides the default, even after terminal color
resets.

## Status panel

Preference > Panel controls the top-right clock, date, Wi-Fi/network,
battery, CPU, RAM, disk, and network download/upload indicators.
Enable or hide each one, choose 12/24-hour
time and seconds, a short/ISO/day-month date, and optional interface
names. Save applies and persists the settings immediately. Cancel
or Escape discards changes. Color presets and Reset to Femboy
preserve panel options. Ctrl+Tab cycles all four Preference tabs;
Alt+4 opens Panel, and Alt+G selects the date format.

The clock uses local time. Device and resource readings refresh in
the background every three seconds, without extra tools or root.
WiFi shows an approximate signal percentage when available, or
WiFi up. Net up covers wired and other routed connections. Net
offline means no usable local link was detected; Net N/A means the
status could not be read. This does not test Internet access.
Battery is hidden when unavailable; + means charging. Multiple
system batteries use their mean percentage; peripheral batteries
are excluded.

CPU is average busy time across all cores between samples. RAM
usage subtracts available memory, including reclaimable caches,
from total RAM. Disk shows usage of the root filesystem (/),
accounting for reserved blocks. Network rates are per second for
the active connection interface, received (Rx) and transmitted (Tx).
CPU and traffic rates need two valid samples; missing readings and
reset/interface changes show N/A instead of a false zero.

The status panel is on the right; application menus stay on the left.
When space is tight, compact labels show W for Wi-Fi, B for battery,
C for CPU, M for RAM, D for disk, v for received traffic and ^ for
transmitted traffic. Compact traffic units B/K/M/G mean bytes, KiB,
MiB or GiB per second; compact dates are day/month. Hover over the
status for full multiline readings. Complete indicators are omitted
only if they still cannot fit, prioritizing clock and connection.
Apps replaces individual application menus when needed: choose an
application to open its normal Start, Hide/Show, Close, and extra
commands. Indicators and menus are hidden during Terminal Lock.

## Security and passwords

In Preference > Security, choose Set / change saved password,
enter and confirm the new password, then Save. To change or remove
an existing password, enter the current password. Keep current
setting preserves it during color edits. Cancel or Escape discards
all edits. Ctrl+Tab cycles all four tabs; Alt+U opens Security.
Alt+L selects the password action. Alt+O, Alt+N, and Alt+F focus
current, new, and confirmation fields. Ctrl+U clears a field.

Only a salted Argon2id hash is saved in preferences.toml in the
application configuration folder. Plaintext passwords never go to
disk. The file uses owner-only permissions (0600), and its folder
uses 0700. Password and palette changes are saved atomically.
Symlinked settings and unsafe password-storage permissions are
rejected. Restore file permissions with chmod 600 and folder
permissions with chmod 700 if necessary. Passwords cannot be
recovered. This setting does not change your Linux password.

## Terminal Lock

Desktop > Terminal Lock locks this Desktop-TUI environment while
applications keep running. If a password is saved in Preference >
Security, locking starts immediately and reuses that password,
including after restarting Desktop-TUI. Otherwise, set and confirm
a temporary password, then choose Lock. Cancel or Escape dismisses
setup without locking. Temporary passwords are never saved.

The lock screen hides desktop menus and app windows. Enter the
same password and choose Unlock or press Enter to return. Escape
and closing controls cannot dismiss the locked screen. Incorrect
attempts require a one-second wait. Ctrl+U clears password input.
Native windows and editor documents are included. Visible windows
return after unlocking; previously hidden windows
stay hidden. Applications keep their content and running processes.

This protects the current TUI session. It does not lock Linux or
other terminals, and a person who can terminate the process can
bypass it. Use Screen Lock for the graphical desktop's system lock.
Temporary lock passwords cannot be recovered if forgotten.

## Screen Lock

Desktop > Screen Lock asks your graphical Linux desktop to lock
the current session. Authentication belongs to your desktop screen
locker. Screen Lock does not ask for a password inside the TUI. It tries
loginctl lock-session with `XDG_SESSION_ID` when available, then
xdg-screensaver lock if needed. A graphical session with `DISPLAY`
or `WAYLAND_DISPLAY` and an enabled locker is required. A
successful helper request does not verify that the desktop
actually locked.

## Shutdown and Restart

These actions control the computer. Save your work first. The
confirmation dialog initially focuses Cancel; Escape cancels.
Confirmed requests use systemctl with password prompts disabled
and explicit application inhibitor checks. Compatible Linux
systemd tooling, appropriate session permissions, and no blocking
application inhibitors are required. Failures are reported;
requests are never retried automatically.

Only one system request runs at a time. Each helper has a
15-second timeout. A timed-out helper is terminated and reaped;
its outcome is uncertain, so check the computer's state before
trying again. Desktop-TUI and applications keep running while a
request is pending or fails. Test actual power actions only in a
disposable VM.

## Exit

Desktop > Exit closes Desktop-TUI and its managed applications. It
does not lock, shut down, or restart the computer.
