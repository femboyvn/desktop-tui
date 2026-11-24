use anyhow::{Context, Result, bail, ensure};
use std::future::Future;
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};
use tokio::runtime::{Builder, Runtime};

use super::{Broker, Control};

const POLL: Duration = Duration::from_millis(45);
const STALL: Duration = Duration::from_secs(90);

// The direct child stays waitable until its process group has been signalled.
// Keeping the zombie until that point also prevents PID reuse during cleanup.
pub(super) struct Group {
    pub child: Child,
    stderr: ChildStderr,
    diagnostic: Vec<u8>,
    reaped: bool,
}

impl Group {
    pub fn spawn(mut command: Command) -> Result<Self> {
        // SAFETY: setsid uses only the calling child's process state and is async-signal-safe.
        unsafe { command.pre_exec(|| {
            if libc::setsid() < 0 { return Err(io::Error::last_os_error()); }
            Ok(())
        }); }
        let mut child = command.stderr(Stdio::piped()).spawn()
            .with_context(|| format!("Could not start {}", command.get_program().to_string_lossy()))?;
        let stderr = child.stderr.take().expect("requested stderr pipe");
        let group = Self { child, stderr, diagnostic: Vec::new(), reaped: false };
        let fd = group.stderr.as_raw_fd();
        // SAFETY: the descriptor is owned by group and both fcntl calls take scalar arguments.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(group)
    }

    fn drain(&mut self) -> Result<()> {
        let mut buffer = [0; 4096];
        for _ in 0..16 {
            match self.stderr.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let keep = n.min(4096 - self.diagnostic.len());
                    self.diagnostic.extend_from_slice(&buffer[..keep]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn context(&self, error: anyhow::Error) -> anyhow::Error {
        let diagnostic = String::from_utf8_lossy(&self.diagnostic);
        let diagnostic: String = diagnostic.trim().chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        if diagnostic.is_empty() { error } else { error.context(diagnostic) }
    }

    fn finish(&mut self) -> Result<ExitStatus> {
        // SAFETY: the unreaped child is the leader of the private session/group created above.
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL); }
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if !self.reaped { let _ = self.finish(); }
    }
}

fn exited(pid: u32) -> Result<bool> {
    // SAFETY: waitid initializes the output structure. WNOWAIT leaves reaping to Group.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let status = unsafe { libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) };
    if status < 0 { return Err(io::Error::last_os_error().into()); }
    Ok(unsafe { info.si_pid() } != 0)
}

pub(super) struct Driver {
    pub runtime: Runtime,
    pub broker: Broker,
}

impl Driver {
    pub fn new() -> Result<Self> {
        Ok(Self { runtime: Builder::new_current_thread().enable_all().build()?, broker: Broker::new()? })
    }

    pub fn run<T>(&self, ctl: &Control, group: &mut Group, future: impl Future<Output = Result<T>>) -> Result<T> {
        self.monitor(ctl, group, |_| Ok(false), future)
    }

    // Progress reports and time spent answering a dialog each restart the inactivity timer.
    pub fn monitor<T>(&self, ctl: &Control, group: &mut Group, mut progress: impl FnMut(u32) -> Result<bool>,
                      future: impl Future<Output = Result<T>>) -> Result<T> {
        self.runtime.block_on(async {
            tokio::pin!(future);
            let mut last = Instant::now();
            loop {
                ctl.check()?;
                group.drain()?;
                if self.broker.service(ctl)? || progress(group.child.id())? { last = Instant::now(); }
                ensure!(last.elapsed() < STALL, "SSH operation timed out");
                tokio::select! {
                    result = &mut future => {
                        group.drain()?;
                        return result.map_err(|error| group.context(error));
                    }
                    _ = tokio::time::sleep(POLL) => {}
                }
            }
        })
    }

    pub fn wait(&self, ctl: &Control, group: &mut Group, progress: impl FnMut(u32) -> Result<bool>) -> Result<()> {
        let pid = group.child.id();
        self.monitor(ctl, group, progress, async {
            while !exited(pid)? { tokio::time::sleep(POLL).await; }
            Ok(())
        })?;
        let status = group.finish()?;
        group.drain()?;
        if !status.success() { return Err(group.context(anyhow::anyhow!("OpenSSH exited with {status}"))); }
        Ok(())
    }

    pub fn wait_for_master(&self, ctl: &Control, master: &mut Group, socket: &std::path::Path) -> Result<()> {
        let pid = master.child.id();
        self.run(ctl, master, async {
            while !socket.exists() {
                if exited(pid)? { bail!("SSH connection failed before authentication completed"); }
                tokio::time::sleep(POLL).await;
            }
            Ok(())
        })
    }
}
