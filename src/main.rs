mod args;
mod desktop;
mod desktop_dialogs;
mod help;
mod keyboard;
mod linux_status;
mod palette_draft;
mod palette_presets;
mod palette_theme;
mod panel;
mod preferences;
mod resource_usage;
mod security_preferences;
mod shortcut;
mod system_actions;
mod terminal_emulation;
mod terminal_lock;
#[cfg(test)]
mod test_support;
mod tui_window;

use crate::args::Args;
use crate::desktop::MyDesktop;
use crate::shortcut::parse_shortcut_dir;
use crate::system_actions::{LinuxSystemRunner, ProcessRunner, Session};
use appcui::backend::Type;
use appcui::prelude::App;
use clap::Parser;
use std::fs;
use std::process::exit;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let settings_path = preferences::settings_path()?;
    let (preferences, startup_warning) = preferences::Preferences::load(&settings_path);
    // Shortcut overrides never redirect the application's own settings.
    let shortcut_dir = args
        .shortcut_dir
        .unwrap_or_else(|| settings_path.parent().unwrap().to_path_buf());
    fs::create_dir_all(&shortcut_dir)?;
    let desktop_shortcuts = parse_shortcut_dir(shortcut_dir, &settings_path)?;
    let theme = palette_theme::build_theme(&preferences.desktop);
    let runner = Arc::new(LinuxSystemRunner {
        helper: ProcessRunner,
        session: Session::current(),
    });

    let app = App::with_backend(Type::CrossTerm)
        .desktop(MyDesktop::new(
            desktop_shortcuts,
            preferences,
            settings_path,
            startup_warning,
            runner,
        ))
        .app_bar()
        .theme(theme)
        .color_schema(false)
        .build()?;

    app.run();

    exit(0);
}
