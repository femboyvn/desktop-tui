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

Linux PTYs and SSH are the primary runtime. A graphical session, X11 forwarding,
`DISPLAY`, and Wayland are not required. Allocate a terminal when connecting:

```sh
ssh -t user@host 'desktop-tui /path/to/shortcuts'
```

Commands and file operations run on that host. Copy/paste within Desktop-TUI
uses an internal clipboard; your SSH client's normal paste reaches the focused
control through terminal paste events. Graphical clipboard providers are optional.

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
- [x] Categorized Application menu with starter launchers
- [x] Searchable command palette, independent terminals, and normal child Ctrl+C
- [x] Bounded terminal history, Unicode selection/search, and clipboard fallback
- [x] Background file operations, favorites, and saved Open with choices
- [x] File multi-select, recursive filename search, and Trash restore
- [x] Launcher editing/reload, working directories, and noninteractive diagnostics
- [x] Four workspaces with an offered restore after restart
- [x] Desktop activity for File Browser and AI Chat results
- [x] Managed Carboxyl browser launcher and grapheme-aware Unicode terminals
- [x] Native File Browser, Image Viewer, PDF Reader, Music Player, and ASCII Paint
- [x] Built-in Snake, 2048, and Minesweeper
- [x] Clock
- [x] Offline Help and saved desktop/terminal palettes
- [x] Saved ASCII desktop backgrounds from text files
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
- `src/config/`: preferences models, storage, palette presets and editing drafts, plus shortcut definitions, discovery, workflow settings, and saved-session records.
- `src/runtime/`: terminal emulation/history, clipboard, file/media workers, and password credentials.
- `src/platform/linux/`: status readings, resource usage, and system actions.
- `vendor/appcui/`: local AppCUI dependency with Unicode rendering and input support; see [patch notes](vendor/README.md).

Groups use a named entry file alongside their child folder (for example, `ui/desktop.rs` and `ui/desktop/`). Entry files declare private implementation modules and expose shared types and entry points within the crate. `main.rs`, `args.rs`, and test-only `test_support.rs` stay at the source root; `utils.rs` remains outside the module tree.

## Shortcut file

Example `helix.toml` shortcut file:

