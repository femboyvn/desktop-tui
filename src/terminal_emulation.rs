use crate::preferences::TerminalPalette;
use appcui::prelude::{CharFlags, Character, Color, Surface};

// Keep provenance: an RGB value can equal a palette color without referring to it.
#[derive(Debug, Clone, Copy)]
enum ColorSource {
    DefaultForeground,
    DefaultBackground,
    Ansi(u8),
    Fixed(Color),
}
#[derive(Clone, Copy)]
struct CellColors {
    foreground: ColorSource,
    background: ColorSource,
}
impl Default for CellColors {
    fn default() -> Self {
        Self {
            foreground: ColorSource::DefaultForeground,
            background: ColorSource::DefaultBackground,
        }
    }
}
#[derive(Debug, Clone, Copy)]
struct TerminalState {
    foreground: ColorSource,
    background: ColorSource,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    cursor_x: i32,
    cursor_y: i32,
}
impl TerminalState {
    fn reset(&mut self) {
        self.foreground = ColorSource::DefaultForeground;
        self.background = ColorSource::DefaultBackground;
        self.bold = false;
        self.dim = false;
        self.italic = false;
        self.underline = false;
    }
}
pub struct TerminalParser {
    width: u32,
    height: u32,
    state: TerminalState,
    palette: TerminalPalette,
    background_override: Option<Color>,
    cells: Vec<CellColors>,
    cursor_visible: bool,
}
impl TerminalParser {
    pub fn with_palette(
        width: u32,
        height: u32,
        palette: &TerminalPalette,
        background_override: Option<Color>,
    ) -> Self {
        let width = width.clamp(1, 10000);
        let height = height.clamp(1, 10000);
        Self {
            width,
            height,
            palette: palette.clone(),
            background_override,
            cells: vec![CellColors::default(); (width * height) as usize],
            cursor_visible: false,
            state: TerminalState {
                foreground: ColorSource::DefaultForeground,
                background: ColorSource::DefaultBackground,
                bold: false,
                dim: false,
                italic: false,
                underline: false,
                cursor_x: 0,
                cursor_y: 0,
            },
        }
    }
    fn resolve(&self, source: ColorSource) -> Color {
        match source {
            ColorSource::DefaultForeground => self.palette.foreground.color(),
            ColorSource::DefaultBackground => self
                .background_override
                .unwrap_or(self.palette.background.color()),
            ColorSource::Ansi(index) => self.palette.ansi[index as usize].color(),
            ColorSource::Fixed(color) => color,
        }
    }
    pub fn new_surface(&self) -> Surface {
        let mut surface = Surface::new(self.width, self.height);
        surface.clear(Character::new(
            ' ',
            self.resolve(ColorSource::DefaultForeground),
            self.resolve(ColorSource::DefaultBackground),
            CharFlags::None,
        ));
        surface
    }
    pub fn apply_palette(&mut self, palette: &TerminalPalette, surface: &mut Surface) {
        self.palette = palette.clone();
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                let colors = self.cells[(y as u32 * self.width + x as u32) as usize];
                if let Some(mut character) = surface.char(x, y).copied() {
                    character.foreground = self.resolve(colors.foreground);
                    character.background = self.resolve(colors.background);
                    surface.write_char(x, y, character);
                }
            }
        }
    }
    pub fn resize_surface(&mut self, width: u32, height: u32, surface: &mut Surface) {
        let width = width.clamp(1, 10000);
        let height = height.clamp(1, 10000);
        let mut next = Surface::new(width, height);
        next.clear(Character::new(
            ' ',
            self.resolve(ColorSource::DefaultForeground),
            self.resolve(ColorSource::DefaultBackground),
            CharFlags::None,
        ));
        let mut cells = vec![CellColors::default(); (width * height) as usize];
        for y in 0..height.min(self.height) {
            for x in 0..width.min(self.width) {
                if let Some(character) = surface.char(x as i32, y as i32) {
                    next.write_char(x as i32, y as i32, *character);
                }
                cells[(y * width + x) as usize] = self.cells[(y * self.width + x) as usize];
            }
        }
        self.width = width;
        self.height = height;
        self.cells = cells;
        self.state.cursor_x = self.state.cursor_x.clamp(0, width as i32 - 1);
        self.state.cursor_y = self.state.cursor_y.clamp(0, height as i32 - 1);
        if self.cursor_visible {
            next.set_cursor(self.state.cursor_x, self.state.cursor_y);
        }
        *surface = next;
    }
    fn put_cell(&mut self, x: i32, y: i32, code: char, flags: CharFlags, surface: &mut Surface) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        self.cells[(y as u32 * self.width + x as u32) as usize] = CellColors {
            foreground: self.state.foreground,
            background: self.state.background,
        };
        surface.write_char(
            x,
            y,
            Character::new(
                code,
                self.resolve(self.state.foreground),
                self.resolve(self.state.background),
                flags,
            ),
        );
    }
    pub fn parse_to_surface(&mut self, data: &[u8], mut surface: Surface) -> Surface {
        let text = String::from_utf8_lossy(data);
        let chars: Vec<char> = text.chars().collect();

        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '\u{1b}' && i + 1 < chars.len() && chars[i + 1] == '[' {
                // Re-encode remaining chars into bytes for ANSI parsing
                let slice: String = chars[i..].iter().collect();
                let consumed = self.parse_ansi_sequence(slice.as_bytes(), &mut surface);
                // convert consumed bytes -> consumed chars
                let consumed_chars = String::from_utf8_lossy(&slice.as_bytes()[..consumed])
                    .chars()
                    .count();
                i += consumed_chars;
            } else {
                // Handle regular character
                self.write_character(chars[i], &mut surface);
                i += 1;
            }
        }

        if self.cursor_visible {
            surface.set_cursor(self.state.cursor_x, self.state.cursor_y);
        }
        surface
    }

    fn parse_ansi_sequence(&mut self, data: &[u8], surface: &mut Surface) -> usize {
        if data.len() < 3 {
            return 1; // Skip invalid sequence
        }

        let mut i = 2; // Skip '\x1b['
        let mut params = Vec::new();
        let mut current_param = String::new();
        let mut private_mode = false;

        // Handle private mode prefix '?'
        if i < data.len() && data[i] == b'?' {
            private_mode = true;
            i += 1;
        }

        // Parse parameters
        while i < data.len() {
            let byte = data[i];
            match byte {
                b'0'..=b'9' => current_param.push(byte as char),
                b';' => {
                    params.push(current_param.parse::<u32>().unwrap_or(0));
                    current_param.clear();
                }
                b'A'..=b'Z' | b'a'..=b'z' => {
                    // End of sequence
                    if !current_param.is_empty() {
                        params.push(current_param.parse::<u32>().unwrap_or(0));
                    }
                    if private_mode {
                        self.handle_private_ansi_command(byte as char, &params, surface);
                    } else {
                        self.handle_ansi_command(byte as char, &params, surface);
                    }
                    return i + 1;
                }
                _ => break,
            }
            i += 1;
        }

        1 // Skip if we couldn't parse
    }

    fn handle_ansi_command(&mut self, command: char, params: &[u32], surface: &mut Surface) {
        match command {
            'H' | 'f' => {
                // Cursor position
                let row = params.get(0).unwrap_or(&1).saturating_sub(1) as i32;
                let col = params.get(1).unwrap_or(&1).saturating_sub(1) as i32;
                self.state.cursor_x = col.min(self.width as i32 - 1);
                self.state.cursor_y = row.min(self.height as i32 - 1);
            }
            'A' => {
                // Cursor up
                let count = params.get(0).unwrap_or(&1);
                self.state.cursor_y = (self.state.cursor_y - *count as i32).max(0);
            }
            'B' => {
                // Cursor down
                let count = params.get(0).unwrap_or(&1);
                self.state.cursor_y =
                    (self.state.cursor_y + *count as i32).min(self.height as i32 - 1);
            }
            'C' => {
                // Cursor right
                let count = params.get(0).unwrap_or(&1);
                self.state.cursor_x =
                    (self.state.cursor_x + *count as i32).min(self.width as i32 - 1);
            }
            'D' => {
                // Cursor left
                let count = params.get(0).unwrap_or(&1);
                self.state.cursor_x = (self.state.cursor_x - *count as i32).max(0);
            }
            'm' => {
                // SGR (Select Graphic Rendition) - colors and attributes
                if params.is_empty() {
                    // Reset all attributes
                    self.state.reset();
                } else {
                    self.handle_sgr_params(params);
                }
            }
            'J' => {
                // Clear screen
                let mode = params.get(0).copied().unwrap_or(0);
                self.handle_erase_display(mode, surface);
            }
            'K' => {
                // Clear line
                let mode = params.get(0).copied().unwrap_or(0);
                self.handle_erase_line(mode, surface);
            }
            _ => {
                // Ignore unknown sequences
            }
        }
    }

    fn handle_private_ansi_command(
        &mut self,
        command: char,
        params: &[u32],
        surface: &mut Surface,
    ) {
        if !params.contains(&25) {
            return;
        }
        match command {
            'l' => {
                self.cursor_visible = false;
                // Hide cursor
                surface.hide_cursor()
            }
            'h' => {
                self.cursor_visible = true;
                // Show cursor
                surface.set_cursor(self.state.cursor_x, self.state.cursor_y);
            }
            _ => {
                // ignore unknown private sequences
            }
        }
    }

    fn handle_erase_display(&mut self, param: u32, surface: &mut Surface) {
        match param {
            0 => {
                // clear from cursor to end of screen
                for y in self.state.cursor_y..self.height as i32 {
                    let start_x = if y == self.state.cursor_y {
                        self.state.cursor_x
                    } else {
                        0
                    };
                    for x in start_x..self.width as i32 {
                        self.put_cell(x, y, ' ', CharFlags::None, surface);
                    }
                }
            }
            1 => {
                // clear from beginning of screen to cursor
                for y in 0..=self.state.cursor_y {
                    let end_x = if y == self.state.cursor_y {
                        self.state.cursor_x
                    } else {
                        self.width as i32 - 1
                    };
                    for x in 0..=end_x {
                        self.put_cell(x, y, ' ', CharFlags::None, surface);
                    }
                }
            }
            2 => {
                // clear entire screen
                for y in 0..self.height as i32 {
                    for x in 0..self.width as i32 {
                        self.put_cell(x, y, ' ', CharFlags::None, surface);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_erase_line(&mut self, param: u32, surface: &mut Surface) {
        match param {
            0 => {
                // clear from cursor to end of line
                for x in self.state.cursor_x..self.width as i32 {
                    self.put_cell(x, self.state.cursor_y, ' ', CharFlags::None, surface);
                }
            }
            1 => {
                // clear from beginning of line to cursor
                for x in 0..=self.state.cursor_x {
                    self.put_cell(x, self.state.cursor_y, ' ', CharFlags::None, surface);
                }
            }
            2 => {
                // clear entire line
                for x in 0..self.width as i32 {
                    self.put_cell(x, self.state.cursor_y, ' ', CharFlags::None, surface);
                }
            }
            _ => {}
        }
    }

    fn handle_sgr_params(&mut self, params: &[u32]) {
        let mut iter = params.iter().copied().peekable();

        while let Some(param) = iter.next() {
            match param {
                0 => self.state.reset(), // Reset
                1 => self.state.bold = true,
                2 => self.state.dim = true,
                3 => self.state.italic = true,
                4 => self.state.underline = true,
                22 => {
                    self.state.bold = false;
                    self.state.dim = false;
                }
                23 => self.state.italic = false,
                24 => self.state.underline = false,

                39 => self.state.foreground = ColorSource::DefaultForeground,
                49 => self.state.background = ColorSource::DefaultBackground,

                // 16-color standard + bright
                30..=37 => self.state.foreground = ColorSource::Ansi((param - 30) as u8),
                40..=47 => self.state.background = ColorSource::Ansi((param - 40) as u8),
                90..=97 => self.state.foreground = ColorSource::Ansi((param - 90 + 8) as u8),
                100..=107 => self.state.background = ColorSource::Ansi((param - 100 + 8) as u8),

                // Extended color sequences
                38 | 48 => {
                    let is_foreground = param == 38;

                    if let Some(mode) = iter.next() {
                        match mode {
                            5 => {
                                // 256-color: 38;5;<idx> or 48;5;<idx>
                                if let Some(idx) = iter.next() {
                                    let color = if idx < 16 {
                                        ColorSource::Ansi(idx as u8)
                                    } else {
                                        ColorSource::Fixed(ansi_256_color(idx))
                                    };
                                    if is_foreground {
                                        self.state.foreground = color;
                                    } else {
                                        self.state.background = color;
                                    }
                                }
                            }
                            2 => {
                                // Truecolor: 38;2;<r>;<g>;<b> or 48;2;<r>;<g>;<b>
                                if let (Some(r), Some(g), Some(b)) =
                                    (iter.next(), iter.next(), iter.next())
                                {
                                    let color = ColorSource::Fixed(Color::RGB(
                                        r.min(255) as u8,
                                        g.min(255) as u8,
                                        b.min(255) as u8,
                                    ));

                                    if is_foreground {
                                        self.state.foreground = color;
                                    } else {
                                        self.state.background = color;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }

                _ => {
                    // Ignore unknown
                }
            }
        }
    }

    fn write_character(&mut self, ch: char, surface: &mut Surface) {
        match ch {
            '\r' => {
                self.state.cursor_x = 0;
            }
            '\n' => {
                self.state.cursor_x = 0;
                self.state.cursor_y += 1;
                if self.state.cursor_y >= self.height as i32 {
                    self.state.cursor_y = self.height as i32 - 1;
                }
            }
            '\t' => {
                // Tab to next 8-character boundary
                self.state.cursor_x = ((self.state.cursor_x / 8) + 1) * 8;
                self.cursor_forward()
            }
            '\x08' => {
                // Backspace
                if self.state.cursor_x > 0 {
                    self.state.cursor_x -= 1;
                }
            }
            c if c.is_control() => {
                // Ignore other control characters
            }
            c => {
                // Regular printable character
                let mut flags = CharFlags::None;
                if self.state.bold {
                    flags |= CharFlags::Bold;
                }
                if self.state.dim {
                    //flags |= CharFlags::Dim;
                }
                if self.state.italic {
                    flags |= CharFlags::Italic;
                }
                if self.state.underline {
                    flags |= CharFlags::Underline;
                }

                self.put_cell(self.state.cursor_x, self.state.cursor_y, c, flags, surface);
                self.cursor_forward();
            }
        }
    }

    pub fn cursor_forward(&mut self) {
        // Advance cursor
        self.state.cursor_x += 1;
        if self.state.cursor_x >= self.width as i32 {
            self.state.cursor_x = 0;
            self.state.cursor_y += 1;
            if self.state.cursor_y >= self.height as i32 {
                self.state.cursor_y = self.height as i32 - 1;
            }
        }
    }
}

/// Fixed xterm colors (indices 16–255 never follow user palette changes).
fn ansi_256_color(idx: u32) -> Color {
    match idx {
        16..=231 => {
            let n = idx - 16;
            let level = |v| if v == 0 { 0 } else { (55 + 40 * v) as u8 };
            Color::RGB(level((n / 36) % 6), level((n / 6) % 6), level(n % 6))
        }
        232..=255 => {
            let level = (8 + (idx - 232) * 10) as u8;
            Color::RGB(level, level, level)
        }
        _ => Color::RGB(0, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::{Preferences, Rgb};
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
}
