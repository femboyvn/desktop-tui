//! Passive, binary FTP. All entry points run on the File Transfer worker.
mod deadline;
use deadline::Deadline;
use super::*;
use anyhow::anyhow;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Instant;
use suppaftp::list::ListParser;
use suppaftp::{FtpError, FtpResult, FtpStream, Mode, Status, TransferStream, TlsStream};

const TIMEOUT: Duration = Duration::from_secs(8);
const LIST_BYTES: usize = 4 * 1024 * 1024;
const LIST_ENTRIES: usize = 10_000;
const OPEN_DATA: &[Status] = &[Status::AlreadyOpen, Status::AboutToSend];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Listing { Mlsd, List }

struct Ftp {
    stream: FtpStream,
    listing: Listing,
    deadline: Deadline,
    directory_ids: HashMap<OsString, String>,
}

fn argument(value: &str) -> Result<&str> {
    ensure!(!value.chars().any(char::is_control), "FTP arguments cannot contain control characters");
    Ok(value)
}

fn pathname(path: &Path) -> Result<&str> {
    argument(path.to_str().context("FTP requires UTF-8 pathnames")?)
}

fn socket_timeouts(socket: &TcpStream, interval: Duration) -> io::Result<()> {
    socket.set_read_timeout(Some(interval))?;
    socket.set_write_timeout(Some(interval))
}

fn dial(site: &Site, ctl: &Control) -> Result<TcpStream> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let result = runtime.block_on(async {
        let dial = tokio::time::timeout(TIMEOUT, tokio::net::TcpStream::connect((site.host.as_str(), site.port.unwrap_or(21))));
        tokio::pin!(dial);
        loop {
            ctl.check()?;
            tokio::select! {
                result = &mut dial => return Ok::<_,anyhow::Error>(result.context("FTP connection timed out")??.into_std()?),
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
        }
    });
    // A system resolver may still be returning after cancellation.
    runtime.shutdown_background();
    let socket = result.context("Cannot connect to FTP server")?;
    socket.set_nonblocking(false)?;
    socket_timeouts(&socket, TIMEOUT)?;
    Ok(socket)
}

// A reply body is controlled by the server and may echo credentials.
fn ftp_error(error: FtpError) -> anyhow::Error {
    match error {
        FtpError::UnexpectedResponse(reply) => anyhow!("FTP {} ({})", reply.status.code(), reply.status),
        other => anyhow!(other),
    }
}

fn negative_reply(error: &FtpError) -> bool {
    matches!(error, FtpError::UnexpectedResponse(r) if (400..600).contains(&r.status.code())
        && !matches!(r.status.code(), 421 | 426))
}

pub(super) fn connect(site: &Site, ctl: &Control) -> Result<(Box<dyn Remote>, PathBuf)> {
    ctl.check()?;
    let user = if site.username.is_empty() { "anonymous" } else { argument(&site.username)? };
    let password = if user.eq_ignore_ascii_case("anonymous") {
        Zeroizing::new("anonymous@".to_owned())
    } else {
        ctl.prompt("FTP sends this password in clear text. Enter password", true)?
    };
    argument(&password)?;
    let socket = dial(site, ctl)?;
    let peer = socket.peer_addr()?;
    let deadline = Deadline::new(&socket)?;
    let mut stream = deadline.during(Some(ctl), || FtpStream::connect_with_stream(socket)).map_err(ftp_error)?
        .passive_stream_builder(|address| {
            let socket = TcpStream::connect_timeout(&address, TIMEOUT).map_err(FtpError::ConnectionError)?;
            socket_timeouts(&socket, Duration::from_millis(250)).map_err(FtpError::ConnectionError)?;
            Ok(socket)
        });
    stream.set_passive_nat_workaround(true);
    if peer.is_ipv6() { stream.set_mode(Mode::ExtendedPassive); }
    ctl.check()?;
    deadline.during(Some(ctl), || stream.login(user, &password)).map_err(ftp_error).context("FTP login failed")?;
    drop(password);
    let mut client = Ftp { stream, listing: Listing::Mlsd, deadline, directory_ids: HashMap::new() };
    client.command(ctl, |ftp| ftp.transfer_type(suppaftp::types::FileType::Binary)).map_err(ftp_error)?;
    if !site.remote_folder.as_os_str().is_empty() {
        let path = pathname(&site.remote_folder)?;
        client.command(ctl, |ftp| ftp.cwd(path)).map_err(ftp_error).context("Cannot enter FTP start folder")?;
    }
    ctl.check()?;
    let response = client.command(ctl, |ftp| ftp.custom_command("PWD", &[Status::PathCreated])).map_err(ftp_error)?;
    let folder = working_directory(&response.body)?;
    Ok((Box::new(client), folder))
}

