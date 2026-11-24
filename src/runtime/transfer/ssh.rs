mod askpass;
mod legacy;
mod modern;
mod process;

pub(crate) use askpass::askpass_if_requested;

use super::{Control, Item, Kind, Protocol, Publication, Remote, Site, open_source, validate_name};
use anyhow::{Result, bail};
use askpass::Broker;
use process::{Driver, Group};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_ENTRIES: usize = 10_000;

struct Endpoint {
    host: String,
    username: String,
    port: Option<u16>,
    ssh: PathBuf,
    scp: PathBuf,
}

impl Endpoint {
    fn command(&self, copy: bool, driver: &Driver, control: Option<&Path>) -> Result<Command> {
        let mut command = Command::new(if copy { &self.scp } else { &self.ssh });
        command.stdin(Stdio::null()).stdout(Stdio::null());
        driver.broker.configure(&mut command)?;
        command.args(["-o", "ConnectTimeout=20", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3",
            "-o", "ControlPersist=no", "-o", "ForkAfterAuthentication=no", "-o", "ClearAllForwardings=yes",
            "-o", "PermitLocalCommand=no", "-o", "RemoteCommand=none"]);
        if let Some(socket) = control {
            let mut option = std::ffi::OsString::from("ControlPath=");
            option.push(socket);
            command.arg("-o").arg(option);
        } else {
            command.args(["-o", "ControlPath=none"]);
        }
        if let Some(port) = self.port {
            command.arg(if copy { "-P" } else { "-p" }).arg(port.to_string());
        }
        if !self.username.is_empty() {
            command.arg("-o").arg(format!("User={}", self.username));
        }
        if copy { command.arg("-S").arg(&self.ssh).args(["-O", "-q"]); }
        else { command.arg("-T"); }
        Ok(command)
    }
}

pub(super) fn connect(site: &Site, ctl: &Control) -> Result<(Box<dyn Remote>, PathBuf)> {
    site.validate()?;
    let endpoint = Endpoint {
        host: site.host.clone(), username: site.username.clone(), port: site.port,
        ssh: "ssh".into(), scp: "scp".into(),
    };
    let driver = Driver::new()?;
    let start = if site.remote_folder.as_os_str().is_empty() { Path::new(".") } else { &site.remote_folder };
    match site.protocol {
        Protocol::Sftp => modern::connect(endpoint, driver, start, ctl),
        Protocol::Scp => legacy::connect(endpoint, driver, start, ctl),
        Protocol::Ftp => bail!("Expected an SSH site"),
    }
}

#[cfg(test)]
mod tests;