```toml
# Optional stable identity; keep it when renaming the launcher
id = "personal.editor"
# Window name
name = "Text editor"
# Optional; omitted categories appear under User
category = "Utility"

# Command to execute
command = "hx"
# Each command argument
args = []
# Optional; relative to this TOML file, not the desktop process
# working_directory = "../project"

[taskbar]
# Shortcut ordering within Application categories
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

## Launcher management and diagnostics

Open **Desktop > Launchers...** to create or edit launcher TOML. Select an entry
and **Edit**, or choose **New**; keep `command` as one executable and `args` as
an array of separate arguments. **Save** validates and atomically writes the
source; **Reload** applies saved changes without restarting or stopping existing
windows. Editing a starter creates an override. Unsaved drafts can be discarded
or kept; invalid drafts leave the source intact.

Optional `id` preserves identity across name changes; otherwise the name is the
identity. Names and explicit/fallback IDs must be unique, including starters.
Discovery reads exact lowercase `.toml` extensions recursively in sorted path
order, then orders by taskbar position. Read, traversal and TOML errors identify
the failing path. Relative `working_directory` values use the containing TOML's
parent. An explicit folder/file context from File Browser overrides that default.

**Desktop > Diagnostics...** checks configuration and executable availability in
the background. The same report is available without opening a terminal UI:

```sh
cargo run --locked -- --doctor ./shortcuts
```

It checks selected/default configuration, external executables, bundled Carboxyl
resolution, Poppler helpers, graphical-session variables and clipboard providers.
It launches no configured command and creates no configuration directories.
Configuration errors and an unavailable Terminal executable return nonzero;
missing optional tools produce warnings. Availability does not guarantee a
clipboard provider can connect to the current session.

## Keyboard control and terminal history

Press **Ctrl+Space** to open the searchable command palette, or use
**Desktop > Command palette...** if the outer terminal/input method intercepts
that key. Type words to filter, use arrows or PageUp/PageDown to choose, and
Enter to run; Escape cancels. The palette launches apps, focuses visible or
hidden windows, and offers Hide/Close and terminal actions. **New terminal**
creates an independent instance of the configured Terminal; an ordinary
application launcher restores its existing window.

**Desktop > Desktop shortcut...** saves another modified key or function key;
Ctrl+C stays available to the child. Press the configured shortcut again while
the palette is open to forward that key to the terminal that opened it. With
the default binding, **Ctrl+Space twice** sends a literal Ctrl+Space.
Menus, modal dialogs and Terminal Lock retain their own input.

**Ctrl+C** is a normal child interrupt. Use a window's **Close** action or close
button to shut it down explicitly; a child that exits also closes its window.

| Terminal action | Controls |
| --- | --- |
| Scroll retained output | Shift+PageUp / Shift+PageDown |
| Start selection | Palette: Terminal: Copy mode |
| Select with keyboard | Arrows move; Space marks the anchor; arrows extend; Enter copies |
| Select with mouse | In Copy mode, press/drag/release; wheel scrolls |
| Search retained text | Palette: Terminal: Search history; type a literal query, Enter / Shift+Enter for next / previous |
| Leave Copy/Search | Escape; use Terminal: Return to live to resume following output |
| Paste clipboard | Palette: Terminal: Paste, outside Copy/Search mode |

History stays at the retained text while new output arrives; reaching the bottom
by scrolling does not itself resume live output. Selection copies whole Unicode
graphemes, joins soft wraps and retains hard line breaks. Resize cancels
selection/search and tries to keep the same visible text; eviction clamps the
view and clears an evicted selection. The alternate screen has no retained history.

History retains at most 2,000 rows, further bounded by 200,000 cells and an 8 MiB
history-text budget. Search accepts 256 Unicode scalars and stores up to 4,096
matches; it scans the retained output present when the query is entered. Change
the query or re-enter Search to include later output. Clipboard/selection text
is limited to 256 KiB. Title status distinguishes system clipboard success,
internal fallback and errors. Wayland uses `wl-copy`/`wl-paste`; X11 uses `xclip`
or `xsel`. Native controls and terminal windows share a bounded in-memory
clipboard; this is normal PTY/SSH operation when graphical providers are absent.
Native text controls use that internal clipboard without connecting to a display.
No clipboard text is saved to disk. Child mouse reporting and bracketed paste
continue outside desktop selection/search modes.

## Workspaces and restoration

Use **Desktop > Workspaces > Workspace 1–4**, or the palette's **Switch to
workspace N**. With a window focused, the palette's **Move current window to
workspace N** moves it without terminating it. Switching preserves deliberately
hidden windows. Focusing a window through the palette or its launcher switches
to its workspace. Tiling applies to visible windows in the current workspace.

**Desktop > Exit** saves workspace membership, layout, hidden state, launcher
identity, working directories and saved document paths before cleaning up
managed applications. On the next start, **Restore** is offered before any
saved external application is relaunched. Cancel leaves the desktop available;
**Desktop > Restore saved workspace...** offers it again. Missing apps/files
are reported individually. Restoration recreates windows from saved launch
information; terminal output, child process memory, unsaved editor/artwork
buffers and game progress are not restored.

Session data is separate from shortcut discovery: `session.toml` in the
application state directory, normally `$XDG_STATE_HOME/desktop-tui` or
`~/.local/state/desktop-tui` on Linux. At most 256 windows and a 1 MiB session
file are accepted. This is an exit snapshot; it does not attach to surviving
processes or provide crash recovery.

## Applications and files

The top bar starts with **Desktop | Application | Tilling**. Application groups
launchers under **System**, **Utility**, **User**, and **Games**, followed by
additional named categories in alphabetical order. Empty categories show a
disabled **No apps yet**. Selecting a launcher starts its window or restores
and focuses the existing instance. Kanban launches a separate board window each time.

The space after **Tilling |** shows open, visible windows in opening order,
including native apps, separate editor documents, and Help. Opening a window’s
button focuses it and offers **Hide (run in background)** and **Close**. Hide
removes its button while keeping the window and process alive. Restore any
hidden window through **Application > Background windows**, or select its
application launcher again. Unopened apps have no taskbar button. **Apps** groups
visible windows when space is limited; selecting one opens its Focus/Hide/Close
actions. The status panel remains responsive.
External windows with additional commands expose them through **App commands...**.

Thirteen starters are available even with an empty shortcut directory:

| Category | Application | Executable |
| --- | --- | --- |
| System | File Browser | Native |
| System | Terminal | `sh` |
| Utility | System Performance | `btm` ([bottom](https://github.com/ClementTsang/bottom)) |
| Utility | Image Viewer | Native |
| Utility | PDF Reader | Native (Poppler rendering) |
| Utility | Text editor | `nano` |
| Utility | Web Browser | `carboxyl` 0.2.0 (bundled in the Linux package) |
| Utility | Music Player | Native (compiled audio engine) |
| Utility | ASCII Paint | Native |
| Utility | Kanban | Native |
| Games | Snake | Native |
| Games | 2048 | Native |
| Games | Minesweeper | Native |

The System Performance starter uses bottom’s `default-light` theme for readable
text on the default light terminal background. Override its shortcut to choose
a different theme or arguments.

Other external tools must be installed separately and available on PATH. A shortcut
named Terminal, System Performance, Text editor, or Web Browser replaces that external
starter, including its command, options, and category. A missing category
places a shortcut in User. Native apps and games keep their own identities even
when custom shortcuts use the same names.

**Application > Utility > Web Browser** opens
[Carboxyl 0.2.0](https://github.com/carboxyl-rs/carboxyl/releases/tag/v0.2.0)
on its blank page, using native terminal text and quadrant graphics at 20 FPS.
The portable Linux x86-64 package includes the browser beside Desktop-TUI;
keep `libexec/carboxyl` in the extracted directory. The upstream binary requires
glibc 2.43 or newer. Source/Cargo installations can use Carboxyl from PATH.
See [browser setup and packaging](HOW_TO_RUN.md#use-the-bundled-carboxyl-browser) and the
[sample shortcut](shortcuts/carboxyl.toml). Selecting the launcher restores its
existing window. Hide, taskbar actions, tiling, Terminal Lock, close/reopen, and
child cleanup use the same managed terminal path as other external apps.
Carboxyl owns navigation and website support; Desktop-TUI adds no browser tabs
or profiles. Click Carboxyl's address field on its top row, then type or paste a URL.

Terminal windows preserve complete Unicode graphemes, including decomposed
Vietnamese, CJK, flags, skin tones, and joined emoji, with width-aware placement.
Fonts and shaping depend on the outer terminal. UTF-8 keyboard input, modified
navigation, child-requested mouse reporting, and paste are forwarded; bracketed
paste follows the child's mode. Ctrl+C is forwarded as a normal child interrupt.
The focused, visible terminal alone displays its child cursor. Default and ANSI
colors follow Terminal preferences; explicit RGB colors keep their values.
Graphics protocols are not passed through: Carboxyl uses quadrant characters.

**Games** are bundled under **Application > Games** and need no extra installation.
Each game opens its own movable, resizable window. Press **R** for a new round.

| Game | Controls and rules |
| --- | --- |
| Snake | Arrows or WASD steer; Space starts, and Space/P pauses or resumes. Eat `*` to grow and score; walls and your body end the round. |
| 2048 | Arrows or WASD slide tiles. Equal tiles merge once per move; reach 2048 to win. A move that changes nothing adds no tile. |
| Minesweeper | Arrows or WASD select; Space/Enter reveals, F flags. Left-click reveals and right-click flags. Reveal all 71 safe cells on the 9×9 board with ten mines. The first reveal and its neighbors are safe. |

Hiding or locking preserves every board. Snake also pauses when it loses focus;
press Space/P to resume after returning. Windows too small for the board show a
resize prompt and ignore game input. Score and session best for Snake/2048 stay
in memory until the window closes. Close and relaunch for a fresh window.
Game windows stay within the available desktop area when the terminal shrinks.

**Kanban** opens under **Application > Utility > Kanban**. Each launch or
**File > New** creates a separate board window. Boards start with **To do →
In progress → Review → Done**. **Columns / Alt+C** adds, renames, reorders, or
deletes any column. Keep at least one; deleting a populated column asks where
to move its tasks. Column dialog changes apply together or cancel with Escape.

Cards have a required title, multiline notes, Low/Medium/High priority (default
Medium), and an optional valid `YYYY-MM-DD` due date. Cards show title, priority,
and due date; open the editor for notes. Ordering is manual. Click a card to
select it, double-click to edit, or drag to move/reorder. Escape, an invalid drop,
focus loss, hiding, Terminal Lock, or resizing cancels a drag. Wheel vertically
scrolls the column under the pointer; Shift+wheel or horizontal wheel scrolls
columns. Keyboard navigation reveals the selected card.

| Kanban action | Shortcut / menu |
| --- | --- |
| New board / Open board | Ctrl+N / Ctrl+O; File (Alt+F) |
| Save / Save As | Ctrl+S / Ctrl+Shift+S; File |
| Navigate / edit / add / delete | Arrows / Enter / Insert / Delete; Task (Alt+T) |
| Move between adjacent columns / reorder | Shift+Left/Right / Shift+Up/Down; Task |
| Manage columns / rename board | Columns (Alt+C) / File > Rename board |

Save explicitly to a version 1 TOML `.kanban` file (up to 2 MiB). Each board has
its own file and an unsaved `*` marker. Editing pauses during background file
operations. Open and New leave the current board open; opening the same canonical
path focuses its existing window. Save As refuses a file owned by another open
board. Failed operations preserve the current board, saved state, and path.
Saved board paths participate in workspace restoration; untitled boards and
unsaved contents do not enter the session file. Limits are 64 columns, 4096 tasks,
256-byte board/column names, 1024-byte task titles, and 64 KiB notes per task.

Closing a dirty board offers **Save / Discard / Cancel**. Desktop Exit, Shutdown,
and Restart guard Paint and every board in turn, revealing hidden documents.
Approved documents stay frozen until all approve. Cancel or a failed save stops
the action and keeps every document and other app open. Saving is explicit;
autosave, reminders, collaboration, and dragging across board windows are outside
this version.

**ASCII Paint** opens under **Application > Utility > ASCII Paint**. It keeps one
canvas window; selecting the launcher again restores it. A fresh canvas is 80×24
cells. **New** and **Resize** accept 1–200 columns and 1–80 rows. Click the size
at the bottom left or use **Resize / Ctrl+R** to change an existing canvas.
Artwork stays anchored at the top left; growing adds blank cells, and Undo
restores cropped cells after shrinking. Cursor coordinates appear at the bottom
right. Resizing the window changes only the view; palette changes preserve cell
RGB colors.

Use **Brush / RGB** to select a printable ASCII character and foreground/background
`#RRGGBB` colors. Printable keys select the brush character. Left/right-click a
palette swatch to set foreground/background. Left-click or drag paints; right-click
or drag erases to the canvas's original blank colors. **Eraser** toggles erasing.
Arrows move the cursor and reveal it, **Space/Enter** stamps, and
**Backspace/Delete** erases. The mouse wheel scrolls vertically or horizontally;
**PageUp/PageDown** scroll vertically. **Undo/Redo** retains up to 64 actions,
groups a drag into one stroke, and can undo **Clear**.

