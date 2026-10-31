use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// A portable package keeps the browser relative to this executable, independent
/// of the working directory or PATH. Explicit paths and other commands are kept.
pub(crate) fn resolve_program(program: &OsStr) -> OsString {
    if program != OsStr::new("carboxyl") { return program.to_owned(); }
    std::env::current_exe().map_or_else(|_| program.to_owned(), |desktop| resolve_next_to(program, &desktop))
}

fn resolve_next_to(program: &OsStr, desktop: &Path) -> OsString {
    if program == OsStr::new("carboxyl") && let Some(directory) = desktop.parent() {
        let candidate = directory.join("libexec/carboxyl");
        if std::fs::metadata(&candidate).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0) {
            return candidate.into_os_string();
        }
    }
    program.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn bare_browser_prefers_executable_beside_desktop_without_changing_custom_commands() {
        let dir = TestDir::new();
        let desktop = dir.path().join("desktop-tui");
        let browser = dir.path().join("libexec/carboxyl");
        std::fs::create_dir(browser.parent().unwrap()).unwrap();
        std::fs::write(&browser, b"browser fixture").unwrap();
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(resolve_next_to(OsStr::new("carboxyl"), &desktop), browser.as_os_str());
        for command in ["/opt/carboxyl", "./carboxyl", "other-browser", "sh"] {
            assert_eq!(resolve_next_to(OsStr::new(command), &desktop), OsStr::new(command));
        }
    }

    #[test]
    fn missing_or_nonexecutable_bundle_retains_path_lookup() {
        let dir = TestDir::new();
        let desktop = dir.path().join("desktop-tui");
        assert_eq!(resolve_next_to(OsStr::new("carboxyl"), &desktop), "carboxyl");
        let browser = dir.path().join("libexec/carboxyl");
        std::fs::create_dir(browser.parent().unwrap()).unwrap();
        std::fs::write(&browser, b"not executable").unwrap();
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(resolve_next_to(OsStr::new("carboxyl"), &desktop), "carboxyl");
        std::fs::remove_file(&browser).unwrap();
        std::fs::create_dir(&browser).unwrap();
        assert_eq!(resolve_next_to(OsStr::new("carboxyl"), &desktop), "carboxyl");
    }

    #[test]
    fn bundle_location_preserves_spaces_unicode_and_opaque_os_path_bytes() {
        let dir = TestDir::new();
        let package = dir.path().join(OsString::from_vec(["bundle 文 ".as_bytes(), b"\xff"].concat()));
        let browser = package.join("libexec/carboxyl");
        std::fs::create_dir_all(browser.parent().unwrap()).unwrap();
        std::fs::write(&browser, b"browser fixture").unwrap();
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(resolve_next_to(OsStr::new("carboxyl"), &package.join("desktop-tui")), browser.as_os_str());
    }
}
