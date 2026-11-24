use super::*;
use std::time::Instant;

// SO_RCVTIMEO limits an individual read. A slowly trickling FTP reply could otherwise keep
// the library inside a single control command indefinitely, even after the window closed.
type Limit = (Instant, Option<Control>);
#[derive(Default)]
struct State { active: Option<Limit>, stopped: bool }

pub(super) struct Deadline {
    state: Arc<(Mutex<State>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl Deadline {
    pub(super) fn new(socket: &TcpStream) -> io::Result<Self> {
        let socket = socket.try_clone()?;
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let watch = state.clone();
        let worker = std::thread::Builder::new().name("ftp-control-timeout".into()).spawn(move || {
            let (lock, wake) = &*watch;
            let mut state = lock.lock().unwrap();
            while !state.stopped {
                state = if let Some((until, ctl)) = &state.active {
                    let remaining = until.saturating_duration_since(Instant::now());
                    if remaining.is_zero() || ctl.as_ref().is_some_and(|ctl| ctl.check().is_err()) {
                        let _ = socket.shutdown(Shutdown::Both);
                        break;
                    }
                    wake.wait_timeout(state, remaining.min(Duration::from_millis(100))).unwrap().0
                } else {
                    wake.wait(state).unwrap()
                };
            }
        })?;
        Ok(Self { state, worker: Some(worker) })
    }

    pub(super) fn during<T>(&self, ctl: Option<&Control>, operation: impl FnOnce() -> T) -> T {
        let (lock, wake) = &*self.state;
        lock.lock().unwrap().active = Some((Instant::now() + TIMEOUT, ctl.cloned()));
        wake.notify_one();
        let result = operation();
        lock.lock().unwrap().active = None;
        wake.notify_one();
        result
    }
}

impl Drop for Deadline {
    fn drop(&mut self) {
        self.state.0.lock().unwrap().stopped = true;
        self.state.1.notify_one();
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}
