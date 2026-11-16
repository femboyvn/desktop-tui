use crate::config::{catalog, parse_shortcut_dir, preferences};
use std::ffi::{OsStr, OsString};
use std::fmt::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub(crate) struct DiagnosticReport { pub text: String, pub healthy: bool }

/// Read-only diagnostics. Call from a worker when used by the desktop UI.
pub(crate) fn report(shortcut_dir: &Path, settings_path: &Path) -> String {
    diagnose(shortcut_dir, settings_path).text
}

pub(crate) fn diagnose(shortcut_dir: &Path, settings_path: &Path) -> DiagnosticReport {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    diagnose_with(shortcut_dir, settings_path, std::env::var_os("PATH").as_deref(), &cwd)
}

fn diagnose_with(shortcut_dir: &Path, settings_path: &Path, search_path: Option<&OsStr>, cwd: &Path) -> DiagnosticReport {
    let mut result = DiagnosticReport { text: String::from("Desktop-TUI diagnostics (read only; no launcher commands executed)\n"), healthy: true };
    let _ = writeln!(result.text, "Launcher directory: {shortcut_dir:?}\nSettings file: {settings_path:?}");
    let (_, warning) = preferences::Preferences::load(settings_path);
    if let Some(warning) = warning {
        result.healthy = false;
        let _ = writeln!(result.text, "ERROR settings: {warning}");
    } else {
        let _ = writeln!(result.text, "OK settings: {}", if settings_path.exists() { "valid" } else { "absent; defaults apply" });
    }
    let browser = super::bundled::resolve_program(OsStr::new("carboxyl"));
    match executable(&browser, search_path, cwd) {
        Some(path) => { let _ = writeln!(result.text, "OK Carboxyl resolution (bundled companion preferred): {path:?}"); }
        None => { let _ = writeln!(result.text, "WARN Carboxyl resolution: no executable companion or PATH entry"); }
    }
    match parse_shortcut_dir(shortcut_dir.to_owned(), settings_path) {
        Err(error) => {
            result.healthy = false;
            let _ = writeln!(result.text, "ERROR launchers: {error:#}");
        }
        Ok(shortcuts) => {
            let _ = writeln!(result.text, "OK configuration: {} custom launcher(s)", shortcuts.len());
            for shortcut in catalog::with_starters(shortcuts) {
                let directory = shortcut.working_directory.as_deref().unwrap_or(cwd);
                if !directory.is_dir() {
                    result.healthy = false;
                    let _ = writeln!(result.text, "ERROR {:?}: working directory is missing or inaccessible: {directory:?}", shortcut.name);
                }
                let resolved = super::bundled::resolve_program(OsStr::new(&shortcut.command));
                match executable(&resolved, search_path, directory) {
                    Some(path) => { let _ = writeln!(result.text, "OK executable {:?}: {:?} -> {path:?}", shortcut.name, shortcut.command); }
                    None => {
                        let required = shortcut.name == "Terminal";
                        if required { result.healthy = false; }
                        let _ = writeln!(result.text, "{} executable {:?}: {:?} is missing or not executable", if required { "ERROR" } else { "WARN" }, shortcut.name, shortcut.command);
                    }
                }
            }
        }
    }
    let display = std::env::var_os("DISPLAY").filter(|value| !value.is_empty());
    let wayland = std::env::var_os("WAYLAND_DISPLAY").filter(|value| !value.is_empty());
    let mut helpers = vec!["pdfinfo", "pdftoppm"];
    if wayland.is_some() { helpers.extend(["wl-copy", "wl-paste"]); }
    if display.is_some() { helpers.extend(["xclip", "xsel"]); }
    for helper in helpers {
        match executable(OsStr::new(helper), search_path, cwd) {
            Some(path) => { let _ = writeln!(result.text, "OK optional helper {helper}: {path:?}"); }
            None => { let _ = writeln!(result.text, "WARN optional helper {helper}: unavailable"); }
        }
    }
    let _ = writeln!(result.text, "PTY/SSH: supported without a graphical session\nOptional graphical session: DISPLAY={display:?}, WAYLAND_DISPLAY={wayland:?}");
    let clipboard = if wayland.is_some() && executable(OsStr::new("wl-copy"),search_path,cwd).is_some() { "Wayland wl-copy (availability only)" }
        else if display.is_some() && ["xclip", "xsel"].iter().any(|tool| executable(OsStr::new(tool),search_path,cwd).is_some()) { "X11 provider (availability only)" }
        else { "internal clipboard (normal PTY/SSH mode; graphical providers are optional)" };
    let _ = writeln!(result.text, "Clipboard: {clipboard}\nResult: {}", if result.healthy { "OK (optional warnings may limit features)" } else { "ERROR (fix configuration or Terminal executable)" });
    result
}

