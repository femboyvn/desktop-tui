use super::CrossTerm;
use crate::graphics::{CharFlags, Character, Color, Size, Surface};

fn rerun_with_colors(test: &str) -> bool {
    if std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty()) {
        return false;
    }
    // Keep the parent's environment untouched; Crossterm memoizes NO_COLOR.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed;"), "child did not run {test}");
    true
}

fn renderer(size: Size) -> CrossTerm {
    CrossTerm {
        size,
        use_color_schema: false,
        screen_chars: vec![Character::new(0, Color::Transparent, Color::Transparent, CharFlags::None); (size.width * size.height) as usize],
        screen_graphemes: std::collections::BTreeMap::new(),
    }
}

#[test]
fn terminal_input_modes_capture_paste_and_restore_outer_terminal() {
    let mut bytes = Vec::new();
    CrossTerm::write_input_modes(&mut bytes, true).unwrap();
    let enabled = std::str::from_utf8(&bytes).unwrap();
    assert!(enabled.contains("\x1b[?2004h"), "outer terminal must delimit actual pastes");
    assert!(enabled.contains("\x1b[?1006h"));
    bytes.clear();
    CrossTerm::write_input_modes(&mut bytes, false).unwrap();
    let disabled = std::str::from_utf8(&bytes).unwrap();
    assert!(disabled.contains("\x1b[?2004l"));
    assert!(disabled.contains("\x1b[?1006l"));
}

// Interpret SGR resets and true colors before each printed marker. Cursor and
// visibility commands do not affect these attributes. This checks the emitted
// terminal state independently of AppCUI's correctly colored Surface.
fn colors_at(output: &[u8], marker: char) -> (Option<Color>, Option<Color>, bool) {
    let text = std::str::from_utf8(output).unwrap();
    let mut foreground = None;
    let mut background = None;
    let mut bold = false;
    let mut remaining = text;
    while !remaining.is_empty() {
        if let Some(csi) = remaining.strip_prefix("\x1b[") {
            let end = csi.find(|c: char| ('@'..='~').contains(&c)).unwrap();
            if csi.as_bytes()[end] == b'm' {
                let params: Vec<u8> = csi[..end].split(';').map(|n| if n.is_empty() { 0 } else { n.parse().unwrap() }).collect();
                match params.as_slice() {
                    [0] => {
                        foreground = None;
                        background = None;
                        bold = false;
                    }
                    [1] => bold = true,
                    [38, 2, r, g, b] => foreground = Some(Color::RGB(*r, *g, *b)),
                    [48, 2, r, g, b] => background = Some(Color::RGB(*r, *g, *b)),
                    _ => {}
                }
            }
            remaining = &csi[end + 1..];
        } else {
            let character = remaining.chars().next().unwrap();
            if character == marker {
                return (foreground, background, bold);
            }
            remaining = &remaining[character.len_utf8()..];
        }
    }
    panic!("marker {marker} was not printed: {text:?}");
}

#[test]
fn style_changes_preserve_foreground_and_background() {
    if rerun_with_colors("backend::crossterm::implementation::regression_tests::style_changes_preserve_foreground_and_background") {
        return;
    }
    let mut backend = renderer(Size::new(3, 1));
    let mut surface = Surface::new(3, 1);
    let foreground = Color::RGB(91, 39, 71);
    let background = Color::RGB(246, 217, 238);
    for (x, marker, flags) in [(0, 'A', CharFlags::Bold), (1, 'B', CharFlags::None), (2, 'C', CharFlags::Italic)] {
        surface.write_char(x, 0, Character::new(marker, foreground, background, flags));
    }
    let mut output = Vec::new();
    backend.write_screen(&mut output, &surface);
    for (marker, bold) in [('A', true), ('B', false), ('C', false)] {
        assert_eq!(colors_at(&output, marker), (Some(foreground), Some(background), bold), "wrong terminal state for {marker}: {:?}", String::from_utf8_lossy(&output));
    }
}

