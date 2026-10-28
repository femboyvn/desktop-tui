use async_channel::{Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use virtual_terminal::{Command, Output};

type Tasks = Mutex<Vec<tokio::task::JoinHandle<()>>>;
static TASKS: OnceLock<Tasks> = OnceLock::new();

pub(crate) struct ProcessGuard { stop: Sender<()>, done: Arc<AtomicBool> }
impl ProcessGuard {
    pub fn close(&self) { let _ = self.stop.try_send(()); }
    pub fn finished(&self) -> bool { self.done.load(Ordering::Acquire) }
}
impl Drop for ProcessGuard { fn drop(&mut self) { self.close(); } }

/// Observe PID delivery independently of UI polling, including closure before launch.
/// The runner retains the child until wait() completes; cleanup runs on Tokio.
pub(crate) fn start(command: Command) -> (ProcessGuard, Receiver<Output>) {
    let raw = command.out_rx();
    let (tx, rx) = async_channel::bounded(8192);
    let (stop, stopped) = async_channel::bounded(1);
    let done = Arc::new(AtomicBool::new(false));
    let completed = done.clone();
    let task = tokio::spawn(async move {
        let runner = tokio::spawn(command.run());
        let mut pid = None;
        let mut closing = false;
        loop {
            if closing {
                tx.close();
                if let Some(pid) = pid {
                    raw.close();
                    bmart::process::kill_pstree(pid, Some(Duration::from_millis(250)), true).await;
                    break;
                }
            }
            let output = tokio::select! {
                _ = stopped.recv(), if !closing => { closing = true; continue; }
                output = raw.recv() => match output { Ok(output) => output, Err(_) => break },
            };
            match &output {
                Output::Pid(value) => pid = Some(*value),
                Output::Terminated(_) | Output::Error(_) => {
                    if !closing {
                        tokio::select! { _ = stopped.recv() => {}, _ = tx.send(output) => {} }
                    }
                    break;
                }
                _ => {}
            }
            if !closing {
                tokio::select! {
                    _ = stopped.recv() => closing = true,
                    result = tx.send(output) => if result.is_err() { closing = true; },
                }
            }
        }
        // Reap through the PTY runner rather than aborting a child-owning future.
        let _ = runner.await;
        completed.store(true, Ordering::Release);
    });
    if let Ok(mut tasks) = TASKS.get_or_init(|| Mutex::new(Vec::new())).lock() {
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
    }
    (ProcessGuard { stop, done }, rx)
}

pub(crate) async fn finish_shutdown() {
    let tasks = TASKS.get().and_then(|tasks| tasks.lock().ok().map(|mut tasks| std::mem::take(&mut *tasks))).unwrap_or_default();
    for task in tasks { let _ = task.await; }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn repeated_close_terminates_and_reaps_a_running_child() {
        let cmd = virtual_terminal::Command::new("/bin/sh").args(["-c", "sleep 60 & wait"]);
        let (guard, rx) = start(cmd);
        let pid = match tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await.unwrap().unwrap() {
            virtual_terminal::Output::Pid(pid) => pid,
            other => panic!("{other:?}"),
        };
        guard.close();
        guard.close();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(4);
        while !guard.finished() {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::task::yield_now().await;
        }
        assert!(!std::path::Path::new("/proc").join(pid.to_string()).exists());
    }
    #[tokio::test]
    async fn close_before_pid_delivery_still_cleans_up() {
        let cmd = virtual_terminal::Command::new("/bin/sh").args(["-c", "sleep 60"]);
        let (guard, _rx) = start(cmd);
        guard.close();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(4);
        while !guard.finished() {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::task::yield_now().await;
        }
    }
}
