use appcui::prelude::*;

#[CustomControl(overwrite = OnPaint)]
pub(super) struct ColorSwatch {
    color: Option<Color>,
}
impl ColorSwatch {
    pub(super) fn new(layout: Layout, color: Option<Color>) -> Self {
        Self {
            base: ControlBase::new(layout, false),
            color,
        }
    }
    pub(super) fn set_color(&mut self, color: Option<Color>) {
        self.color = color;
    }
}
impl OnPaint for ColorSwatch {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        if let Some(color) = self.color {
            surface.clear(Character::new(' ', color, color, CharFlags::None));
        } else {
            surface.clear(Character::with_attributes(' ', theme.text.normal));
            surface.write_string(0, 0, "Invalid #RRGGBB", theme.text.error, false);
        }
    }
}
#[CustomContainer(overwrite = OnKeyPressed)]
pub(super) struct PreferencesContent {
    tabs: Handle<Tab>,
}
impl PreferencesContent {
    pub(super) fn new() -> Self {
        Self {
            base: ContainerBase::new(layout!("d:f"), true),
            tabs: Handle::None,
        }
    }
    pub(super) fn set_tabs(&mut self, tabs: Handle<Tab>) {
        self.tabs = tabs;
    }
}
impl OnKeyPressed for PreferencesContent {
    fn on_key_pressed(&mut self, key: Key, _character: char) -> EventProcessStatus {
        let change: fn(&mut Tab) = match key.value() {
            key!("Alt+D") => |tab| {
                tab.set_current_tab(0);
                tab.request_focus();
            },
            key!("Alt+T") => |tab| {
                tab.set_current_tab(1);
                tab.request_focus();
            },
            key!("Alt+U") => |tab| {
                tab.set_current_tab(2);
                tab.request_focus();
            },
            key!("Alt+4") => |tab| {
                tab.set_current_tab(3);
                tab.request_focus();
            },
            key!("Alt+5") => |tab| {
                tab.set_current_tab(4);
                tab.request_focus();
            },
            key!("Ctrl+Tab") => |tab| {
                tab.set_current_tab((tab.current_tab().unwrap_or(0) + 1) % 5);
                tab.request_focus();
            },
            key!("Ctrl+Shift+Tab") => |tab| {
                tab.set_current_tab((tab.current_tab().unwrap_or(0) + 4) % 5);
                tab.request_focus();
            },
            _ => return EventProcessStatus::Ignored,
        };
        Window::update_control(self.tabs, change);
        EventProcessStatus::Processed
    }
}
