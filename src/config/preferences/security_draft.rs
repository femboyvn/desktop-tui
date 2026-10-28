use crate::config::preferences::Preferences;
use crate::runtime::LockCredential;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PasswordChange {
    Keep,
    Replace,
    Remove,
}
pub struct SecurityDraft {
    original: Option<LockCredential>,
    retry_at: Instant,
}
impl SecurityDraft {
    pub fn new(original: Option<LockCredential>) -> Self {
        Self {
            original,
            retry_at: Instant::now(),
        }
    }
    pub fn has_password(&self) -> bool {
        self.original.is_some()
    }
    fn validate(
        &mut self,
        action: PasswordChange,
        current: &str,
        new: &str,
        confirmation: &str,
        now: Instant,
    ) -> anyhow::Result<Option<LockCredential>> {
        if action == PasswordChange::Keep {
            return Ok(self.original.clone());
        }
        if let Some(original) = &self.original {
            anyhow::ensure!(
                now >= self.retry_at,
                "Wait one second before trying the current password again."
            );
            if !original.verify(current) {
                self.retry_at = now + Duration::from_secs(1);
                anyhow::bail!(
                    "Current password is incorrect. Enter it again to change or remove the saved password."
                );
            }
        }
        match action {
            PasswordChange::Replace => Ok(Some(LockCredential::new(new, confirmation)?)),
            PasswordChange::Remove => Ok(None),
            PasswordChange::Keep => unreachable!(),
        }
    }
    pub fn save(
        &mut self,
        path: &Path,
        mut palette: Preferences,
        action: PasswordChange,
        current: &str,
        new: &str,
        confirmation: &str,
        now: Instant,
    ) -> anyhow::Result<Preferences> {
        anyhow::ensure!(
            Preferences::stored_credential(path)? == self.original,
            "The saved password changed in another instance. Cancel and reopen Preference before saving."
        );
        palette.terminal_lock = self.validate(action, current, new, confirmation, now)?;
        palette.save(path, Some(&self.original))?;
        Ok(palette)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    #[test]
    fn failed_save_under_contention_keeps_old_password_and_palette() {
        use std::os::fd::AsRawFd;
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        let mut active = Preferences::default();
        active.terminal_lock = Some(LockCredential::new("original", "original").unwrap());
        active.save(&path, None).unwrap();
        let original_bytes = std::fs::read(&path).unwrap();
        let held_directory = std::fs::File::open(dir.path()).unwrap();
        assert_eq!(
            unsafe { libc::flock(held_directory.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        let mut draft = SecurityDraft::new(active.terminal_lock.clone());
        assert!(
            draft
                .save(
                    &path,
                    crate::config::preferences::PalettePreset::Asexual.preferences(),
                    PasswordChange::Replace,
                    "original",
                    "changed",
                    "changed",
                    Instant::now()
                )
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
        assert_eq!(Preferences::load(&path).0, active);
        drop(held_directory);
        let saved = draft
            .save(
                &path,
                crate::config::preferences::PalettePreset::Asexual.preferences(),
                PasswordChange::Replace,
                "original",
                "changed",
                "changed",
                Instant::now(),
            )
            .unwrap();
        assert!(saved.terminal_lock.as_ref().unwrap().verify("changed"));
    }
    #[test]
    fn changing_and_removing_passwords_require_current_password_and_throttle_failures() {
        let old = LockCredential::new("old password", "old password").unwrap();
        let mut draft = SecurityDraft::new(Some(old.clone()));
        let now = Instant::now();
        assert!(
            draft
                .validate(
                    PasswordChange::Replace,
                    "wrong",
                    "new password",
                    "new password",
                    now
                )
                .is_err()
        );
        assert!(
            draft
                .validate(PasswordChange::Remove, "old password", "", "", now)
                .is_err()
        );
        let later = now + Duration::from_secs(1);
        assert!(
            draft
                .validate(
                    PasswordChange::Replace,
                    "old password",
                    "new password",
                    "mismatch",
                    later
                )
                .is_err()
        );
        let replacement = draft
            .validate(
                PasswordChange::Replace,
                "old password",
                "new password",
                "new password",
                later,
            )
            .unwrap()
            .unwrap();
        assert!(replacement.verify("new password"));
        assert!(!replacement.verify("old password"));
        assert_eq!(
            draft
                .validate(PasswordChange::Keep, "", "ignored", "", later)
                .unwrap(),
            Some(old)
        );
        assert!(
            draft
                .validate(PasswordChange::Remove, "wrong", "", "", later)
                .is_err()
        );
        assert!(
            draft
                .validate(
                    PasswordChange::Remove,
                    "old password",
                    "",
                    "",
                    later + Duration::from_secs(1)
                )
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn all_or_nothing_save_preserves_password_across_palettes_and_rejects_stale_dialogs() {
        let dir = TestDir::new();
        let path = dir.path().join("preferences.toml");
        let mut draft = SecurityDraft::new(None);
        let now = Instant::now();
        let active = draft
            .save(
                &path,
                Preferences::default(),
                PasswordChange::Replace,
                "",
                "saved password",
                "saved password",
                now,
            )
            .unwrap();
        assert!(
            active
                .terminal_lock
                .as_ref()
                .unwrap()
                .verify("saved password")
        );
        let bytes = std::fs::read(&path).unwrap();
        let mut current = SecurityDraft::new(active.terminal_lock.clone());
        let now = Instant::now();
        assert!(
            current
                .save(
                    &path,
                    Preferences::default(),
                    PasswordChange::Remove,
                    "wrong",
                    "",
                    "",
                    now
                )
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let later = now + Duration::from_secs(1);
        let edited = current
            .save(
                &path,
                crate::config::preferences::PalettePreset::Transgender.preferences(),
                PasswordChange::Keep,
                "",
                "",
                "",
                later,
            )
            .unwrap();
        assert_eq!(edited.terminal_lock, active.terminal_lock);
        assert_eq!(
            crate::config::preferences::PalettePreset::matching(&edited),
            Some(crate::config::preferences::PalettePreset::Transgender)
        );
        let mut stale = SecurityDraft::new(active.terminal_lock.clone());
        let changed = current
            .save(
                &path,
                edited,
                PasswordChange::Replace,
                "saved password",
                "second password",
                "second password",
                later,
            )
            .unwrap();
        assert!(
            stale
                .save(
                    &path,
                    Preferences::default(),
                    PasswordChange::Keep,
                    "",
                    "",
                    "",
                    later
                )
                .is_err()
        );
        assert_eq!(Preferences::load(&path).0, changed);
        let mut fresh = SecurityDraft::new(changed.terminal_lock.clone());
        let removed = fresh
            .save(
                &path,
                changed,
                PasswordChange::Remove,
                "second password",
                "",
                "",
                Instant::now(),
            )
            .unwrap();
        assert!(removed.terminal_lock.is_none());
        assert!(Preferences::stored_credential(&path).unwrap().is_none());
    }
}
