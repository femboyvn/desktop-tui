use super::Shortcut;
use super::shortcuts::{TaskbarOptions, TerminalOptions, WindowOptions};

/// External IDs index the immutable shortcut catalog, never a menu position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AppId {
    External(usize),
    FileBrowser,
    ImageViewer,
}

pub(crate) struct Application {
    pub id: AppId,
    pub name: String,
    pub category: String,
}

pub(crate) fn with_starters(mut shortcuts: Vec<Shortcut>) -> Vec<Shortcut> {
    for (name, command, category) in [
        ("Terminal", "sh", "System"),
        ("System Performance", "btm", "Utility"),
        ("Text editor", "nano", "Utility"),
    ] {
        if !shortcuts.iter().any(|shortcut| shortcut.name == name) {
            shortcuts.push(Shortcut {
                name: name.into(), category: Some(category.into()), command: command.into(), args: vec![],
                taskbar: TaskbarOptions { position: None, additional_commands: vec![] },
                window: WindowOptions { resizable: true, close_button: true, fixed_position: false, size: None },
                terminal: TerminalOptions { padding: None, background_color: None },
            });
        }
    }
    shortcuts
}

pub(crate) fn applications(shortcuts: &[Shortcut]) -> Vec<Application> {
    let mut apps = vec![
        Application { id: AppId::FileBrowser, name: "File Browser".into(), category: "System".into() },
        Application { id: AppId::ImageViewer, name: "Image Viewer".into(), category: "Utility".into() },
    ];
    apps.extend(shortcuts.iter().enumerate().map(|(index, shortcut)| Application {
        id: AppId::External(index), name: shortcut.name.clone(),
        category: shortcut.category.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("User").into(),
    }));
    apps
}

pub(crate) fn categories(apps: &[Application]) -> Vec<String> {
    let mut result: Vec<String> = ["System", "Utility", "User", "Games"].iter().map(|s| (*s).into()).collect();
    let mut extra: Vec<_> = apps.iter().map(|app| app.category.clone()).filter(|s| !result.contains(s)).collect();
    extra.sort();
    extra.dedup();
    result.extend(extra);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn starters_and_categories_are_available_without_shortcuts() {
        let shortcuts = with_starters(vec![]);
        let apps = applications(&shortcuts);
        assert_eq!(apps.len(), 5);
        assert_eq!(categories(&apps), ["System", "Utility", "User", "Games"]);
        assert_eq!(shortcuts.iter().map(|s| s.command.as_str()).collect::<Vec<_>>(), ["sh", "btm", "nano"]);
    }
    #[test]
    fn overrides_keep_native_identity_and_category_order_does_not_change_dispatch() {
        let mut custom = with_starters(vec![]).remove(2);
        custom.command = "hx".into();
        custom.category = Some("Writing".into());
        let mut native_name = custom.clone();
        native_name.name = "Image Viewer".into();
        native_name.category = None;
        let shortcuts = with_starters(vec![custom, native_name]);
        assert_eq!(shortcuts.iter().filter(|s| s.name == "Text editor").count(), 1);
        let apps = applications(&shortcuts);
        assert_eq!(categories(&apps), ["System", "Utility", "User", "Games", "Writing"]);
        assert!(apps.iter().any(|a| a.id == AppId::ImageViewer));
        assert!(apps.iter().any(|a| a.name == "Image Viewer" && a.id == AppId::External(1) && a.category == "User"));
        assert_eq!(shortcuts[0].command, "hx");
    }
}
