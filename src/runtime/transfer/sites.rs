use super::Site;
use anyhow::{Context, Result, ensure};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

const MAX_BYTES:u64 = 512 * 1024;
fn validate(sites:&[Site]) -> Result<()> {
    ensure!(sites.len() <= 256, "At most 256 saved sites are supported");
    let mut names = HashSet::new();
    for site in sites {
        site.validate()?;
        ensure!(!site.name.trim().is_empty() && names.insert(&site.name), "Each saved site needs a unique name");
    }
    Ok(())
}
pub(super) fn load(path:&Path) -> Result<Vec<Site>> {
    let file = match File::options().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path) {
        Ok(file) => file, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()), Err(e) => return Err(e.into()),
    };
    let meta = file.metadata()?;
    ensure!(meta.is_file() && meta.uid() == unsafe {libc::geteuid()} && meta.len() <= MAX_BYTES, "Invalid saved sites file");
    let mut bytes = Vec::new(); file.take(MAX_BYTES+1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_BYTES, "Saved sites file too large");
    let sites:Vec<Site> = serde_json::from_slice(&bytes).context("Cannot read saved sites JSON")?;
    validate(&sites)?; Ok(sites)
}
pub(super) fn save(path:&Path, expected:&[Site], sites:&[Site]) -> Result<()> {
    validate(sites)?;
    let data = serde_json::to_vec_pretty(sites)?;
    ensure!(data.len() as u64 <= MAX_BYTES,"Saved sites file too large");
    let dir = path.parent().context("Missing sites directory")?;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let directory = File::options().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(dir)?;
    ensure!(directory.metadata()?.uid() == unsafe {libc::geteuid()}, "Sites directory has a different owner");
    let lock = OpenOptions::new().write(true).read(true).create(true).truncate(false).mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(dir.join(".sites.lock"))?;
    ensure!(lock.metadata()?.is_file(),"Invalid sites lock");
    lock.try_lock().context("Another instance is saving sites")?;
    ensure!(load(path)? == expected, "Saved sites changed in another instance. Reopen File Transfer to reload.");
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&data)?; tmp.as_file().sync_all()?; tmp.persist(path)?; directory.sync_all()?;
    Ok(())
}
