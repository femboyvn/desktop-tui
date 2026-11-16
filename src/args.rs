use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    pub shortcut_dir: Option<PathBuf>,
    /// Inspect configuration and optional tools without starting the desktop.
    #[arg(long)]
    pub doctor: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doctor_accepts_selected_directory_without_starting_ui() {
        let args = Args::try_parse_from(["desktop-tui", "--doctor", "my launchers"]).unwrap();
        assert!(args.doctor);
        assert_eq!(args.shortcut_dir, Some(PathBuf::from("my launchers")));
        assert!(!Args::try_parse_from(["desktop-tui"]).unwrap().doctor);
    }
}
