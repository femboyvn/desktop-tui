use std::sync::{Arc, mpsc};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemAction {
    Lock,
    Shutdown,
    Restart,
}
impl SystemAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lock => "Screen Lock",
            Self::Shutdown => "Shutdown",
            Self::Restart => "Restart",
        }
    }
}
pub trait SystemActionRunner: Send + Sync + 'static {
    fn run(&self, action: SystemAction) -> Result<(), String>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestStatus {
    Started,
    Cancelled,
    Busy,
}
pub struct SystemActions {
    runner: Arc<dyn SystemActionRunner>,
    pending: Option<(SystemAction, mpsc::Receiver<Result<(), String>>)>,
}
impl SystemActions {
    pub fn new(runner: Arc<dyn SystemActionRunner>) -> Self {
        Self {
            runner,
            pending: None,
        }
    }
    pub fn request(
        &mut self,
        action: SystemAction,
        confirmed: bool,
    ) -> Result<RequestStatus, String> {
        if !confirmed && action != SystemAction::Lock {
            return Ok(RequestStatus::Cancelled);
        }
        if self.is_pending() {
            return Ok(RequestStatus::Busy);
        }
        let (tx, rx) = mpsc::channel();
        let runner = self.runner.clone();
        std::thread::Builder::new()
            .name("desktop-system-action".into())
            .spawn(move || {
                let result = runner.run(action);
                let _ = tx.send(result);
            })
            .map_err(|error| format!("Could not start the system request: {error}"))?;
        self.pending = Some((action, rx));
        Ok(RequestStatus::Started)
    }
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn poll(&mut self) -> Option<(SystemAction, Result<(), String>)> {
        let (action, rx) = self.pending.as_ref()?;
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("The system helper worker stopped. The outcome is uncertain; check the system before trying again.".into()),
        };
        let action = *action;
        self.pending = None;
        Some((action, result))
    }
}
#[derive(Debug, Clone)]
pub struct Session {
    pub graphical: bool,
    pub id: Option<String>,
}
impl Session {
    pub fn current() -> Self {
        let nonempty = |key| std::env::var(key).ok().filter(|value| !value.is_empty());
        Self {
            graphical: nonempty("DISPLAY").is_some() || nonempty("WAYLAND_DISPLAY").is_some(),
            id: nonempty("XDG_SESSION_ID"),
        }
    }
}
#[derive(Debug, Clone)]
pub enum HelperError {
    Unavailable(String),
    Failed(String),
    TimedOut(String),
}
impl std::fmt::Display for HelperError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => write!(formatter, "Helper unavailable: {message}"),
            Self::Failed(message) => write!(formatter, "{message}"),
            Self::TimedOut(message) => write!(
                formatter,
                "{message} timed out. The outcome is uncertain; check the system before trying again."
            ),
        }
    }
}
pub trait HelperRunner: Send + Sync + 'static {
    fn run(&self, program: &str, args: &[String], timeout: Duration) -> Result<(), HelperError>;
}
pub struct ProcessRunner;
impl HelperRunner for ProcessRunner {
    fn run(&self, program: &str, args: &[String], timeout: Duration) -> Result<(), HelperError> {
        use std::io::{Read, Seek, SeekFrom};
        use std::process::{Command, Stdio};
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::Instant;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let capture_path = std::env::temp_dir().join(format!(
            "desktop-tui-helper-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut capture = options.open(&capture_path).map_err(|error| {
            HelperError::Failed(format!("Could not capture helper output: {error}"))
        })?;
        // An unlinked file cannot block on inherited pipe handles or leave stale output files.
        std::fs::remove_file(&capture_path)
            .map_err(|error| HelperError::Failed(error.to_string()))?;
        let output = capture
            .try_clone()
            .map_err(|error| HelperError::Failed(error.to_string()))?;
        let error_output = capture
            .try_clone()
            .map_err(|error| HelperError::Failed(error.to_string()))?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(error_output)
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    HelperError::Unavailable(format!("{program}: {error}"))
                } else {
                    HelperError::Failed(format!("Could not start {program}: {error}"))
                }
            })?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Ok(None) => {
                    let killed = child.kill();
                    let reaped = child.wait();
                    let detail = match (killed, reaped) {
                        (Ok(()), Ok(_)) => program.to_owned(),
                        (kill, wait) => {
                            format!("{program} (cleanup: kill={kill:?}, wait={wait:?})")
                        }
                    };
                    return Err(HelperError::TimedOut(detail));
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(HelperError::Failed(format!(
                        "Could not monitor {program}: {error}. The outcome is uncertain."
                    )));
                }
            }
        };
        if status.success() {
            return Ok(());
        }
        let mut output = String::new();
        if capture.seek(SeekFrom::Start(0)).is_ok() {
            let mut bytes = Vec::new();
            let _ = capture.take(8192).read_to_end(&mut bytes);
            output = String::from_utf8_lossy(&bytes).trim().to_owned();
        }
        Err(HelperError::Failed(format!(
            "{program} exited with {status}: {output}"
        )))
    }
}
pub struct LinuxSystemRunner<R = ProcessRunner> {
    pub helper: R,
    pub session: Session,
}
impl<R: HelperRunner> SystemActionRunner for LinuxSystemRunner<R> {
    fn run(&self, action: SystemAction) -> Result<(), String> {
        if !cfg!(target_os = "linux") {
            return Err("System actions require Linux with systemd tooling and a graphical desktop session.".into());
        }
        let timeout = Duration::from_secs(15);
        if action == SystemAction::Lock {
            if !self.session.graphical {
                return Err("Screen Lock requires a graphical desktop session. Launch Desktop-TUI inside your Linux desktop with DISPLAY or WAYLAND_DISPLAY set and a desktop screen locker enabled.".into());
            }
            let mut args = vec!["lock-session".into()];
            if let Some(id) = &self.session.id {
                args.push(id.clone());
            }
            return match self.helper.run("loginctl",&args,timeout) {
                Ok(()) => Ok(()),
                Err(first) => self.helper.run("xdg-screensaver",&["lock".into()],timeout).map_err(|second| format!("Screen locking failed. {first}\n{second}\nEnable your desktop's screen locker and install systemd loginctl or xdg-utils.")),
            };
        }
        let verb = if action == SystemAction::Shutdown {
            "poweroff"
        } else {
            "reboot"
        };
        let args = [
            "--no-ask-password",
            "--no-wall",
            "--check-inhibitors=yes",
            verb,
        ]
        .map(str::to_owned);
        self.helper.run("systemctl",&args,timeout).map_err(|error| format!("{} request failed: {error}\nUse a local Linux systemd session with compatible systemctl tooling. Check session permissions and application inhibitors. No automatic retry was made.",action.label()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct FakeActionRunner {
        entered: mpsc::Sender<SystemAction>,
        release: Mutex<mpsc::Receiver<Result<(), String>>>,
    }
    impl SystemActionRunner for FakeActionRunner {
        fn run(&self, action: SystemAction) -> Result<(), String> {
            self.entered.send(action).unwrap();
            self.release.lock().unwrap().recv().unwrap()
        }
    }
    #[test]
    fn cancellation_never_runs_and_pending_requests_do_not_duplicate() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut actions = SystemActions::new(Arc::new(FakeActionRunner {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        }));
        assert_eq!(
            actions.request(SystemAction::Shutdown, false).unwrap(),
            RequestStatus::Cancelled
        );
        assert!(entered_rx.try_recv().is_err());
        assert_eq!(
            actions.request(SystemAction::Restart, true).unwrap(),
            RequestStatus::Started
        );
        assert_eq!(
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            SystemAction::Restart
        );
        assert!(actions.is_pending());
        assert_eq!(
            actions.request(SystemAction::Lock, true).unwrap(),
            RequestStatus::Busy
        );
        release_tx.send(Err("permission denied".into())).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let result = loop {
            if let Some(result) = actions.poll() {
                break result;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(
            result,
            (SystemAction::Restart, Err("permission denied".into()))
        );
        assert!(!actions.is_pending());
        assert!(entered_rx.try_recv().is_err());
    }
    struct FakeHelper {
        outcomes: Mutex<std::collections::VecDeque<Result<(), HelperError>>>,
        calls: Mutex<Vec<(String, Vec<String>, Duration)>>,
    }
    impl HelperRunner for FakeHelper {
        fn run(
            &self,
            program: &str,
            args: &[String],
            timeout: Duration,
        ) -> Result<(), HelperError> {
            self.calls
                .lock()
                .unwrap()
                .push((program.into(), args.to_vec(), timeout));
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected retry")
        }
    }
    fn fake(
        outcomes: Vec<Result<(), HelperError>>,
        graphical: bool,
        id: Option<&str>,
    ) -> LinuxSystemRunner<FakeHelper> {
        LinuxSystemRunner {
            helper: FakeHelper {
                outcomes: Mutex::new(outcomes.into()),
                calls: Mutex::new(vec![]),
            },
            session: Session {
                graphical,
                id: id.map(str::to_owned),
            },
        }
    }
    #[test]
    fn power_requests_check_inhibitors_and_never_retry_failures_or_timeouts() {
        for (action, verb) in [
            (SystemAction::Shutdown, "poweroff"),
            (SystemAction::Restart, "reboot"),
        ] {
            for failure in [
                HelperError::Failed("permission denied".into()),
                HelperError::Unavailable("systemctl".into()),
                HelperError::TimedOut("systemctl".into()),
            ] {
                let runner = fake(vec![Err(failure)], true, None);
                assert!(runner.run(action).is_err());
                let calls = runner.helper.calls.lock().unwrap();
                assert_eq!(calls.len(), 1);
                assert_eq!(
                    calls[0],
                    (
                        "systemctl".into(),
                        vec![
                            "--no-ask-password".into(),
                            "--no-wall".into(),
                            "--check-inhibitors=yes".into(),
                            verb.into()
                        ],
                        Duration::from_secs(15)
                    )
                );
            }
        }
    }
    #[test]
    fn graphical_lock_uses_session_and_falls_back_when_unavailable_or_unsuccessful() {
        for failure in [
            HelperError::Unavailable("loginctl".into()),
            HelperError::Failed("no session".into()),
        ] {
            let runner = fake(vec![Err(failure), Ok(())], true, Some("42"));
            runner.run(SystemAction::Lock).unwrap();
            let calls = runner.helper.calls.lock().unwrap();
            assert_eq!(calls[0].0, "loginctl");
            assert_eq!(calls[0].1, vec!["lock-session", "42"]);
            assert_eq!(calls[1].0, "xdg-screensaver");
            assert_eq!(calls[1].1, vec!["lock"]);
        }
        let runner = fake(vec![Ok(())], true, None);
        runner.run(SystemAction::Lock).unwrap();
        assert_eq!(
            runner.helper.calls.lock().unwrap()[0].1,
            vec!["lock-session"]
        );
        let runner = fake(vec![], false, Some("42"));
        assert!(
            runner
                .run(SystemAction::Lock)
                .unwrap_err()
                .contains("graphical")
        );
        assert!(runner.helper.calls.lock().unwrap().is_empty());
    }
    #[test]
    fn lock_reports_both_helper_failures() {
        let runner = fake(
            vec![
                Err(HelperError::Unavailable("loginctl".into())),
                Err(HelperError::Failed("no locker".into())),
            ],
            true,
            None,
        );
        let error = runner.run(SystemAction::Lock).unwrap_err();
        assert!(error.contains("loginctl"));
        assert!(error.contains("no locker"));
    }
    #[test]
    fn process_runner_captures_failure_and_reports_missing_command() {
        assert!(matches!(
            ProcessRunner.run(
                "/definitely/missing/desktop-tui-helper",
                &[],
                Duration::from_secs(1)
            ),
            Err(HelperError::Unavailable(_))
        ));
        let args = vec!["-c".into(), "printf 'permission denied' >&2; exit 1".into()];
        match ProcessRunner.run("/bin/sh", &args, Duration::from_secs(1)) {
            Err(HelperError::Failed(error)) => assert!(error.contains("permission denied")),
            result => panic!("{result:?}"),
        }
    }
    #[test]
    fn timed_out_helpers_are_terminated_and_reaped() {
        let dir = crate::test_support::TestDir::new();
        let pidfile = dir.path().join("pid");
        let args = vec![
            "-c".into(),
            "echo $$ > \"$1\"; while :; do :; done".into(),
            "helper".into(),
            pidfile.to_string_lossy().into_owned(),
        ];
        let started = std::time::Instant::now();
        assert!(matches!(
            ProcessRunner.run("/bin/sh", &args, Duration::from_millis(150)),
            Err(HelperError::TimedOut(_))
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        let pid = std::fs::read_to_string(pidfile).unwrap();
        assert!(!std::path::Path::new("/proc").join(pid.trim()).exists());
    }
}
