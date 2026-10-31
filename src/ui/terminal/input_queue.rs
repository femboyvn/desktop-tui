use async_channel::{Sender, TrySendError};
use std::collections::VecDeque;
use virtual_terminal::Input;

const PENDING_LIMIT: usize = 256 * 1024;
const WRITE_CHUNK: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InputError { Closed, Full }
impl std::fmt::Display for InputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Closed => "The child input channel has closed.",
            Self::Full => "The child input queue exceeded 256 KiB. This application will close.",
        })
    }
}

enum Pending {
    Data(Vec<u8>),
    Motion(Vec<u8>),
    Resize(usize, usize),
}
impl Pending {
    fn cost(&self) -> usize {
        match self { Self::Data(bytes) | Self::Motion(bytes) => bytes.len(), Self::Resize(..) => 16 }
    }
}

/// UI-owned ordered input. Only adjacent motion/resize samples are replaceable.
/// Feed the PTY at most one small queued message ahead, bounding its byte backlog too.
pub(super) struct InputQueue {
    tx: Sender<Input>,
    pending: VecDeque<Pending>,
    bytes: usize,
}
impl InputQueue {
    pub fn new(tx: Sender<Input>) -> Self { Self { tx, pending: VecDeque::new(), bytes: 0 } }
    pub fn data(&mut self, data: Vec<u8>) -> Result<(), InputError> {
        if data.is_empty() { return Ok(()); }
        self.push(Pending::Data(data))
    }
    pub fn motion(&mut self, data: Vec<u8>) -> Result<(), InputError> {
        if data.is_empty() { return Ok(()); }
        self.push(Pending::Motion(data))
    }
    pub fn resize(&mut self, width: usize, height: usize) -> Result<(), InputError> {
        self.push(Pending::Resize(width, height))
    }
    fn push(&mut self, entry: Pending) -> Result<(), InputError> {
        if self.tx.is_closed() { return Err(InputError::Closed); }
        self.flush()?;
        let replace = matches!((self.pending.back(), &entry),
            (Some(Pending::Motion(_)), Pending::Motion(_)) | (Some(Pending::Resize(..)), Pending::Resize(..)));
        let old_cost = if replace { self.pending.back().map_or(0, Pending::cost) } else { 0 };
        let next_bytes = self.bytes - old_cost + entry.cost();
        if next_bytes > PENDING_LIMIT { return Err(InputError::Full); }
        if replace { self.pending.pop_back(); }
        self.bytes = next_bytes;
        self.pending.push_back(entry);
        self.flush()
    }
    pub fn flush(&mut self) -> Result<(), InputError> {
        if self.tx.is_closed() { return Err(InputError::Closed); }
        // This channel is also read by an asynchronous writer. Small frames avoid
        // moving an entire paste into its otherwise message-count-only buffer.
        for _ in 0..64 {
            if !self.tx.is_empty() { break; }
            let Some(entry) = self.pending.pop_front() else { break; };
            let cost = entry.cost();
            let (input, remainder) = match entry {
                Pending::Data(mut bytes) if bytes.len() > WRITE_CHUNK => {
                    let rest = bytes.split_off(WRITE_CHUNK);
                    (Input::Data(bytes), Some(Pending::Data(rest)))
                }
                Pending::Data(bytes) | Pending::Motion(bytes) => (Input::Data(bytes), None),
                Pending::Resize(width, height) => (Input::Resize((width, height)), None),
            };
            match self.tx.try_send(input) {
                Ok(()) => {
                    self.bytes -= cost;
                    if let Some(rest) = remainder { self.bytes += rest.cost(); self.pending.push_front(rest); }
                }
                Err(TrySendError::Full(input)) => {
                    // Only a legacy direct sender can fill the channel between
                    // is_empty and try_send; retain the complete logical entry.
                    if let Some(rest) = remainder { self.pending.push_front(rest); }
                    self.pending.push_front(match input {
                        Input::Data(bytes) => Pending::Data(bytes),
                        Input::Resize((width, height)) => Pending::Resize(width, height),
                        Input::Terminate => unreachable!(),
                    });
                    break;
                }
                Err(TrySendError::Closed(_)) => return Err(InputError::Closed),
            }
        }
        Ok(())
    }
    #[cfg(test)]
    fn pending_bytes(&self) -> usize { self.bytes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_text_and_button_transitions_survive_backpressure_in_order() {
        let (tx, rx) = async_channel::bounded(1);
        tx.try_send(Input::Data(b"busy".to_vec())).unwrap();
        let mut queue = InputQueue::new(tx);
        queue.data("URL ế".as_bytes().to_vec()).unwrap();
        queue.motion(b"old motion".to_vec()).unwrap();
        queue.motion(b"new motion".to_vec()).unwrap();
        queue.data(b"press".to_vec()).unwrap();
        queue.motion(b"drag".to_vec()).unwrap();
        queue.data(b"release".to_vec()).unwrap();
        queue.flush().unwrap();
        assert_eq!(rx.try_recv().unwrap(), Input::Data(b"busy".to_vec()));
        for expected in ["URL ế", "new motion", "press", "drag", "release"] {
            queue.flush().unwrap();
            assert_eq!(rx.try_recv().unwrap(), Input::Data(expected.as_bytes().to_vec()));
        }
        assert_eq!(queue.pending_bytes(), 0);
    }

    #[test]
    fn resize_coalesces_without_overtaking_text_or_button_events() {
        let (tx, rx) = async_channel::bounded(1);
        tx.try_send(Input::Data(b"busy".to_vec())).unwrap();
        let mut queue = InputQueue::new(tx);
        queue.resize(40, 10).unwrap();
        queue.resize(60, 12).unwrap();
        queue.data(b"click".to_vec()).unwrap();
        queue.resize(70, 15).unwrap();
        rx.try_recv().unwrap();
        for expected in [Input::Resize((60, 12)), Input::Data(b"click".to_vec()), Input::Resize((70, 15))] {
            queue.flush().unwrap();
            assert_eq!(rx.try_recv().unwrap(), expected);
        }
    }

    #[test]
    fn queue_limit_rejects_excess_and_recovers_after_draining() {
        let (tx, rx) = async_channel::bounded(1);
        tx.try_send(Input::Data(b"busy".to_vec())).unwrap();
        let mut queue = InputQueue::new(tx);
        queue.data(vec![b'a'; 256 * 1024]).unwrap();
        assert_eq!(queue.pending_bytes(), 256 * 1024);
        assert!(queue.data(vec![b'b']).is_err());
        rx.try_recv().unwrap();
        let mut count = 0;
        while queue.pending_bytes() > 0 {
            queue.flush().unwrap();
            let Input::Data(data) = rx.try_recv().unwrap() else { panic!("expected text") };
            assert!(data.len() <= 8192);
            assert!(data.iter().all(|byte| *byte == b'a'));
            count += data.len();
        }
        assert_eq!(count, 256 * 1024);
        queue.data(b"recovered".to_vec()).unwrap();
        assert_eq!(rx.try_recv().unwrap(), Input::Data(b"recovered".to_vec()));
        rx.close();
        assert!(queue.data(b"closed".to_vec()).is_err());
    }
}
