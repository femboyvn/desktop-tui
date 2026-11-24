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
Image Viewer, PDF Reader, Text editor (nano), Web Browser (Carboxyl), native Music Player, ASCII Paint, Kanban, and the games
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

## Command palette

Ctrl+Space opens the searchable command palette. Desktop > Command
palette opens it when the outer terminal or input method consumes that key.
Type words to filter, arrows/PageUp/PageDown to choose, Enter to select,
and Escape to cancel. Launch apps, focus visible/hidden windows, and
choose Hide or Close. Desktop > New terminal, also in the palette, creates
an independent configured Terminal; ordinary launchers restore an existing
window.

Desktop > Desktop shortcut saves another modified key or function key.
Ctrl+C remains a child interrupt. Press the configured shortcut again
while the palette is open to forward it to the originating terminal:
Ctrl+Space twice sends literal Ctrl+Space by default. Menus, modal dialogs
and Terminal Lock retain their own input.

## Terminal history and clipboard

Ctrl+C sends a normal interrupt to the child. Choose Close or the window
close button for explicit shutdown. A child that exits closes its window.
Shift+PageUp/PageDown scroll retained output. New output keeps the current
history position; use palette > Terminal: Return to live to follow output
again, even after scrolling to the bottom.

With a terminal focused, palette > Terminal: Copy mode starts a caret.
Arrows move it; Space sets an anchor; subsequent arrows extend the
selection; Enter copies. Mouse press/drag/release selects and the wheel
scrolls in Copy mode. Home/End/PageUp/PageDown also move the caret.
Escape leaves Copy mode without sending Escape to the child.

Palette > Terminal: Search history accepts literal text and Backspace.
Enter/Shift+Enter finds next/previous, wrapping through results. Escape
leaves Search. Selection copies complete Unicode graphemes, joins soft
wraps and preserves hard line breaks. Resize clears selection/search and
tries to keep visible text anchored; eviction may clamp it. Alternate
screens have no retained history.

History is bounded to 2,000 rows, 200,000 cells and 8 MiB of retained text.
Search accepts 256 Unicode scalars and up to 4,096 stored matches. It
searches output present when the query was entered; change the query or
re-enter Search for later output. Selection/clipboard text is at most
256 KiB. Palette > Terminal: Paste sends clipboard text outside Copy/Search.

Wayland uses wl-copy/wl-paste, X11 uses xclip or xsel. Missing tools or
provider failures use a shared in-memory clipboard. PTY/SSH sessions need no
graphical session: native text controls and terminal windows share the internal
clipboard, and client-terminal paste reaches the focused control. Copying to
the SSH client's operating-system clipboard depends on that terminal client.
Window-title status
shows success, fallback and errors. Nothing is saved to disk. Child mouse
reporting and bracketed paste continue outside desktop Copy/Search modes.

## Workspaces and restoration

Desktop > Workspaces selects one of four numbered workspaces. The palette
offers Switch to workspace N, and Move current window to workspace N
when a window was focused. Switching keeps applications running and
preserves deliberate Hide state. Focusing an app on another workspace
switches there. Tiling arranges visible windows in the current workspace.

Desktop > Exit saves window layout, workspace/hidden state, launcher
identity, working directories and saved document paths before cleanup.
A later start offers Restore before relaunching saved applications.
Cancel leaves them closed; Desktop > Restore saved workspace offers it
again. Missing apps/files are reported individually. Restoring recreates
windows; it does not restore process memory, terminal output, unsaved
buffers or game progress.

The separate session.toml lives in the application state directory,
normally ~/.local/state/desktop-tui or $XDG_STATE_HOME/desktop-tui on Linux.
Sessions accept up to 256 windows and 1 MiB. They are exit snapshots,
not crash recovery or attachment to surviving processes.

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

## Kanban

Application > Utility > Kanban opens a new board window each time. New boards
start with To do, In progress, Review, and Done. Each board saves separately.
File (Alt+F) provides New/Open, Save/Save As, and Rename board. Ctrl+N/Ctrl+O
open separate windows; Ctrl+S/Ctrl+Shift+S save. Open focuses an already-open
canonical file. Save As cannot replace another open board's file.

Columns (Alt+C) adds, renames, reorders, and deletes columns, including defaults.
Keep one column. Deleting a populated column requires another task destination.
Apply commits column changes; Cancel/Escape preserves the original board.

Arrows navigate, Enter edits, Insert adds, and Delete requests task deletion.
Shift+Left/Right moves a card between adjacent columns; Shift+Up/Down reorders.
Task (Alt+T) provides the same actions. Cards show title, priority, and due date.
The editor also holds multiline notes. Title is required; priority defaults to
Medium; optional due dates must be valid YYYY-MM-DD dates.