| ASCII Paint action | Shortcut |
| --- | --- |
| New / Open | Ctrl+N / Ctrl+O |
| Save / Save As | Ctrl+S / Ctrl+Shift+S |
| Export Text | Ctrl+E |
| Resize canvas / Image to ASCII | Ctrl+R / Ctrl+I |
| Undo / Redo | Ctrl+Z / Ctrl+Y |

**Image to ASCII / Ctrl+I** converts PNG, JPEG, WebP, GIF, or BMP into an
editable untitled drawing. Choose maximum columns/rows and whether to preserve
image colors; disable that option for monochrome. Conversion fits the image
within those dimensions, preserving proportions and accounting for tall terminal
cells. Brightness selects ASCII character density; transparency blends onto black.
The existing limits of 32 MiB input and 16 megapixels apply. Images honor
orientation metadata and use the first animation frame. Conversion runs in the
background and follows the
unsaved-work prompt before replacing artwork; cancellation or failure keeps the
current drawing. Save the result as a project or export it as text.

**Save** writes a colored `.asciipaint` project (version 1 TOML, up to 2 MiB).
**Open** also imports `.txt`/`.asc`: printable ASCII only, LF/CRLF line endings,
tabs expanded to four-column stops, with both input and normalized text limited
to 16 KiB and the same canvas dimension limits. Imported text is an unsaved,
untitled project. These text files still open in Text editor from File Browser;
`.asciipaint` files open in Paint. **Export Text** writes colorless, padded rows
with LF endings to `.txt`/`.asc`; exporting does not mark the colored project saved.
Only one file operation runs at a time, with editing paused until it completes.
Failed opens preserve the canvas; failed saves preserve the existing destination.
Writes replace the destination atomically.

