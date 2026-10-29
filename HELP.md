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
Image Viewer, PDF Reader, Text editor (nano), native Music Player, and the games
Snake, 2048, and Minesweeper are
available with an empty shortcut folder. External tools must be installed separately.
System Performance uses bottom’s default-light theme to match the
default terminal background. A custom shortcut can choose another theme.
Shortcuts with the same external starter names replace them.
Native app and game identities remain separate.

Window buttons after Tilling | show only open, visible windows.
A button focuses its window and offers Hide (run in background) and Close.
Unopened applications have no button. App commands... provides configured
additional commands, which can open a file or
folder picker when a shortcut uses `<FILE_PATH>` or
`<FOLDER_PATH>`. Additional commands open separate windows.
Choose Close to stop the primary application process.

## Games

Application > Games offers built-in Snake, 2048, and Minesweeper.
No external game programs are needed. Each has its own movable,
resizable window, taskbar button, and current board. R starts a new
round. Closing and reopening starts a fresh window; scores are not saved.

Snake: arrows or WASD steer; Space starts, Space/P pauses or resumes.
Eat * to grow and score ten points. Hitting walls or your body ends
the round. Snake pauses on lost focus, Hide, Terminal Lock, or a
window too small for its board. Space/P resumes when you return.

2048: arrows or WASD slide. Equal adjacent tiles merge once per
move, adding their merged value to the score. Only moves that change
the board add a new tile. Reach 2048 to win; no possible moves ends
the round. Snake and 2048 show the best score for the current window.

Minesweeper: a 9 by 9 board hides ten mines. Arrows or WASD select,
Space/Enter reveals, and F toggles a flag. Left-click reveals;
right-click flags. The first reveal and its neighbors are safe.
Numbers count adjacent mines; empty areas reveal nearby safe cells.
Flags block reveals. Reveal all 71 safe cells to win. A mine ends
the round and shows the mines.

Hide and Terminal Lock preserve every game's board. Restore through
Application > Background windows or the game's launcher. If the
window shows Resize window to play, enlarge it to use game controls.

## Music Player

Application > Utility > Music Player opens the built-in player. No
separate player installation is needed. Click Open or press Ctrl+O to
choose a local MP3, WAV, FLAC, or Ogg Vorbis file, up to 128 MiB and eight channels.
Sample rates from 8 to 192 kHz are supported.
Click Play/Pause or press Space; S stops playback. Left/Right seek ten
seconds and +/- changes volume by 5%. The window shows the filename,
elapsed/total time, volume, and inline errors. One file plays at a time.

Hide (run in background) keeps playing; Application > Background windows
restores the player. Terminal Lock preserves playback. Close and Desktop
Exit stop it. File reads and audio operations run in a background worker.
The encoded file stays in bounded memory while decoded samples stream.

The compiled audio engine uses the system's sound device and audio
session. If output is unavailable, check system sound and reopen the file.
The optional shortcuts/mp3blaster.toml example is an independent external
player; only that example requires mp3blaster to be installed separately.

## File Browser

Application > System > File Browser starts in your home directory.
Enter an absolute or relative path, or double-click a folder in the listing.
Double-click a regular file to open it; Enter opens the selected row.
Up (Alt+U) opens the parent, Refresh (Alt+R) reloads, and Hidden
(Alt+H) toggles dot files. Directories sort before files; columns
show name, type, byte size, and local modification time. Failed
background reads leave the previous listing usable.

Open supported images in Image Viewer and PDFs in PDF Reader. Other regular files open in
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

## PDF Reader

Application > Utility > PDF Reader opens the native reader. Click Open
or press Ctrl+O to choose a local PDF; File Browser also opens PDFs here.
Prev/Next, PageUp/PageDown, or P/N change pages. Home/End jump to the
first/last page. +/- or the wheel zoom by 1.25; F or Fit fits the page.
Arrows and dragging pan. Zoom ranges from 25 to 1600 percent of Fit;
Fit adapts to window size. Opening another PDF replaces the current one.

Install Poppler's pdfinfo and pdftoppm on PATH (poppler-utils on
Debian/Ubuntu, poppler on Arch). Missing helpers, invalid input, and
PDFs requiring a password show inline errors; Ctrl+O opens another file.
The reader preserves scanned pages and page layout as terminal images.
Text selection, search, editing, and password entry are not supported.

Input is limited to 64 MiB. One page is rasterized at a time, up to
2048 pixels per edge; high zoom cannot add detail beyond that raster.
Helpers time out after 15 seconds. Loading/rendering run in a background
worker. Hide and Terminal Lock preserve the document and page; Close
and Desktop Exit cancel active rendering and reap helper processes.

## Window controls

Drag the title bar to move a window. Drag its bottom-right corner
to resize a resizable window. Use the maximize button to maximize
or restore it. Close buttons stop the window; Ctrl+C in a terminal
stops its running command.

Hide (run in background) removes a window from the taskbar while its
process keeps running. Application > Background windows restores hidden
windows, including independent editor documents. Selecting an application
launcher also restores its existing primary window. Closed windows release
their taskbar buttons. Reopening creates a new instance.

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
Apps groups visible window buttons when needed: choose a window to
focus it and open its Hide/Close actions. Background windows stay out of this list and can be restored
through Application > Background windows. Indicators and menus are hidden during Terminal Lock.

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