Click to select, double-click to edit, and drag to move or reorder. Invalid
drops, Escape, focus loss, hiding, locking, and resizing cancel dragging.
Wheel scrolls tasks under the pointer; Shift+wheel or horizontal wheel scrolls
columns. Navigation reveals selection. Tiny windows ask for resizing.

Save explicitly to .kanban (version 1 TOML, at most 2 MiB). An asterisk marks
unsaved changes. File I/O freezes editing; errors preserve the board and path.
Limits: 64 columns, 4096 tasks, 256-byte board/column names, 1024-byte titles,
and 64 KiB notes. Saved paths restore with workspaces; untitled boards and
unsaved contents are excluded from session storage.

Close offers Save/Discard/Cancel. Exit, Shutdown, and Restart reveal and guard
Paint and all boards sequentially. Approvals freeze editing until every document
approves; Cancel or a failed save keeps every document and other application open.
There is no autosave, reminder, collaboration, or drag across board windows.

## ASCII Paint

Application > Utility > ASCII Paint opens one canvas window; its launcher
restores that window. New defaults to 80 by 24 cells and accepts 1–200
columns and 1–80 rows. Resize / Ctrl+R or the size at bottom left changes
canvas dimensions, preserving the top-left artwork and blank-filling new cells.
Undo restores cropped cells. Cursor coordinates sit at bottom right.
Window resizing changes only the view; theme changes preserve cell RGB colors.

Brush / RGB sets a printable ASCII character and foreground/background
#RRGGBB colors. Printable keys select the brush character. Left-click a
palette swatch for foreground; right-click for background. Left-click or
drag paints; right-click or drag erases to the original blank colors.
Eraser toggles erasing. Arrows move and reveal the cursor; Space/Enter
stamps and Backspace/Delete erases. Wheel scrolls vertically/horizontally;
PageUp/PageDown scroll vertically. Undo/Redo keeps 64 actions, groups
mouse strokes, and can undo Clear.

Ctrl+N: New. Ctrl+O: Open. Ctrl+S: Save. Ctrl+Shift+S: Save As.
Ctrl+E: Export Text. Ctrl+Z: Undo. Ctrl+Y: Redo.
Ctrl+R: Resize canvas. Ctrl+I: Image to ASCII.

Image to ASCII converts PNG/JPEG/WebP/GIF/BMP to editable untitled art.
Choose maximum columns/rows and preserved colors or monochrome. It fits
proportions with tall-cell correction, maps brightness to ASCII density,
and blends transparency onto black. Image limits are 32 MiB and 16 megapixels;
only the first animation frame is used. Failed/canceled conversion preserves
artwork; unsaved work is prompted before replacement.

Save uses colored .asciipaint projects, version 1 TOML up to 2 MiB.
Open also imports .txt/.asc as unsaved untitled artwork: printable ASCII,
LF/CRLF, tabs expanded to four-column stops, at most 16 KiB before and
after normalization, within the canvas dimension limits. File Browser
opens .asciipaint in Paint; text files keep their Text editor association.
Export Text writes padded, colorless LF rows to .txt/.asc and does not
mark the project saved. File operations run in the background one at a
time and pause editing. Errors preserve artwork and existing destinations;
saves and exports replace files atomically.

New, Open, Close, Desktop Exit, Shutdown, and Restart offer Save,
Discard, or Cancel for unsaved artwork. Save must succeed before continuing;
Cancel keeps the desktop and other apps running. Hide and Terminal Lock
preserve artwork. Prompts wait until visible; restore with the launcher
or Application > Background windows.

## Music Player

Application > Utility > Music Player opens the built-in player. No
separate player installation is needed. Click Open or press Ctrl+O to
choose a local MP3, WAV, FLAC, or Ogg Vorbis file, up to 128 MiB and eight channels.
Sample rates from 8 to 192 kHz are supported.
Opening MP3, WAV, FLAC, or Ogg Vorbis files in File Browser starts playback
in the same Music Player window. A new track replaces the current one and
restores the player if hidden.
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

Application > System > File Browser remembers the last successful folder;
the initial location is your home directory.
Enter an absolute or relative path, or double-click a folder in the listing.
Double-click a regular file to open it; Enter opens the selected entry.
The initial view is an icon grid. View (Alt+V) switches Grid/Details while
keeping the selection visible. Use arrows, Home/End, PageUp/PageDown,
or the mouse wheel. Long names are clipped only for display.
Up (Alt+U) opens the parent, Refresh (Alt+R) reloads, and Hidden
(Alt+H) toggles dot files. Directories sort before files; Details shows
name, type, byte size, and local modification time. Failed background
reads leave the previous listing usable.