fn working_directory(reply: &[u8]) -> Result<PathBuf> {
    // RFC 959 doubles quotes embedded in the directory name.
    let line = std::str::from_utf8(reply)?.lines().last().context("Empty FTP PWD reply")?;
    let mut chars = line.strip_prefix("257 \"").context("Invalid FTP PWD reply")?.chars().peekable();
    let mut path = String::new();
    while let Some(ch) = chars.next() {
        if ch == '"' && chars.next_if_eq(&'"').is_none() {
            let folder = PathBuf::from(argument(&path)?);
            ensure!(folder.is_absolute(), "FTP server returned a relative working directory");
            return Ok(folder);
        }
        path.push(ch);
    }
    bail!("Invalid quoted FTP working directory")
}

impl Ftp {
    fn invalidate(&self) { let _ = self.stream.get_ref().shutdown(Shutdown::Both); }

    fn command<T>(&mut self, ctl: &Control, call: impl FnOnce(&mut FtpStream) -> FtpResult<T>) -> FtpResult<T> {
        let result = self.deadline.during(Some(ctl), || call(&mut self.stream));
        if let Err(error) = &result && !negative_reply(error) {
            self.invalidate();
            if ctl.check().is_err() {
                return Err(FtpError::ConnectionError(io::Error::new(io::ErrorKind::Interrupted, "FTP operation canceled")));
            }
        }
        result
    }

    // Even on early cancellation, close data and consume its completion reply. A failed finish
    // can leave an extra 226 or partial reply pending, so abandon that control session.
    fn finish<T: TlsStream, R>(&self, stream: TransferStream<T>, payload: Result<R>) -> Result<R> {
        if let Err(error) = self.deadline.during(None, || stream.finish()) {
            self.invalidate();
            let failure = ftp_error(error);
            return Err(payload.err().unwrap_or(failure).context("FTP transfer interrupted. Reconnect to continue"));
        }
        payload
    }
}

fn data_io<T>(ctl: &Control, mut op: impl FnMut() -> io::Result<T>) -> Result<T> {
    let start = Instant::now();
    loop {
        ctl.check()?;
        match op() {
            Err(e) if matches!(e.kind(), io::ErrorKind::Interrupted | io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {
                ensure!(start.elapsed() < TIMEOUT, "FTP data connection timed out");
            }
            result => return Ok(result?),
        }
    }
}

fn read_listing(stream: &mut impl Read, ctl: &Control) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buf = [0; 16 * 1024];
    loop {
        let n = data_io(ctl, || stream.read(&mut buf))?;
        if n == 0 { break; }
        ensure!(bytes.len() + n <= LIST_BYTES, "FTP listing exceeds {LIST_BYTES} bytes");
        bytes.extend_from_slice(&buf[..n]);
    }
    Ok(bytes)
}

fn item(name: &str, kind: Kind, size: Option<u64>) -> Result<Option<Item>> {
    if matches!(name, "." | "..") { return Ok(None); }
    validate_name(OsStr::new(argument(name)?))?;
    Ok(Some(Item { name: name.into(), kind, size }))
}

