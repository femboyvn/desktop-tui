use crate::config::preferences::DesktopPalette;
use appcui::prelude::*;
pub fn build_theme(p: &DesktopPalette) -> Theme {
    let mut t = Theme::new(appcui::system::Themes::Light);
    let attr = |fg: Color, bg: Color| CharAttribute::new(fg, bg, CharFlags::None);
    let normal = attr(p.text.color(), p.surface.color());
    let muted = attr(p.muted_text.color(), p.surface.color());
    let accent = attr(p.text.color(), p.accent.color());
    let selection = attr(p.text.color(), p.selection.color());
    let line = attr(p.border.color(), Color::Transparent);
    let mut state = t.menu.text;
    state.normal = normal;
    state.focused = selection;
    state.hovered = accent;
    state.inactive = muted;
    state.pressed_or_selected = selection;
    for target in [
        &mut t.menu.text,
        &mut t.menu.hotkey,
        &mut t.menu.shortcut,
        &mut t.menu.symbol,
        &mut t.parent_menu.text,
        &mut t.parent_menu.hotkey,
        &mut t.parent_menu.shortcut,
        &mut t.parent_menu.symbol,
        &mut t.button.regular.text,
        &mut t.button.regular.hotkey,
        &mut t.button.bevel.text,
        &mut t.button.bevel.hotkey,
        &mut t.tab.text,
        &mut t.tab.hotkey,
        &mut t.accordion.text,
        &mut t.accordion.hotkey,
        &mut t.editor,
        &mut t.header.text,
        &mut t.header.hotkey,
        &mut t.header.symbol,
        &mut t.toggle_button.selected,
        &mut t.toggle_button.unselected,
        &mut t.scrollbar.arrow,
        &mut t.scrollbar.bar,
        &mut t.scrollbar.position,
    ] {
        *target = state;
    }
    t.border = state;
    t.border.normal = line;
    t.border.focused = line;
    t.border.hovered = line;
    t.border.inactive = attr(p.muted_text.color(), Color::Transparent);
    t.lines = t.border;
    for target in [
        &mut t.text.normal,
        &mut t.text.hot_key,
        &mut t.text.error,
        &mut t.text.warning,
        &mut t.text.hovered,
        &mut t.text.focused,
        &mut t.text.highlighted,
        &mut t.text.enphasized_1,
        &mut t.text.enphasized_2,
        &mut t.text.enphasized_3,
        &mut t.symbol.checked,
        &mut t.symbol.unchecked,
        &mut t.symbol.unknown,
        &mut t.symbol.arrows,
        &mut t.symbol.close,
        &mut t.symbol.maximized,
        &mut t.symbol.resize,
        &mut t.markdown.text,
        &mut t.markdown.bold,
        &mut t.markdown.italic,
        &mut t.markdown.link,
        &mut t.markdown.h1,
        &mut t.markdown.h2,
        &mut t.markdown.h3,
        &mut t.markdown.ordered_list,
        &mut t.markdown.unordered_list,
        &mut t.markdown.table,
        &mut t.markdown.table_header,
        &mut t.window.normal,
        &mut t.window.error,
        &mut t.window.warning,
        &mut t.window.info,
        &mut t.tooltip.text,
        &mut t.tooltip.arrow,
        &mut t.searchbar.normal,
        &mut t.searchbar.count,
        &mut t.list_current_item.normal,
        &mut t.list_current_item.icon,
    ] {
        *target = normal;
    }
    t.desktop.character =
        Character::new(' ', p.text.color(), p.background.color(), CharFlags::None);
    t.text.inactive = muted;
    t.symbol.inactive = muted;
    t.window.inactive = normal;
    t.symbol.hovered = accent;
    t.symbol.pressed = selection;
    t.window.bar.focus = accent;
    t.window.bar.normal = muted;
    t.window.bar.resizing = selection;
    for target in [
        &mut t.window.bar.close_button,
        &mut t.window.bar.maximize_button,
        &mut t.window.bar.tag,
        &mut t.window.bar.hotkey,
    ] {
        *target = attr(p.text.color(), Color::Transparent);
    }
    t.button.regular.text.normal = accent;
    t.button.bevel.text.normal = accent;
    t.button.regular.shadow = attr(p.border.color(), p.surface.color());
    t.button.bevel.dark_margin = line;
    t.button.bevel.light_margin = normal;
    t.markdown.code = selection;
    t.markdown.code_block = selection;
    t.searchbar.focused = selection;
    t.list_current_item.focus = selection;
    t.list_current_item.selected = selection;
    t.list_current_item.over_selection = selection;
    t.list_current_item.over_inactive = muted;
    t.progressbar.background = p.surface.color();
    t.progressbar.progress = p.accent.color();
    t.progressbar.text = p.text.color();
    t
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::preferences::{Preferences, Rgb};
    #[test]
    fn theme_uses_configured_desktop_roles_in_controls_and_windows() {
        let mut p = Preferences::default().desktop;
        p.background = Rgb(1, 2, 3);
        p.surface = Rgb(4, 5, 6);
        p.text = Rgb(7, 8, 9);
        p.border = Rgb(10, 11, 12);
        p.accent = Rgb(13, 14, 15);
        p.selection = Rgb(16, 17, 18);
        let theme = build_theme(&p);
        assert_eq!(theme.desktop.character.background, p.background.color());
        assert_eq!(theme.window.normal.background, p.surface.color());
        assert_eq!(theme.text.normal.foreground, p.text.color());
        assert_eq!(theme.menu.text.normal.background, p.surface.color());
        assert_eq!(
            theme.editor.pressed_or_selected.background,
            p.selection.color()
        );
        assert_eq!(theme.window.bar.focus.background, p.accent.color());
        assert_eq!(theme.border.normal.foreground, p.border.color());
    }
}
