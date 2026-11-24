use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const LIMIT: usize = 64;
const DETAIL_LIMIT: usize = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source { Files, AiChat }
impl Source {
    pub fn label(self) -> &'static str {
        match self { Self::Files => "File Browser", Self::AiChat => "AI Chat" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status { Completed, Failed, Attention, Canceled }
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Completed => "Completed", Self::Failed => "Failed",
            Self::Attention => "Needs attention", Self::Canceled => "Canceled",
        }
    }
}

#[derive(Clone)]
pub(crate) struct Entry {
    pub id: u64,
    pub time: SystemTime,
    pub source: Source,
    pub status: Status,
    pub detail: String,
}

#[derive(Default)]
struct State { entries: VecDeque<Entry>, unread: usize, revision: u64 }

/// One session's recent results. The lock protects a small in-memory buffer
/// and is released before rendering, file I/O, or other worker operations.
#[derive(Clone, Default)]
pub(crate) struct ActivityLog(Arc<Mutex<State>>);
impl ActivityLog {
    pub fn record(&self, source: Source, status: Status, detail: &str) {
        let mut chars = detail.chars();
        let mut detail: String = chars.by_ref().take(DETAIL_LIMIT)
            .map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
        if chars.next().is_some() { detail.push('…'); }
        let time = SystemTime::now();
        let Ok(mut state) = self.0.lock() else { return; };
        state.revision = state.revision.wrapping_add(1);
        let entry = Entry { id: state.revision,time,source,status,detail };
        if state.entries.len() == LIMIT { state.entries.pop_back(); }
        state.entries.push_front(entry);
        state.unread = (state.unread+1).min(LIMIT);
    }

    pub fn unread(&self) -> usize { self.0.lock().map_or(0,|state| state.unread) }

    /// The view acknowledges only the entries included in this snapshot.
    pub fn read_since(&self, revision: Option<u64>) -> Option<(u64,Vec<Entry>)> {
        let mut state = self.0.lock().ok()?;
        if revision == Some(state.revision) { return None; }
        state.unread = 0;
        Some((state.revision,state.entries.iter().cloned().collect()))
    }

    pub fn clear(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.entries.clear();
            state.unread = 0;
            state.revision = state.revision.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_activity_is_bounded_and_read_only_when_observed() {
        let log = ActivityLog::default();
        let producer = log.clone();
        for i in 0..=LIMIT { producer.record(Source::Files,Status::Completed,&format!("copy {i}")); }
        assert_eq!(log.unread(),LIMIT);
        let (revision,entries) = log.read_since(None).unwrap();
        assert_eq!(entries.len(),LIMIT);
        assert_eq!(entries.first().unwrap().detail,format!("copy {LIMIT}"));
        assert_eq!(entries.last().unwrap().detail,"copy 1");
        assert_eq!(log.unread(),0);
        assert!(log.read_since(Some(revision)).is_none());
        producer.record(Source::AiChat,Status::Failed,&format!("error\n\u{1b}{}", "文".repeat(500)));
        assert_eq!(log.unread(),1);
        let (revision,entries) = log.read_since(Some(revision)).unwrap();
        assert_eq!(entries[0].source,Source::AiChat);
        assert!(!entries[0].detail.chars().any(char::is_control));
        assert_eq!(entries[0].detail.chars().count(),DETAIL_LIMIT+1);
        log.clear();
        assert!(log.read_since(Some(revision)).unwrap().1.is_empty());
    }
}