fn machine_entry(line: &str, directories: &mut HashMap<OsString, String>) -> Result<Option<Item>> {
    let (facts, name) = line.split_once(' ').context("Malformed MLSD entry")?;
    ensure!(facts.is_empty() || facts.ends_with(';'), "Malformed MLSD facts");
    let mut entry_type = "";
    let mut unique = None;
    let mut size = None;
    for fact in facts.split_terminator(';') {
        let (key, value) = fact.split_once('=').context("Malformed MLSD fact")?;
        if key.eq_ignore_ascii_case("type") { entry_type = value; }
        else if key.eq_ignore_ascii_case("unique") && !value.is_empty() { unique = Some(value); }
        else if key.eq_ignore_ascii_case("size") { size = Some(value.parse().context("Invalid MLSD size")?); }
    }
    if entry_type.eq_ignore_ascii_case("pdir") { return Ok(None); }
    let current = entry_type.eq_ignore_ascii_case("cdir");
    let kind = if current || entry_type.eq_ignore_ascii_case("dir") { Kind::Directory }
        else if entry_type.eq_ignore_ascii_case("file") { Kind::File } else { Kind::Other };
    if kind == Kind::Directory && let Some(id) = unique {
        let key = if current { "." } else { name };
        ensure!(directories.insert(key.into(), id.to_owned()).is_none(), "Duplicate directory identity in MLSD listing");
    }
    if current { return Ok(None); }
    item(name, kind, size)
}

fn legacy_entry(line: &str) -> Result<Option<Item>> {
    if line.strip_prefix("total ").is_some_and(|n| n.parse::<u64>().is_ok()) { return Ok(None); }
    if line.starts_with(['-', 'd', 'l', 'b', 'c', 'p', 's']) {
        // A long Unix listing has eight metadata fields. Consume exactly one separator after
        // the date so that leading spaces in the actual name survive the round trip.
        let mut rest = line;
        let mut columns = [""; 8];
        for column in &mut columns {
            let (field, tail) = rest.trim_start_matches([' ', '\t']).split_once([' ', '\t']).context("Malformed Unix FTP listing")?;
            *column = field;
            rest = tail;
        }
        ensure!(columns[0].len() >= 10 && columns[1].parse::<u64>().is_ok(), "Malformed Unix FTP listing");
        let kind = match columns[0].as_bytes()[0] { b'-' => Kind::File, b'd' => Kind::Directory, _ => Kind::Other };
        let name = if columns[0].starts_with('l') && let Some((name, target)) = rest.split_once(" -> ") {
            ensure!(!target.contains(" -> "), "Ambiguous symlink name in Unix FTP listing");
            name
        } else { rest };
        return item(name, kind, columns[4].parse::<u64>().ok());
    }
    let file = ListParser::parse_dos(line).context("Unrecognized FTP directory listing")?;
    let kind = if file.is_directory() { Kind::Directory } else { Kind::File };
    item(file.name(), kind, Some(file.size() as u64))
}

fn copy_stream(src: &mut impl Read, dst: &mut impl Write, ctl: &Control) -> Result<()> {
    let mut buf = [0; 64 * 1024];
    loop {
        let n = data_io(ctl, || src.read(&mut buf))?;
        if n == 0 { return Ok(()); }
        let mut chunk = &buf[..n];
        while !chunk.is_empty() {
            let written = data_io(ctl, || dst.write(chunk))?;
            ensure!(written != 0, "FTP copy could not write the next chunk");
            ctl.progress(written as u64);
            chunk = &chunk[written..];
        }
    }
}

impl Remote for Ftp {
    fn directory_ids(&self) -> Option<&HashMap<OsString, String>> {
        (!self.directory_ids.is_empty()).then_some(&self.directory_ids)
    }

