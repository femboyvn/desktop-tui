use anyhow::{Context, Result, ensure};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use zeroize::Zeroizing;

use super::Control;
use super::super::Event;

const SOCKET_VAR: &str = "DESKTOP_TUI_SSH_ASKPASS_SOCKET";
const LIMIT: usize = 8192;

pub(super) struct Broker {
    directory: tempfile::TempDir,
    listener: UnixListener,
}

impl Broker {
    pub(super) fn new() -> Result<Self> {
        // A short path also leaves room for OpenSSH's temporary multiplex socket suffix.
        let directory = tempfile::Builder::new().prefix("desktop-ssh-")
            .permissions(std::fs::Permissions::from_mode(0o700)).tempdir_in("/tmp")?;
        let listener = UnixListener::bind(directory.path().join("ask"))?;
        listener.set_nonblocking(true)?;
        Ok(Self { directory, listener })
    }

    pub(super) fn directory(&self) -> &Path {
        self.directory.path()
    }

    pub(super) fn configure(&self, command: &mut Command) -> Result<()> {
        command.env("SSH_ASKPASS", std::env::current_exe()?)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env_remove("SSH_ASKPASS_PROMPT")
            .env(SOCKET_VAR, self.directory.path().join("ask"));
        Ok(())
    }

    pub(super) fn service(&self, ctl: &Control) -> Result<bool> {
        let (mut socket, _) = match self.listener.accept() {
            Ok(pair) => pair,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        socket.set_read_timeout(Some(Duration::from_secs(2)))?;
        socket.set_write_timeout(Some(Duration::from_secs(2)))?;
        let mut mode = [0];
        socket.read_exact(&mut mode)?;
        let message = read_frame(&mut socket)?;
        let message = String::from_utf8_lossy(&message);
        let message: String = message.chars().map(|c| if c.is_control() && c != '\n' { ' ' } else { c }).collect();
        if mode[0] == 2 {
            // OpenSSH uses "none" for informational notices (e.g. touch a
            // hardware key). No credential input is involved in a notification.
            ctl.emit(Event::Status(message));
            let _ = write_frame(&mut socket, &[]);
            return Ok(true);
        }
        // OpenSSH uses confirm-mode exit status for permission prompts and ordinary
        // textual replies for host fingerprints. Treat all other challenges as secrets.
        let fingerprint = message.starts_with("The authenticity of host ") && message.contains("yes/no");
        let secret = mode[0] == 0 && !fingerprint;
        let answer = ctl.prompt(&message, secret)?;
        write_frame(&mut socket, answer.as_bytes())?;
        Ok(true)
    }
}

fn read_frame(reader: &mut impl Read) -> Result<Zeroizing<Vec<u8>>> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let len = u32::from_be_bytes(header) as usize;
    ensure!(len <= LIMIT, "SSH prompt exceeds the size limit");
    let mut bytes = Zeroizing::new(vec![0; len]);
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn write_frame(writer: &mut impl Write, data: &[u8]) -> Result<()> {
    ensure!(data.len() <= LIMIT, "SSH prompt exceeds the size limit");
    writer.write_all(&(data.len() as u32).to_be_bytes())?;
    writer.write_all(data)?;
    Ok(())
}

fn exchange(path: &OsStr, question: &OsStr, mode: u8, stdout: &mut impl Write) -> Result<i32> {
    let mut socket = UnixStream::connect(path)?;
    socket.write_all(&[mode])?;
    write_frame(&mut socket, question.as_encoded_bytes())?;
    let response = read_frame(&mut socket)?;
    if mode == 2 { return Ok(0); }
    if mode == 1 {
        return Ok(if response.eq_ignore_ascii_case(b"yes") || response.eq_ignore_ascii_case(b"y") { 0 } else { 1 });
    }
    ensure!(!response.iter().any(|b| matches!(b, b'\n' | b'\r' | 0)), "Invalid SSH prompt response");
    stdout.write_all(&response)?;
    stdout.write_all(b"\n")?;
    Ok(0)
}

fn respond(path: &OsStr) -> Result<i32> {
    let question = std::env::args_os().nth(1).context("Missing SSH prompt")?;
    let mode = match std::env::var_os("SSH_ASKPASS_PROMPT").as_deref() {
        Some(value) if value == "confirm" => 1,
        Some(value) if value == "none" => 2,
        _ => 0,
    };
    exchange(path, &question, mode, &mut std::io::stdout().lock())
}

pub(crate) fn askpass_if_requested() -> Option<i32> {
    std::env::var_os(SOCKET_VAR).map(|socket| respond(&socket).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::transfer::{Answer, PromptKind};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;

    #[test]
    fn password_and_fingerprint_prompts_use_private_socket() {
        let broker = Broker::new().unwrap();
        assert_eq!(std::fs::metadata(broker.directory()).unwrap().permissions().mode() & 0o777, 0o700);
        for (question, reply, secret) in [
            ("Key passphrase", "fixture password", true),
            ("The authenticity of host fixture is unknown. Continue (yes/no)?", "no", false),
        ] {
            let path = broker.directory().join("ask");
            let (events, received) = mpsc::channel();
            let ctl = Control::new(events);
            let ui = ctl.clone();
            let answer = std::thread::spawn(move || {
                let Event::Prompt { message, kind, .. } = received.recv_timeout(Duration::from_secs(4)).unwrap() else { panic!("Missing prompt"); };
                let mut pending = ui.shared.prompt.lock().unwrap();
                pending.answer = Some(Answer::Text(Zeroizing::new(reply.to_owned())));
                ui.shared.wake.notify_all();
                drop(pending);
                assert_eq!(message, question);
                assert_eq!(matches!(kind, PromptKind::Secret), secret);
            });
            let helper = std::thread::spawn(move || {
                let mut out = Vec::new();
                assert_eq!(exchange(path.as_os_str(), OsStr::new(question), 0, &mut out).unwrap(), 0);
                assert_eq!(out, format!("{reply}\n").as_bytes());
            });
            let deadline = std::time::Instant::now() + Duration::from_secs(4);
            while !broker.service(&ctl).unwrap() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            helper.join().unwrap();
            answer.join().unwrap();
        }
    }
}
