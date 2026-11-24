use super::*;
use anyhow::{Context, ensure};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use tokio::io::AsyncReadExt;

const MAX_LIST: usize = 8 * 1024 * 1024;

// Emit filename bytes separately from metadata. Fixed leading fields of POSIX
// ls -ldn supply a regular file's size; filenames are never parsed from ls.
const LIST: &str = r#"
CDPATH= cd -P "$1" && [ -r . ] && [ -x . ] || exit 1
export LC_ALL=C
for p in ./* ./.[!.]* ./..?*; do
    if [ -L "$p" ]; then k=o
    elif [ -d "$p" ]; then k=d
    elif [ -f "$p" ]; then k=f
    elif [ -e "$p" ]; then k=o
    else continue
    fi
    size=
    if [ "$k" = f ]; then
        row=$(ls -ldn "$p") || exit 1
        set -f
        set -- $row
        set +f
        size=$5
    fi
    printf '%s\000%s\000%s\000' "$k" "$size" "${p#./}" || exit 1
done"#;

struct Legacy {
    endpoint: Endpoint,
    driver: Driver,
    socket: PathBuf,
    _master: Group,
}

// ssh concatenates its command arguments for the remote login shell. Quote
// every byte sequence, including the fixed sh program and its positional args.
fn word(output: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    ensure!(!bytes.contains(&0), "Remote path contains a null byte");
    output.push(b'\'');
    for &b in bytes {
        if b == b'\'' { output.extend_from_slice(b"'\\''"); }
        else { output.push(b); }
    }
    output.push(b'\'');
    Ok(())
}

fn shell_command(script: &str, args: &[&OsStr]) -> Result<OsString> {
    let mut bytes = b"exec sh -c ".to_vec();
    word(&mut bytes, script.as_bytes())?;
    bytes.extend_from_slice(b" sh");
    for arg in args { bytes.push(b' '); word(&mut bytes, arg.as_bytes())?; }
    Ok(OsString::from_vec(bytes))
}

// The remote shell expands this operand, and the local scp checks the returned
// basename with fnmatch. Backslash escaping serves both parsers; surrounding
// quotes would become literal characters in scp's received-name check.
fn scp_operand(host: &str, path: &Path) -> Result<OsString> {
    ensure!(path.is_absolute(), "Expected an absolute SCP path");
    let mut bytes = if host.contains(':') { format!("[{host}]:") } else { format!("{host}:") }.into_bytes();
    for &b in path.as_os_str().as_bytes() {
        ensure!(!matches!(b, 0 | b'\n' | b'\r'), "Legacy SCP cannot transfer filenames containing line breaks");
        if b.is_ascii() && !b.is_ascii_alphanumeric() && !b"/_-.".contains(&b) {
            bytes.push(b'\\');
        }
        bytes.push(b);
    }
    Ok(OsString::from_vec(bytes))
}

pub(super) fn connect(endpoint: Endpoint, driver: Driver, start: &Path, ctl: &Control) -> Result<(Box<dyn Remote>, PathBuf)> {
    let socket = driver.broker.directory().join("session");
    let mut command = endpoint.command(false, &driver, Some(&socket))?;
    command.args(["-M", "-N", "--", &endpoint.host]);
    let mut master = Group::spawn(command)?;
    driver.wait_for_master(ctl, &mut master, &socket)?;
    let legacy = Legacy { endpoint, driver, socket, _master: master };
    let path = if start.is_relative() { Path::new(".").join(start) } else { start.to_owned() };
    let result = legacy.metadata(r#"CDPATH= cd -P "$1" && printf '%s\000' "$PWD""#, &[path.as_os_str()], 4097, ctl)?;
    let path = PathBuf::from(OsString::from_vec(result.strip_suffix(&[0]).context("Invalid remote home folder")?.to_vec()));
    ensure!(path.is_absolute() && !path.as_os_str().as_bytes().contains(&0), "Invalid remote home folder");
    Ok((Box::new(legacy), path))
}

impl Legacy {
    fn metadata(&self, script: &str, args: &[&OsStr], limit: usize, ctl: &Control) -> Result<Vec<u8>> {
        ctl.check()?;
        let mut command = self.endpoint.command(false, &self.driver, Some(&self.socket))?;
        command.args(["-o", "ControlMaster=no", "--", &self.endpoint.host])
            .arg(shell_command(script, args)?).stdout(Stdio::piped());
        let mut group = Group::spawn(command)?;
        let pipe = group.child.stdout.take().context("Missing SSH output")?;
        let output = self.driver.run(ctl, &mut group, async {
            let mut output = Vec::new();
            tokio::process::ChildStdout::from_std(pipe)?.take(limit as u64 + 1).read_to_end(&mut output).await?;
            ensure!(output.len() <= limit, "Legacy SCP metadata exceeds the listing limit");
            Ok(output)
        })?;
        self.driver.wait(ctl, &mut group, |_| Ok(false))?;
        Ok(output)
    }

    fn copy(&self, local: &Path, remote: &Path, sending: bool, ctl: &Control) -> Result<()> {
        ctl.check()?;
        let operand = scp_operand(&self.endpoint.host, remote)?;
        let mut command = self.endpoint.command(true, &self.driver, Some(&self.socket))?;
        command.args(["-o", "ControlMaster=no", "--"]);
        let source = if sending {
            let file = open_source(local)?;
            let meta = file.metadata()?;
            command.stdin(file).arg("/proc/self/fd/0").arg(&operand);
            Some(meta)
        } else {
            self.metadata(r#"[ ! -L "$1" ] && [ -f "$1" ]"#, &[remote.as_os_str()], 0, ctl)?;
            let parent = local.parent().context("Missing download staging directory")?;
            let basename = local.file_name().context("Missing download staging filename")?;
            command.current_dir(parent).arg(&operand).arg(Path::new(".").join(basename));
            None
        };
        let mut group = Group::spawn(command)?;
        let mut sent = 0;
        self.driver.wait(ctl, &mut group, |pid| {
            let position = match &source {
                Some(meta) => scp_read_position(pid, meta).unwrap_or(sent).min(meta.len()),
                None => std::fs::metadata(local)?.len(),
            };
            let change = position.saturating_sub(sent);
            if change > 0 { ctl.progress(change); sent = position; }
            Ok(change != 0)
        })?;
        let total = match source { Some(meta) => meta.len(), None => std::fs::metadata(local)?.len() };
        ctl.progress(total.saturating_sub(sent));
        Ok(())
    }
}

// SCP suppresses its terminal progress meter when detached. Sample the offset
// of its actual source descriptor for upload progress, including long files.
fn scp_read_position(pid: u32, source: &std::fs::Metadata) -> Option<u64> {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let mut best = None;
    for entry in std::fs::read_dir(root.join("fd")).ok()?.flatten() {
        let Some(meta) = std::fs::metadata(entry.path()).ok() else { continue };
        if meta.dev() != source.dev() || meta.ino() != source.ino() { continue; }
        let Ok(info) = std::fs::read_to_string(root.join("fdinfo").join(entry.file_name())) else { continue };
        if let Some(offset) = info.lines().find_map(|s| s.strip_prefix("pos:")?.trim().parse::<u64>().ok()) {
            best = Some(best.unwrap_or(0).max(offset));
        }
    }
    best
}

fn parse_listing(output: &[u8]) -> Result<Vec<Item>> {
    let mut fields = output.split(|b| *b == 0);
    let mut items = Vec::new();
    loop {
        let kind = match fields.next() {
            Some(b"f") => Kind::File, Some(b"d") => Kind::Directory, Some(b"o") => Kind::Other,
            Some(b"") if fields.next().is_none() => break,
            _ => bail!("Invalid remote directory entry"),
        };
        let field = fields.next().context("Missing size field")?;
        let size = if kind == Kind::File { Some(std::str::from_utf8(field)?.parse::<u64>()?) } else { None };
        let name = OsStr::from_bytes(fields.next().context("Missing filename field")?);
        ensure!(items.len() < MAX_ENTRIES && name.len() <= 4096, "Remote directory exceeds the listing limit");
        validate_name(name)?;
        items.push(Item { name: name.to_owned(), kind, size });
    }
    Ok(items)
}

impl Remote for Legacy {
    fn list(&mut self, path: &Path, ctl: &Control) -> Result<Vec<Item>> {
        parse_listing(&self.metadata(LIST, &[path.as_os_str()], MAX_LIST, ctl)?)
    }
    fn mkdir(&mut self, path: &Path, ctl: &Control) -> Result<()> {
        self.metadata(r#"umask 077; mkdir "$1""#, &[path.as_os_str()], 0, ctl)?;
        Ok(())
    }
    fn download(&mut self, remote: &Path, local: &Path, ctl: &Control) -> Result<()> {
        self.copy(local, remote, false, ctl)
    }
    fn upload(&mut self, local: &Path, remote: &Path, ctl: &Control) -> Result<()> {
        self.copy(local, remote, true, ctl)
    }
    fn publish(&mut self, from: &Path, to: &Path, replace: bool, ctl: &Control) -> Result<Publication> {
        ensure!(from.file_name() == to.file_name(), "Staged and destination filenames differ");
        // Target the parent so a newly appearing directory named like the
        // destination cannot absorb the file as an extra path component.
        let parent = to.parent().context("Missing publication directory")?;
        let (script, outcome) = if replace { (r#"mv -f "$1" "$2""#, Publication::Moved) }
            else { (r#"ln "$1" "$2""#, Publication::Linked) };
        self.metadata(script, &[from.as_os_str(), parent.as_os_str()], 0, ctl)?;
        Ok(outcome)
    }
    fn remove(&mut self, path: &Path, ctl: &Control) -> Result<()> {
        self.metadata(r#"rm "$1""#, &[path.as_os_str()], 0, ctl)?;
        Ok(())
    }
    fn rmdir(&mut self, path: &Path, ctl: &Control) -> Result<()> {
        self.metadata(r#"rmdir "$1""#, &[path.as_os_str()], 0, ctl)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn posix_listing_preserves_hidden_opaque_names_and_skips_symlink_targets() {
        let dir = tempfile::tempdir().unwrap();
        let funny = OsString::from_vec(b".q' ?; \n\xff".to_vec());
        fs::write(dir.path().join(&funny), b"payload").unwrap();
        std::os::unix::fs::symlink(dir.path().join(&funny), dir.path().join("not-followed")).unwrap();
        let command = shell_command(LIST, &[dir.path().as_os_str()]).unwrap();
        let result = Command::new("sh").arg("-c").arg(command).output().unwrap();
        assert!(result.status.success());
        let items = parse_listing(&result.stdout).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.name == funny && i.kind == Kind::File && i.size == Some(7)));
        assert!(items.iter().any(|i| i.name == "not-followed" && i.kind == Kind::Other));
        assert!(scp_operand("host", &dir.path().join(funny)).is_err());
    }

    #[test]
    fn listing_rejects_invalid_names_and_truncated_fields() {
        assert!(parse_listing(b"f\x007\0../outside\0").is_err());
        assert!(parse_listing(b"f\x007\0name").is_err());
        assert!(parse_listing(b"f\0").is_err());
        assert!(parse_listing(b"").unwrap().is_empty());
    }
}
