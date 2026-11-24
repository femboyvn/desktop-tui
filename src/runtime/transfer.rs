//! File Transfer owns one remote connection and one outstanding operation per window.
mod copy;
mod ftp;
mod sites;
mod ssh;
pub(crate) use ssh::askpass_if_requested;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak, mpsc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) enum Protocol { Ftp, #[default] Sftp, Scp }
impl Protocol {
    pub fn label(self) -> &'static str {
        match self { Self::Ftp => "FTP (unencrypted)", Self::Sftp => "Modern OpenSSH (SFTP)", Self::Scp => "Legacy SCP" }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Site {
    pub name: String, pub protocol: Protocol, pub host: String, pub port: Option<u16>,
    pub username: String, pub local_folder: PathBuf, pub remote_folder: PathBuf,
}
impl Site {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.host.is_empty() && self.host.len() <= 253 && !self.host.starts_with('-')
            && self.host.bytes().all(|b| b.is_ascii_alphanumeric() || b".:_-%".contains(&b)),
            "Enter a hostname, IP address, or SSH alias");
        ensure!(self.port != Some(0), "Port must be between 1 and 65535");
        ensure!(self.name.len() <= 256 && !self.name.chars().any(char::is_control), "Invalid site name");
        ensure!(self.username.len() <= 256 && !self.username.chars().any(char::is_control), "Invalid username");
        if self.protocol != Protocol::Ftp {
            ensure!(!self.username.starts_with('-') && self.username.bytes().all(|b| b.is_ascii_alphanumeric() || b"@_.-\\".contains(&b)), "Invalid SSH username");
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side { Local, Remote }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind { File, Directory, Other }
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item { pub name: OsString, pub kind: Kind, pub size: Option<u64> }

pub(crate) enum Command { Connect(Site), Disconnect, List(Side, PathBuf), Copy { from: Side, names: Vec<OsString> }, SaveSites(Vec<Site>) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromptKind { Secret, Text, Conflict }
pub(crate) enum Answer { Text(Zeroizing<String>), Replace, Skip, Cancel }
pub(crate) enum Event {
    Listing(Side, PathBuf, Vec<Item>), Connected(String), Disconnected, Sites(Vec<Site>),
    Status(String), Busy(bool), Prompt { id:u64, message:String, kind:PromptKind },
    Progress { file:String, files:u64, bytes:u64 }, Done(String), Error(String),
}

#[derive(Debug, PartialEq, Eq)]
enum Publication { Moved, Linked }

trait Remote {
    // FTP servers may expose links as directories. RFC 3659 Unique facts identify aliases.
    fn directory_ids(&self) -> Option<&std::collections::HashMap<OsString,String>> { None }
    fn list(&mut self, path:&Path, ctl:&Control) -> Result<Vec<Item>>;
    fn mkdir(&mut self, path:&Path, ctl:&Control) -> Result<()>;
    fn download(&mut self, remote:&Path, local:&Path, ctl:&Control) -> Result<()>;
    fn upload(&mut self, local:&Path, remote:&Path, ctl:&Control) -> Result<()>;
    fn publish(&mut self, from:&Path, to:&Path, replace:bool, ctl:&Control) -> Result<Publication>;
    fn remove(&mut self, path:&Path, ctl:&Control) -> Result<()>;
    fn rmdir(&mut self, path:&Path, ctl:&Control) -> Result<()>;
}

fn validate_name(name: &OsStr) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    ensure!(name.len() <= 4096, "Directory entry name exceeds 4096 bytes");
    ensure!(Path::new(name).file_name() == Some(name) && !name.as_bytes().contains(&0),
        "Invalid directory entry name");
    Ok(())
}
fn open_source(path: &Path) -> Result<File> {
    let file = File::options().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path)?;
    ensure!(file.metadata()?.is_file(), "Source is no longer a regular file");
    Ok(file)
}

#[derive(Default)]
struct Progress { file:String, files:u64, bytes:u64, dirty:bool }
#[derive(Default)]
struct Interaction { id:u64, waiting:bool, answer:Option<Answer> }
struct Shared {
    canceled:AtomicBool, closed:AtomicBool, progress:Mutex<Progress>, prompt:Mutex<Interaction>, wake:Condvar,
}
#[derive(Clone)]
struct Control { shared:Arc<Shared>, events:mpsc::Sender<Event>, cleanup_until:Option<std::time::Instant> }
impl Control {
    fn new(events:mpsc::Sender<Event>) -> Self {
        Self { events, cleanup_until:None, shared:Arc::new(Shared { canceled:false.into(),closed:false.into(),progress:Mutex::default(),prompt:Mutex::default(),wake:Condvar::new() }) }
    }
    #[cfg(test)]
    fn for_test() -> Self { Self::new(mpsc::channel().0) }
    fn cleanup(&self) -> Self {
        let mut control = Self::new(self.events.clone());
        control.cleanup_until = Some(std::time::Instant::now() + Duration::from_secs(3));
        control
    }
    fn check(&self) -> Result<()> {
        ensure!(self.cleanup_until.is_none_or(|time| std::time::Instant::now() < time), "Cleanup timed out");
        ensure!(!self.shared.canceled.load(Ordering::Acquire) && !self.shared.closed.load(Ordering::Acquire), "Transfer canceled");
        Ok(())
    }
    fn emit(&self, event:Event) { let _ = self.events.send(event); }
    fn progress(&self, bytes:u64) {
        let mut p = self.shared.progress.lock().unwrap();
        p.bytes = p.bytes.saturating_add(bytes); p.dirty = true;
    }
    fn ask(&self, message:&str, kind:PromptKind) -> Result<Answer> {
        ensure!(self.cleanup_until.is_none(),"Cleanup requires a new login");
        let mut pending = self.shared.prompt.lock().unwrap();
        self.check()?;
        ensure!(!pending.waiting, "Another login or copy prompt is pending");
        pending.id += 1; pending.waiting = true; pending.answer = None;
        if self.events.send(Event::Prompt { id:pending.id,message:message.into(),kind }).is_err() {
            pending.waiting = false; bail!("Transfer window closed");
        }
        while pending.answer.is_none() && self.check().is_ok() {
            pending = self.shared.wake.wait(pending).unwrap();
        }
        pending.waiting = false;
        self.check()?;
        match pending.answer.take() {
            None | Some(Answer::Cancel) => bail!("Transfer canceled"),
            Some(answer) => Ok(answer),
        }
    }
    fn prompt(&self, message:&str, secret:bool) -> Result<Zeroizing<String>> {
        match self.ask(message,if secret { PromptKind::Secret } else { PromptKind::Text })? {
            Answer::Text(value) => Ok(value), _ => bail!("Login canceled"),
        }
    }
    fn cancel(&self) {
        let _pending = self.shared.prompt.lock().unwrap();
        self.shared.canceled.store(true,Ordering::Release); self.shared.wake.notify_all();
    }
    fn close(&self) { self.shared.closed.store(true,Ordering::Release); self.cancel(); }
}

struct Worker {
    remote:Option<Box<dyn Remote>>, local_path:PathBuf, remote_path:PathBuf, sites:Vec<Site>, site_path:PathBuf,
}
impl Worker {
    fn list(&mut self, side:Side, path:PathBuf, ctl:&Control) -> Result<()> {
        let (path,items) = match side {
            Side::Local => { let path = path.canonicalize()?; let items = copy::local_list(&path,ctl)?; (path,items) },
            Side::Remote => {
                ensure!(path.is_absolute(), "Expected an absolute remote folder");
                let items = self.remote.as_mut().context("Connect to a site first")?.list(&path,ctl)?;
                (path,items)
            }
        };
        let items = copy::checked_listing(items)?;
        match side { Side::Local => self.local_path = path.clone(), Side::Remote => self.remote_path = path.clone() }
        ctl.emit(Event::Listing(side,path,items));
        Ok(())
    }
    fn run(&mut self, cmd:Command, ctl:&Control) -> Result<()> {
        match cmd {
            Command::List(side,path) => self.list(side,path,ctl),
            Command::Disconnect => {
                self.remote = None; self.remote_path.clear(); ctl.emit(Event::Disconnected); Ok(())
            }
            Command::SaveSites(sites) => {
                sites::save(&self.site_path,&self.sites,&sites)?;
                self.sites = sites; ctl.emit(Event::Sites(self.sites.clone())); Ok(())
            }
            Command::Connect(site) => {
                site.validate()?; ctl.check()?;
                let (mut remote,path) = match site.protocol {
                    Protocol::Ftp => ftp::connect(&site,ctl)?, _ => ssh::connect(&site,ctl)?,
                };
                ensure!(path.is_absolute(), "Server returned a relative start folder");
                let items = copy::checked_listing(remote.list(&path,ctl)?)?;
                if !site.local_folder.as_os_str().is_empty() { self.list(Side::Local,site.local_folder.clone(),ctl)?; }
                ctl.check()?;
                self.remote_path = path.clone(); self.remote = Some(remote);
                ctl.emit(Event::Connected(format!("{} — {}", site.host,site.protocol.label())));
                ctl.emit(Event::Listing(Side::Remote,path,items)); Ok(())
            }
            Command::Copy { from,names } => {
                let remote = self.remote.as_mut().context("Connect to a site first")?;
                ensure!(!self.local_path.as_os_str().is_empty(), "Choose a local folder first");
                ctl.emit(Event::Status("Checking selection…".into()));
                let result = copy::batch(from,names,&self.local_path,&self.remote_path,remote.as_mut(),ctl);
                let (side,path) = match from { Side::Remote => (Side::Local,self.local_path.clone()), Side::Local => (Side::Remote,self.remote_path.clone()) };
                // Do not open another connection or prompt when the user has canceled.
                if ctl.check().is_err() {
                    let summary = result.unwrap_or_else(|e|format!("{e:#}"));
                    bail!("{summary} Refresh the destination folder to see completed files.");
                }
                let refresh = self.list(side,path,ctl);
                match (result,refresh) {
                    (Ok(summary),Ok(())) => { ctl.emit(Event::Done(summary)); Ok(()) },
                    (Err(error),Ok(())) => Err(error),
                    (result,Err(error)) => {
                        let summary = result.unwrap_or_else(|e|format!("{e:#}"));
                        bail!("{summary} Could not refresh the destination: {error:#}");
                    }
                }
            }
        }
    }
}

type Registry = Mutex<Vec<(Weak<Shared>,JoinHandle<()>)>>;
static WORKERS:OnceLock<Registry> = OnceLock::new();
pub(crate) fn finish_shutdown() {
    let jobs = WORKERS.get().map(|registry| std::mem::take(&mut *registry.lock().unwrap())).unwrap_or_default();
    for (control,_) in &jobs {
        if let Some(shared) = control.upgrade() {
            let _pending = shared.prompt.lock().unwrap();
            shared.closed.store(true,Ordering::Release); shared.wake.notify_all();
        }
    }
    for (_,job) in jobs { let _ = job.join(); }
}

pub(crate) struct TransferService {
    ctl:Control, commands:mpsc::SyncSender<Command>, events:mpsc::Receiver<Event>, outstanding:usize,
}
impl TransferService {
    pub fn new() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("com","Julien-cpsn","desktop-tui").context("No project configuration directory")?;
        let home = directories::BaseDirs::new().context("No home directory")?.home_dir().to_owned();
        Self::at(dirs.config_dir().join("file-transfer/sites.json"),home)
    }
    pub(crate) fn at(site_path:PathBuf, home:PathBuf) -> Result<Self> {
        let (out,events) = mpsc::channel();
        let ctl = Control::new(out);
        let (commands,receiver) = mpsc::sync_channel(2);
        let control = ctl.clone();
        let mut jobs = WORKERS.get_or_init(Mutex::default).lock().unwrap();
        let job = std::thread::Builder::new().name("file-transfer".into()).spawn(move || {
            let ctl = control;
            let mut worker = Worker { remote:None, local_path:PathBuf::new(),remote_path:PathBuf::new(),site_path,sites:Vec::new() };
            match sites::load(&worker.site_path) {
                Ok(sites) => { worker.sites = sites.clone(); ctl.emit(Event::Sites(sites)); },
                Err(e) => ctl.emit(Event::Error(format!("Cannot load sites: {e:#}"))),
            }
            if let Err(e) = worker.list(Side::Local,home,&ctl) { ctl.emit(Event::Error(format!("Cannot list home directory: {e:#}"))); }
            ctl.emit(Event::Busy(false));
            loop {
                let cmd = match receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(cmd) => cmd,
                    Err(mpsc::RecvTimeoutError::Timeout) if !ctl.shared.closed.load(Ordering::Acquire) => continue,
                    Err(_) => break,
                };
                if let Err(e) = worker.run(cmd,&ctl) { ctl.emit(Event::Error(format!("{e:#}"))); }
                ctl.emit(Event::Busy(false));
                if ctl.shared.closed.load(Ordering::Acquire) { break; }
            }
        })?;
        jobs.retain(|(_,job)| !job.is_finished()); jobs.push((Arc::downgrade(&ctl.shared),job));
        Ok(Self { ctl,commands,events,outstanding:1 })
    }
    pub fn submit(&mut self, cmd:Command) -> Result<()> {
        ensure!(!self.ctl.shared.closed.load(Ordering::Acquire),"File Transfer is closed");
        if self.outstanding != 0 {
            ensure!(matches!(cmd,Command::Disconnect) && self.outstanding == 1,"File Transfer is busy");
            self.cancel();
        } else {
            self.ctl.shared.canceled.store(false,Ordering::Release);
            *self.ctl.shared.progress.lock().unwrap() = Progress::default();
        }
        self.commands.try_send(cmd).map_err(|_| anyhow::anyhow!("File Transfer worker unavailable"))?;
        self.outstanding += 1;
        Ok(())
    }
    pub fn poll(&mut self) -> Option<Event> {
        if let Ok(event) = self.events.try_recv() {
            if matches!(event,Event::Busy(false)) {
                self.outstanding = self.outstanding.saturating_sub(1);
                self.ctl.shared.progress.lock().unwrap().dirty = false;
                return Some(Event::Busy(self.outstanding != 0));
            }
            return Some(event);
        }
        let mut p = self.ctl.shared.progress.lock().unwrap();
        if !p.dirty { return None; }
        p.dirty = false;
        Some(Event::Progress { file:p.file.clone(),files:p.files,bytes:p.bytes })
    }
    pub fn answer(&self, id:u64, answer:Answer) {
        let mut pending = self.ctl.shared.prompt.lock().unwrap();
        if pending.waiting && pending.id == id { pending.answer = Some(answer); self.ctl.shared.wake.notify_all(); }
    }
    pub fn cancel(&self) { self.ctl.cancel(); }
    pub fn shutdown(&self) { self.ctl.close(); }
}
impl Drop for TransferService { fn drop(&mut self) { self.shutdown(); } }

#[cfg(test)]
mod tests;
