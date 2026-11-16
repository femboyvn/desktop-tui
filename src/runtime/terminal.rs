use crate::config::preferences::TerminalPalette;
use appcui::prelude::{CharAttribute, CharFlags, Character, Color, Surface};
use std::io::{self, Write};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tattoy_wezterm_term::{
    CellAttributes, CursorVisibility, Intensity, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind, Terminal, TerminalConfiguration, TerminalSize, Underline, UnicodeVersion,
};
use tattoy_wezterm_term::color::{ColorAttribute, ColorPalette, RgbColor};

// Replies are small protocol messages. Bound unsolicited child-query traffic
// independently of the PTY's bounded output and input queues.
const MAX_HISTORY_ROWS: usize = 2000;
const MAX_HISTORY_CELLS: usize = 200_000;
const MAX_HISTORY_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_COPY_BYTES: usize = 256 * 1024;
const MAX_SEARCH_MATCHES: usize = 4096;
const MAX_REPLY_BYTES: usize = 64 * 1024;
pub(crate) const MAX_TERMINAL_DIMENSION: u32 = 10000;

#[derive(Clone)]
struct ReplyWriter { bytes: Arc<Mutex<Vec<u8>>>, overflow: Arc<AtomicBool> }
impl Write for ReplyWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut buffer = self.bytes.lock().unwrap_or_else(|error| error.into_inner());
        if bytes.len() <= MAX_REPLY_BYTES.saturating_sub(buffer.len()) {
            buffer.extend_from_slice(bytes);
        } else { self.overflow.store(true, Ordering::Relaxed); }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