Unsaved artwork prompts **Save / Discard / Cancel** before New, Open, closing,
Desktop Exit, Shutdown, or Restart. Save must finish successfully before the
action continues; cancellation leaves the desktop and other apps running.
Hide and Terminal Lock preserve artwork, and prompts wait until Paint is visible.
Restore it through its launcher or **Application > Background windows**.

**Music Player** is built into Desktop-TUI; it requires no separate player installation.
Open **Application > Utility > Music Player**, then click **Open** or press **Ctrl+O**
to select a local MP3, WAV, FLAC, or Ogg Vorbis file (up to 128 MiB, eight channels,
and 8–192 kHz). Click
**Play/Pause** or press **Space**, use **S** to stop, **Left/Right** to seek ten
seconds, and **+/-** to change volume by 5%. The window shows the filename,
elapsed/total time, volume, and any errors. It streams decoded samples while
retaining the bounded encoded file in memory; file loading and audio operations
run outside the UI thread. This version plays one file at a time. Opening audio
from File Browser starts playback in the same player window, replacing the current
track and restoring the window if it was hidden.

Below the controls and wrapped help, 16 live frequency bars use ASCII `#` with
falling `-` peak markers inside a rectangle in the current desktop theme. Columns
stay at most two characters wide, with frequency labels beneath the frame
(40, 63, 125, 250, 500, 1k, 2k, 4k, 8k, 16k). Bass is on the left and treble on
the right; labels follow the track's
sample-rate limit and crowded labels are omitted. The visualization follows playback
volume, including mute; it does not change the audio's frequency balance. Narrow windows merge neighboring
bands. The framed spectrum needs six rows after text: three for bars, two for the
border, and one for labels.
Pause, Stop, seeking, replacing a track, completion, errors, and Close clear the
bars and peaks. Hidden or locked playback keeps the analysis current for restore.

