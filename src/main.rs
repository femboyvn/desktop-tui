mod args;
mod config;
mod platform;
mod runtime;
#[cfg(test)]
mod test_support;
mod ui;

use crate::args::Args;
use crate::config::preferences;
use crate::config::parse_shortcut_dir;
use crate::platform::linux::{LinuxSystemRunner, ProcessRunner, Session};
use crate::ui::desktop::MyDesktop;
use appcui::backend::Type;
use appcui::prelude::App;
use clap::Parser;
use std::fs;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Some(status) = runtime::transfer::askpass_if_requested() { std::process::exit(status); }
    let args = Args::parse();

    let settings_path = preferences::settings_path()?;
    // Shortcut overrides never redirect the application's own settings.
    let shortcut_dir = args
        .shortcut_dir
        .unwrap_or_else(|| settings_path.parent().unwrap().to_path_buf());
    if args.doctor {
        let report = platform::diagnostics::diagnose(&shortcut_dir, &settings_path);
        print!("{}", report.text);
        if !report.healthy { anyhow::bail!("Diagnostics found configuration or Terminal errors"); }
        return Ok(());
    }
    let (preferences, startup_warning) = preferences::Preferences::load(&settings_path);
    fs::create_dir_all(&shortcut_dir)?;
    let desktop_shortcuts = parse_shortcut_dir(shortcut_dir.clone(), &settings_path)?;
    let theme = ui::build_theme(&preferences.desktop);
    let runner = Arc::new(LinuxSystemRunner {
        helper: ProcessRunner,
        session: Session::current(),
    });

    let mut desktop = MyDesktop::new(
        desktop_shortcuts,
        preferences,
        settings_path,
        startup_warning,
        runner,
    );
    desktop.set_shortcut_dir(shortcut_dir);
    let app = App::with_backend(Type::CrossTerm)
        .desktop(desktop)
        .app_bar()
        .timers_count(255)
        .theme(theme)
        .color_schema(false)
        .build()?;

    app.run();

    runtime::ai::finish_shutdown();
    runtime::files::finish_shutdown();
    runtime::transfer::finish_shutdown();
    runtime::clipboard::finish_shutdown();
    runtime::pdf::finish_shutdown();
    runtime::music::finish_shutdown();
    runtime::processes::finish_shutdown().await;
    Ok(())
}