Right-click selects without opening. Shift+F10 opens the selected entry's
menu; Escape dismisses it. Regular files offer Open, Open with, and
Open in terminal. Folders offer Open and Open in terminal. Special
entries offer a terminal at their parent. Right-click blank space for
Open terminal here in the current folder. All menus offer Refresh,
Hidden, Grid, and Details.

File information in an entry's menu, or Alt+Enter on the selected entry,
opens a read-only dialog with name, full path, type, byte size, and modified
time. Scroll long text; Tab switches between the text and Close button.
Close or Escape dismisses the dialog. Values come from the latest listing;
Refresh updates them. Folder and special-entry sizes are not calculated.

Open with lists compatible native viewers and every configured shortcut,
labeled native/shortcut. Enter or double-click chooses; Escape cancels.
Check Always use for .extension to save an extension default in preferences.
Saved defaults take precedence over built-in associations. A missing saved
launcher reports an error; choose another app through Open with.
External choices create independent windows. FILE_PATH templates use the
selected file; FOLDER_PATH uses its parent. With no FILE_PATH template,
the selected file is appended once. Paths retain their original bytes,
including literal template text. The child starts in the file's parent.
Open in terminal creates a fresh configured Terminal window in the selected
folder, file's parent, or current folder. Its FOLDER_PATH binding uses that
folder. Graphical Linux apps are not discovered.

Open supported images in Image Viewer, PDFs in PDF Reader, and MP3, WAV,
FLAC, or Ogg Vorbis files in Music Player, and .asciipaint projects in
ASCII Paint, and .kanban boards in Kanban. File extensions are case-insensitive.
Other regular files open in
independent Text editor document windows, preserving any running
editor. The selected absolute path is one OS argument, appended
after configured templates are resolved. Display labels may replace
invalid UTF-8 bytes; navigation and launch arguments retain them.
Special files are not opened as documents.

Press Space or Ctrl+Click to mark items, Shift+Click or Shift+Arrow to
mark a range, Ctrl+A to mark all listed items (up to 1,000), and Escape to
clear the marks. An asterisk identifies each marked item. F5, F6, and Delete
act on the marks when present. Right-click a marked item for batch actions.

With the listing focused, Ctrl+N creates an empty file and Ctrl+Shift+N
creates a folder. F2 renames, F5 copies, F6 moves, and Delete asks to move
the entry to Trash. These actions are also in context menus. Copy/Move
accept a new destination filename for a single item. A batch uses the
original filenames and one destination folder. Select folders separately
from their contents and pick distinct filenames for Copy/Move. Replacing
a file or symlink requires confirmation. Choose a new name or location
when a destination folder already exists. A background
worker reports progress. A batch stops at a failure, declined replacement,
or cancellation. Already published content remains. Failed transfers
preserve the source, and incomplete cleanup reports retained source/backup paths.

Recursive copies preserve symlinks without following them and reject
special files. Limits are 100,000 entries and depth 128. Basic permissions
are copied; timestamps, ownership, ACLs, extended attributes and hardlink
relationships are not preserved. Trash (Alt+T) opens the user's Trash.
Details shows original locations and deletion dates. Enter restores the
focused or marked items after confirmation, and Alt+Enter shows the full original path.
The original folder must exist and the destination must be vacant.
Trash and restore use a same-filesystem move. Back (Alt+U) returns to the folder.

Use Add/remove favorite and Favorites for up to 64 folders.
Favorites also appear in destination dialogs. In the browser Favorites chooser,
Remove deletes a saved entry even if its folder no longer exists.
Enter a filename substring in Filter and press
Enter; matching ignores case. Last location, view, hidden toggle, filter
(up to 512 bytes) and favorites persist separately in state/browser.toml
under the application state directory. State errors are visible and
malformed state is not overwritten.

Find (Ctrl+F from the listing) searches the current folder and its subfolders
for a filename substring, ignoring case. Results show relative paths and
respect the Hidden toggle. The search skips symlinked directories and reports
unreadable entries and limits in the status bar. Back (Alt+U) returns to the
folder and its current filter.

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
or restore it. Close buttons request explicit shutdown. Ctrl+C in a terminal
is forwarded to the child as a normal interrupt.

Hide (run in background) removes a window from the taskbar while its
process keeps running. Application > Background windows restores hidden
windows, including independent editor documents. Selecting an application
launcher also restores its existing primary window. Closed windows release
their taskbar buttons. Reopening creates a new instance.

## Tiling

The Tilling menu offers No arrangement, Cascade, Vertical,
Horizontal, and Grid. The chosen arrangement is reapplied when
visible windows open or close in the current workspace. Choose No arrangement
to position windows yourself.

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
padding, optional working_directory and a terminal background override.
An optional top-level category (for example `category = "Utility"`) controls
the Application menu grouping; omitted categories use User. See the
README for a complete example.