Hide the window to keep playing, then restore it through **Application > Background
windows**. Terminal Lock also preserves playback; Close and Desktop Exit stop it.
The compiled [Symphonia decoders](https://github.com/pdeljanov/Symphonia) and
[miniaudio output backend](https://github.com/mackron/miniaudio) use the system's
working sound device and audio session. Building from source needs a C compiler.
The optional [mp3blaster shortcut](shortcuts/mp3blaster.toml)
launches the separate external player if you prefer its interface; it is independent
of the native Music Player and still needs mp3blaster installed on PATH.

**File Browser** starts in your remembered folder, using your home directory
initially. Enter an absolute or relative path in the path field and press Enter, or open a directory in the listing.
Use **Up** (Alt+U), **Refresh** (Alt+R), and **Hidden** (Alt+H). Directories appear
first, then entries sorted by name. The initial view is an icon grid;
**View** (Alt+V) switches to Details with name, type, byte size, and local
modification time. Switching views keeps the selection visible. Arrow keys,
Home/End, PageUp/PageDown, and the mouse wheel navigate the listing; click to
select and Enter or double-click to open. Long names are clipped for display.
Directory reads happen in the background. Failed reads leave
the previous listing available. Filesystem names are preserved as OS paths;
non-UTF-8 bytes appear as replacement characters only in labels.

Right-click an entry, or press **Shift+F10** for the selected entry's menu.
Regular files offer **Open**, **Open with…**, and **Open in terminal**;
folders offer Open and Open in terminal. Special entries can open a terminal
at their parent but cannot be opened as documents. Right-click empty space for
**Open terminal here** in the current folder. Every menu includes Refresh, Hidden, Grid, and Details;
Escape dismisses it. Right-click selects an entry without opening it.

Choose **File information…** in an entry's menu, or select it and press
**Alt+Enter**, to open a read-only dialog with its name, full path, type, byte
size, and modification time. Long text can be scrolled; Tab moves between the
text and Close button. Close or Escape dismisses the dialog. Information uses
the most recent directory listing; Refresh updates it. Folder/special-entry
sizes display as unavailable.

Use **Space** or **Ctrl+Click** to mark items. **Shift+Click** or **Shift+Arrow**
marks a range, **Ctrl+A** marks all listed items, and **Escape** clears the marks.
Marked rows show an asterisk in Grid and Details. You can mark up to 1,000 items.
**F5**, **F6**, and **Delete** act on the marks, or on the focused entry when
there are no marks. Right-click a marked item to see batch actions.

File/entry menus also offer **New file / Ctrl+N**, **New folder / Ctrl+Shift+N**,
**Rename / F2**, **Copy to / F5**, **Move to / F6**, and **Move to trash / Delete**
while the listing is focused. Copy and Move accept a new destination filename
for a single item. Batch items keep their original filenames, including raw OS
bytes, and share a destination folder. Select each folder separately from its
contents and use distinct filenames when copying or moving a batch. Replacing
a file or symlink requires confirmation. Choose a new name or location when
a destination folder already exists. Operations run in the background with progress
and **Cancel operation**. A batch stops at the first failure, declined
replacement, or cancellation. Already published work remains.
Hidden batches keep processing until an item needs a prompt or reports a
problem. Prompts wait until the window is visible again.
Failed transfers preserve the source; incomplete cleanup reports any retained
source/backup path. Recursive copies reject special files, preserve symlinks
without following them, and are limited to 100,000 entries and depth 128. Basic
permissions are retained; ownership, ACLs, extended attributes, timestamps and
hardlink relationships are not copied.

**Trash** (Alt+T) shows the user's Trash entries. Details includes original
locations and deletion dates. Press **Enter** to restore the focused or marked
items, or **Alt+Enter** to inspect the original location. Restoring requires an existing
original parent folder and a vacant destination. Trash and restore use a
same-filesystem move. **Back** (Alt+U) returns to the folder.

**Add/remove favorite** adds/removes the current folder; **Favorites** chooses
a saved folder, including in destination dialogs. The browser Favorites chooser
also removes entries, including missing folders. Enter a filename substring in **Filter**
and press Enter; matching is case-insensitive. Location, Grid/Details, hidden
files, filter and up to 64 favorites persist in `state/browser.toml` under the
application state directory, outside shortcut discovery. The filter is limited
to 512 bytes. State errors appear in the browser; malformed state is preserved.

**Find** (**Ctrl+F** from the listing) searches filenames in the current folder
and its subfolders for a case-insensitive substring. Search respects the Hidden
toggle, skips symlinked directories, and shows relative paths. The background
search visits at most 100,000 entries, searches up to 128 levels deep, and returns
up to 5,000 matches. Unreadable or limited results are reported in the status
line. **Back** returns to the current folder and its saved filter.

**Open with…** lists compatible native viewers and all configured Desktop-TUI
shortcuts, labeled as native or shortcut. Choose with Enter or double-click;
Escape cancels. Check **Always use for .extension** to save the selected app as
that extension's default in preferences. A missing saved launcher reports an
error; choose another with Open with. External choices create independent
windows and bind
`<FILE_PATH>` to the selected file and `<FOLDER_PATH>` to its parent, without
selection dialogs. If the configured executable/arguments have no `<FILE_PATH>`,
the file is appended once as a separate argument. The child's working directory
is the file's parent. **Open in terminal** creates a fresh window using the
configured Terminal shortcut, with the selected folder (or a file's parent) as
its working directory and `<FOLDER_PATH>` binding. Existing windows stay open.
There is no graphical-app discovery.

Saved Open with associations take precedence. Otherwise files use these default
Desktop-TUI apps; extensions are case-insensitive:

| File extensions | Default app |
| --- | --- |
| `.mp3`, `.wav`, `.flac`, `.ogg` (Ogg Vorbis) | Music Player |
| `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`, `.bmp` | Image Viewer |
| `.pdf` | PDF Reader |
| `.asciipaint` | ASCII Paint |
| `.kanban` | Kanban |
| Other regular files | Configured Text editor, in independent document windows |

For Text editor documents, each
selected absolute path is appended as one argument after resolving the editor's
configured command/argument templates. Spaces, Unicode, raw OS filename bytes,
and literal placeholder text in that selected path are preserved. An already
running editor remains open. Special files are not opened.

**Image Viewer** opens through Application > Utility > Image Viewer and its
file picker, or from File Browser. The file picker preserves the colors of windows behind it. It shows the filename, oriented dimensions,
and zoom in its title. Ctrl+O opens another image; **F** fits, **1** selects
actual size, **+/-** or the mouse wheel zoom by 1.25 (up zooms in, down zooms out),
and arrows or mouse dragging pan. One source
pixel occupies half a terminal cell vertically at actual size. Explicit zoom
is limited to 5–400%; Fit can go below 5% to fit a large image in a small window.
Aspect ratio and orientation are preserved, and transparency uses the current
window theme background. Animated formats show their first frame. Input is
limited to 32 MiB and 16 megapixels, with decoder allocation limits. Decoding
and viewport preparation run in a bounded worker that reuses decoded pixels.

**PDF Reader** opens under **Application > Utility > PDF Reader**, or by opening
a PDF in File Browser. Click **Open** or press **Ctrl+O** to choose a local PDF.
Use **Prev/Next**, **PageUp/PageDown**, or **P/N** to change pages; **Home/End**
jump to the first/last page. **+/-** and the mouse wheel zoom by 1.25; **F** or
**Fit** fits the page. Arrows and mouse dragging pan. Zoom is 25–1600% relative
to Fit, which adapts when the window resizes. The window shows the filename,
page number, loading state, and inline errors. Opening another PDF replaces the
document in the same reader window.

PDF rendering requires **`pdfinfo` and `pdftoppm` on PATH**, provided by
[Poppler](https://poppler.freedesktop.org/) (`poppler-utils` on Debian/Ubuntu,
`poppler` on Arch). No extra Rust dependency is needed. Input is limited to
64 MiB; one page at a time is rasterized to at most 2048 pixels per edge, so
detail at high zoom is limited by that raster. Each helper has a 15-second
timeout. Loading and rendering run in a bounded background worker. Close and
Desktop Exit cancel active helpers and reap them. Password-protected PDFs that
require a password show an error; password entry, text selection, search, and
PDF editing are outside this version.

Native windows and document windows participate in tiling, palette updates,
Terminal Lock visibility restoration, and Desktop > Exit. Closing and reopening
a worker-backed app creates a fresh worker; results from its previous instance are
discarded. ASCII Paint saves its own artwork; File Browser file mutations, raster
image editing, and animation playback are outside this version.

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

## AI Chat

Open **Application > Utility > AI Chat**, or search for AI Chat in the desktop
command palette. Click **Settings** inside the AI Chat window to choose the
provider for new chats and enter a model ID for OpenAI, Claude, or Gemini. Select **Replace
key**, paste your provider API key into the masked field, and Save. Keep preserves
an existing key; Remove deletes it. Keys are saved in recoverable, unencrypted
form in AI Chat's own `ai.toml` (0600, configuration directory 0700), normally
`~/.config/desktop-tui/ai-chat/ai.toml`. This dialog saves independently of system
Preferences; Cancel discards its edits. Existing keys and model choices from the
old system `[ai]` section are imported automatically when AI Chat first opens.

Chat supports multiple locally saved conversations. Enter adds a newline;
**Alt+S** sends, **Alt+A** attaches a file, **Alt+X** stops a reply, and **Ctrl+N**
starts a chat. Chats selects saved conversations; Delete asks before removing a
conversation and its copied attachments. Wide windows also show the conversation
list beside the transcript. Scroll with arrows, Page Up/Down, Home/End, or the
wheel; streaming follows new output only while at the bottom. Copy copies the
latest reply to the shared internal clipboard (256 KiB maximum).

Change the provider or model in the chat header; **Use model** applies a typed
model ID. Switching sends nothing by itself. The next Send replays conversation
text and included files to the displayed provider. Replies retain their original
provider/model labels. Stop the active reply before changing providers/models;
one reply runs at a time, while other chats remain browsable. Retry repeats an
incomplete answer without adding the same user message again. Partial answers
remain visible and are excluded from subsequent requests.

The file picker allows any extension. Each selected regular file is copied into
the chat's private local storage; changing or deleting the original does not
change that snapshot. UTF-8 text and code work across providers. Other supported
inputs depend on the provider/model: common images and PDFs, OpenAI Office
inputs, and Gemini audio/video. Unsupported inputs show an explanation and block
Send. **Files** lets you include/exclude attachments from future requests, or
remove pending files; files are never silently omitted. No attachment is sent
until Send. There are no file-conversion or AI command-execution tools.

Limits are 10 MiB per file, 20 MiB of included file data, 32 MiB per encoded
request, 64 KiB of composed text, 1 MiB per reply, and 4 MiB per transcript.
Provider/model limits can be stricter. A missing key/model, authentication error,
rate limit, oversized context, unsupported input, or interrupted connection is
reported in the chat; retries are explicit.

Chats and attachment snapshots live under the application state directory's
`ai/chats` folder (normally `$XDG_STATE_HOME/desktop-tui/ai/chats` or
`~/.local/state/desktop-tui/ai/chats`). History is private and saved atomically.
A second instance opens the same history read-only. Hiding, switching workspaces,
or Terminal Lock keeps a reply running. Closing stops generation and saves its
partial output. If saving fails, Retry / Discard / Cancel protects unsaved work
before Close, Exit, Shutdown, or Restart. Restoring a saved workspace opens the
chat window without resuming an old network request.

## Background activity

Open **Desktop > Activity...** or search for **Activity** in the command
palette. The view shows the last 64 results from the File Browser and AI Chat,
newest first, including work that finishes in another workspace. A batch reports
one result with its completed-item count. Terminal Lock hides the activity view
and indicator while background work continues.

The top bar shows the unread count as **Activity 2**, or **A2** in a narrower
terminal. Opening Activity acknowledges the visible history. New results
appear while it is open, and **Clear** empties this session's history.
Select an entry to read its details below the list.
AI Chat activity shows a brief outcome. Details are available in the chat.
When a file replacement needs confirmation, return to the File Browser.

## Desktop menu and preferences

Linux is the supported target. **Desktop** contains Help, the command palette,
New terminal, Preference, Desktop shortcut, Launchers, Diagnostics, Activity, Workspaces,
Restore saved workspace, Terminal Lock, Screen Lock, Shutdown, Restart, and Exit.
Help opens a resizable offline guide with categories on the left and scrollable
details on the right; its contents are also available in [HELP.md](HELP.md).

**Preference** has **Desktop**, **Terminal**, **Security**, **Panel**, and **Background** tabs. Choose from **Femboy**, **Transgender**, **Asexual**, **Nonbinary**, and **Monochrome** presets, then click **Apply** to update the color draft across the Desktop and Terminal tabs. Browsing presets keeps existing edits until Apply is clicked. Monochrome uses neutral grays. Custom identifies individually edited palettes. Select a color role, enter a `#RRGGBB` color, and inspect the swatch. Desktop roles include the background, surfaces, text, muted text, borders, accents, and selection. Terminal roles include default foreground/background and all 16 normal and bright ANSI colors.

The default Femboy palette uses pale pink `#F6D9EE`, white `#FFF7FC`, plum `#3F2A4A`, pink accents `#E8A4C9`, and lavender selection `#C8B5EA`. **Reset to Femboy** changes the color draft and preserves password settings. **Save** validates and persists all edits, then immediately recolors the desktop and running terminal windows, including hidden windows. **Cancel** or Escape discards edits. Failed saves leave the active palette and saved password unchanged.

Settings live in `preferences.toml` in the application configuration folder, normally `~/.config/desktop-tui` on Linux (or `$XDG_CONFIG_HOME/desktop-tui`). A custom shortcut folder does not change this location. The system preferences file and AI Chat’s own `ai-chat/ai.toml` are excluded from shortcut discovery. Missing settings use defaults. Invalid settings produce one startup warning and use default colors without overwriting the original file. Terminal Lock refuses to start when password settings are malformed or unreadable.

The **Security** tab lets you set, change, or remove a persistent Terminal Lock password. Choose **Set / change saved password**, enter and confirm the new password, then **Save**. Changing or removing an existing password requires the current password. **Keep current setting** preserves it while editing colors. Cancel or Escape discards every tab's edits. Saved passwords are reused for Terminal Lock after restarting Desktop-TUI; when none is saved, locking asks for a temporary password each time. This setting does not change your Linux account password.

Only a salted **Argon2id** password verifier is saved; plaintext passwords are never written to disk. The verifier is stored with the palette in `preferences.toml`, using owner-only permissions: `0600` on the file and `0700` on the application configuration folder. Saves replace the file atomically, so password and color edits succeed together. Symlinked settings and unsafe password-storage permissions are rejected. If permissions were changed externally, restore them with `chmod 700 ~/.config/desktop-tui` and `chmod 600 ~/.config/desktop-tui/preferences.toml` (adjust for `$XDG_CONFIG_HOME`). Passwords cannot be recovered. Anyone with access to your account or permission to terminate Desktop-TUI can bypass its TUI lock; **Screen Lock** locks the graphical Linux session.

**Preference > Background** sets an ASCII desktop image. Press **Alt+5**, click
**Browse...** (Alt+B), and choose a plain `.txt` or `.asc` file, such as the
included [cat example](backgrounds/cat.asc). Review the scrollable preview, then
click **Save**. Loading runs in the background; Save becomes available when it
finishes. Failed imports keep the previous art. Save stores a copy of the text
in `preferences.toml`, so the source file can be moved or deleted afterward.
**Clear art** (Alt+E), then Save, restores the solid background. Cancel or Escape
discards background edits along with the other tabs. Color presets and Reset to
Femboy preserve the selected art.

Art is centered behind windows, uses the desktop Text and Background colors,
and is clipped without wrapping when the terminal is smaller than the image.
Resizing centers it again. Spaces and blank lines are preserved; LF and CRLF
line endings are supported, and tabs expand to four-column stops. Input and
expanded text are limited to 16 KiB. Use printable ASCII characters; ANSI color
codes, Unicode art, and raster images are unsupported for this background.
Empty files also restore the solid background. Existing settings without
`ascii_background` retain the solid background.

**Preference > Panel** configures the top-right status display. Enable or hide the panel, clock, date, Wi-Fi/network status, battery, CPU usage, RAM usage, disk usage, and network download/upload rates. Choose a 12- or 24-hour clock, seconds, a short/ISO/day-month date, and optional network interface names. **Save** applies these options immediately and persists them with the palette and password; Cancel or Escape discards them. Color presets and Reset to Femboy preserve panel settings. Ctrl+Tab cycles all five tabs; Alt+4 opens Panel and Alt+G focuses its date format.

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

Tests use fake system runners to cover command selection, cancellation, duplicate requests, permission errors, unavailable commands, and timeouts. Harmless helper processes check timeout cleanup, and terminal processes check live recoloring of visible and hidden windows.
Debug-backend tests and attended PTY sessions cover the workflow controls at
80×24 and 120×36, including operation without graphical-session variables.
Live PTY resize is checked from 80×24 to 120×36, down to 40×12 and back.
Physical terminal fonts and individual SSH clients may differ; no network SSH
server was required for these PTY checks. Graphical clipboard interoperability
is optional. Verify real screen locking in a graphical desktop and actual power
actions only in a disposable VM.
