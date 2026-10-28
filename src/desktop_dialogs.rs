use crate::palette_draft::{DESKTOP_ROLES, PaletteDraft, TERMINAL_ROLES};
use crate::palette_presets::PalettePreset;
use crate::panel::{DateFormat, PanelPreferences};
use crate::preferences::{Preferences, Rgb};
use crate::security_preferences::{PasswordChange, SecurityDraft};
use crate::system_actions::SystemAction;
use crate::terminal_lock::SecretInput;
use appcui::prelude::*;
use std::path::PathBuf;

#[ModalWindow(response: bool, events: ButtonEvents)]
pub struct ConfirmSystemAction {
    cancel: Handle<Button>,
    confirm: Handle<Button>,
}
impl ConfirmSystemAction {
    pub fn new(action: SystemAction) -> Self {
        let mut dialog = Self {
            base: ModalWindow::new(
                action.label(),
                layout!("a:c,w:58,h:10"),
                window::Flags::None,
            ),
            cancel: Handle::None,
            confirm: Handle::None,
        };
        let verb = if action == SystemAction::Shutdown {
            "Shut down"
        } else {
            "Restart"
        };
        dialog.add(Label::new(&format!("{verb} this computer?\nAll applications in this session may be closed.\nSave your work before continuing."),layout!("l:2,t:1,r:2,h:4")));
        // Register Cancel first and explicitly request focus: Enter must never power off by default.
        dialog.cancel = dialog.add(button!("&Cancel,l:10,b:0,w:14"));
        dialog.confirm = dialog.add(Button::new(&format!("&{verb}"), layout!("r:10,b:0,w:14")));
        dialog.base.request_focus_for_control(dialog.cancel);
        dialog
    }
}
impl ButtonEvents for ConfirmSystemAction {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.confirm {
            self.exit_with(true);
        } else {
            self.exit();
        }
        EventProcessStatus::Processed
    }
}

