use crate::terminal_lock::LockCredential;
use anyhow::Context;
use appcui::prelude::Color;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb(pub u8, pub u8, pub u8);
impl TryFrom<String> for Rgb {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let bytes = value.as_bytes();
        if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
            return Err("Use a color in #RRGGBB format (for example #F6D9EE).".into());
        }
        Ok(Self(
            u8::from_str_radix(&value[1..3], 16).unwrap(),
            u8::from_str_radix(&value[3..5], 16).unwrap(),
            u8::from_str_radix(&value[5..7], 16).unwrap(),
        ))
    }
}
impl From<Rgb> for String {
    fn from(value: Rgb) -> Self {
        format!("#{:02X}{:02X}{:02X}", value.0, value.1, value.2)
    }
}
impl Rgb {
    pub fn color(self) -> Color {
        Color::RGB(self.0, self.1, self.2)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopPalette {
    pub background: Rgb,
    pub surface: Rgb,
    pub text: Rgb,
    pub muted_text: Rgb,
    pub border: Rgb,
    pub accent: Rgb,
    pub selection: Rgb,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalPalette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub ansi: [Rgb; 16],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub desktop: DesktopPalette,
    pub terminal: TerminalPalette,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_lock: Option<LockCredential>,
    #[serde(default)]
    pub panel: crate::panel::PanelPreferences,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            terminal_lock: None,
            panel: crate::panel::PanelPreferences::default(),
            desktop: DesktopPalette {
                background: Rgb(246, 217, 238),
                surface: Rgb(255, 247, 252),
                text: Rgb(63, 42, 74),
                muted_text: Rgb(132, 104, 144),
                border: Rgb(177, 140, 181),
                accent: Rgb(232, 164, 201),
                selection: Rgb(200, 181, 234),
            },
            terminal: TerminalPalette {
                foreground: Rgb(63, 42, 74),
                background: Rgb(255, 247, 252),
                // Plum, rose, sage, ochre, blue, mauve, teal, and pearl.
                ansi: [
                    Rgb(63, 42, 74),
                    Rgb(177, 67, 104),
                    Rgb(77, 128, 94),
                    Rgb(157, 117, 47),
                    Rgb(100, 110, 177),
                    Rgb(157, 89, 166),
                    Rgb(63, 131, 142),
                    Rgb(218, 199, 219),
                    Rgb(132, 104, 144),
                    Rgb(219, 112, 151),
                    Rgb(126, 172, 139),
                    Rgb(203, 164, 96),
                    Rgb(153, 159, 215),
                    Rgb(199, 146, 205),
                    Rgb(119, 183, 193),
                    Rgb(255, 247, 252),
                ],
            },
        }
    }
}
impl Preferences {
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match read_settings(path) {
            Ok(None) => (Self::default(), None),
            Ok(Some(contents)) => match toml::from_str::<Self>(&contents).and_then(|preferences| {
                if preferences.terminal_lock.is_some() {
                    check_private_storage(path).map_err(serde::de::Error::custom)?;
                }
                Ok(preferences)
            }) {
                Ok(preferences) => (preferences, None),
                Err(error) => (
                    Self::default(),
                    Some(format!(
                        "Could not load {}: {error}\nUsing Femboy defaults. The original file has been kept.",
                        path.display()
                    )),
                ),
            },
            Err(error) => (
                Self::default(),
                Some(format!(
                    "Could not read {}: {error}\nUsing Femboy defaults.",
                    path.display()
                )),
            ),
        }
    }

    /// Read authentication separately from the palette, and never downgrade a broken
    /// settings file into an unauthenticated temporary-password setup.
    pub fn stored_credential(path: &Path) -> anyhow::Result<Option<LockCredential>> {
        #[derive(Deserialize)]
        struct SecurityOnly {
            #[serde(default)]
            terminal_lock: Option<LockCredential>,
        }
        let Some(contents) = read_settings(path)? else {
            return Ok(None);
        };
        let security: SecurityOnly = toml::from_str(&contents)
            .context("Cannot read Terminal Lock settings. Repair the settings file before changing passwords or locking.")?;
        if security.terminal_lock.is_some() {
            check_private_storage(path)?;
        }
        Ok(security.terminal_lock)
    }

    /// Atomically save settings. UI saves compare the original credential while
    /// holding the directory lock so another instance cannot be overwritten.
    pub fn save(
        &self,
        path: &Path,
        expected: Option<&Option<LockCredential>>,
    ) -> anyhow::Result<()> {
        let contents = toml::to_string_pretty(self)?;
        let parent = path
            .parent()
            .context("Settings path has no parent directory")?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .with_context(|| format!("Could not create {}", parent.display()))?;
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(parent)
            .context("The settings directory must be a real, owned directory (not a symlink).")?;
        anyhow::ensure!(
            directory.metadata()?.uid() == effective_uid(),
            "The settings directory must belong to the current user."
        );
        directory.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        // Serialize cooperating instances across the read/compare/atomic replacement.
        // Nonblocking avoids waiting on another process from the UI thread.
        let _lock = SettingsLock::acquire(directory)?;
        if let Some(expected) = expected {
            anyhow::ensure!(
                &Self::stored_credential(path)? == expected,
                "The saved password changed in another instance. Cancel and reopen Preference before saving."
            );
        }
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_file() && metadata.uid() == effective_uid(),
                "The settings file must be a regular file owned by the current user, not a symlink."
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = parent.join(format!(
            ".preferences-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Create beside the destination so rename is atomic, and never truncate the old settings.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let result = (|| -> anyhow::Result<()> {
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.with_context(|| format!("Could not save {}", path.display()))
    }
}

struct SettingsLock(std::fs::File);
impl SettingsLock {
    fn acquire(directory: std::fs::File) -> anyhow::Result<Self> {
        if unsafe { libc::flock(directory.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("Another instance is saving settings. Try again shortly.");
        }
        Ok(Self(directory))
    }
}
impl Drop for SettingsLock {
    fn drop(&mut self) {
        // A concurrently forked PTY child can inherit the open-file description
        // until exec. Unlock explicitly so closing our fd does not leave that
        // child temporarily holding a completed save's lock.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn effective_uid() -> u32 {
    // geteuid has no arguments, allocation, or failure state.
    unsafe { libc::geteuid() }
}
fn read_settings(path: &Path) -> anyhow::Result<Option<String>> {
    let parent = path
        .parent()
        .context("Settings path has no parent directory")?;
    if let Ok(metadata) = std::fs::symlink_metadata(parent) {
        anyhow::ensure!(
            metadata.is_dir(),
            "The settings directory must not be a symlink."
        );
    }
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("Could not open the settings file safely"),
    };
    anyhow::ensure!(
        file.metadata()?.is_file(),
        "Settings must be a regular file."
    );
    let mut contents = String::new();
    file.take(65537).read_to_string(&mut contents)?;
    anyhow::ensure!(contents.len() <= 65536, "Settings file exceeds 64 KiB.");
    Ok(Some(contents))
}
fn check_private_storage(path: &Path) -> anyhow::Result<()> {
    for entry in [
        path,
        path.parent()
            .context("Settings path has no parent directory")?,
    ] {
        let metadata = std::fs::symlink_metadata(entry)?;
        anyhow::ensure!(
            metadata.uid() == effective_uid() && metadata.mode() & 0o077 == 0,
            "Saved password storage must be owned by you and private. Run chmod 700 on the application configuration directory and chmod 600 on preferences.toml, then try again."
        );
    }
    Ok(())
}

pub fn settings_path() -> anyhow::Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("com", "Julien-cpsn", "desktop-tui")
        .context("Could not find the application configuration directory")?;
    Ok(dirs.config_dir().join("preferences.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    #[test]
    fn completed_save_lock_is_released_even_if_a_fork_inherited_the_descriptor() {
        let dir = TestDir::new();
        let lock = SettingsLock::acquire(std::fs::File::open(dir.path()).unwrap()).unwrap();
        // dup and fork both share the original open-file description. Keep that
        // description alive just as a PTY child does before it execs.
        let inherited = lock.0.try_clone().unwrap();
        drop(lock);
        let next = SettingsLock::acquire(std::fs::File::open(dir.path()).unwrap());
        assert!(
            next.is_ok(),
            "a completed save must not leave the directory locked by inherited descriptors"
        );
        drop(inherited);
    }
    #[test]
    fn saved_password_round_trips_privately_without_storing_plaintext() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = TestDir::new();
        let path = dir.path().join("config/preferences.toml");
        let mut p = Preferences::default();
        p.terminal_lock = Some(
            crate::terminal_lock::LockCredential::new(
                "persistent 🔒 password",
                "persistent 🔒 password",
            )
            .unwrap(),
        );
        p.save(&path, None).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("persistent"));
        assert!(contents.contains("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            std::fs::metadata(path.parent().unwrap()).unwrap().mode() & 0o777,
            0o700
        );
        let loaded = Preferences::load(&path);
        assert_eq!(loaded, (p.clone(), None));
        assert!(
            Preferences::stored_credential(&path)
                .unwrap()
                .unwrap()
                .verify("persistent 🔒 password")
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Preferences::stored_credential(&path).is_err());
        // An intentional save restores private permissions through atomic replacement.
        p.save(&path, None).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        std::fs::set_permissions(
            path.parent().unwrap(),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(Preferences::stored_credential(&path).is_err());
    }
    #[test]
    fn legacy_settings_have_no_password_and_bad_credentials_never_become_temporary_locks() {
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        assert!(Preferences::stored_credential(&path).unwrap().is_none());
        Preferences::default().save(&path, None).unwrap();
        assert!(Preferences::stored_credential(&path).unwrap().is_none());
        let original = std::fs::read_to_string(&path).unwrap();
        assert!(!original.contains("terminal_lock"));
        for malformed in [
            "terminal_lock = 'plaintext'\n",
            "terminal_lock = ['$argon2id$']\n",
            "broken = [\n",
        ] {
            std::fs::write(&path, format!("{malformed}{original}")).unwrap();
            assert!(Preferences::stored_credential(&path).is_err());
            assert!(Preferences::load(&path).1.is_some());
        }
    }
    #[test]
    fn secure_settings_reject_symlinks_and_leave_targets_untouched() {
        use std::os::unix::fs::symlink;
        let dir = TestDir::new();
        let target = dir.path().join("original");
        std::fs::write(&target, "untouched").unwrap();
        let path = dir.path().join("preferences.toml");
        symlink(&target, &path).unwrap();
        assert!(Preferences::default().save(&path, None).is_err());
        assert!(Preferences::stored_credential(&path).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "untouched");
        let link_dir = dir.path().join("link-dir");
        symlink(dir.path(), &link_dir).unwrap();
        assert!(
            Preferences::default()
                .save(&link_dir.join("settings.toml"), None)
                .is_err()
        );
        assert!(!dir.path().join("settings.toml").exists());
    }
    #[test]
    fn defaults_are_light_femboy() {
        let p = Preferences::default();
        assert_eq!(p.desktop.background, Rgb(246, 217, 238));
        assert_eq!(p.desktop.surface, Rgb(255, 247, 252));
        assert_eq!(p.desktop.text, Rgb(63, 42, 74));
        assert_eq!(p.desktop.accent, Rgb(232, 164, 201));
        assert_eq!(p.desktop.selection, Rgb(200, 181, 234));
        assert_ne!(p.terminal.ansi[0], p.terminal.ansi[8]);
    }
    #[test]
    fn validates_rgb_without_accepting_partial_or_non_ascii_values() {
        assert_eq!(
            Rgb::try_from("#aBcD09".to_owned()).unwrap(),
            Rgb(171, 205, 9)
        );
        assert_eq!(String::from(Rgb(171, 205, 9)), "#ABCD09");
        for invalid in [
            "red", "123456", "#123", "#1234567", "#GG0000", "#é1234", " #123456",
        ] {
            assert!(Rgb::try_from(invalid.to_owned()).is_err(), "{invalid}");
        }
    }
    #[test]
    fn missing_file_uses_defaults_without_creating_file() {
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        assert_eq!(Preferences::load(&path), (Preferences::default(), None));
        assert!(!path.exists());
    }
    #[test]
    fn saves_and_loads_every_palette_role() {
        let dir = TestDir::new();
        let path = dir.path().join("new-config/preferences.toml");
        let mut p = Preferences::default();
        p.terminal.ansi[15] = Rgb(1, 2, 3);
        p.desktop.background = Rgb(4, 5, 6);
        p.save(&path, None).unwrap();
        assert_eq!(Preferences::load(&path), (p.clone(), None));
        p.terminal.background = Rgb(7, 8, 9);
        p.save(&path, None).unwrap();
        assert_eq!(Preferences::load(&path), (p, None));
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }
    #[test]
    fn invalid_settings_warn_and_leave_original_untouched() {
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        let valid = toml::to_string(&Preferences::default()).unwrap();
        for bad in [
            "broken = [".to_owned(),
            valid.replace("#F6D9EE", "#nope"),
            "[desktop]".to_owned(),
        ] {
            std::fs::write(&path, &bad).unwrap();
            let (loaded, warning) = Preferences::load(&path);
            assert_eq!(loaded, Preferences::default());
            assert!(warning.unwrap().contains("preferences.toml"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
        }
    }
    #[test]
    fn failed_save_does_not_replace_previous_settings() {
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("sentinel"), "original").unwrap();
        assert!(Preferences::default().save(&path, None).is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("sentinel")).unwrap(),
            "original"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
