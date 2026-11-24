use super::*;
use anyhow::{Context, ensure};
use futures_util::{StreamExt, TryFutureExt};
use openssh_sftp_client::{Sftp, SftpOptions};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncWriteExt};

// Check wire lengths before the typed client reserves a buffer for a reply.
// This task and both ends of its pipe belong to the connection's runtime.
fn bounded_packets(mut source: impl AsyncRead + Unpin + Send + 'static) -> impl AsyncRead {
    let (reader, mut writer) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let mut packet = Vec::new();
        while let Ok(len) = source.read_u32().await {
            if !(5..=1024 * 1024).contains(&len) { break; }
            packet.resize(len as usize, 0);
            if source.read_exact(&mut packet).await.is_err() { break; }
            // DATA includes a request ID and a counted byte string.
            if packet[0] == 103 && (len < 9 || packet[5..9] != (len - 9).to_be_bytes()) { break; }
            if writer.write_u32(len).await.is_err() || writer.write_all(&packet).await.is_err() { break; }
        }
    });
    reader
}

struct Modern {
    sftp: Sftp,
    child: Group,
    driver: Driver,
}

pub(super) fn connect(endpoint: Endpoint, driver: Driver, path: &Path, ctl: &Control) -> Result<(Box<dyn Remote>, PathBuf)> {
    let mut command = endpoint.command(false, &driver, None)?;
    command.args(["-o", "ControlMaster=no", "-s", "--", &endpoint.host, "sftp"])
        .stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = Group::spawn(command)?;
    let (stdin, stdout) = {
        let _enter = driver.runtime.enter();
        (tokio::process::ChildStdin::from_std(child.child.stdin.take().context("Missing SSH input")?)?,
         bounded_packets(tokio::process::ChildStdout::from_std(child.child.stdout.take().context("Missing SSH output")?)?))
    };
    let sftp = driver.run(ctl, &mut child, Sftp::new(stdin, stdout, SftpOptions::new()).err_into())?;
    let path = driver.run(ctl, &mut child, sftp.fs().canonicalize(path).err_into())?;
    ensure!(path.is_absolute(), "SFTP server returned a relative home folder");
    Ok((Box::new(Modern { sftp, child, driver }), path))
}

impl Remote for Modern {
    fn list(&mut self, path: &Path, ctl: &Control) -> Result<Vec<Item>> {
        let mut fs = self.sftp.fs();
        let driver = &self.driver;
        let dir = driver.run(ctl, &mut self.child, fs.open_dir(path).err_into())?;
        let mut stream = Box::pin(dir.read_dir());
        let mut items = Vec::new();
        while let Some(entry) = driver.run(ctl, &mut self.child, async { Ok(stream.next().await.transpose()?) })? {
            let name = entry.filename().as_os_str();
            if name == OsStr::new(".") || name == OsStr::new("..") { continue; }
            validate_name(name)?;
            ensure!(items.len() < MAX_ENTRIES && name.len() <= 4096, "Remote directory exceeds the listing limit");
            let metadata = if entry.file_type().is_some() { entry.metadata() }
                else { driver.run(ctl, &mut self.child, fs.symlink_metadata(path.join(name)).err_into())? };
            let kind = match metadata.file_type() {
                Some(t) if t.is_dir() => Kind::Directory,
                Some(t) if t.is_file() => Kind::File,
                _ => Kind::Other,
            };
            items.push(Item { name: name.to_owned(), kind, size: (kind == Kind::File).then(|| metadata.len()).flatten() });
        }
        Ok(items)
    }

    fn mkdir(&mut self, path: &Path, ctl: &Control) -> Result<()> {
        self.driver.run(ctl, &mut self.child,
            self.sftp.fs().dir_builder().permissions(0o700.into()).create(path).err_into())
    }

    fn download(&mut self, remote: &Path, local: &Path, ctl: &Control) -> Result<()> {
        let driver = &self.driver;
        let child = &mut self.child;
        let metadata = driver.run(ctl, child, self.sftp.fs().symlink_metadata(remote).err_into())?;
        ensure!(metadata.file_type().is_some_and(|t| t.is_file()), "Source is not a regular file");
        let mut file = driver.run(ctl, child, self.sftp.open(remote).err_into())?;
        let opened = driver.run(ctl, child, file.metadata().err_into())?;
        ensure!(opened.file_type().is_some_and(|t| t.is_file()), "Source changed during transfer");
        let mut dest = std::fs::OpenOptions::new().write(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(local)?;
        ensure!(dest.metadata()?.is_file(), "Invalid download staging file");
        dest.set_len(0)?;
        let mut buffer = Default::default();
        let mut total = 0u64;
        while let Some(data) = driver.run(ctl, child, file.read(64 * 1024, buffer).err_into())? {
            ensure!(!data.is_empty(), "Empty SFTP data packet");
            dest.write_all(&data)?;
            total += data.len() as u64;
            // The v3 server may return fewer bytes than requested. This client
            // advances by the request count; resume from the actual byte count.
            if file.offset() != total { std::pin::Pin::new(&mut file).start_seek(std::io::SeekFrom::Start(total))?; }
            ctl.progress(data.len() as u64);
            buffer = data;
            buffer.clear();
        }
        driver.run(ctl, child, file.close().err_into())?;
        ensure!(opened.len().is_none_or(|expected| expected == total), "Remote file size changed during download");
        dest.flush()?;
        Ok(())
    }

    fn upload(&mut self, local: &Path, remote: &Path, ctl: &Control) -> Result<()> {
        let mut src = open_source(local)?;
        let expected = src.metadata()?.len();
        let driver = &self.driver;
        let child = &mut self.child;
        let mut file = driver.run(ctl, child, self.sftp.options().write(true).create_new(true).open(remote).err_into())?;
        let mut buffer = [0; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = src.read(&mut buffer)?;
            if n == 0 { break; }
            driver.run(ctl, child, file.write_all(&buffer[..n]).err_into())?;
            total += n as u64;
            ctl.progress(n as u64);
        }
        driver.run(ctl, child, file.close().err_into())?;
        ensure!(expected == total, "Local file size changed during upload");
        Ok(())
    }

    fn publish(&mut self, from: &Path, to: &Path, replace: bool, ctl: &Control) -> Result<Publication> {
        let mut fs = self.sftp.fs();
        if !replace && self.sftp.support_hardlink() {
            self.driver.run(ctl, &mut self.child, fs.hard_link(from, to).err_into())?;
            return Ok(Publication::Linked);
        }
        // V3 RENAME fails on an existing destination. The client's POSIX
        // extension instead replaces it, which requires explicit approval.
        ensure!(replace || !self.sftp.support_posix_rename(), "Server cannot safely publish without replacing an existing file");
        self.driver.run(ctl, &mut self.child, fs.rename(from, to).err_into())?;
        Ok(Publication::Moved)
    }

    fn remove(&mut self, path: &Path, ctl: &Control) -> Result<()> {
        self.driver.run(ctl, &mut self.child, self.sftp.fs().remove_file(path).err_into())
    }

    fn rmdir(&mut self, path: &Path, ctl: &Control) -> Result<()> {
        self.driver.run(ctl, &mut self.child, self.sftp.fs().remove_dir(path).err_into())
    }
}