#[CustomControl(overwrite = OnPaint)]
struct ColorSwatch {
    color: Option<Color>,
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
struct PreferencesContent {
    tabs: Handle<Tab>,
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
            key!("Ctrl+Tab") => |tab| {
                tab.set_current_tab((tab.current_tab().unwrap_or(0) + 1) % 4);
                tab.request_focus();
            },
            key!("Ctrl+Shift+Tab") => |tab| {
                tab.set_current_tab((tab.current_tab().unwrap_or(0) + 3) % 4);
                tab.request_focus();
            },
            _ => return EventProcessStatus::Ignored,
        };
        Window::update_control(self.tabs, change);
        EventProcessStatus::Processed
    }
}
#[ModalWindow(response: Preferences, events: ButtonEvents+ComboBoxEvents+TextFieldEvents+WindowEvents)]
pub struct PreferencesDialog {
    draft: crate::palette_draft::PaletteDraft,
    path: PathBuf,
    selectors: [Handle<ComboBox>; 2],
    fields: [Handle<TextField>; 2],
    swatches: [Handle<ColorSwatch>; 2],
    selected: [usize; 2],
    save: Handle<Button>,
    cancel: Handle<Button>,
    reset: Handle<Button>,
    preset: Handle<ComboBox>,
    apply_preset: Handle<Button>,
    security: Option<SecurityDraft>,
    security_error: Option<String>,
    password_action: Handle<ComboBox>,
    passwords: [Handle<SecretInput>; 3],
    panel_checks: [Handle<CheckBox>; 12],
    date_format: Handle<ComboBox>,
}
impl PreferencesDialog {
    pub fn new(preferences: &Preferences, path: PathBuf) -> Self {
        let draft = PaletteDraft::new(preferences);
        let (security, security_error) = match Preferences::stored_credential(&path) {
            Ok(credential) => (Some(SecurityDraft::new(credential)), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        };
        let mut dialog = Self {
            base: ModalWindow::new("Preference", layout!("a:c,w:66,h:20"), window::Flags::None),
            draft,
            path,
            selectors: [Handle::None; 2],
            fields: [Handle::None; 2],
            swatches: [Handle::None; 2],
            selected: [0; 2],
            save: Handle::None,
            cancel: Handle::None,
            reset: Handle::None,
            preset: Handle::None,
            apply_preset: Handle::None,
            security,
            security_error,
            password_action: Handle::None,
            passwords: [Handle::None; 3],
            panel_checks: [Handle::None; 12],
            date_format: Handle::None,
        };
        let mut content = PreferencesContent {
            base: ContainerBase::new(layout!("d:f"), true),
            tabs: Handle::None,
        };
        content.add(Label::new("Preset", layout!("l:1,t:0,w:7,h:1")));
        let mut preset = ComboBox::new(layout!("l:9,t:0,r:15,h:1"), combobox::Flags::None);
        preset.set_hotkey(key!("Alt+P"));
        preset.add("Custom");
        for value in PalettePreset::ALL {
            preset.add(value.label());
        }
        dialog.preset = content.add(preset);
        dialog.apply_preset = content.add(Button::with_type(
            "&Apply",
            layout!("r:1,t:0,w:12,h:1"),
            button::Type::Flat,
        ));
        let mut tabs = Tab::new(layout!("l:1,t:2,r:1,b:4"), tab::Flags::None);
        tabs.add_tab("&Desktop");
        tabs.add_tab("&Terminal");
        tabs.add_tab("Sec&urity");
        tabs.add_tab("Panel");
        for group in 0..2 {
            tabs.add(group, Label::new("Color role", layout!("l:1,t:1,w:42,h:1")));
            let mut selector = ComboBox::new(layout!("l:1,t:2,r:1,h:1"), combobox::Flags::None);
            let roles: &[&str] = if group == 0 {
                &DESKTOP_ROLES
            } else {
                &TERMINAL_ROLES
            };
            for role in roles {
                selector.add(role);
            }
            selector.set_index(0);
            dialog.selectors[group as usize] = tabs.add(group, selector);
            tabs.add(
                group,
                Label::new(
                    "Value (#RRGGBB)       Color preview",
                    layout!("l:1,t:4,r:1,h:1"),
                ),
            );
            let value = if group == 0 {
                &dialog.draft.desktop[0]
            } else {
                &dialog.draft.terminal[0]
            };
            dialog.fields[group as usize] = tabs.add(
                group,
                TextField::new(value, layout!("l:1,t:5,w:14,h:1"), textfield::Flags::None),
            );
            dialog.swatches[group as usize] = tabs.add(
                group,
                ColorSwatch {
                    base: ControlBase::new(layout!("l:22,t:5,w:22,h:2"), false),
                    color: Rgb::try_from(value.clone()).ok().map(Rgb::color),
                },
            );
            tabs.add(
                group,
                Label::new(
                    "Ctrl+Tab switches tabs. Save applies the draft.",
                    layout!("l:1,t:8,r:1,h:1"),
                ),
            );
        }
        tabs.add(
            2,
            Label::new("Password action", layout!("l:1,t:1,w:18,h:1")),
        );
        let mut action = ComboBox::new(layout!("l:20,t:1,r:1,h:1"), combobox::Flags::None);
        action.set_hotkey(key!("Alt+L"));
        for label in [
            "Keep current setting",
            "Set / change saved password",
            "Remove saved password",
        ] {
            action.add(label);
        }
        action.set_index(0);
        action.set_enabled(dialog.security.is_some());
        dialog.password_action = tabs.add(2, action);
        for (index, (label, hotkey)) in [
            ("Current password", key!("Alt+O")),
            ("New password", key!("Alt+N")),
            ("Confirm password", key!("Alt+F")),
        ]
        .into_iter()
        .enumerate()
        {
            let top = 3 + index as i32 * 2;
            tabs.add(
                2,
                Label::new(
                    label,
                    LayoutBuilder::new().x(1).y(top).width(18).height(1).build(),
                ),
            );
            let mut input = SecretInput::new(
                LayoutBuilder::new()
                    .x(20)
                    .y(top)
                    .width(38)
                    .height(1)
                    .build(),
            );
            input.set_hotkey(hotkey);
            input.set_enabled(false);
            dialog.passwords[index] = tabs.add(2, input);
        }
        let security_status = if dialog.security_error.is_some() {
            "Password settings unavailable. Repair preferences.toml\nbefore saving. The original file is kept."
        } else if dialog
            .security
            .as_ref()
            .is_some_and(SecurityDraft::has_password)
        {
            "Saved password configured. Changes require current password.\nSave stores only a salted Argon2id hash."
        } else {
            "No saved password; Terminal Lock asks each time.\nSet a password to reuse it after restarting."
        };
        tabs.add(2, Label::new(security_status, layout!("l:1,t:9,r:1,h:2")));
        let panel = &preferences.panel;
        let options = [
            ("Enable status panel", panel.enabled),
            ("24-hour clock", panel.use_24_hour),
            ("Show clock", panel.show_clock),
            ("Show seconds", panel.show_seconds),
            ("Show date", panel.show_date),
            ("Wi-Fi / network status", panel.show_network),
            ("Battery (when available)", panel.show_battery),
            ("Show interface name", panel.show_interface),
            ("CPU usage", panel.show_cpu),
            ("RAM usage", panel.show_ram),
            ("Disk usage (/)", panel.show_disk),
            ("Network download/upload", panel.show_network_usage),
        ];
        for (index, (caption, checked)) in options.into_iter().enumerate() {
            dialog.panel_checks[index] = tabs.add(
                3,
                CheckBox::new(
                    caption,
                    LayoutBuilder::new()
                        .x(if index % 2 == 0 { 1 } else { 32 })
                        .y(1 + index as i32 / 2)
                        .width(28)
                        .height(1)
                        .build(),
                    checked,
                ),
            );
        }
        tabs.add(3, Label::new("Date format", layout!("l:1,t:8,w:18,h:1")));
        let mut date_format = ComboBox::new(layout!("l:20,t:8,r:1,h:1"), combobox::Flags::None);
        date_format.set_hotkey(key!("Alt+G"));
        for caption in ["Tue 27 Oct", "2026-10-27 (ISO)", "27/10 (day/month)"] {
            date_format.add(caption);
        }
        date_format.set_index(match panel.date_format {
            DateFormat::Short => 0,
            DateFormat::Iso => 1,
            DateFormat::DayMonth => 2,
        });
        dialog.date_format = tabs.add(3, date_format);
        tabs.add(
            3,
            Label::new(
                "Top right; hover for full readings. Alt+4: Panel.",
                layout!("l:1,t:10,r:1,h:1"),
            ),
        );
        content.tabs = content.add(tabs);
        content.add(Label::new(
            "Save applies all tabs. Cancel or Escape discards edits.",
            layout!("l:2,b:3,r:1,h:1"),
        ));
        dialog.reset = content.add(button!("'&Reset to Femboy',l:1,b:0,w:22"));
        dialog.cancel = content.add(button!("&Cancel,r:14,b:0,w:12"));
        dialog.save = content.add(button!("&Save,r:1,b:0,w:12"));
        dialog.add(content);
        dialog.sync_preset_choice();
        dialog.base.request_focus_for_control(dialog.selectors[0]);
        dialog
    }
    fn refresh(&mut self, group: usize) {
        let value = if group == 0 {
            self.draft.desktop[self.selected[group]].clone()
        } else {
            self.draft.terminal[self.selected[group]].clone()
        };
        if let Some(field) = self.base.control_mut(self.fields[group]) {
            field.set_text(&value);
        }
        self.update_swatch(group, &value);
    }
    fn update_swatch(&mut self, group: usize, value: &str) {
        if let Some(swatch) = self.base.control_mut(self.swatches[group]) {
            swatch.color = Rgb::try_from(value.to_owned()).ok().map(Rgb::color);
        }
    }
    fn save_draft(&mut self) {
        let palette = self.draft.validate();
        match palette {
            Err(error) => {
                self.clear_passwords();
                dialogs::error("Invalid color", &error);
            }
            Ok(mut preferences) => {
                preferences.panel = self.panel_draft();
                let action = self.password_change();
                let result = match &mut self.security {
                    None => Err(anyhow::anyhow!(
                        self.security_error.clone().unwrap_or_default()
                    )),
                    Some(security) => {
                        let current = self.base.control(self.passwords[0]).unwrap();
                        let new = self.base.control(self.passwords[1]).unwrap();
                        let confirmation = self.base.control(self.passwords[2]).unwrap();
                        security.save(
                            &self.path,
                            preferences,
                            action,
                            &current.value,
                            &new.value,
                            &confirmation.value,
                            std::time::Instant::now(),
                        )
                    }
                };
                self.clear_passwords();
                match result {
                    Ok(preferences) => self.exit_with(preferences),
                    Err(error) => dialogs::error(
                        "Preferences were not saved",
                        &format!("{error:#}\nThe active palette and password are unchanged."),
                    ),
                }
            }
        }
    }
    fn panel_draft(&self) -> PanelPreferences {
        let checked = |index: usize| self.control(self.panel_checks[index]).unwrap().is_checked();
        PanelPreferences {
            enabled: checked(0),
            use_24_hour: checked(1),
            show_clock: checked(2),
            show_seconds: checked(3),
            show_date: checked(4),
            show_network: checked(5),
            show_battery: checked(6),
            show_interface: checked(7),
            show_cpu: checked(8),
            show_ram: checked(9),
            show_disk: checked(10),
            show_network_usage: checked(11),
            date_format: match self
                .control(self.date_format)
                .and_then(ComboBox::index)
                .unwrap_or(0)
            {
                1 => DateFormat::Iso,
                2 => DateFormat::DayMonth,
                _ => DateFormat::Short,
            },
        }
    }
    fn password_change(&self) -> PasswordChange {
        match self
            .control(self.password_action)
            .and_then(ComboBox::index)
            .unwrap_or(0)
        {
            1 => PasswordChange::Replace,
            2 => PasswordChange::Remove,
            _ => PasswordChange::Keep,
        }
    }
    fn clear_passwords(&mut self) {
        for handle in self.passwords {
            self.base.control_mut(handle).unwrap().clear();
        }
    }
    fn refresh_password_fields(&mut self) {
        self.clear_passwords();
        let action = self.password_change();
        let has_password = self
            .security
            .as_ref()
            .is_some_and(SecurityDraft::has_password);
        for index in 0..3 {
            let enabled = self.security.is_some()
                && if index == 0 {
                    has_password && action != PasswordChange::Keep
                } else {
                    action == PasswordChange::Replace
                };
            self.base
                .control_mut(self.passwords[index])
                .unwrap()
                .set_enabled(enabled);
        }
    }
    fn sync_preset_choice(&mut self) {
        let selected = self
            .draft
            .validate()
            .ok()
            .and_then(|p| PalettePreset::matching(&p))
            .and_then(|preset| PalettePreset::ALL.iter().position(|value| *value == preset))
            .map_or(0, |index| index + 1);
        self.base
            .control_mut(self.preset)
            .unwrap()
            .set_index(selected as u32);
        self.base
            .control_mut(self.apply_preset)
            .unwrap()
            .set_enabled(selected != 0);
    }
}
impl ButtonEvents for PreferencesDialog {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.cancel {
            self.clear_passwords();
            self.exit();
        } else if handle == self.reset {
            self.draft = PaletteDraft::new(&Preferences::default());
            self.refresh(0);
            self.refresh(1);
            self.sync_preset_choice();
        } else if handle == self.apply_preset {
            let index = self
                .control(self.preset)
                .and_then(ComboBox::index)
                .unwrap_or(0) as usize;
            if let Some(preset) = index
                .checked_sub(1)
                .and_then(|index| PalettePreset::ALL.get(index))
            {
                self.draft = PaletteDraft::new(&preset.preferences());
                self.refresh(0);
                self.refresh(1);
                self.sync_preset_choice();
            }
        } else if handle == self.save {
            self.save_draft();
        } else {
            return EventProcessStatus::Ignored;
        }
        EventProcessStatus::Processed
    }
}
impl ComboBoxEvents for PreferencesDialog {
    fn on_selection_changed(&mut self, handle: Handle<ComboBox>) -> EventProcessStatus {
        if handle == self.password_action {
            self.refresh_password_fields();
            return EventProcessStatus::Processed;
        }
        if handle == self.preset {
            let enabled = self.control(handle).and_then(ComboBox::index).unwrap_or(0) != 0;
            self.base
                .control_mut(self.apply_preset)
                .unwrap()
                .set_enabled(enabled);
            return EventProcessStatus::Processed;
        }
        if let Some(group) = self
            .selectors
            .iter()
            .position(|selector| *selector == handle)
        {
            self.selected[group] =
                self.control(handle).and_then(ComboBox::index).unwrap_or(0) as usize;
            self.refresh(group);
            EventProcessStatus::Processed
        } else {
            EventProcessStatus::Ignored
        }
    }
}
impl WindowEvents for PreferencesDialog {
    fn on_accept(&mut self) {
        self.save_draft();
    }
    fn on_cancel(&mut self) -> ActionRequest {
        self.clear_passwords();
        ActionRequest::Allow
    }
}
impl TextFieldEvents for PreferencesDialog {
    fn on_text_changed(&mut self, handle: Handle<TextField>) -> EventProcessStatus {
        if let Some(group) = self.fields.iter().position(|field| *field == handle) {
            let value = self.control(handle).unwrap().text().to_owned();
            if group == 0 {
                self.draft.desktop[self.selected[group]] = value.clone();
            } else {
                self.draft.terminal[self.selected[group]] = value.clone();
            }
            self.update_swatch(group, &value);
            self.sync_preset_choice();
            EventProcessStatus::Processed
        } else {
            EventProcessStatus::Ignored
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::Rgb;
    use crate::test_support::TestDir;
    fn debug(script: &str) -> appcui::system::Builder {
        // Let queued control events run before the debug backend ends the application.
        App::debug(80, 24, &format!("{script}\nPaint('end')"))
    }
    #[test]
    fn panel_tab_saves_all_options_and_presets_preserve_panel_and_password() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        let mut active = Preferences::default();
        active.terminal_lock =
            Some(crate::terminal_lock::LockCredential::new("keep", "keep").unwrap());
        active.save(&path, None).unwrap();
        let original = std::fs::read(&path).unwrap();
        let edits = "Key.Pressed(Alt+4)\nMouse.Click(12,8,left)\nMouse.Click(44,8,left)\nMouse.Click(12,11,left)\nMouse.Click(44,11,left)\nMouse.Click(12,12,left)\nMouse.Click(44,12,left)\nKey.Pressed(Alt+G)\nKey.Pressed(End)\nKey.Pressed(Enter)\nKey.Pressed(Alt+P)\nKey.Pressed(Home)\nKey.Pressed(Down,2)\nKey.Pressed(Alt+A)\nKey.Pressed(Alt+R)";
        let app = debug(&format!("{edits}\nKey.Pressed(Escape)"))
            .build()
            .unwrap();
        assert!(
            PreferencesDialog::new(&active, path.clone())
                .show()
                .is_none()
        );
        app.run();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let app = debug(&format!("{edits}\nKey.Pressed(Alt+S)"))
            .build()
            .unwrap();
        let saved = PreferencesDialog::new(&active, path.clone())
            .show()
            .unwrap();
        app.run();
        assert!(!saved.panel.show_clock);
        assert!(saved.panel.show_seconds);
        assert!(!saved.panel.show_cpu);
        assert!(!saved.panel.show_ram);
        assert!(!saved.panel.show_disk);
        assert!(!saved.panel.show_network_usage);
        assert_eq!(saved.panel.date_format, crate::panel::DateFormat::DayMonth);
        assert_eq!(saved.terminal_lock, active.terminal_lock);
        assert_eq!(saved.desktop, Preferences::default().desktop);
        assert_eq!(Preferences::load(&path).0, saved);
    }
    #[test]
    fn security_tab_saves_only_on_save_and_palette_presets_preserve_password() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        let active = Preferences::default();
        active.save(&path, None).unwrap();
        let setup = "Key.Pressed(Alt+U)\nKey.Pressed(Alt+L)\nKey.Pressed(Home)\nKey.Pressed(Down)\nKey.Pressed(Enter)\nKey.Pressed(Alt+N)\nKey.TypeText('reusable password')\nKey.Pressed(Tab)\nKey.TypeText('reusable password')";
        let app = debug(&format!("{setup}\nKey.Pressed(Escape)"))
            .build()
            .unwrap();
        assert!(
            PreferencesDialog::new(&active, path.clone())
                .show()
                .is_none()
        );
        app.run();
        assert!(Preferences::stored_credential(&path).unwrap().is_none());
        let app = debug(&format!("{setup}\nKey.Pressed(Alt+S)"))
            .build()
            .unwrap();
        let saved = PreferencesDialog::new(&active, path.clone())
            .show()
            .expect("Security Save must return updated preferences");
        app.run();
        assert!(
            saved
                .terminal_lock
                .as_ref()
                .expect("password must be saved")
                .verify("reusable password")
        );
        let app = debug("Key.Pressed(Alt+P)\nKey.Pressed(Home)\nKey.Pressed(Down,2)\nKey.Pressed(Alt+A)\nKey.Pressed(Alt+R)\nKey.Pressed(Alt+S)").build().unwrap();
        let reset = PreferencesDialog::new(&saved, path.clone()).show().unwrap();
        app.run();
        assert_eq!(reset.terminal_lock, saved.terminal_lock);
        assert_eq!(reset.desktop, Preferences::default().desktop);
        assert_eq!(Preferences::load(&path).0, reset);
    }
    #[test]
    fn security_tab_authenticates_changes_and_removal_without_partial_saves() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        let mut active = Preferences::default();
        active.desktop.background = Rgb(1, 2, 3);
        active.terminal_lock =
            Some(crate::terminal_lock::LockCredential::new("original", "original").unwrap());
        active.save(&path, None).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let remove = "Key.Pressed(Alt+U)\nKey.Pressed(Alt+L)\nKey.Pressed(End)\nKey.Pressed(Enter)\nKey.Pressed(Alt+O)";
        let app = debug(&format!("Key.Pressed(Alt+R)\n{remove}\nKey.TypeText('wrong')\nKey.Pressed(Alt+S)\nKey.Pressed(Enter)\nKey.Pressed(Escape)")).build().unwrap();
        assert!(
            PreferencesDialog::new(&active, path.clone())
                .show()
                .is_none()
        );
        app.run();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let replace = "Key.Pressed(Alt+U)\nKey.Pressed(Alt+L)\nKey.Pressed(Home)\nKey.Pressed(Down)\nKey.Pressed(Enter)\nKey.Pressed(Alt+O)\nKey.TypeText('original')\nKey.Pressed(Tab)\nKey.TypeText('changed')\nKey.Pressed(Tab)\nKey.TypeText('changed')\nKey.Pressed(Alt+S)";
        let app = debug(replace).build().unwrap();
        let changed = PreferencesDialog::new(&active, path.clone())
            .show()
            .unwrap();
        app.run();
        assert!(changed.terminal_lock.as_ref().unwrap().verify("changed"));
        assert!(!changed.terminal_lock.as_ref().unwrap().verify("original"));
        assert_eq!(changed.desktop.background, Rgb(1, 2, 3));
        let app = debug(&format!(
            "{remove}\nKey.TypeText('changed')\nKey.Pressed(Alt+S)"
        ))
        .build()
        .unwrap();
        let removed = PreferencesDialog::new(&changed, path.clone())
            .show()
            .unwrap();
        app.run();
        assert!(removed.terminal_lock.is_none());
        assert_eq!(Preferences::load(&path).0, removed);
    }
    #[test]
    fn invalid_palette_save_clears_password_inputs() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = debug("Key.Pressed(Enter)").build().unwrap();
        let mut dialog =
            PreferencesDialog::new(&Preferences::default(), dir.path().join("preferences.toml"));
        let password = dialog.passwords[1];
        OnKeyPressed::on_key_pressed(dialog.base.control_mut(password).unwrap(), Key::None, 'x');
        dialog.draft.desktop[0] = "invalid".into();
        dialog.save_draft();
        assert!(dialog.base.control(password).unwrap().value.is_empty());
        app.run();
    }
    #[test]
    fn preset_selection_applies_to_draft_and_save_only() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        let active = Preferences::default();
        active.save(&path, None).unwrap();
        let choose_trans =
            "Key.Pressed(Alt+P)\nKey.Pressed(Home)\nKey.Pressed(Down,2)\nKey.Pressed(Alt+A)";
        let app = debug(&format!("{choose_trans}\nKey.Pressed(Escape)"))
            .build()
            .unwrap();
        assert_eq!(PreferencesDialog::new(&active, path.clone()).show(), None);
        app.run();
        assert_eq!(Preferences::load(&path).0, active);
        for (index, preset) in crate::palette_presets::PalettePreset::ALL
            .into_iter()
            .enumerate()
        {
            let script = format!(
                "Key.Pressed(Alt+P)\nKey.Pressed(Home)\nKey.Pressed(Down,{})\nKey.Pressed(Alt+A)\nKey.Pressed(Alt+S)",
                index + 1
            );
            let app = debug(&script).build().unwrap();
            assert_eq!(
                PreferencesDialog::new(&active, path.clone()).show(),
                Some(preset.preferences())
            );
            app.run();
            assert_eq!(Preferences::load(&path).0, preset.preferences());
        }
    }
    #[test]
    fn dialogs_cancel_by_default_and_preferences_save_only_valid_drafts_at_80x24() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        for key in ["Enter", "Escape"] {
            let app = debug(&format!("Key.Pressed({key})")).build().unwrap();
            assert_eq!(
                ConfirmSystemAction::new(SystemAction::Shutdown).show(),
                None
            );
            app.run();
        }
        let app = debug("Key.Pressed(Tab)\nKey.Pressed(Enter)")
            .build()
            .unwrap();
        assert_eq!(
            ConfirmSystemAction::new(SystemAction::Restart).show(),
            Some(true)
        );
        app.run();
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        let mut active = Preferences::default();
        active.desktop.background = Rgb(1, 2, 3);
        let app = debug("Key.Pressed(Alt+R)\nKey.Pressed(Escape)")
            .build()
            .unwrap();
        assert_eq!(PreferencesDialog::new(&active, path.clone()).show(), None);
        app.run();
        assert!(!path.exists());
        let app = debug("Key.Pressed(Alt+R)\nKey.Pressed(Alt+S)")
            .build()
            .unwrap();
        assert_eq!(
            PreferencesDialog::new(&active, path.clone()).show(),
            Some(Preferences::default())
        );
        app.run();
        assert_eq!(Preferences::load(&path).0, Preferences::default());
        let invalid = "Key.Pressed(Tab)\nKey.Pressed(Ctrl+A)\nKey.TypeText('invalid')\nKey.Pressed(Alt+S)\nKey.Pressed(Enter)\nKey.Pressed(Escape)";
        let app = debug(invalid).build().unwrap();
        assert_eq!(PreferencesDialog::new(&active, path.clone()).show(), None);
        app.run();
        assert_eq!(Preferences::load(&path).0, Preferences::default());
        // A failed save remains in the dialog, allowing Escape to discard it.
        let failed_path = dir.path().join("directory");
        std::fs::create_dir(&failed_path).unwrap();
        let app = debug("Key.Pressed(Alt+S)\nKey.Pressed(Enter)\nKey.Pressed(Escape)")
            .build()
            .unwrap();
        assert_eq!(PreferencesDialog::new(&active, failed_path).show(), None);
        app.run();
        let edits = "Mouse.Click(16,11,left)\nKey.Pressed(Ctrl+A)\nKey.TypeText('#112233')\nMouse.Click(20,8,left)\nKey.Pressed(Down)\nKey.Pressed(Enter)\nMouse.Click(16,11,left)\nKey.Pressed(Ctrl+A)\nKey.TypeText('#445566')\nMouse.Click(30,5,left)\nMouse.Click(16,11,left)\nKey.Pressed(Ctrl+A)\nKey.TypeText('#778899')\nKey.Pressed(Alt+S)";
        let app = debug(edits).build().unwrap();
        let saved = PreferencesDialog::new(&active, path.clone())
            .show()
            .unwrap();
        app.run();
        assert_eq!(saved.desktop.background, Rgb(17, 34, 51));
        assert_eq!(saved.desktop.surface, Rgb(68, 85, 102));
        assert_eq!(saved.terminal.foreground, Rgb(119, 136, 153));
        assert_eq!(Preferences::load(&path).0, saved);
        let mut app = debug("Key.Pressed(PageDown)\nKey.Pressed(Escape)")
            .build()
            .unwrap();
        app.add_window(help_window());
        app.run();
    }
}

pub use crate::help::help_window;
