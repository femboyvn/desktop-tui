//! AI Chat owns this file and its writer lock independently of system preferences.
use super::{AiPreferences,AiProvider};
use anyhow::{Context,Result,ensure};
use std::fs::{self,File,OpenOptions};
use std::io::{Read,Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt,MetadataExt,OpenOptionsExt,PermissionsExt};
use std::path::{Path,PathBuf};
use std::sync::atomic::{AtomicU64,Ordering};
use zeroize::Zeroizing;

#[derive(Clone)]
pub(crate) struct AiSettingsStore { pub path: PathBuf, legacy: PathBuf }
impl AiSettingsStore {
    pub fn new() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("com","Julien-cpsn","desktop-tui").context("Cannot find AI Chat configuration directory")?;
        Ok(Self::at(dirs.config_dir().join("ai-chat/ai.toml"),dirs.config_dir().join("preferences.toml")))
    }
    pub fn path_next_to(system: &Path) -> PathBuf { system.with_file_name("ai-chat").join("ai.toml") }
    pub fn at(path: PathBuf,legacy: PathBuf) -> Self { Self { path,legacy } }
    pub fn load(&self) -> Result<AiPreferences> {
        if let Some(settings) = self.read()? { return Ok(settings); }
        // Only this compatibility reader knows about the former system [ai] section.
        // An unrelated invalid/unreadable system file does not disable AI settings.
        let Some((document,legacy)) = self.legacy_section()? else { return Ok(AiPreferences::default()); };
        let ai: AiPreferences = legacy.clone().try_into().map_err(|_| anyhow::anyhow!("Legacy AI settings are invalid; original preferences.toml was kept."))?;
        if let Err(error) = self.save_inner(&ai,&AiPreferences::default(),true) {
            // Another window can finish migration while this reader is loading.
            if let Some(current) = self.read()? { return Ok(current); }
            return Err(error);
        }
        // Copy first. Cleanup failure keeps the old keys as a recoverable backup;
        // the independent file remains authoritative and is never reimported.
        let _ = crate::config::preferences::Preferences::retire_legacy_ai(&self.legacy,&document["ai"]);
        Ok(ai)
    }
    fn legacy_section(&self) -> Result<Option<(toml::Value,toml::Value)>> {
        let Ok(Some((text,file))) = read_owned(&self.legacy) else { return Ok(None); };
        let Ok(document) = toml::from_str::<toml::Value>(&text) else { return Ok(None); };
        let Some(legacy) = document.get("ai").cloned() else { return Ok(None); };
        private_file(&file,&self.legacy)?;
        Ok(Some((document,legacy)))
    }
    fn read(&self) -> Result<Option<AiPreferences>> {
        let Some((text,file)) = read_owned(&self.path)? else { return Ok(None); };
        private_file(&file,&self.path)?;
        let ai = toml::from_str::<AiPreferences>(&text).map_err(|_| anyhow::anyhow!("Invalid ai.toml. Repair AI Chat settings; original file kept."))?;
        validate(&ai)?; Ok(Some(ai))
    }
    /// Compare and replace only AI settings; the lock never blocks system preference saves.
    pub fn save(&self,ai: &AiPreferences,expected: &AiPreferences) -> Result<()> { self.save_inner(ai,expected,false) }
    fn save_inner(&self,ai: &AiPreferences,expected: &AiPreferences,must_be_missing: bool) -> Result<()> {
        validate(ai)?;
        let parent = self.path.parent().context("AI settings path has no parent")?;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
        let directory = OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY|libc::O_NOFOLLOW).open(parent)?;
        ensure!(directory.metadata()?.uid() == unsafe {libc::geteuid()},"AI settings directory must belong to you");
        directory.set_permissions(fs::Permissions::from_mode(0o700))?;
        let lease = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW|libc::O_NONBLOCK).open(parent.join(".ai-settings.lock"))?;
        let metadata = lease.metadata()?;
        ensure!(metadata.is_file() && metadata.uid() == unsafe {libc::geteuid()} && metadata.nlink() == 1,"Invalid AI settings lock");
        ensure!(unsafe {libc::flock(lease.as_raw_fd(),libc::LOCK_EX|libc::LOCK_NB)} == 0,"Another instance is saving AI settings. Try again.");
        let _lease = Lease(lease);
        let current = self.read()?;
        ensure!(!must_be_missing || current.is_none(),"Independent AI settings already exist");
        ensure!(&current.unwrap_or_default() == expected,"AI settings changed in another instance. Cancel and reopen AI Chat Settings.");
        let contents = Zeroizing::new(toml::to_string_pretty(ai)?);
        ensure!(contents.len() <= 65536,"AI settings exceed 64 KiB");
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = parent.join(format!(".ai-{}-{}.tmp",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&temporary)?;
            file.write_all(contents.as_bytes())?; file.sync_all()?; fs::rename(&temporary,&self.path)?; directory.sync_all()?; Ok(())
        })();
        if result.is_err() { let _ = fs::remove_file(&temporary); }
        result.context("AI Chat settings were not saved")
    }
}
struct Lease(File);
impl Drop for Lease { fn drop(&mut self) { unsafe {libc::flock(self.0.as_raw_fd(),libc::LOCK_UN);} } }
fn read_owned(path: &Path) -> Result<Option<(Zeroizing<String>,File)>> {
    if let Some(parent) = path.parent() && let Ok(metadata) = fs::symlink_metadata(parent) { ensure!(metadata.is_dir(),"Settings directory cannot be a symlink"); }
    let file = match OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW|libc::O_NONBLOCK).open(path) {
        Ok(file) => file,Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),Err(error) => return Err(error).context("Cannot safely read settings"),
    };
    let metadata = file.metadata()?;
    ensure!(metadata.is_file() && metadata.uid() == unsafe {libc::geteuid()} && metadata.nlink() == 1,"Settings must be an owned regular file");
    ensure!(metadata.len() <= 65536,"Settings exceed 64 KiB");
    let mut text = Zeroizing::new(String::new()); (&file).take(65537).read_to_string(&mut text)?;
    ensure!(text.len() <= 65536,"Settings exceed 64 KiB"); Ok(Some((text,file)))
}
fn private_file(file: &File,path: &Path) -> Result<()> {
    ensure!(file.metadata()?.mode() & 0o077 == 0,"AI settings must be private (chmod 600)");
    let parent = fs::symlink_metadata(path.parent().context("Invalid AI settings path")?)?;
    ensure!(parent.uid() == unsafe {libc::geteuid()} && parent.mode() & 0o077 == 0,"AI settings directory must be private (chmod 700)"); Ok(())
}
fn validate(ai: &AiPreferences) -> Result<()> {
    for provider in AiProvider::ALL {
        let model = &ai.settings(provider).model;
        ensure!(model.len() <= 256 && !model.chars().any(char::is_control),"Model IDs must be at most 256 characters without controls");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ai::ApiKey;
    use crate::config::preferences::Preferences;
    use crate::test_support::TestDir;
    fn configured() -> AiPreferences {
        let mut ai = AiPreferences::default(); ai.openai.api_key = ApiKey::new("fake-preserved-key").unwrap(); ai.openai.model = "chosen-model".into(); ai
    }
    #[test]
    fn migration_commit_never_replaces_a_newly_created_default_file() {
        let dir = TestDir::new(); let store = AiSettingsStore::at(dir.path().join("ai.toml"),dir.path().join("preferences.toml"));
        store.save(&AiPreferences::default(),&AiPreferences::default()).unwrap();
        assert!(store.save_inner(&configured(),&AiPreferences::default(),true).is_err());
        assert_eq!(store.load().unwrap(),AiPreferences::default());
    }

    #[test]
    fn ai_and_system_writers_use_independent_locks() {
        let dir = TestDir::new(); let system = dir.path().join("preferences.toml"); Preferences::default().save(&system,None).unwrap();
        let store = AiSettingsStore::at(dir.path().join("ai.toml"),system.clone());
        let system_lock = File::open(dir.path()).unwrap(); assert_eq!(unsafe {libc::flock(system_lock.as_raw_fd(),libc::LOCK_EX|libc::LOCK_NB)},0);
        store.save(&configured(),&AiPreferences::default()).unwrap(); unsafe {libc::flock(system_lock.as_raw_fd(),libc::LOCK_UN);}
        let ai_lock = File::open(dir.path().join(".ai-settings.lock")).unwrap(); assert_eq!(unsafe {libc::flock(ai_lock.as_raw_fd(),libc::LOCK_EX|libc::LOCK_NB)},0);
        Preferences::default().save(&system,Some(&None)).unwrap(); unsafe {libc::flock(ai_lock.as_raw_fd(),libc::LOCK_UN);}
    }

    #[test]
    fn ai_settings_are_private_atomic_and_independent_of_system_preferences() {
        let dir = TestDir::new(); let path = dir.path().join("ai.toml"); let legacy = dir.path().join("preferences.toml");
        let store = AiSettingsStore::at(path.clone(),legacy.clone()); let ai = configured(); store.save(&ai,&AiPreferences::default()).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777,0o600);
        fs::write(&legacy,"broken system configuration").unwrap(); assert_eq!(store.load().unwrap(),ai);
        let original = fs::read(&path).unwrap(); let mut changed = ai.clone(); changed.openai.model = "other-model".into();
        assert!(store.save(&changed,&AiPreferences::default()).is_err()); assert_eq!(fs::read(&path).unwrap(),original);
        fs::write(&path,"[openai]\napi_key='fake-secret' invalid").unwrap();
        assert!(!format!("{:#}",store.load().unwrap_err()).contains("fake-secret"));
        fs::remove_file(&path).unwrap(); fs::write(&legacy,toml::to_string(&Preferences::default()).unwrap()).unwrap();
        let (system,warning) = Preferences::load(&legacy); assert!(warning.is_none()); system.save(&legacy,None).unwrap();
    }
    #[test]
    fn migration_preserves_keys_and_current_system_values_and_new_file_takes_precedence() {
        let dir = TestDir::new(); let path = dir.path().join("ai.toml"); let legacy = dir.path().join("preferences.toml");
        let system = Preferences::default(); system.save(&legacy,None).unwrap();
        let ai = configured(); let mut document = toml::Value::try_from(&system).unwrap();
        document.as_table_mut().unwrap().insert("ai".into(),toml::Value::try_from(&ai).unwrap()); fs::write(&legacy,toml::to_string(&document).unwrap()).unwrap();
        let store = AiSettingsStore::at(path.clone(),legacy.clone()); assert_eq!(store.load().unwrap(),ai);
        let after: toml::Value = toml::from_str(&fs::read_to_string(&legacy).unwrap()).unwrap();
        assert!(after.get("ai").is_none()); assert_eq!(Preferences::load(&legacy).0,system);
        let mut changed = ai.clone(); changed.openai.model = "updated-model".into(); store.save(&changed,&ai).unwrap();
        fs::write(&legacy,toml::to_string(&document).unwrap()).unwrap(); assert_eq!(store.load().unwrap(),changed);
    }
    #[test]
    fn ai_save_does_not_change_system_settings_and_system_save_does_not_touch_ai() {
        let dir = TestDir::new(); let legacy = dir.path().join("preferences.toml"); let path = dir.path().join("ai.toml");
        let system = Preferences::default(); system.save(&legacy,None).unwrap(); let original = fs::read(&legacy).unwrap();
        let store = AiSettingsStore::at(path.clone(),legacy.clone()); store.save(&configured(),&AiPreferences::default()).unwrap();
        assert_eq!(fs::read(&legacy).unwrap(),original); let original_ai = fs::read(&path).unwrap();
        let mut system = system; system.panel.show_seconds = true; system.save(&legacy,Some(&None)).unwrap(); assert_eq!(fs::read(&path).unwrap(),original_ai);
    }
    #[test]
    fn invalid_ai_file_does_not_block_system_saves_and_ai_rejects_symlinks_or_public_keys() {
        let dir = TestDir::new(); let legacy = dir.path().join("preferences.toml"); let path = dir.path().join("ai.toml");
        Preferences::default().save(&legacy,None).unwrap(); let store = AiSettingsStore::at(path.clone(),legacy.clone());
        store.save(&configured(),&AiPreferences::default()).unwrap(); fs::set_permissions(&path,fs::Permissions::from_mode(0o644)).unwrap(); assert!(store.load().is_err());
        fs::write(&path,"invalid AI config").unwrap(); Preferences::default().save(&legacy,Some(&None)).unwrap();
        fs::remove_file(&path).unwrap(); std::os::unix::fs::symlink(&legacy,&path).unwrap(); assert!(store.load().is_err()); assert!(store.save(&configured(),&AiPreferences::default()).is_err());
    }
}