#[test]
fn repaint_after_styled_frame_restores_plain_background() {
    if rerun_with_colors("backend::crossterm::implementation::regression_tests::repaint_after_styled_frame_restores_plain_background") {
        return;
    }
    let mut backend = renderer(Size::new(2, 1));
    let foreground = Color::RGB(91, 39, 71);
    let background = Color::RGB(246, 217, 238);
    let mut surface = Surface::new(2, 1);
    surface.clear(Character::new('H', foreground, background, CharFlags::Bold));
    let mut output = Vec::new();
    backend.write_screen(&mut output, &surface);

    // Moving a window exposes one cell; the other styled cell remains unchanged
    // and is skipped by the renderer's incremental update.
    surface.write_char(0, 0, Character::new('D', foreground, background, CharFlags::None));
    backend.write_screen(&mut output, &surface);
    assert_eq!(colors_at(&output, 'D'), (Some(foreground), Some(background), false));
}

fn visible_text(output: &[u8]) -> String {
    let text = std::str::from_utf8(output).unwrap();
    let mut remaining = text;
    let mut printed = String::new();
    while !remaining.is_empty() {
        if let Some(csi) = remaining.strip_prefix("\x1b[") {
            let end = csi.find(|c: char| ('@'..='~').contains(&c)).unwrap();
            remaining = &csi[end + 1..];
        } else {
            let ch = remaining.chars().next().unwrap();
            printed.push(ch);
            remaining = &remaining[ch.len_utf8()..];
        }
    }
    printed
}

#[test]
fn grapheme_renderer_prints_clusters_once_and_caches_metadata() {
    let mut backend = renderer(Size::new(6, 1));
    let mut surface = Surface::new(6, 1);
    let attr = crate::graphics::CharAttribute::with_color(Color::White, Color::Black);
    let combining = format!("a{}", "\u{301}".repeat(40));
    surface.write_grapheme(0, 0, &combining, 1, attr);
    surface.write_grapheme(1, 0, "界", 2, attr);
    surface.write_grapheme(3, 0, "👩🏽‍💻", 2, attr);
    surface.write_char(5, 0, Character::with_char('!'));
    let mut output = Vec::new();
    backend.write_screen(&mut output, &surface);
    assert_eq!(visible_text(&output), format!("{combining}界👩🏽‍💻!"));
    output.clear();
    backend.write_screen(&mut output, &surface);
    assert_eq!(visible_text(&output), "");
    surface.write_grapheme(0, 0, "a\u{302}", 1, attr);
    output.clear();
    backend.write_screen(&mut output, &surface);
    assert_eq!(visible_text(&output), "a\u{302}");
}

#[test]
fn grapheme_renderer_erases_old_continuation_on_narrow_replacement() {
    let mut backend = renderer(Size::new(4, 1));
    let mut surface = Surface::new(4, 1);
    let attr = crate::graphics::CharAttribute::with_color(Color::White, Color::Black);
    surface.write_grapheme(0, 0, "界", 2, attr);
    surface.write_char(2, 0, Character::with_char('#'));
    let mut output = Vec::new();
    backend.write_screen(&mut output, &surface);
    surface.write_char(0, 0, Character::with_char('x'));
    output.clear();
    backend.write_screen(&mut output, &surface);
    assert_eq!(visible_text(&output), "x ");
    assert!(!visible_text(&output).contains('#'));
    surface.write_grapheme(0, 0, "🙂", 2, attr);
    output.clear();
    backend.write_screen(&mut output, &surface);
    assert_eq!(visible_text(&output), "🙂");
}

#[test]
fn grapheme_renderer_recolors_full_glyph_and_handles_multiple_rows() {
    let mut backend = renderer(Size::new(2, 2));
    let mut surface = Surface::new(2, 2);
    let attr = crate::graphics::CharAttribute::with_color(Color::White, Color::Black);
    surface.write_grapheme(0, 0, "界", 2, attr);
    surface.write_grapheme(0, 1, "🙂", 2, attr);
    let mut output = Vec::new();
    backend.write_screen(&mut output, &surface);
    assert_eq!(visible_text(&output), "界🙂");
    surface.write_char(1, 0, Character::with_color(Color::Red, Color::Blue));
    output.clear();
    backend.write_screen(&mut output, &surface);
    assert_eq!(visible_text(&output), "界");
}