#[derive(Debug)]
struct EmulatorConfiguration {
    palette: ColorPalette,
    history: Arc<AtomicUsize>,
}
impl TerminalConfiguration for EmulatorConfiguration {
    fn color_palette(&self) -> ColorPalette { self.palette.clone() }
    fn scrollback_size(&self) -> usize { self.history.load(Ordering::Relaxed) }
    fn use_threaded_writer(&self) -> bool { false }
    fn enable_images(&self) -> bool { false }
    fn terminfo_name(&self) -> Option<&str> { Some("xterm-256color") }
    fn unicode_version(&self) -> UnicodeVersion {
        UnicodeVersion { version: 14, ambiguous_are_wide: false, cell_widths: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalMouseKind { Press, Release, Move }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalMouseButton { Left, Middle, Right, WheelUp, WheelDown, WheelLeft, WheelRight, None }

/// Stateful emulator facade. Advancing bytes preserves partial UTF-8 and escape
/// sequences; projecting is separate so a UI tick paints once after a PTY batch.
#[derive(Clone)]
struct Selection { anchor: (isize, usize), caret: (isize, usize), marked: bool }
impl Selection {
    fn range(&self) -> ((isize, usize), (isize, usize)) { (self.anchor.min(self.caret), self.anchor.max(self.caret)) }
}
struct HistorySearch {
    query: String, next: isize, end: isize, cell: usize, byte: usize,
    prefix: Vec<usize>, matched: usize, positions: VecDeque<(isize, usize)>,
    matches: Vec<((isize, usize), (isize, usize))>, current: Option<usize>,
}
impl HistorySearch {
    fn new(next: isize, end: isize) -> Self {
        Self { query: String::new(), next, end, cell: 0, byte: 0, prefix: Vec::new(), matched: 0,
            positions: VecDeque::new(), matches: Vec::new(), current: None }
    }
    fn reset_partial(&mut self) { self.matched = 0; self.positions.clear(); }
    fn feed(&mut self, byte: u8, point: (isize, usize)) -> Option<((isize, usize), (isize, usize))> {
        if self.query.is_empty() { return None; }
        self.positions.push_back(point);
        if self.positions.len() > self.query.len() { self.positions.pop_front(); }
        while self.matched > 0 && byte != self.query.as_bytes()[self.matched] { self.matched = self.prefix[self.matched - 1]; }
        if byte == self.query.as_bytes()[self.matched] { self.matched += 1; }
        if self.matched == self.query.len() {
            self.matched = self.prefix[self.matched - 1];
            return self.positions.front().map(|begin| (*begin, point));
        }
        None
    }
    fn set_query(&mut self, query: String) {
        self.query = query.chars().take(256).collect();
        self.prefix = vec![0; self.query.len()];
        for i in 1..self.query.len() {
            let mut matched = self.prefix[i - 1];
            while matched > 0 && self.query.as_bytes()[i] != self.query.as_bytes()[matched] { matched = self.prefix[matched - 1]; }
            if self.query.as_bytes()[i] == self.query.as_bytes()[matched] { matched += 1; }
            self.prefix[i] = matched;
        }
    }
}
pub struct TerminalParser {
    width: u32,
    height: u32,
    terminal: Terminal,
    replies: Arc<Mutex<Vec<u8>>>,
    reply_overflow: Arc<AtomicBool>,
    palette: TerminalPalette,
    background_override: Option<Color>,
    history_limit: usize,
    history_capacity: Arc<AtomicUsize>,
    viewport: Option<isize>,
    selection: Option<Selection>,
    search: Option<HistorySearch>,
}
impl TerminalParser {
    pub fn with_palette(width: u32, height: u32, palette: &TerminalPalette, background_override: Option<Color>) -> Self {
        let width = width.clamp(1, MAX_TERMINAL_DIMENSION);
        let height = height.clamp(1, MAX_TERMINAL_DIMENSION);
        let replies = Arc::new(Mutex::new(Vec::new()));
        let reply_overflow = Arc::new(AtomicBool::new(false));
        let history_capacity = Arc::new(AtomicUsize::new(MAX_HISTORY_ROWS.min(MAX_HISTORY_CELLS / width as usize)));
        let terminal = Terminal::new(
            terminal_size(width, height),
            Arc::new(EmulatorConfiguration { palette: emulator_palette(palette, background_override), history: history_capacity.clone() }),
            "Desktop-TUI", env!("CARGO_PKG_VERSION"),
            Box::new(ReplyWriter { bytes: replies.clone(), overflow: reply_overflow.clone() }),
        );
        Self { width, height, terminal, replies, reply_overflow, palette: palette.clone(), background_override, history_limit: MAX_HISTORY_ROWS, history_capacity, viewport: None, selection: None, search: None }
    }
    pub fn new_surface(&self) -> Surface {
        let mut surface = Surface::new(self.width, self.height);
        surface.clear(Character::new(' ', self.palette.foreground.color(),
            self.background_override.unwrap_or(self.palette.background.color()), CharFlags::None));
        surface
    }
    pub fn apply_palette(&mut self, palette: &TerminalPalette, surface: &mut Surface) {
        self.palette = palette.clone();
        let colors = emulator_palette(palette, self.background_override);
        self.history_capacity.store(self.history_capacity(), Ordering::Relaxed);
        let history = self.history_capacity.clone();
        self.terminal.set_config(Arc::new(EmulatorConfiguration { palette: colors.clone(), history }));
        *self.terminal.palette_mut() = colors;
        self.project(surface);
    }
    pub fn resize_surface(&mut self, width: u32, height: u32, surface: &mut Surface) {
        // Keep a small content anchor through the core's width reflow, whose
        // physical stable row numbers can change when rows merge or split.
        let anchor = self.viewport.map(|_| self.viewport_anchor());
        self.width = width.clamp(1, MAX_TERMINAL_DIMENSION);
        self.height = height.clamp(1, MAX_TERMINAL_DIMENSION);
        self.update_config();
        self.terminal.resize(terminal_size(self.width, self.height));
        self.enforce_history_budget();
        if let Some(anchor) = anchor { self.reveal_resize_anchor(&anchor); }
        self.clamp_view();
        self.selection = None;
        self.search = None;
        *surface = self.new_surface();
        self.project(surface);
    }
    pub fn advance_bytes(&mut self, bytes: &[u8]) {
        let alternate = self.terminal.is_alt_screen_active();
        let previous_live = self.live_start();
        self.terminal.advance_bytes(bytes);
        if alternate != self.terminal.is_alt_screen_active() { self.live(); }
        if previous_live != self.live_start() { self.enforce_history_budget(); }
        self.clamp_view();
    }
    fn enforce_history_budget(&mut self) {
        let rows = self.history_capacity();
        // Apply to primary history even when the child currently displays its
        // alternate screen; discard oldest rows intact, preserving recent text.
        self.terminal.enforce_scrollback_limits(rows, MAX_HISTORY_TEXT_BYTES);
    }
    pub fn parse(&mut self, bytes: &[u8], surface: &mut Surface) {
        self.advance_bytes(bytes);
        self.project(surface);
    }
    pub fn parse_to_surface(&mut self, bytes: &[u8], mut surface: Surface) -> Surface {
        self.parse(bytes, &mut surface);
        surface
    }
    pub fn project(&self, surface: &mut Surface) {
        if surface.size().width != self.width || surface.size().height != self.height {
            *surface = self.new_surface();
        }
        let palette = self.terminal.palette();
        surface.clear(Character::new(' ', color(palette.foreground), self.background_override.unwrap_or(color(palette.background)), CharFlags::None));
        let screen = self.terminal.screen();
        let start = screen.stable_row_to_phys(self.view_start()).unwrap_or_else(|| screen.phys_row(0));
        let lines = screen.lines_in_phys_range(start..start + self.height as usize);
        for (y, line) in lines.iter().enumerate() {
            for cell in line.visible_cells() {
                let x = cell.cell_index();
                if x >= self.width as usize { break; }
                let mut attr = cell_attributes(cell.attrs(), &palette, self.background_override, self.terminal.get_reverse_video());
                let point = (screen.phys_to_stable_row_index(start + y), x);
                let selected = self.selection.as_ref().is_some_and(|selection| {
                    let (a, b) = selection.range();
                    point >= a && point <= b || (point.0 == a.0 && x < a.1 && x + cell.width() > a.1)
                });
                let matched = self.search.as_ref().and_then(|search| search.current.and_then(|i| search.matches.get(i)))
                    .is_some_and(|(a,b)| point >= *a && point <= *b);
                if selected || matched { std::mem::swap(&mut attr.foreground, &mut attr.background); }
                let text = if cell.attrs().invisible() { if cell.width() == 2 { "  " } else { " " } } else { cell.str() };
                surface.write_grapheme(x as i32, y as i32, text, cell.width(), attr);
            }
        }
        surface.hide_cursor();
        if let Some((x, y)) = self.cursor() { surface.set_cursor(x as i32, y as i32); }
    }
    pub fn cursor(&self) -> Option<(u32, u32)> {
        if let Some(selection) = &self.selection {
            let y = selection.caret.0 - self.view_start();
            return (y >= 0 && y < self.height as isize).then_some((selection.caret.1 as u32, y as u32));
        }
        if self.viewport.is_some() || self.search.is_some() { return None; }
        let cursor = self.terminal.cursor_pos();
        if cursor.visibility == CursorVisibility::Hidden || cursor.y < 0 { return None; }
        Some(((cursor.x as u32).min(self.width - 1), (cursor.y as u32).min(self.height - 1)))
    }
    fn viewport_anchor(&self) -> String {
        let screen = self.terminal.screen();
        let start = screen.stable_row_to_phys(self.view_start()).unwrap_or(0);
        let mut text = String::new();
        screen.for_each_phys_line(|index, line| {
            if index != start { return; }
            for cell in line.visible_cells() {
                if text.len() + cell.str().len() > 128 { break; }
                text.push_str(cell.str());
            }
        });
        text.truncate(text.trim_end_matches(' ').len()); text
    }
    fn reveal_resize_anchor(&mut self, anchor: &str) {
        if anchor.is_empty() { return; }
        let mut search = HistorySearch::new(self.first_row(), self.terminal.screen().phys_to_stable_row_index(self.terminal.screen().scrollback_rows()));
        search.set_query(anchor.into());
        // Reflow is already a bounded synchronous core operation. Inspect only
        // retained text, without cloning the full history or allocating a snapshot.
        let screen = self.terminal.screen();
        let old = self.viewport.unwrap_or_else(|| self.live_start());
        let mut nearest: Option<isize> = None;
        let mut budget = MAX_HISTORY_TEXT_BYTES;
        screen.for_each_phys_line(|index, line| {
            let row = screen.phys_to_stable_row_index(index);
            for cell in line.visible_cells() {
                for byte in cell.str().as_bytes() {
                    if budget == 0 { return; } budget -= 1;
                    if let Some((begin, _)) = search.feed(*byte, (row, cell.cell_index()))
                        && nearest.is_none_or(|previous| begin.0.abs_diff(old) < previous.abs_diff(old)) { nearest = Some(begin.0); }
                }
            }
            if !line.last_cell_was_wrapped() { search.reset_partial(); }
        });
        if let Some(row) = nearest { self.viewport = Some(row); }
    }
    fn history_capacity(&self) -> usize { self.history_limit.min(MAX_HISTORY_CELLS / self.width as usize) }
    fn update_config(&mut self) {
        self.history_capacity.store(self.history_capacity(), Ordering::Relaxed);
        let history = self.history_capacity.clone();
        self.terminal.set_config(Arc::new(EmulatorConfiguration {
            palette: emulator_palette(&self.palette, self.background_override), history,
        }));
    }
    #[cfg(test)]
    fn set_history_limit(&mut self, lines: usize) {
        self.history_limit = lines.min(MAX_HISTORY_ROWS);
        self.update_config();
        // The core applies reduced capacity on its next scroll. Reclaim old
        // history immediately when reducing a populated buffer.
        if self.history_rows() > self.history_capacity() { self.terminal.screen_mut().erase_scrollback(); }
        self.clamp_view();
    }
    pub(crate) fn history_rows(&self) -> usize { self.terminal.screen().phys_row(0) }
    fn first_row(&self) -> isize { self.terminal.screen().phys_to_stable_row_index(0) }
    pub(crate) fn live_start(&self) -> isize { self.terminal.screen().phys_to_stable_row_index(self.history_rows()) }
    pub(crate) fn view_start(&self) -> isize { self.viewport.unwrap_or_else(|| self.live_start()).clamp(self.first_row(), self.live_start()) }
    fn clamp_view(&mut self) {
        if self.viewport.is_some() { self.viewport = Some(self.view_start()); }
        let first = self.first_row();
        if self.selection.as_ref().is_some_and(|selection| selection.anchor.0 < first || selection.caret.0 < first) {
            self.selection = None;
        }
        if let Some(search) = &mut self.search {
            let selected = search.current.and_then(|index| search.matches.get(index).copied());
            search.matches.retain(|(start, _)| start.0 >= first);
            search.current = selected.and_then(|selected| search.matches.iter().position(|range| *range == selected));
        }
    }
    pub(crate) fn scroll(&mut self, rows: isize) {
        self.viewport = Some(self.view_start().saturating_sub(rows).clamp(self.first_row(), self.live_start()));
    }
    pub(crate) fn live(&mut self) { self.viewport = None; self.selection = None; self.search = None; }
    pub(crate) fn copy_mode(&self) -> bool { self.selection.is_some() }
    pub(crate) fn start_copy(&mut self) {
        self.search = None;
        let point = (self.view_start(), 0);
        self.viewport = Some(self.view_start());
        self.selection = Some(Selection { anchor: point, caret: point, marked: false });
    }
    pub(crate) fn exit_mode(&mut self) { self.selection = None; self.search = None; }
    pub(crate) fn select_at(&mut self, x: usize, y: usize, anchor: bool) {
        let point = (self.view_start() + y.min(self.height as usize - 1) as isize, x.min(self.width as usize - 1));
        if let Some(selection) = &mut self.selection {
            selection.caret = point;
            if anchor { selection.anchor = point; selection.marked = true; }
        }
    }
    pub(crate) fn mark_selection(&mut self) {
        if let Some(selection) = &mut self.selection { selection.anchor = selection.caret; selection.marked = true; }
    }
    pub(crate) fn move_caret(&mut self, dx: isize, dy: isize) {
        let last = self.terminal.screen().phys_to_stable_row_index(self.terminal.screen().scrollback_rows() - 1);
        let first = self.first_row();
        let Some(selection) = &mut self.selection else { return; };
        selection.caret.0 = (selection.caret.0 + dy).clamp(first, last);
        selection.caret.1 = selection.caret.1.saturating_add_signed(dx).min(self.width as usize - 1);
        if !selection.marked { selection.anchor = selection.caret; }
        let row = selection.caret.0;
        let start = self.view_start();
        if row < start { self.viewport = Some(row); }
        else if row >= start + self.height as isize { self.viewport = Some(row - self.height as isize + 1); }
    }
    pub(crate) fn selected_text(&self) -> Result<String, &'static str> {
        let Some(selection) = &self.selection else { return Err("Enter Copy mode first"); };
        let (start, end) = selection.range();
        let screen = self.terminal.screen();
        let (Some(first), Some(last)) = (screen.stable_row_to_phys(start.0), screen.stable_row_to_phys(end.0)) else {
            return Err("Selection was evicted from history");
        };
        let mut result = String::new();
        let mut overflow = false;
        screen.for_each_phys_line(|physical, line| {
            if physical < first || physical > last || overflow { return; }
            let index = physical - first;
            let mut row = String::new();
            for cell in line.visible_cells() {
                let x = cell.cell_index();
                if index == 0 && x + cell.width().max(1) <= start.1 { continue; }
                if index == last - first && x > end.1 { break; }
                if result.len() + row.len() + cell.str().len() > MAX_COPY_BYTES { overflow = true; return; }
                row.push_str(cell.str());
            }
            // A partial final endpoint explicitly includes its space cells.
            // At a physical line boundary, omit trailing visual padding.
            let partial_endpoint = index == last - first && end.1 < self.width as usize - 1;
            if !line.last_cell_was_wrapped() && !partial_endpoint { row.truncate(row.trim_end_matches(' ').len()); }
            result.push_str(&row);
            if index < last - first && !line.last_cell_was_wrapped() {
                if result.len() == MAX_COPY_BYTES { overflow = true; return; }
                result.push('\n');
            }
        });
        if overflow { Err("Selection exceeds 256 KiB; select less text") } else { Ok(result) }
    }
    pub(crate) fn search_mode(&self) -> bool { self.search.is_some() }
    pub(crate) fn start_search(&mut self) {
        self.selection = None;
        self.viewport = Some(self.view_start());
        self.search = Some(HistorySearch::new(self.first_row(), self.terminal.screen().phys_to_stable_row_index(self.terminal.screen().scrollback_rows())));
    }
    pub(crate) fn search_query(&self) -> &str { self.search.as_ref().map_or("", |search| search.query.as_str()) }
    pub(crate) fn set_search_query(&mut self, query: String) {
        self.start_search();
        if let Some(search) = &mut self.search { search.set_query(query); }
    }
    pub(crate) fn search_pending(&self) -> bool { self.search.as_ref().is_some_and(|search| !search.query.is_empty() && search.next < search.end) }
    pub(crate) fn search_match_count(&self) -> usize { self.search.as_ref().map_or(0, |search| search.matches.len()) }
    /// Scan at most 16 rows / 128 KiB each tick. KMP keeps byte work linear,
    /// supports overlapping literals and matches inside complete graphemes.
    pub(crate) fn search_step(&mut self) -> bool {
        if !self.search_pending() { return false; }
        let screen = self.terminal.screen();
        let Some(search) = &mut self.search else { return false; };
        let mut budget = 128 * 1024;
        for _ in 0..16 {
            if search.next >= search.end || budget == 0 { break; }
            let stable = search.next;
            let Some(physical) = screen.stable_row_to_phys(stable) else {
                search.next += 1; search.cell = 0; search.byte = 0; search.reset_partial(); continue;
            };
            let mut completed = true;
            screen.for_each_phys_line(|index, line| {
                if index != physical { return; }
                for (index, cell) in line.visible_cells().enumerate() {
                    if index < search.cell { continue; }
                    let text = cell.str().as_bytes();
                    let from = if index == search.cell { search.byte } else { 0 };
                    for (offset, byte) in text.iter().enumerate().skip(from) {
                        if budget == 0 {
                            search.cell = index; search.byte = offset; completed = false; return;
                        }
                        budget -= 1;
                        if let Some(range) = search.feed(*byte, (stable, cell.cell_index()))
                            && search.matches.len() < MAX_SEARCH_MATCHES { search.matches.push(range); }
                    }
                }
                if !line.last_cell_was_wrapped() { search.reset_partial(); }
            });
            if completed { search.next += 1; search.cell = 0; search.byte = 0; }
        }
        if search.current.is_none() && !search.matches.is_empty() { self.search_next(false); }
        true
    }
    pub(crate) fn search_next(&mut self, previous: bool) {
        let Some(search) = &mut self.search else { return; };
        if search.matches.is_empty() { return; }
        let len = search.matches.len();
        let index = match search.current { None => if previous { len - 1 } else { 0 },
            Some(index) => if previous { (index + len - 1) % len } else { (index + 1) % len } };
        search.current = Some(index);
        self.viewport = Some(search.matches[index].0.0);
        self.clamp_view();
    }
    pub(crate) fn status(&self) -> String {
        if let Some(search) = &self.search {
            format!("Search: {} | {} matches{} | Enter next, Shift+Enter previous, Esc exit", search.query,
                self.search_match_count(), if self.search_match_count() == MAX_SEARCH_MATCHES { "+ (limit)" } else if self.search_pending() { " (searching)" } else { "" })
        } else if self.copy_mode() { "Copy | arrows move, Space marks, Enter copies, Esc exits".into() }
        else if self.viewport.is_some() { format!("History: {} rows back | Shift+PageDown or Live", self.live_start() - self.view_start()) }
        else { String::new() }
    }
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut *self.replies.lock().unwrap_or_else(|error| error.into_inner()))
    }
    /// Returns and clears the reply-buffer exhaustion flag. The owning window
    /// must report this and close the child instead of silently losing replies.
    pub fn take_reply_error(&self) -> bool { self.reply_overflow.swap(false, Ordering::Relaxed) }
    pub fn application_cursor_keys(&self) -> bool { self.terminal.application_cursor_keys() }
    pub fn mouse_reporting_enabled(&self) -> bool { self.terminal.is_mouse_grabbed() }
    pub fn paste(&self, text: &str) -> Vec<u8> {
        if self.terminal.bracketed_paste_enabled() {
            // Match the emulator's bracket defanging while retaining the entire
            // paste; the UI input queue supplies the explicit size limit.
            let text = text.replace("\x1b[200~", "").replace("\x1b[201~", "");
            let mut bytes = b"\x1b[200~".to_vec();
            bytes.extend_from_slice(text.as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
            bytes
        } else { text.as_bytes().to_vec() }
    }
    pub fn mouse_event(&mut self, kind: TerminalMouseKind, x: usize, y: usize, button: TerminalMouseButton,
        modifier: appcui::input::KeyModifier) -> Vec<u8> {
        if !self.mouse_reporting_enabled() || x >= self.width as usize || y >= self.height as usize {
            return Vec::new();
        }
        let mut modifiers = KeyModifiers::empty();
        if modifier.contains(appcui::input::KeyModifier::Shift) { modifiers |= KeyModifiers::SHIFT; }
        if modifier.contains(appcui::input::KeyModifier::Alt) { modifiers |= KeyModifiers::ALT; }
        if modifier.contains(appcui::input::KeyModifier::Ctrl) { modifiers |= KeyModifiers::CTRL; }
        let kind = match kind { TerminalMouseKind::Press => MouseEventKind::Press,
            TerminalMouseKind::Release => MouseEventKind::Release, TerminalMouseKind::Move => MouseEventKind::Move };
        let button = match button {
            TerminalMouseButton::Left => MouseButton::Left, TerminalMouseButton::Middle => MouseButton::Middle,
            TerminalMouseButton::Right => MouseButton::Right, TerminalMouseButton::WheelUp => MouseButton::WheelUp(1),
            TerminalMouseButton::WheelDown => MouseButton::WheelDown(1), TerminalMouseButton::WheelLeft => MouseButton::WheelLeft(1),
            TerminalMouseButton::WheelRight => MouseButton::WheelRight(1), TerminalMouseButton::None => MouseButton::None,
        };
        let before = self.replies.lock().unwrap_or_else(|error| error.into_inner()).len();
        // The configured writer is synchronous and memory-only, so this cannot
        // block a callback on a PTY write or create a background writer thread.
        let _ = self.terminal.mouse_event(MouseEvent { kind, x, y: y as i64,
            x_pixel_offset: 0, y_pixel_offset: 0, button, modifiers });
        self.replies.lock().unwrap_or_else(|error| error.into_inner()).split_off(before)
    }
}

fn terminal_size(width: u32, height: u32) -> TerminalSize {
    TerminalSize { rows: height as usize, cols: width as usize, ..TerminalSize::default() }
}
fn color(value: tattoy_wezterm_term::color::SrgbaTuple) -> Color {
    let (r, g, b, _) = value.to_srgb_u8();
    Color::RGB(r, g, b)
}
fn emulator_palette(palette: &TerminalPalette, background_override: Option<Color>) -> ColorPalette {
    let background = match background_override.unwrap_or(palette.background.color()) {
        Color::RGB(r, g, b) => RgbColor::new_8bpc(r, g, b).into(),
        _ => RgbColor::new_8bpc(palette.background.0, palette.background.1, palette.background.2).into(),
    };
    let mut colors = ColorPalette {
        foreground: RgbColor::new_8bpc(palette.foreground.0, palette.foreground.1, palette.foreground.2).into(),
        background,
        ..ColorPalette::default()
    };
    for (index, entry) in palette.ansi.iter().enumerate() {
        colors.colors.0[index] = RgbColor::new_8bpc(entry.0, entry.1, entry.2).into();
    }
    // Keep the previous facade's fixed xterm colors at indices 16..255.
    for index in 16..256 {
        let (r, g, b) = if index < 232 {
            let value = index - 16;
            let level = |v| if v == 0 { 0 } else { (55 + 40 * v) as u8 };
            (level(value / 36), level((value / 6) % 6), level(value % 6))
        } else { let value = (8 + (index - 232) * 10) as u8; (value, value, value) };
        colors.colors.0[index] = RgbColor::new_8bpc(r, g, b).into();
    }
    colors
}
fn cell_attributes(attributes: &CellAttributes, palette: &ColorPalette, background_override: Option<Color>, screen_reverse: bool) -> CharAttribute {
    let foreground = color(palette.resolve_fg(attributes.foreground()));
    let background = if attributes.background() == ColorAttribute::Default {
        background_override.unwrap_or(color(palette.background))
    } else { color(palette.resolve_bg(attributes.background())) };
    let (foreground, background) = if attributes.reverse() ^ screen_reverse { (background, foreground) }
        else { (foreground, background) };
    let mut flags = CharFlags::None;
    if attributes.intensity() == Intensity::Bold { flags |= CharFlags::Bold; }
    if attributes.italic() { flags |= CharFlags::Italic; }
    if attributes.strikethrough() { flags |= CharFlags::StrikeThrough; }
    flags |= match attributes.underline() {
        Underline::None => CharFlags::None, Underline::Double => CharFlags::DoubleUnderline,
        Underline::Curly => CharFlags::CurlyUnderline, Underline::Dotted => CharFlags::DottedUnderline,
        _ => CharFlags::Underline,
    };
    CharAttribute::new(foreground, background, flags)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::preferences::{Preferences, Rgb};
    #[test]
    fn review_pruning_preserves_visible_rows_cursor_and_stable_ids() {
        let mut parser = parser(100,2);
        parser.advance_bytes(&vec![b'x';190_000]);
        parser.terminal.resize(terminal_size(10,2));
        let before_cursor = parser.terminal.cursor_pos();
        let before_start = parser.terminal.screen().visible_row_to_stable_row(0);
        let before_rows: Vec<String> = parser.terminal.screen().lines_in_phys_range(parser.history_rows()..parser.history_rows()+2).iter().map(|line| line.as_str().into_owned()).collect();
        parser.terminal.enforce_scrollback_limits(MAX_HISTORY_ROWS,MAX_HISTORY_TEXT_BYTES);
        assert_eq!(parser.history_rows(),MAX_HISTORY_ROWS);
        assert_eq!(parser.terminal.cursor_pos(),before_cursor);
        assert_eq!(parser.terminal.screen().visible_row_to_stable_row(0),before_start);
        let after_rows: Vec<String> = parser.terminal.screen().lines_in_phys_range(parser.history_rows()..parser.history_rows()+2).iter().map(|line| line.as_str().into_owned()).collect();
        assert_eq!(after_rows,before_rows);
    }
    #[test]
    fn review_text_budget_prunes_oldest_rows_under_alternate_screen() {
        let mut parser = parser(10,2);
        let oversized = format!("a{}","\u{301}".repeat(16));
        let seqno = parser.terminal.current_seqno();
        parser.terminal.screen_mut().set_cell_grapheme(0,0,&oversized,1,CellAttributes::default(),seqno);
        parser.terminal.advance_bytes(b"\r\nkeep\r\nlast\r\nlive\x1b[?1049hALT");
        parser.terminal.resize(terminal_size(20,2));
        // Scale the public helper's byte budget instead of constructing a
        // multi-megabyte grapheme; the facade supplies its 8 MiB production cap.
        parser.terminal.enforce_scrollback_limits(MAX_HISTORY_ROWS,32);
        parser.terminal.advance_bytes(b"\x1b[?1049l");
        assert_eq!(parser.history_rows(),1,"drop oversized oldest row, retain recent history");
        assert_eq!(parser.terminal.screen().lines_in_phys_range(0..1)[0].as_str(),"keep");
    }
    #[test]
    fn review_quiet_reflow_enforces_row_and_cell_limits() {
        let mut parser = parser(100,2);
        parser.advance_bytes(&vec![b'x';190_000]);
        let mut surface = parser.new_surface();
        parser.resize_surface(10,2,&mut surface);
        assert!(parser.history_rows() <= MAX_HISTORY_ROWS,"quiet narrow: {} rows",parser.history_rows());
        assert!(parser.history_rows() * 10 <= MAX_HISTORY_CELLS);

        let mut parser = self::parser(10,2);
        for index in 0..1500 { parser.advance_bytes(format!("{index:04}\r\n").as_bytes()); }
        parser.scroll(isize::MAX);
        let mut surface = parser.new_surface();
        parser.resize_surface(500,2,&mut surface);
        assert!(parser.history_rows() <= MAX_HISTORY_CELLS / 500,"quiet widen: {} rows",parser.history_rows());
        assert_eq!(parser.view_start(),parser.first_row(),"evicted anchor must clamp to retained history");
        assert!(parser.history_rows() > 0,"keep the most recent history");
    }
    #[test]
    fn review_alternate_screen_resize_limits_inactive_primary_history() {
        let mut parser = parser(100,2);
        parser.advance_bytes(&vec![b'x';190_000]);
        parser.advance_bytes(b"\x1b[?1049hALT");
        let mut surface = parser.new_surface();
        parser.resize_surface(10,2,&mut surface);
        assert_eq!(parser.history_rows(),0);
        // Bypass the facade's next-output accounting: the resize itself must
        // already have bounded the inactive primary screen.
        parser.terminal.advance_bytes(b"\x1b[?1049l");
        assert!(parser.history_rows() <= MAX_HISTORY_ROWS,"inactive primary: {} rows",parser.history_rows());
        assert!(parser.history_rows() > 0);
    }
    #[test]
    fn review_copy_preserves_explicit_space_endpoints() {
        let mut parser = parser(10,3);
        parser.advance_bytes("a b\r\n中 x".as_bytes()); parser.start_copy();
        parser.select_at(0,0,true); parser.select_at(1,0,false);
        assert_eq!(parser.selected_text().unwrap(),"a ");
        parser.select_at(1,0,true); parser.select_at(1,0,false);
        assert_eq!(parser.selected_text().unwrap()," ");
        parser.select_at(0,0,true); parser.select_at(2,1,false);
        assert_eq!(parser.selected_text().unwrap(),"a b\n中 ");
        let mut parser = self::parser(10,2);
        parser.advance_bytes(b"a "); parser.start_copy();
        parser.select_at(0,0,true); parser.select_at(1,0,false);
        assert_eq!(parser.selected_text().unwrap(),"a ");
        parser.select_at(9,0,false);
        assert_eq!(parser.selected_text().unwrap(),"a","full physical-line selection trims visual trailing padding");
    }
    #[test]
    fn history_limits_bound_rows_cells_and_evicted_selection() {
        let mut parser = parser(500,2);
        parser.set_history_limit(usize::MAX);
        for _ in 0..600 { parser.advance_bytes(b"line\r\n"); }
        assert_eq!(parser.history_rows(),MAX_HISTORY_CELLS / 500);
        parser.scroll(isize::MAX);
        parser.start_copy(); parser.select_at(0,0,true);
        parser.advance_bytes(b"new\r\n");
        assert!(!parser.copy_mode(),"eviction must invalidate the old selection");
        assert_eq!(parser.view_start(),parser.first_row());
        parser.set_history_limit(1);
        assert!(parser.history_rows() <= 1);
    }
    #[test]
    fn history_search_yields_and_resumes_inside_a_long_unicode_row() {
        let mut parser = parser(3000,2);
        let text = format!("a{}", "\u{301}".repeat(30)).repeat(3000);
        parser.advance_bytes(text.as_bytes());
        parser.set_search_query("a".into());
        parser.search_step();
        assert!(parser.search_pending(),"one tick must not scan more than 128 KiB");
        while parser.search_pending() { parser.search_step(); }
        assert_eq!(parser.search_match_count(),3000);
    }
    #[test]
    fn paused_viewport_reveals_the_same_content_after_reflow() {
        let mut parser = parser(4, 2);
        parser.advance_bytes(b"abcdefgh\r\nijkl\r\nmnop\r\nqrst");
        parser.scroll(2);
        let mut surface = parser.new_surface(); parser.project(&mut surface);
        assert_eq!(surface.cell_text(0,0).unwrap(), "e");
        parser.resize_surface(8,2,&mut surface);
        let row: String = (0..8).map(|x| surface.cell_text(x,0).unwrap().into_owned()).collect();
        assert!(row.contains("efgh"), "resizing lost the anchored content: {row:?}");
        assert!(parser.viewport.is_some());
    }
    #[test]
    fn literal_search_finds_subgraphemes_soft_wraps_and_navigates_both_directions() {
        let mut parser = parser(4,3);
        parser.advance_bytes("e\u{301}abce\u{301}\r\nlast".as_bytes());
        parser.start_search(); parser.set_search_query("e".into());
        while parser.search_pending() { parser.search_step(); }
        assert_eq!(parser.search_match_count(),2);
        let first = parser.search.as_ref().unwrap().current;
        parser.search_next(false); assert_ne!(parser.search.as_ref().unwrap().current,first);
        parser.search_next(true); assert_eq!(parser.search.as_ref().unwrap().current,first);
        parser.set_search_query("ce\u{301}".into());
        while parser.search_pending() { parser.search_step(); }
        assert_eq!(parser.search_match_count(),1);
    }
    #[test]
    fn history_anchor_eviction_unicode_selection_and_search() {
        let mut parser = parser(8, 2);
        parser.set_history_limit(3);
        parser.advance_bytes("one\r\ntwo\r\n中e\u{301}😀\r\nfour\r\nfive".as_bytes());
        assert!(parser.history_rows() > 0);
        parser.scroll(2);
        let anchor = parser.view_start();
        parser.advance_bytes(b"\r\nsix");
        assert_eq!(parser.view_start(), anchor);
        assert!(parser.history_rows() <= 3);
        parser.scroll(-1);
        parser.start_copy();
        parser.select_at(0, 0, true);
        parser.select_at(4, 0, false);
        assert_eq!(parser.selected_text().unwrap(), "中e\u{301}😀");
        parser.start_search();
        parser.set_search_query("four".into());
        while parser.search_pending() { parser.search_step(); }
        assert_eq!(parser.search_match_count(), 1);
        parser.live();
        assert_eq!(parser.view_start(), parser.live_start());
    }
    #[test]
    fn selection_joins_soft_wraps_but_keeps_hard_newlines_and_alt_screen_separate() {
        let mut parser = parser(4, 3);
        parser.advance_bytes(b"abcdef\r\nxy");
        parser.start_copy();
        parser.select_at(0, 0, true);
        parser.select_at(1, 2, false);
        assert_eq!(parser.selected_text().unwrap(), "abcdef\nxy");
        parser.advance_bytes(b"\x1b[?1049hALT");
        assert_eq!(parser.history_rows(), 0);
        assert!(!parser.copy_mode());
        parser.advance_bytes(b"\x1b[?1049l");
        assert_eq!(parser.history_rows(), 0);
    }
    #[test]
    fn recolors_default_standard_bright_and_indexed_colors_without_rgb_collisions() {
        let old = Preferences::default().terminal;
        let mut parser = TerminalParser::with_palette(30, 3, &old, None);
        let surface = parser.new_surface();
        let fixed = old.ansi[1];
        let text = format!(
            "a\x1b[31;44;1;3;4mb\x1b[91mc\x1b[38;5;1md\x1b[38;2;{};{};{}me\x1b[38;5;200mf\x1b[0mg",
            fixed.0, fixed.1, fixed.2
        );
        let mut surface = parser.parse_to_surface(text.as_bytes(), surface);
        let mut next = old.clone();
        next.foreground = Rgb(1, 2, 3);
        next.background = Rgb(4, 5, 6);
        next.ansi[1] = Rgb(11, 12, 13);
        next.ansi[9] = Rgb(21, 22, 23);
        next.ansi[4] = Rgb(31, 32, 33);
        let before = *surface.char(1, 0).unwrap();
        parser.apply_palette(&next, &mut surface);
        assert_eq!(
            surface.char(0, 0).unwrap().foreground,
            next.foreground.color()
        );
        assert_eq!(
            surface.char(0, 0).unwrap().background,
            next.background.color()
        );
        assert_eq!(surface.char(1, 0).unwrap().foreground, next.ansi[1].color());
        assert_eq!(surface.char(1, 0).unwrap().background, next.ansi[4].color());
        assert_eq!(surface.char(1, 0).unwrap().flags, before.flags);
        assert_eq!(surface.char(1, 0).unwrap().code, 'b');
        assert_eq!(surface.char(2, 0).unwrap().foreground, next.ansi[9].color());
        assert_eq!(surface.char(3, 0).unwrap().foreground, next.ansi[1].color());
        assert_eq!(surface.char(4, 0).unwrap().foreground, fixed.color());
        assert_eq!(
            surface.char(5, 0).unwrap().foreground,
            Color::RGB(255, 0, 215)
        );
        assert_eq!(
            surface.char(6, 0).unwrap().foreground,
            next.foreground.color()
        );
        // SGR resets and recoloring do not move the cursor or change future writes.
        surface = parser.parse_to_surface(b"h", surface);
        assert_eq!(surface.char(7, 0).unwrap().code, 'h');
        assert_eq!(
            surface.char(7, 0).unwrap().foreground,
            next.foreground.color()
        );
    }
    #[test]
    fn shortcut_override_survives_reset_erase_resize_and_palette_changes() {
        let old = Preferences::default().terminal;
        let override_color = Color::RGB(7, 8, 9);
        let mut parser = TerminalParser::with_palette(10, 3, &old, Some(override_color));
        let surface = parser.new_surface();
        let mut surface = parser.parse_to_surface(b"\x1b[41mA\x1b[49mB\x1b[0mC\x1b[2K", surface);
        parser.resize_surface(12, 4, &mut surface);
        let mut next = old.clone();
        next.background = Rgb(99, 98, 97);
        next.ansi[1] = Rgb(80, 81, 82);
        parser.apply_palette(&next, &mut surface);
        for (x, y) in [(0, 0), (2, 0), (0, 1), (11, 3)] {
            assert_eq!(surface.char(x, y).unwrap().background, override_color);
        }
        surface = parser.parse_to_surface(b"D", surface);
        assert_eq!(surface.char(3, 0).unwrap().code, 'D');
        assert_eq!(surface.char(3, 0).unwrap().background, override_color);
    }
    #[test]
    fn erase_and_resize_keep_color_metadata_aligned() {
        let old = Preferences::default().terminal;
        let mut parser = TerminalParser::with_palette(6, 3, &old, None);
        let mut surface = parser.parse_to_surface(
            b"\x1b[32mAB\x1b[2;2H\x1b[104mX\x1b[0K",
            parser.new_surface(),
        );
        parser.resize_surface(9, 4, &mut surface);
        let mut next = old.clone();
        next.ansi[2] = Rgb(1, 2, 3);
        next.ansi[12] = Rgb(4, 5, 6);
        parser.apply_palette(&next, &mut surface);
        assert_eq!(surface.char(0, 0).unwrap().code, 'A');
        assert_eq!(surface.char(0, 0).unwrap().foreground, next.ansi[2].color());
        assert_eq!(surface.char(1, 1).unwrap().code, 'X');
        assert_eq!(surface.char(5, 1).unwrap().code, ' ');
        assert_eq!(
            surface.char(5, 1).unwrap().background,
            next.ansi[12].color()
        );
        assert_eq!(
            surface.char(8, 3).unwrap().background,
            next.background.color()
        );
        parser.resize_surface(3, 2, &mut surface);
        assert_eq!(surface.char(1, 1).unwrap().code, 'X');
        surface = parser.parse_to_surface(b"\x1b[2J", surface);
        next.ansi[12] = Rgb(50, 51, 52);
        parser.apply_palette(&next, &mut surface);
        assert_eq!(
            surface.char(0, 0).unwrap().background,
            next.ansi[12].color()
        );
    }
    fn parser(width: u32, height: u32) -> TerminalParser {
        TerminalParser::with_palette(width, height, &Preferences::default().terminal, None)
    }
    fn snapshot(surface: &Surface) -> Vec<(String, usize, Character)> {
        (0..surface.size().height as i32).flat_map(|y| {
            (0..surface.size().width as i32).map(move |x| (
                surface.cell_text(x, y).unwrap().into_owned(), surface.cell_width(x, y).unwrap(),
                *surface.char(x, y).unwrap(),
            ))
        }).collect()
    }
    #[test]
    fn graphemes_are_complete_immediately_at_every_byte_boundary() {
        let long = format!("a{}", "\u{0301}".repeat(96));
        for (text, width) in [
            ("e\u{0302}\u{0301}", 1), ("中", 2), ("❤️", 2), ("👍🏽", 2),
            ("🇻🇳", 2), ("👩‍💻", 2), ("👩‍👩‍👧‍👦", 2), (long.as_str(), 1),
        ] {
            for split in 0..=text.len() {
                let mut parser = parser(24, 3);
                let mut surface = parser.new_surface();
                parser.parse(&text.as_bytes()[..split], &mut surface);
                parser.parse(&text.as_bytes()[split..], &mut surface);
                assert_eq!(surface.cell_text(0, 0).unwrap(), text, "{text:?}, split {split}");
                assert_eq!(surface.cell_width(0, 0), Some(width), "{text:?}, split {split}");
                assert_eq!(parser.cursor(), Some((width as u32, 0)));
                if width == 2 { assert_eq!(surface.cell_width(1, 0), Some(0)); }
            }
            let mut parser = parser(24, 3);
            let mut surface = parser.new_surface();
            for byte in text.as_bytes() { parser.parse(&[*byte], &mut surface); }
            assert_eq!(surface.cell_text(0, 0).unwrap(), text);
        }
    }
    #[test]
    fn scalar_feed_extensions_do_not_merge_adjacent_emoji_or_move_cursor_twice() {
        let text = "a👩‍💻😀👍🏽🇻🇳e\u{0302}\u{0301}中!";
        let mut expected = parser(30, 3);
        let mut expected_surface = expected.new_surface();
        expected.parse(text.as_bytes(), &mut expected_surface);
        let mut parser = parser(30, 3);
        let mut surface = parser.new_surface();
        for scalar in text.chars() {
            let mut bytes = [0; 4];
            parser.parse(scalar.encode_utf8(&mut bytes).as_bytes(), &mut surface);
        }
        assert_eq!(snapshot(&surface), snapshot(&expected_surface));
        assert_eq!(parser.cursor(), expected.cursor());
        assert_eq!(surface.cell_text(1, 0).unwrap(), "👩‍💻");
        assert_eq!(surface.cell_text(3, 0).unwrap(), "😀");
    }
    #[test]
    fn partial_controls_alt_screen_scroll_regions_and_replies_are_streaming() {
        let stream = b"primary\x1b[?1049h\x1b[2J\x1b[2;3r\x1b[2;1HA\r\nB\r\nC\x1b[?25l";
        let mut expected = parser(12, 4);
        let mut expected_surface = expected.new_surface();
        expected.parse(stream, &mut expected_surface);
        assert_eq!(expected_surface.cell_text(0, 1).unwrap(), "B");
        assert_eq!(expected_surface.cell_text(0, 2).unwrap(), "C");
        assert_eq!(expected_surface.cell_text(0, 0).unwrap(), " ");
        for split in 0..=stream.len() {
            let mut parser = parser(12, 4);
            let mut surface = parser.new_surface();
            parser.parse(&stream[..split], &mut surface);
            parser.parse(&stream[split..], &mut surface);
            assert_eq!(snapshot(&surface), snapshot(&expected_surface), "split {split}");
            assert_eq!(parser.cursor(), None);
            parser.parse(b"\x1b[?1049l\x1b[?25h", &mut surface);
            assert_eq!(surface.cell_text(0, 0).unwrap(), "p");
            assert_eq!(parser.cursor(), Some((7, 0)));
        }
        expected.advance_bytes(b"\x1b[?25h\x1b[3;4H\x1b[6n\x1b[c");
        let replies = expected.take_replies();
        assert!(replies.starts_with(b"\x1b[3;4R"));
        assert!(!String::from_utf8_lossy(&replies).contains(";4;"), "must not advertise sixel");
        assert!(expected.take_replies().is_empty());
    }
    #[test]
    fn input_modes_paste_mouse_and_cursor_follow_the_emulator() {
        let mut parser = parser(40, 10);
        assert_eq!(parser.paste("ế\nURL"), "ế\nURL".as_bytes());
        assert!(!parser.application_cursor_keys());
        assert!(!parser.mouse_reporting_enabled());
        parser.advance_bytes(b"\x1b[?1h\x1b[?2004h\x1b[?1000h\x1b[?1006h");
        assert!(parser.application_cursor_keys());
        assert_eq!(parser.paste("ế\x1b[201~"), "\x1b[200~ế\x1b[201~".as_bytes());
        assert_eq!(parser.mouse_event(TerminalMouseKind::Press, 2, 3, TerminalMouseButton::Left,
            appcui::input::KeyModifier::None), b"\x1b[<0;3;4M");
        assert_eq!(parser.mouse_event(TerminalMouseKind::Release, 2, 3, TerminalMouseButton::Left,
            appcui::input::KeyModifier::None), b"\x1b[<0;3;4m");
        parser.advance_bytes(b"\x1b[?1000l\x1b[?1006l\x1b[?1l\x1b[?2004l");
        assert!(!parser.application_cursor_keys());
        assert!(parser.mouse_event(TerminalMouseKind::Press, 2, 3, TerminalMouseButton::Left,
            appcui::input::KeyModifier::None).is_empty());
    }
    #[test]
    fn wrap_erase_insert_and_resize_invalidate_only_the_correct_grapheme() {
        let mut parser = parser(4, 3);
        let mut surface = parser.new_surface();
        parser.parse(b"abcd", &mut surface);
        parser.parse("\u{0301}".as_bytes(), &mut surface);
        assert_eq!(surface.cell_text(3, 0).unwrap(), "d\u{0301}");
        parser.parse(b"E", &mut surface);
        assert_eq!(surface.cell_text(0, 1).unwrap(), "E");
        parser.parse(b"\x1b[1;1H\x1b[4hX", &mut surface);
        parser.parse("\u{0301}".as_bytes(), &mut surface);
        assert_eq!(surface.cell_text(0, 0).unwrap(), "X\u{0301}");
        assert_eq!(surface.cell_text(1, 0).unwrap(), "a");
        parser.parse(b"\x1b[4l\x1b[2KZ", &mut surface);
        parser.resize_surface(8, 4, &mut surface);
        assert_eq!(surface.size().width, 8);
        assert!(parser.cursor().unwrap().0 < 8);
    }

    #[test]
    fn wide_graphemes_wrap_without_clipping_and_split_variation_matches_one_feed() {
        let mut terminal = parser(4, 2);
        let mut surface = terminal.new_surface();
        terminal.parse("abc中".as_bytes(), &mut surface);
        assert_eq!(surface.cell_text(0, 1).unwrap(), "中");
        assert_eq!(surface.cell_width(1, 1), Some(0));
        for text in ["abc❤️", "abc⌚︎", "abcdefg❤️", "abcdefg⌚︎"] {
            let mut expected = parser(4, 2);
            let mut expected_surface = expected.new_surface();
            expected.parse(text.as_bytes(), &mut expected_surface);
            for split in 0..=text.len() {
                let mut parser = parser(4, 2);
                let mut surface = parser.new_surface();
                parser.parse(&text.as_bytes()[..split], &mut surface);
                parser.parse(&text.as_bytes()[split..], &mut surface);
                assert_eq!(snapshot(&surface), snapshot(&expected_surface), "{text:?} split {split}");
                assert_eq!(parser.cursor(), expected.cursor());
            }
        }
    }
    #[test]
    fn reply_exhaustion_is_bounded_and_reported() {
        let mut parser = parser(80, 24);
        for _ in 0..20000 { parser.advance_bytes(b"\x1b[6n"); }
        assert!(parser.take_reply_error());
        assert!(!parser.take_reply_error());
        assert!(parser.take_replies().len() <= MAX_REPLY_BYTES);
    }

    #[test]
    fn every_complete_scalar_prefix_projects_before_the_next_feed() {
        for text in ["e\u{0302}\u{0301}", "👩‍👩‍👧‍👦", "🇻🇳", "👍🏽", "❤️"] {
            let mut parser = parser(20, 3);
            let mut surface = parser.new_surface();
            for (offset, scalar) in text.char_indices() {
                let end = offset + scalar.len_utf8();
                parser.parse(&text.as_bytes()[offset..end], &mut surface);
                assert_eq!(surface.cell_text(0, 0).unwrap(), &text[..end]);
            }
        }
    }
    #[test]
    fn osc_unknown_controls_invalid_utf8_and_styles_do_not_desynchronize_text() {
        let mut parser = parser(20, 3);
        let mut surface = parser.new_surface();
        for byte in b"\x1b]0;title\x07A\x1b[7mB\x1b[0m\x1b[8mC\x1b[0m\xffD" {
            parser.parse(&[*byte], &mut surface);
        }
        assert_eq!(surface.cell_text(0, 0).unwrap(), "A");
        assert_eq!(surface.char(1, 0).unwrap().foreground, Preferences::default().terminal.background.color());
        assert_eq!(surface.cell_text(2, 0).unwrap(), " ");
        // The core drops invalid UTF-8 bytes without moving the cursor.
        assert_eq!(surface.cell_text(3, 0).unwrap(), "D");
    }

    #[test]
    fn queried_capabilities_match_the_text_only_xterm_profile() {
        let mut parser = parser(20, 3);
        parser.advance_bytes(b"\x1bP+q544e\x1b\\\x1b[?2;1S");
        let replies = String::from_utf8(parser.take_replies()).unwrap();
        // XTGETTCAP TN = xterm-256color, XTSMGRAPHICS = invalid item.
        assert!(replies.contains("1+r544E=787465726D2D323536636F6C6F72"));
        assert!(replies.contains("\x1b[?2;1S"));
    }

}