    fn list(&mut self, path: &Path, ctl: &Control) -> Result<Vec<Item>> {
        ctl.check()?;
        let path = pathname(path)?;
        // CWD accepts a literal directory. Some legacy LIST implementations expand path globs.
        self.command(ctl, |ftp| ftp.cwd(path)).map_err(ftp_error)?;
        let mut stream = loop {
            ctl.check()?;
            let verb = match self.listing { Listing::Mlsd => "MLSD", Listing::List => "LIST" };
            match self.command(ctl, |ftp| ftp.custom_data_command(verb, OPEN_DATA)) {
                Ok((_, stream)) => break stream,
                Err(FtpError::UnexpectedResponse(r)) if self.listing == Listing::Mlsd
                    && matches!(r.status.code(), 500 | 501 | 502 | 504) => self.listing = Listing::List,
                Err(error) => return Err(ftp_error(error)),
            }
        };
        let payload = read_listing(&mut stream, ctl);
        let bytes = self.finish(stream, payload)?;
        let mut entries = Vec::new();
        let mut directories = HashMap::new();
        for line in std::str::from_utf8(&bytes).context("FTP listing contains a non-UTF-8 name")?.lines() {
            ctl.check()?;
            if line.is_empty() { continue; }
            let entry = match self.listing { Listing::Mlsd => machine_entry(line, &mut directories)?, _ => legacy_entry(line)? };
            if let Some(entry) = entry {
                ensure!(entries.len() < LIST_ENTRIES, "FTP listing exceeds {LIST_ENTRIES} entries");
                entries.push(entry);
            }
        }
        self.directory_ids = directories;
        Ok(entries)
    }

    fn mkdir(&mut self, path: &Path, ctl: &Control) -> Result<()> {
        ctl.check()?;
        let path = pathname(path)?;
        self.command(ctl, |ftp| ftp.mkdir(path)).map_err(ftp_error)
    }

    fn download(&mut self, remote: &Path, local: &Path, ctl: &Control) -> Result<()> {
        ctl.check()?;
        let remote = pathname(remote)?;
        let mut file = File::options().write(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(local)?;
        ensure!(file.metadata()?.is_file(), "Download staging file is no longer a regular file");
        file.set_len(0)?;
        let mut data = self.command(ctl, |ftp| ftp.retr_as_stream(remote)).map_err(ftp_error)?;
        let result = copy_stream(&mut data, &mut file, ctl);
        self.finish(data, result)
    }

    fn upload(&mut self, local: &Path, remote: &Path, ctl: &Control) -> Result<()> {
        ctl.check()?;
        let remote = pathname(remote)?;
        let mut file = open_source(local)?;
        let mut data = self.command(ctl, |ftp| ftp.put_with_stream(remote)).map_err(ftp_error)?;
        let result = copy_stream(&mut file, &mut data, ctl);
        self.finish(data, result)
    }

    fn publish(&mut self, from: &Path, to: &Path, replace: bool, ctl: &Control) -> Result<Publication> {
        ctl.check()?;
        let source = pathname(from)?;
        let destination = pathname(to)?;
        if !replace {
            let name = to.file_name().context("Missing destination name")?;
            let entries = self.list(to.parent().context("Missing destination directory")?, ctl)?;
            ensure!(!entries.iter().any(|entry| entry.name == name), "FTP destination already exists");
            ensure!(self.listing != Listing::List || !name.to_string_lossy().starts_with('.'),
                "FTP server cannot confirm whether the hidden destination exists");
        }
        ctl.check()?;
        self.command(ctl, |ftp| ftp.custom_command(format!("RNFR {source}"), &[Status::RequestFilePending])).map_err(ftp_error)?;
        ctl.check()?;
        match self.command(ctl, |ftp| ftp.custom_command(format!("RNTO {destination}"), &[Status::RequestedFileActionOk, Status::CommandOk])) {
            Ok(_) => Ok(Publication::Moved),
            Err(e) if negative_reply(&e) => Err(ftp_error(e).context("FTP server refused rename")),
            Err(e) => Err(ftp_error(e).context("FTP rename outcome unknown. Reconnect and inspect the destination")),
        }
    }

    fn remove(&mut self, path: &Path, ctl: &Control) -> Result<()> {
        ctl.check()?;
        let path = pathname(path)?;
        self.command(ctl, |ftp| ftp.rm(path)).map_err(ftp_error)
    }

    fn rmdir(&mut self, path: &Path, ctl: &Control) -> Result<()> {
        ctl.check()?;
        let path = pathname(path)?;
        self.command(ctl, |ftp| ftp.rmdir(path)).map_err(ftp_error)
    }
}

#[cfg(test)]
mod tests;
