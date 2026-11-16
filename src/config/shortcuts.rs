use anyhow::{Context, bail};
use std::collections::HashMap;
use nestify::nest;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{env, fs};
use walkdir::WalkDir;

nest! {
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Shortcut {
        pub name: String,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub id: Option<String>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub working_directory: Option<PathBuf>,

        #[serde(skip)]
        pub source_path: Option<PathBuf>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub category: Option<String>,

        pub command: String,

        #[serde(default)]
        pub args: Vec<String>,

        pub taskbar:
            #[derive(Clone, Debug, Serialize, Deserialize)]
            pub struct TaskbarOptions {
                pub position: Option<u32>,
                #[serde(default)]
                pub additional_commands: Vec<
                    #[derive(Clone, Debug, Serialize, Deserialize)]
                    pub struct TaskbarCommand {
                        pub name: String,
                        pub command: String,
                        pub args: Vec<String>,
                    }
                >
            },

        pub window:
            #[derive(Clone, Debug, Serialize, Deserialize)]
            pub struct WindowOptions {
                pub resizable: bool,
                pub close_button: bool,
                pub fixed_position: bool,
                pub size: Option<
                    #[derive(Clone, Debug, Serialize, Deserialize)]
                    pub struct WindowSize {
                        pub width: u32,
                        pub height: u32,
                    }
                >
            },

        pub terminal:
            #[derive(Clone, Debug, Serialize, Deserialize)]
            pub struct TerminalOptions {
                pub padding: Option<(i32, i32)>,
                // AppCUI-rs needs to add serde
                pub background_color: Option<
                    #[derive(Clone, Debug, Serialize, Deserialize)]
                    pub struct BackgroundColor {
                        pub r: u8,
                        pub g: u8,
                        pub b: u8,
                    }
                >
            }
    }
}

impl Shortcut {
    /// Identity survives renaming when an explicit id is supplied.
    pub fn stable_id(&self) -> String { self.id.as_deref().unwrap_or(&self.name).to_owned() }

    /// Parse a draft without resolving its relative paths, so editing preserves its TOML.
    pub(crate) fn parse_draft(source: &str) -> anyhow::Result<Self> {
        let shortcut: Self = toml::from_str(source).context("Invalid launcher TOML")?;
        if shortcut.name.trim().is_empty() { bail!("Launcher name cannot be empty"); }
        if shortcut.command.trim().is_empty() { bail!("Launcher executable cannot be empty"); }
        if shortcut.id.as_ref().is_some_and(|id| id.trim().is_empty()) { bail!("Launcher id cannot be empty"); }
        Ok(shortcut)
    }

    pub(crate) fn validate_catalog(shortcuts: &[Self]) -> anyhow::Result<()> {
        let mut names = HashMap::new();
        let mut identities = HashMap::new();
        for shortcut in shortcuts {
            let path = shortcut.source_path.as_deref().unwrap_or(Path::new("<draft>"));
            if let Some(first) = names.insert(shortcut.name.clone(), path) {
                bail!("Duplicate launcher name {:?} in {} and {}", shortcut.name, first.display(), path.display());
            }
            if let Some(first) = identities.insert(shortcut.stable_id(), path) {
                bail!("Duplicate launcher id {:?} in {} and {}", shortcut.stable_id(), first.display(), path.display());
            }
        }
        Ok(())
    }
}

