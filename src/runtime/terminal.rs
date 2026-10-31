use crate::config::preferences::TerminalPalette;
use appcui::prelude::{CharAttribute, CharFlags, Character, Color, Surface};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tattoy_wezterm_term::{
    CellAttributes, CursorVisibility, Intensity, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind, Terminal, TerminalConfiguration, TerminalSize, Underline, UnicodeVersion,
};
use tattoy_wezterm_term::color::{ColorAttribute, ColorPalette, RgbColor};

// Replies are small protocol messages. Bound unsolicited child-query traffic
// independently of the PTY's bounded output and input queues.
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
}
impl TerminalConfiguration for EmulatorConfiguration {
    fn color_palette(&self) -> ColorPalette { self.palette.clone() }
    fn scrollback_size(&self) -> usize { 0 }
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
pub struct TerminalParser {
    width: u32,
    height: u32,
    terminal: Terminal,
    replies: Arc<Mutex<Vec<u8>>>,
    reply_overflow: Arc<AtomicBool>,
    palette: TerminalPalette,
    background_override: Option<Color>,
}
impl TerminalParser {
    pub fn with_palette(width: u32, height: u32, palette: &TerminalPalette, background_override: Option<Color>) -> Self {
        let width = width.clamp(1, MAX_TERMINAL_DIMENSION);
        let height = height.clamp(1, MAX_TERMINAL_DIMENSION);
        let replies = Arc::new(Mutex::new(Vec::new()));
        let reply_overflow = Arc::new(AtomicBool::new(false));
        let terminal = Terminal::new(
            terminal_size(width, height),
            Arc::new(EmulatorConfiguration { palette: emulator_palette(palette, background_override) }),
            "Desktop-TUI", env!("CARGO_PKG_VERSION"),
            Box::new(ReplyWriter { bytes: replies.clone(), overflow: reply_overflow.clone() }),
        );
        Self { width, height, terminal, replies, reply_overflow, palette: palette.clone(), background_override }
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
        self.terminal.set_config(Arc::new(EmulatorConfiguration { palette: colors.clone() }));
        *self.terminal.palette_mut() = colors;
        self.project(surface);
    }
    pub fn resize_surface(&mut self, width: u32, height: u32, surface: &mut Surface) {
        self.width = width.clamp(1, MAX_TERMINAL_DIMENSION);
        self.height = height.clamp(1, MAX_TERMINAL_DIMENSION);
        self.terminal.resize(terminal_size(self.width, self.height));
        *surface = self.new_surface();
        self.project(surface);
    }
    pub fn advance_bytes(&mut self, bytes: &[u8]) { self.terminal.advance_bytes(bytes); }
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
        let start = screen.phys_row(0);
        let lines = screen.lines_in_phys_range(start..start + self.height as usize);
        for (y, line) in lines.iter().enumerate() {
            for cell in line.visible_cells() {
                let x = cell.cell_index();
                if x >= self.width as usize { break; }
                let attr = cell_attributes(cell.attrs(), &palette, self.background_override, self.terminal.get_reverse_video());
                let text = if cell.attrs().invisible() { if cell.width() == 2 { "  " } else { " " } } else { cell.str() };
                surface.write_grapheme(x as i32, y as i32, text, cell.width(), attr);
            }
        }
        surface.hide_cursor();
        if let Some((x, y)) = self.cursor() { surface.set_cursor(x as i32, y as i32); }
    }
    pub fn cursor(&self) -> Option<(u32, u32)> {
        let cursor = self.terminal.cursor_pos();
        if cursor.visibility == CursorVisibility::Hidden || cursor.y < 0 { return None; }
        Some(((cursor.x as u32).min(self.width - 1), (cursor.y as u32).min(self.height - 1)))
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