fn executable(program: &OsStr, search_path: Option<&OsStr>, cwd: &Path) -> Option<PathBuf> {
    let candidate = Path::new(program);
    let is_executable = |path: &Path| std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        let path = if candidate.is_absolute() { candidate.to_owned() } else { cwd.join(candidate) };
        return is_executable(&path).then_some(path);
    }
    if program.is_empty() { return None; }
    for entry in std::env::split_paths(search_path.unwrap_or(&OsString::new())) {
        let path = if entry.is_absolute() { entry.join(candidate) } else { cwd.join(entry).join(candidate) };
        if is_executable(&path) { return Some(path); }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    #[test]
    fn resolution_checks_modes_preserves_arguments_and_never_executes_programs() {
        let dir = TestDir::new();
        let executable_path = dir.path().join("tool with spaces");
        std::fs::write(&executable_path, b"#!/bin/sh\nexit 99\n").unwrap();
        std::fs::set_permissions(&executable_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(executable(OsStr::new("tool with spaces"), Some(dir.path().as_os_str()), Path::new("/")),Some(executable_path.clone()));
        assert_eq!(executable(OsStr::new("./tool with spaces"),None,dir.path()),Some(executable_path.clone()));
        std::fs::set_permissions(&executable_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(executable(executable_path.as_os_str(),None,Path::new("/")).is_none());
    }
    #[test]
    fn configured_terminal_failure_is_fatal_optional_tools_are_warnings() {
        let dir = TestDir::new();
        let template = include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/shortcuts/sh.toml"));
        std::fs::write(dir.path().join("terminal.toml"),template.replace("command = \"sh\"","command = \"definitely-missing-desktop-tui-shell\"")).unwrap();
        let result = diagnose_with(dir.path(),&dir.path().join("preferences.toml"),Some(OsStr::new("/nonexistent-desktop-tools")),dir.path());
        assert!(!result.healthy);
        assert!(result.text.contains("ERROR executable \"Terminal\""),"{}",result.text);
        assert!(result.text.contains("WARN optional helper pdfinfo"));
        assert!(!dir.path().join("preferences.toml").exists());
    }
    #[test]
    fn missing_optional_helpers_do_not_fail_a_valid_terminal_or_execute_it() {
        let dir = TestDir::new();
        let program = dir.path().join("sh");
        let marker = dir.path().join("executed");
        std::fs::write(&program,format!("#!/bin/sh\ntouch '{}'\n",marker.display())).unwrap();
        std::fs::set_permissions(&program,std::fs::Permissions::from_mode(0o755)).unwrap();
        let result = diagnose_with(dir.path(),&dir.path().join("preferences.toml"),Some(dir.path().as_os_str()),dir.path());
        assert!(result.healthy,"{}",result.text);
        assert!(result.text.contains("WARN optional helper pdfinfo"));
        assert!(!marker.exists());
        std::fs::write(dir.path().join("preferences.toml"),"invalid").unwrap();
        let result = diagnose_with(dir.path(),&dir.path().join("preferences.toml"),Some(dir.path().as_os_str()),dir.path());
        assert!(!result.healthy);
        assert!(result.text.contains("ERROR settings"));
    }
    #[test]
    fn missing_configuration_is_reported_without_creating_directories() {
        let dir = TestDir::new();
        let missing = dir.path().join("missing");
        let result = diagnose(&missing,&missing.join("preferences.toml"));
        assert!(!result.healthy);
        assert!(result.text.contains("ERROR launchers"));
        assert!(!missing.exists());
    }
}