pub fn parse_shortcut_dir(shortcut_path: PathBuf, settings_path: &Path) -> anyhow::Result<Vec<Shortcut>> {
    let directory = if shortcut_path.is_absolute() { shortcut_path } else { env::current_dir()?.join(shortcut_path) };
    if !directory.is_dir() { bail!("Launcher directory is missing or inaccessible: {}", directory.display()); }
    let settings_path = settings_path.canonicalize().unwrap_or_else(|_| settings_path.to_owned());
    let mut shortcuts = Vec::new();
    for entry in WalkDir::new(&directory).sort_by_file_name() {
        let entry = entry.with_context(|| format!("Cannot traverse launcher directory {}", directory.display()))?;
        let path = entry.path();
        if entry.file_type().is_dir() || path.extension() != Some(std::ffi::OsStr::new("toml")) { continue; }
        if path.canonicalize().as_deref().unwrap_or(path) == settings_path { continue; }
        let source = fs::read_to_string(path).with_context(|| format!("Cannot read launcher {}", path.display()))?;
        let mut shortcut = Shortcut::parse_draft(&source).with_context(|| format!("Cannot load launcher {}", path.display()))?;
        shortcut.source_path = Some(path.to_owned());
        if let Some(cwd) = &shortcut.working_directory && cwd.is_relative() {
            shortcut.working_directory = Some(path.parent().unwrap_or(&directory).join(cwd));
        }
        shortcuts.push(shortcut);
    }
    Shortcut::validate_catalog(&super::catalog::with_starters(shortcuts.clone()))?;
    shortcuts.sort_by(|a, b| a.taskbar.position.unwrap_or(99).cmp(&b.taskbar.position.unwrap_or(99)));
    Ok(shortcuts)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    #[test]
    fn optional_category_preserves_legacy_shortcuts() {
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml")).lines()
            .filter(|line| !line.starts_with("category =")).collect::<Vec<_>>().join("\n");
        let legacy: Shortcut = toml::from_str(&source).unwrap();
        assert!(!toml::to_string(&legacy).unwrap().contains("category ="));
        let categorized: Shortcut = toml::from_str(&format!("category = \"Utility\"\n{source}")).unwrap();
        assert!(toml::to_string(&categorized).unwrap().contains("category = \"Utility\""));
    }
    #[test]
    fn excludes_only_the_application_settings_file() {
        let dir = TestDir::new();
        let settings = dir.path().join("preferences.toml");
        fs::write(&settings, "not a shortcut or even valid toml").unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        fs::write(
            dir.path().join("nested/preferences.toml"),
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml")),
        )
        .unwrap();
        let shortcuts = parse_shortcut_dir(dir.path().to_owned(), &settings).unwrap();
        assert_eq!(shortcuts.len(), 1);
    }
    #[test]
    fn rejects_duplicate_names_instead_of_silently_dropping_a_launcher() {
        let dir = TestDir::new();
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml"));
        fs::write(dir.path().join("b.toml"), source).unwrap();
        fs::write(dir.path().join("a.toml"), source).unwrap();
        let result = parse_shortcut_dir(dir.path().into(), &dir.path().join("preferences.toml"));
        assert!(result.is_err(), "duplicate names must produce a useful error");
    }
    #[test]
    fn loads_only_exact_toml_extensions_and_reports_missing_directories() {
        let dir = TestDir::new();
        fs::write(dir.path().join("invalid.not-toml"), "invalid").unwrap();
        assert!(parse_shortcut_dir(dir.path().into(), &dir.path().join("preferences.toml")).unwrap().is_empty());
        assert!(parse_shortcut_dir(dir.path().join("missing"), &dir.path().join("preferences.toml")).is_err());
    }

    #[test]
    fn optional_identity_and_source_relative_directory_survive_reload() {
        let dir = TestDir::new();
        let path = dir.path().join("launcher.toml");
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml"));
        let legacy: Shortcut = toml::from_str(source).unwrap();
        assert!(legacy.id.is_none());
        assert!(legacy.working_directory.is_none());
        assert_eq!(legacy.stable_id(), legacy.name);
        fs::write(&path, format!("id = \"personal.editor\"\nworking_directory = \"project with spaces\"\n{source}")).unwrap();
        for _ in 0..2 {
            let shortcuts = parse_shortcut_dir(dir.path().into(), &dir.path().join("preferences.toml")).unwrap();
            assert_eq!(shortcuts[0].stable_id(), "personal.editor");
            assert_eq!(shortcuts[0].working_directory.as_deref(), Some(dir.path().join("project with spaces").as_path()));
            assert_eq!(shortcuts[0].source_path.as_deref(), Some(path.as_path()));
        }
    }
    #[test]
    fn equal_positions_follow_sorted_paths_and_relative_directories_use_nested_source_parent() {
        let dir = TestDir::new();
        let mut shortcut: Shortcut = toml::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml"))).unwrap();
        shortcut.taskbar.position = None;
        shortcut.name = "Second".into();
        fs::write(dir.path().join("b.toml"),toml::to_string(&shortcut).unwrap()).unwrap();
        shortcut.name = "First".into();
        shortcut.working_directory = Some(PathBuf::from("../project"));
        fs::create_dir(dir.path().join("a")).unwrap();
        fs::write(dir.path().join("a/first.toml"),toml::to_string(&shortcut).unwrap()).unwrap();
        let loaded = parse_shortcut_dir(dir.path().into(),&dir.path().join("preferences.toml")).unwrap();
        assert_eq!(loaded.iter().map(|shortcut| shortcut.name.as_str()).collect::<Vec<_>>(),["First","Second"]);
        assert_eq!(loaded[0].working_directory.as_deref(),Some(dir.path().join("a/../project").as_path()));
        assert!(toml::to_string(&loaded[0]).unwrap().contains("working_directory"));
        assert!(!toml::to_string(&loaded[0]).unwrap().contains("source_path"));
    }
    #[test]
    fn empty_identity_fields_and_unreadable_sources_are_rejected_with_context() {
        let dir = TestDir::new();
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml"));
        for text in [format!("id = \" \"\n{source}"),source.replace("Text editor"," "),source.replace("command = \"hx\"","command = \"\"")] {
            fs::write(dir.path().join("bad.toml"),text).unwrap();
            let error = parse_shortcut_dir(dir.path().into(),&dir.path().join("preferences.toml")).unwrap_err();
            assert!(format!("{error:#}").contains("bad.toml"));
        }
        fs::remove_file(dir.path().join("bad.toml")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("missing"),dir.path().join("broken.toml")).unwrap();
        let error = parse_shortcut_dir(dir.path().into(),&dir.path().join("preferences.toml")).unwrap_err();
        assert!(format!("{error:#}").contains("Cannot read launcher"));
    }
    #[test]
    fn duplicate_ids_include_legacy_name_fallback_and_errors_identify_source() {
        let dir = TestDir::new();
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml"));
        fs::write(dir.path().join("a.toml"), source).unwrap();
        let other = format!("id = \"Text editor\"\n{}", source.replace("name = \"Text editor\"", "name = \"Other\""));
        fs::write(dir.path().join("b.toml"), other).unwrap();
        let error = parse_shortcut_dir(dir.path().into(), &dir.path().join("preferences.toml")).unwrap_err();
        assert!(error.to_string().contains("Duplicate launcher id"), "{error:#}");
        assert!(error.to_string().contains("a.toml"));
        fs::write(dir.path().join("b.toml"), "invalid").unwrap();
        let error = parse_shortcut_dir(dir.path().into(), &dir.path().join("preferences.toml")).unwrap_err();
        assert!(format!("{error:#}").contains("b.toml"));
    }

}
