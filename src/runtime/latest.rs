use std::sync::{Arc, Condvar, Mutex};

struct State<Q, R> {
    revision: u64,
    request: Option<(u64, Q)>,
    result: Option<(u64, R)>,
    closed: bool,
}

/// One running job, one replaceable pending request, one replaceable result.
/// Dropping the UI owner cancels pending work without waiting for active I/O.
pub(crate) struct LatestWorker<Q, R> {
    state: Arc<(Mutex<State<Q, R>>, Condvar)>,
}
impl<Q: Send + 'static, R: Send + 'static> LatestWorker<Q, R> {
    pub fn new(mut work: impl FnMut(Q) -> R + Send + 'static) -> std::io::Result<Self> {
        let state = Arc::new((Mutex::new(State { revision: 0, request: None, result: None, closed: false }), Condvar::new()));
        let shared = Arc::clone(&state);
        std::thread::Builder::new().name("desktop-native-worker".into()).spawn(move || {
            let (lock, wake) = &*shared;
            loop {
                let (revision, request) = {
                    let Ok(mut state) = lock.lock() else { return; };
                    while state.request.is_none() && !state.closed {
                        let Ok(next) = wake.wait(state) else { return; };
                        state = next;
                    }
                    if state.closed { return; }
                    let Some(request) = state.request.take() else { continue; };
                    request
                };
                let result = work(request);
                let Ok(mut state) = lock.lock() else { return; };
                if state.closed { return; }
                if revision == state.revision { state.result = Some((revision, result)); }
            }
        })?;
        Ok(Self { state })
    }
    pub fn submit(&mut self, request: Q) {
        let (lock, wake) = &*self.state;
        if let Ok(mut state) = lock.lock() {
            if state.closed { return; }
            state.revision = state.revision.wrapping_add(1);
            state.result = None;
            state.request = Some((state.revision, request));
            wake.notify_one();
        }
    }
    pub fn poll(&mut self) -> Option<R> {
        self.state.0.lock().ok()?.result.take().map(|(_, value)| value)
    }
    pub fn close(&mut self) { close(&self.state); }
}
fn close<Q, R>(shared: &Arc<(Mutex<State<Q, R>>, Condvar)>) {
    if let Ok(mut state) = shared.0.lock() {
        state.closed = true;
        state.request = None;
        state.result = None;
        shared.1.notify_one();
    }
}
impl<Q, R> Drop for LatestWorker<Q, R> {
    fn drop(&mut self) { close(&self.state); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coalesces_requests_and_discards_stale_results() {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut worker = LatestWorker::new(move |value| {
            if value == 1 { started_tx.send(()).unwrap(); release_rx.recv().unwrap(); }
            value * 2
        }).unwrap();
        worker.submit(1);
        started_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        worker.submit(2);
        worker.submit(3);
        release_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(value) = worker.poll() { assert_eq!(value, 6); break; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    fn closing_does_not_wait_for_busy_worker_and_reopening_is_independent() {
        let (start_tx, start_rx) = std::sync::mpsc::channel();
        let (end_tx, end_rx) = std::sync::mpsc::channel();
        let mut old = LatestWorker::new(move |_: ()| { start_tx.send(()).unwrap(); end_rx.recv().unwrap(); 9 }).unwrap();
        old.submit(());
        start_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        drop(old);
        let mut new = LatestWorker::new(|_: ()| 7).unwrap();
        new.submit(());
        end_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(value) = new.poll() { assert_eq!(value, 7); break; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