Optional id preserves identity when a launcher is renamed; without it,
the name is its identity. Duplicate names/IDs are errors. Only exact
.toml extensions are loaded, with deterministic path/taskbar ordering.
Relative working_directory uses the TOML file's parent; an explicit
file/folder launch context takes precedence.

Desktop > Launchers creates or edits TOML with separate command and args.
Select a launcher then Edit, or choose New. Save validates and atomically
writes the original source; Reload applies saved changes without stopping
running windows. Editing a starter creates an override. Invalid drafts do
not replace source files. Unsaved changes prompt before discard.

## Diagnostics

Desktop > Diagnostics runs configuration/tool checks in the background.
The noninteractive --doctor flag provides the same report before terminal
initialization or configuration-directory creation. It checks settings,
launchers and working directories, external executables, bundled Carboxyl,
Poppler helpers, DISPLAY/WAYLAND_DISPLAY and clipboard providers.
It does not execute configured commands. Configuration or Terminal
executable failures return nonzero; missing optional tools are warnings.
Provider availability does not prove a graphical connection will succeed.

## Preferences

Desktop > Preference opens Desktop, Terminal, Security, Panel, and Background tabs.
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

## ASCII background

Desktop > Preference > Background (Alt+5) sets an ASCII desktop image.
Click Browse... (Alt+B) and choose a plain .txt or .asc file. The
included backgrounds/cat.asc is a small example. Review the scrollable
preview, then Save. Loading runs in the background; Save is unavailable
until it finishes. Failed imports keep the previous art.

Save keeps a copy in preferences.toml and applies it immediately; the
source file can be moved or deleted afterward. Clear art (Alt+E), then
Save, restores the solid background. An empty file also clears the art.
Cancel or Escape discards edits. Color presets and Reset to Femboy
preserve the selected background.

Art is centered behind windows with the desktop Text and Background
colors. Resizing centers it again; small terminals clip without wrapping.
Spaces and blank lines are preserved. LF and CRLF line endings work;
tabs expand to four-column stops. Both input and expanded text must fit
within 16 KiB. Use printable ASCII, without ANSI escapes or Unicode art.
Raster images are supported by Image Viewer, not as ASCII backgrounds.

## Status panel

Preference > Panel controls the top-right clock, date, Wi-Fi/network,
battery, CPU, RAM, disk, and network download/upload indicators.
Enable or hide each one, choose 12/24-hour
time and seconds, a short/ISO/day-month date, and optional interface
names. Save applies and persists the settings immediately. Cancel
or Escape discards changes. Color presets and Reset to Femboy
preserve panel options. Ctrl+Tab cycles all five Preference tabs;
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
all edits. Ctrl+Tab cycles all five tabs; Alt+U opens Security.
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

## Web Browser

Application > Utility > Web Browser launches Carboxyl 0.2.0
on its blank page with native text and quadrant graphics at 20 FPS. Click its top-row address field,
then type or paste a URL. A same-name shortcut overrides the launcher.
Hide/restore, tiling, locking, closing and child cleanup follow other terminal
windows. Ctrl+C is a child interrupt; Close requests shutdown.
The Linux package includes libexec/carboxyl;
keep it beside the desktop executable's directory. That binary needs glibc 2.43
or newer. Source/Cargo installs can use a separate browser from the pinned Git tag or release;
the crates.io package with the same name is a different project. See HOW_TO_RUN.md.
Unicode graphemes are preserved; fonts and shaping depend on the outer terminal.

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

Desktop > Exit saves the workspace snapshot, then closes Desktop-TUI and
its managed applications. It does not lock, shut down, or restart the computer.

## AI Chat

Launch Application > Utility > AI Chat and click its Settings button to configure
keys and model IDs. Choose Replace key before pasting, then Save. AI Chat Settings
is independent of system Preferences and saves to its own ai.toml. Existing keys
from the old system AI section are imported automatically. Keys are stored locally
in recoverable form with owner-only permissions. Keep
preserves a key; Remove deletes it; Cancel discards edits.

Enter adds a newline; Alt+S sends, Alt+A attaches, Alt+X stops, Ctrl+N starts a
chat. Chats selects saved conversations; Files includes/excludes attachments.
Use model applies the header's model ID. Changing providers/models sends nothing;
the next Send sends conversation text and included files to that provider.
Unsupported files block sending with an explanation. Any file extension can be
selected, but provider/model support and size limits apply. Copy copies the latest
reply to the internal clipboard. Retry replaces an incomplete answer.

Chats survive restart. Hide, workspaces and Terminal Lock keep generation running;
Close cancels and saves partial output. Failed saves offer Retry / Discard / Cancel.
