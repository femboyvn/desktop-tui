//! BYOK chat actor. Commands and snapshots are bounded and never perform UI-thread I/O.
mod provider;
mod sse;
mod store;

use crate::config::ai::{AiPreferences, AiProvider};
use super::activity::{ActivityLog, Source as ActivitySource, Status as ActivityStatus};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, AtomicU64, Ordering}};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Notify};

pub(super) const MAX_DRAFT: usize = 64 * 1024;
pub(super) const MAX_RESPONSE: usize = 1024 * 1024;
pub(super) const MAX_TRANSCRIPT: u64 = 4 * 1024 * 1024;
pub(super) const MAX_FILE: u64 = 10 * 1024 * 1024;
pub(super) const MAX_RAW: u64 = 20 * 1024 * 1024;
pub(super) const MAX_REQUEST: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role { User, Assistant }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageStatus { Complete, Streaming, Stopped, Failed, Interrupted }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatMessage { pub role: Role, pub text: String, pub provider: AiProvider, pub model: String, pub status: MessageStatus }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment { pub id: String, pub name: String, pub mime: String, pub size: u64, pub included: bool, pub message: Option<usize>, pub error: Option<String> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conversation { pub id: String, pub title: String, pub provider: AiProvider, pub model: String, pub messages: Vec<ChatMessage>, pub draft: String, pub attachments: Vec<Attachment> }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatSummary { pub id: String, pub title: String }
#[derive(Clone, Debug)]
pub struct ChatSnapshot {
    pub model_defaults: [String;3],
    pub navigation_revision: u64,
    pub revision: u64, pub flush_revision: u64, pub request_revision: u64, pub chats: Vec<ChatSummary>, pub current: Option<Conversation>,
    pub busy: bool, pub loading: bool, pub error: Option<String>, pub dirty: bool,
    pub writable: bool, pub status: String,
}
pub enum ChatCommand {
    Navigate { token: u64, command: Box<ChatCommand> },
    Scoped { chat: String, command: Box<ChatCommand> },
    Configure(AiPreferences), New, Select(String), Route { provider: AiProvider, model: String }, Draft(String),
    Attach(PathBuf), ToggleAttachment(String), RemoveAttachment(String), Send, Stop, Retry, Delete, Flush, DiscardUnsaved,
}
struct Shared { configuration: Mutex<Option<AiPreferences>>, snapshot: Mutex<ChatSnapshot>, shutdown: AtomicBool, wake: Notify }
pub struct ChatService { tx: mpsc::Sender<ChatCommand>, shared: Arc<Shared>, polled: AtomicU64 }
static WORKERS: OnceLock<Mutex<Vec<std::thread::JoinHandle<()>>>> = OnceLock::new();
impl ChatService {
    pub fn open(activity: ActivityLog) -> anyhow::Result<Self> {
        let dirs = directories::ProjectDirs::from("com", "Julien-cpsn", "desktop-tui").ok_or_else(|| anyhow::anyhow!("Cannot locate AI state directory"))?;
        Self::start(dirs.state_dir().unwrap_or(dirs.data_local_dir()).join("ai"),AiPreferences::default(),Some(crate::config::ai::AiSettingsStore::new()?),activity)
    }
    /// `directory` is the AI state root; transcripts live in its `chats` child.
    #[cfg(test)]
    pub fn new_at(directory: PathBuf, preferences: AiPreferences) -> anyhow::Result<Self> { Self::start(directory,preferences,None,ActivityLog::default()) }
    fn start(directory: PathBuf, preferences: AiPreferences, settings: Option<crate::config::ai::AiSettingsStore>, activity: ActivityLog) -> anyhow::Result<Self> {
        let shared = Arc::new(Shared { configuration: Mutex::new(None), snapshot: Mutex::new(ChatSnapshot { model_defaults: std::array::from_fn(|_| String::new()), navigation_revision: 0, revision: 1, flush_revision: 0, request_revision: 0, chats: Vec::new(), current: None, busy: true, loading: true, error: None, dirty: false, writable: false, status: "Loading chats".into() }), shutdown: AtomicBool::new(false), wake: Notify::new() });
        let (tx, rx) = mpsc::channel(64);
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new().name("desktop-ai-chat".into()).spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
            match runtime {
                Ok(runtime) => {
                    let (preferences,error) = match settings { Some(store) => match store.load() { Ok(ai) => (ai,None),Err(error) => (preferences,Some(format!("AI Chat settings unavailable: {error:#}"))) },None => (preferences,None) };
                    if let Some(error) = error && let Ok(mut snapshot) = worker_shared.snapshot.lock() { snapshot.error = Some(error); }
                    runtime.block_on(Actor::run(directory, preferences, worker_shared, rx, activity));
                },
                Err(_) => if let Ok(mut snapshot) = worker_shared.snapshot.lock() { snapshot.loading = false; snapshot.busy = false; snapshot.error = Some("Cannot start AI worker".into()); snapshot.revision += 1; },
            }
        })?;
        WORKERS.get_or_init(Default::default).lock().map_err(|_| anyhow::anyhow!("AI shutdown registry unavailable"))?.push(worker);
        Ok(Self { tx, shared, polled: AtomicU64::new(0) })
    }
    pub fn submit(&self, command: ChatCommand) -> Result<(), String> {
        if let ChatCommand::Configure(preferences) = command {
            if self.shared.shutdown.load(Ordering::Acquire) { return Err("AI Chat is closing".into()); }
            *self.shared.configuration.lock().map_err(|_| "AI configuration unavailable")? = Some(preferences);
            self.shared.wake.notify_one(); return Ok(());
        }
        let targeted = match &command {
            ChatCommand::Scoped { chat, command } => {
                if chat.len() > 128 || matches!(**command, ChatCommand::Scoped { .. }) { return Err("Invalid scoped chat command".into()); }
                &**command
            }
            command => command,
        };
        if let ChatCommand::Draft(text) = targeted && text.len() > MAX_DRAFT { return Err("Draft exceeds 64 KiB".into()); }
        if self.shared.shutdown.load(Ordering::Acquire) { return Err("AI Chat is closing".into()); }
        self.tx.try_send(command).map_err(|_| "AI Chat is busy; try again".into())
    }
    pub fn poll(&self) -> Option<ChatSnapshot> {
        let snapshot = self.shared.snapshot.try_lock().ok()?;
        let previous = self.polled.load(Ordering::Relaxed);
        if snapshot.revision == previous { return None; }
        self.polled.store(snapshot.revision, Ordering::Relaxed);
        Some(snapshot.clone())
    }
}
impl Drop for ChatService {
    fn drop(&mut self) { self.shared.shutdown.store(true, Ordering::Release); self.shared.wake.notify_one(); }
}
/// Called after AppCUI exits; Drop itself never waits for a worker.
pub fn finish_shutdown() {
    let workers = WORKERS.get_or_init(Default::default).lock().map(|mut workers| std::mem::take(&mut *workers)).unwrap_or_default();
    for worker in workers { let _ = worker.join(); }
}

// Network events carry both chat identity and an independently increasing generation.
struct NetworkEvent { chat: String, generation: u64, event: provider::Update }
struct Active { chat: String, generation: u64, task: tokio::task::JoinHandle<()> }
struct Actor {
    store: Option<store::Store>, preferences: AiPreferences, snapshot: ChatSnapshot,
    loaded: std::collections::BTreeMap<String, Conversation>, unsaved: std::collections::BTreeSet<String>,
    active: Option<Active>, generation: u64, last_save: Instant, discarded: bool, io_busy: bool,
    activity: ActivityLog,
}
impl Actor {
    async fn run(directory: PathBuf, preferences: AiPreferences, shared: Arc<Shared>, mut commands: mpsc::Receiver<ChatCommand>, activity: ActivityLog) {
        let initial = shared.snapshot.lock().map(|snapshot| snapshot.clone()).unwrap_or(ChatSnapshot { model_defaults: std::array::from_fn(|_| String::new()), navigation_revision: 0, revision: 1, flush_revision: 0, request_revision: 0, chats: Vec::new(), current: None, busy: true, loading: true, error: None, dirty: false, writable: false, status: "Loading chats".into() });
        let mut actor = Self { store: None, preferences, snapshot: initial, loaded: Default::default(), unsaved: Default::default(), active: None, generation: 0, last_save: Instant::now(), discarded: false, io_busy: false, activity };
        match store::Store::open(directory) {
            Ok(store) => {
                actor.snapshot.writable = store.writable;
                match store.list() { Ok(chats) => actor.snapshot.chats = chats, Err(error) => actor.snapshot.error = Some(format!("Cannot list AI history: {error}")) }
                actor.store = Some(store);
                if let Some(id) = actor.snapshot.chats.first().map(|chat| chat.id.clone()) { actor.select(&id); }
                else if actor.snapshot.writable { actor.new_chat(); actor.persist(); }
            }
            Err(error) => actor.snapshot.error = Some(format!("Cannot open AI history: {error}")),
        }
        actor.snapshot.loading = false;
        actor.publish(&shared);
        let (events_tx, mut events) = mpsc::channel(64);
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            if shared.shutdown.load(Ordering::Acquire) { break; }
            let preferences = shared.configuration.lock().ok().and_then(|mut value| value.take());
            if let Some(preferences) = preferences { actor.apply_configuration(preferences); actor.publish(&shared); }
            tokio::select! {
                biased;
                _ = shared.wake.notified() => (),
                command = commands.recv() => match command { Some(command) => {
                    let targeted = match &command { ChatCommand::Scoped { command, .. } => &**command, command => command };
                    actor.io_busy = matches!(targeted, ChatCommand::Select(_) | ChatCommand::Attach(_) | ChatCommand::RemoveAttachment(_) | ChatCommand::Send | ChatCommand::Retry | ChatCommand::Delete | ChatCommand::Flush | ChatCommand::Stop);
                    if actor.io_busy { actor.publish(&shared); }
                    actor.command(command, &events_tx); actor.io_busy = false; actor.publish(&shared);
                }, None => break },
                event = events.recv() => if let Some(event) = event { actor.network(event); actor.publish(&shared); },
                _ = tick.tick() => if !actor.unsaved.is_empty() && actor.last_save.elapsed() >= Duration::from_secs(1) { actor.io_busy = true; actor.publish(&shared); actor.persist(); actor.io_busy = false; actor.publish(&shared); },
            }
        }
        actor.stop();
        // Preserve accepted composer edits even if Drop overtakes their queued commands.
        while let Ok(command) = commands.try_recv() {
            let targeted = match &command { ChatCommand::Scoped { command, .. } => &**command, command => command };
            if !matches!(targeted, ChatCommand::Send | ChatCommand::Retry) { actor.command(command, &events_tx); }
        }
        if !actor.discarded { actor.persist(); }
    }
    fn apply_configuration(&mut self,preferences: AiPreferences) {
        self.preferences = preferences;
        if let Some(id) = self.current_id() && let Some(chat) = self.loaded.get_mut(&id)
            && chat.messages.is_empty() && chat.attachments.is_empty() && chat.draft.trim().is_empty() && chat.model.is_empty() {
            chat.provider = self.preferences.default_provider; chat.model = self.preferences.settings(chat.provider).model.clone(); self.changed(&id);
        }
    }
    fn current_id(&self) -> Option<String> { self.snapshot.current.as_ref().map(|chat| chat.id.clone()) }
    fn publish(&mut self, shared: &Shared) {
        self.snapshot.model_defaults = AiProvider::ALL.map(|p| self.preferences.settings(p).model.clone());
        if let Some(id) = self.current_id() { self.snapshot.current = self.loaded.get(&id).cloned(); }
        let current = self.current_id();
        let active = self.active.as_ref().map(|active| active.chat.as_str());
        self.loaded.retain(|id, _| current.as_deref() == Some(id.as_str()) || active == Some(id.as_str()) || self.unsaved.contains(id));
        self.snapshot.dirty = !self.unsaved.is_empty();
        self.snapshot.busy = self.active.is_some() || self.io_busy;
        self.snapshot.status = if self.snapshot.loading { "Loading chats" } else if self.active.is_some() { "Generating" } else if self.io_busy { "Saving or loading locally" } else if !self.snapshot.writable { "Read-only: another instance owns history" } else if self.snapshot.dirty { "Unsaved changes" } else { "Saved locally" }.into();
        self.snapshot.revision = self.snapshot.revision.wrapping_add(1);
        if let Ok(mut snapshot) = shared.snapshot.lock() { *snapshot = self.snapshot.clone(); }
    }
    fn changed(&mut self, id: &str) { self.unsaved.insert(id.into()); self.discarded = false; }
    fn persist(&mut self) {
        self.last_save = Instant::now();
        let Some(store) = &self.store else { return; };
        let mut saved = Vec::new();
        for id in &self.unsaved {
            if let Some(chat) = self.loaded.get(id) {
                if let Err(error) = store.save(chat) { self.snapshot.error = Some(format!("Cannot save AI chat: {error}. Retry saving or explicitly discard changes.")); break; }
                saved.push(id.clone());
            }
        }
        for id in saved { self.unsaved.remove(&id); }
    }
    fn select(&mut self, id: &str) {
        if !self.loaded.contains_key(id) {
            let result = self.store.as_ref().ok_or_else(|| "AI history unavailable".to_owned()).and_then(|store| store.load(id).map_err(|error| format!("Cannot load chat: {error}")));
            match result { Ok(chat) => { self.loaded.insert(id.into(), chat); }, Err(error) => { self.snapshot.error = Some(error); return; } }
        }
        self.snapshot.current = self.loaded.get(id).cloned();
    }
    fn new_chat(&mut self) {
        if !self.snapshot.writable { self.snapshot.error = Some("AI Chat is read-only".into()); return; }
        if self.snapshot.chats.len() >= 4096 { self.snapshot.error = Some("AI history exceeds 4096 chats".into()); return; }
        let provider = self.preferences.default_provider;
        let chat = Conversation { id: store::id("c"), title: "New chat".into(), provider, model: self.preferences.settings(provider).model.clone(), messages: Vec::new(), draft: String::new(), attachments: Vec::new() };
        self.snapshot.chats.insert(0, ChatSummary { id: chat.id.clone(), title: chat.title.clone() });
        self.changed(&chat.id);
        self.loaded.insert(chat.id.clone(), chat.clone());
        self.snapshot.current = Some(chat);
    }
    fn command(&mut self, command: ChatCommand, events: &mpsc::Sender<NetworkEvent>) {
        if let ChatCommand::Navigate { token, command } = command {
            // UI transitions carry their own acknowledgement, including failed selects.
            if matches!(*command, ChatCommand::New | ChatCommand::Select(_) | ChatCommand::Delete | ChatCommand::Scoped { .. }) { self.command(*command,events); }
            self.snapshot.navigation_revision = token;
            return;
        }

        let command = match command {
            ChatCommand::Scoped { chat, command } => {
                if self.current_id().as_deref() != Some(chat.as_str()) { self.snapshot.error = Some("The selected chat changed; try the action again".into()); return; }
                if !matches!(*command, ChatCommand::Route { .. } | ChatCommand::Draft(_) | ChatCommand::Attach(_) | ChatCommand::ToggleAttachment(_) | ChatCommand::RemoveAttachment(_) | ChatCommand::Send | ChatCommand::Retry | ChatCommand::Delete) {
                    self.snapshot.error = Some("Invalid scoped chat command".into()); return;
                }
                *command
            }
            command => command,
        };
        match command {
            ChatCommand::Configure(preferences) => { self.apply_configuration(preferences); return; }
            ChatCommand::Select(id) => { self.select(&id); return; }
            ChatCommand::Stop => { self.stop(); self.persist(); return; }
            ChatCommand::Flush => { self.stop(); self.snapshot.error = None; self.persist(); self.snapshot.flush_revision = self.snapshot.flush_revision.wrapping_add(1); return; }
            ChatCommand::DiscardUnsaved => { self.stop(); self.unsaved.clear(); self.snapshot.error = None; self.discarded = true; self.snapshot.flush_revision = self.snapshot.flush_revision.wrapping_add(1); return; }
            _ => (),
        }
        if !self.snapshot.writable { self.snapshot.error = Some("AI Chat is read-only: another instance owns history".into()); return; }
        if matches!(command, ChatCommand::New) { self.new_chat(); return; }
        let Some(id) = self.current_id() else { self.snapshot.error = Some("Select or create a chat".into()); return; };
        match command {
            ChatCommand::Route { provider, model } => {
                if self.active.is_some() { self.snapshot.error = Some("Stop generation before changing provider or model".into()); return; }
                if model.len() > 256 || model.chars().any(char::is_control) { self.snapshot.error = Some("Invalid model ID".into()); return; }
                if let Some(chat) = self.loaded.get_mut(&id) { chat.provider = provider; chat.model = model.trim().into(); }
                self.changed(&id);
                self.compatibility(&id);
            }
            ChatCommand::Draft(text) => {
                if text.len() > MAX_DRAFT { self.snapshot.error = Some("Draft exceeds 64 KiB".into()); return; }
                if let Some(chat) = self.loaded.get_mut(&id) { if chat.draft == text { return; } chat.draft = text; }
                self.changed(&id);
            }
            ChatCommand::Attach(path) => {
                if self.loaded.get(&id).is_some_and(|chat| chat.attachments.len() >= 1024) { self.snapshot.error = Some("Chat exceeds 1024 attachments".into()); return; }
                let result = self.store.as_ref().ok_or_else(|| anyhow::anyhow!("AI history unavailable")).and_then(|store| store.import(&id, &path));
                match result {
                    Ok(attachment) => { if let Some(chat) = self.loaded.get_mut(&id) { chat.attachments.push(attachment); } self.changed(&id); self.compatibility(&id); self.persist(); }
                    Err(error) => self.snapshot.error = Some(format!("Cannot attach file: {error}")),
                }
            }
            ChatCommand::ToggleAttachment(attachment_id) => {
                if self.active.is_some() { self.snapshot.error = Some("Stop generation before changing included attachments".into()); return; }
                if let Some(attachment) = self.loaded.get_mut(&id).and_then(|chat| chat.attachments.iter_mut().find(|attachment| attachment.id == attachment_id)) { attachment.included = !attachment.included; self.changed(&id); self.compatibility(&id); }
            }
            ChatCommand::RemoveAttachment(attachment_id) => {
                let pending = self.loaded.get(&id).is_some_and(|chat| chat.attachments.iter().any(|attachment| attachment.id == attachment_id && attachment.message.is_none()));
                if !pending { self.snapshot.error = Some("Historical attachments can be excluded using their Include toggle".into()); return; }
                // Save the removal before deleting immutable bytes, so a save error cannot leave a broken transcript.
                let old = self.loaded.get(&id).cloned();
                if let Some(chat) = self.loaded.get_mut(&id) { chat.attachments.retain(|attachment| attachment.id != attachment_id); }
                self.changed(&id); self.persist();
                if self.unsaved.contains(&id) { if let Some(old) = old { self.loaded.insert(id.clone(), old); } }
                else if let Some(store) = &self.store && let Err(error) = store.remove_attachment(&id, &attachment_id) { self.snapshot.error = Some(format!("Cannot remove attachment snapshot: {error}")); }
            }
            ChatCommand::Send => self.send(&id, false, events),
            ChatCommand::Retry => self.send(&id, true, events),
            ChatCommand::Delete => {
                if self.active.as_ref().is_some_and(|active| active.chat == id) { self.stop(); }
                let result = self.store.as_ref().ok_or_else(|| anyhow::anyhow!("AI history unavailable")).and_then(|store| store.delete(&id));
                match result {
                    Ok(()) => {
                        self.loaded.remove(&id); self.unsaved.remove(&id); self.snapshot.chats.retain(|chat| chat.id != id); self.snapshot.current = None;
                        if let Some(next) = self.snapshot.chats.first().map(|chat| chat.id.clone()) { self.select(&next); }
                    }
                    Err(error) => self.snapshot.error = Some(format!("Cannot delete chat: {error}")),
                }
            }
            _ => (),
        }
    }
    fn compatibility(&mut self, id: &str) {
        if let Some(chat) = self.loaded.get(id) {
            self.snapshot.error = chat.attachments.iter().filter(|attachment| attachment.included).find_map(|attachment| provider::compatible(chat.provider, attachment).err());
        }
    }
    fn send(&mut self, id: &str, retry: bool, events: &mpsc::Sender<NetworkEvent>) {
        let before = self.generation;
        self.start_request(id,retry,events);
        if self.generation == before && self.snapshot.error.is_some() {
            self.activity.record(ActivitySource::AiChat,ActivityStatus::Failed,"Could not start the request. See AI Chat for details.");
        }
    }
    fn start_request(&mut self, id: &str, retry: bool, events: &mpsc::Sender<NetworkEvent>) {
        if self.active.is_some() { self.snapshot.error = Some("Only one AI request can run at a time".into()); return; }
        // All outstanding edits must reach disk before constructing another user turn.
        self.snapshot.error = None; self.persist();
        if !self.unsaved.is_empty() { return; }
        let Some(original) = self.loaded.get(id).cloned() else { return; };
        let mut chat = original.clone();
        if retry {
            let Some(last) = chat.messages.last() else { self.snapshot.error = Some("No incomplete answer to retry".into()); return; };
            if last.role != Role::Assistant || last.status == MessageStatus::Complete { self.snapshot.error = Some("Retry is available for a stopped, failed or interrupted answer".into()); return; }
            chat.messages.pop();
            if !chat.messages.last().is_some_and(|message| message.role == Role::User) { self.snapshot.error = Some("No user turn to retry".into()); return; }
        } else {
            if chat.draft.trim().is_empty() && !chat.attachments.iter().any(|attachment| attachment.message.is_none() && attachment.included) { self.snapshot.error = Some("Enter a message or include a file".into()); return; }
            if chat.messages.len() >= 4094 { self.snapshot.error = Some("Chat exceeds 4096 messages; start a new chat".into()); return; }
            let index = chat.messages.len();
            for attachment in &mut chat.attachments { if attachment.message.is_none() { attachment.message = Some(index); } }
            chat.messages.push(ChatMessage { role: Role::User, text: chat.draft.clone(), provider: chat.provider, model: chat.model.clone(), status: MessageStatus::Complete });
            if chat.title == "New chat" { chat.title = chat.draft.trim().chars().take(80).collect(); if chat.title.is_empty() { chat.title = "File conversation".into(); } }
            chat.draft.clear();
        }
        let settings = self.preferences.settings(chat.provider);
        if settings.api_key.is_empty() { self.snapshot.error = Some(format!("Configure a {} API key in AI Chat Settings", chat.provider.label())); return; }
        let Some(store) = &self.store else { return; };
        let request = match provider::prepare(&chat, store) { Ok(request) => request, Err(error) => { self.snapshot.error = Some(error); return; } };
        let key = settings.api_key.clone();
        // The user turn and resumable placeholder reach disk together before networking.
        chat.messages.push(ChatMessage { role: Role::Assistant, text: String::new(), provider: chat.provider, model: chat.model.clone(), status: MessageStatus::Streaming });
        if let Err(error) = store.save(&chat) {
            if let Some(message) = chat.messages.last_mut() { message.status = MessageStatus::Failed; }
            self.snapshot.error = Some(format!("Cannot save user turn: {error}. Retry saving before retrying the answer."));
            self.unsaved.insert(id.into()); self.loaded.insert(id.into(), chat); return;
        }
        for summary in &mut self.snapshot.chats { if summary.id == id { summary.title = chat.title.clone(); } }
        self.loaded.insert(id.into(), chat);
        self.generation = self.generation.wrapping_add(1);
        self.snapshot.request_revision = self.generation;
        let generation = self.generation;
        let task = tokio::spawn(provider::stream(request, key, id.into(), generation, events.clone()));
        self.active = Some(Active { chat: id.into(), generation, task });
    }
    fn stop(&mut self) {
        self.stop_request(ActivityStatus::Canceled,"Response stopped.");
    }
    fn stop_request(&mut self, status: ActivityStatus, detail: &str) {
        let Some(active) = self.active.take() else { return; };
        active.task.abort();
        if let Some(message) = self.loaded.get_mut(&active.chat).and_then(|chat| chat.messages.last_mut()) && message.status == MessageStatus::Streaming { message.status = MessageStatus::Stopped; }
        self.changed(&active.chat);
        self.activity.record(ActivitySource::AiChat,status,detail);
    }
    fn network(&mut self, event: NetworkEvent) {
        if !self.active.as_ref().is_some_and(|active| active.chat == event.chat && active.generation == event.generation) { return; }
        let Some(message) = self.loaded.get_mut(&event.chat).and_then(|chat| chat.messages.last_mut()) else { return; };
        let outcome = match event.event {
            provider::Update::Text(text) => {
                if message.text.len().saturating_add(text.len()) > MAX_RESPONSE {
                    message.status = MessageStatus::Interrupted;
                    self.snapshot.error = Some("AI response exceeds 1 MiB".into());
                    if let Some(active) = self.active.take() { active.task.abort(); }
                    Some(ActivityStatus::Failed)
                } else { message.text.push_str(&text); None }
            }
            provider::Update::Complete => { message.status = MessageStatus::Complete; Some(ActivityStatus::Completed) }
            provider::Update::Failed(error) => { message.status = MessageStatus::Failed; self.snapshot.error = Some(error); Some(ActivityStatus::Failed) }
            provider::Update::Interrupted(error) => { message.status = MessageStatus::Interrupted; self.snapshot.error = Some(error); Some(ActivityStatus::Failed) }
        };
        self.changed(&event.chat);
        if let Some(status) = outcome {
            self.active = None;
            self.persist();
            let (status,detail) = match status {
                ActivityStatus::Completed if self.unsaved.contains(&event.chat) => (ActivityStatus::Attention,"Response completed. Saving it needs attention in AI Chat."),
                ActivityStatus::Completed => (status,"Response completed."),
                _ => (status,"Response failed or was interrupted. See AI Chat for details."),
            };
            self.activity.record(ActivitySource::AiChat,status,detail);
        }
        else if self.last_save.elapsed() >= Duration::from_secs(1) { self.persist(); }
        if !self.unsaved.is_empty() && self.snapshot.error.as_ref().is_some_and(|error| error.starts_with("Cannot save AI chat")) {
            self.stop_request(ActivityStatus::Failed,"Response stopped because it could not be saved. See AI Chat for details.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn actor(directory: PathBuf) -> Actor {
        Actor { store: Some(store::Store::open(directory).unwrap()), preferences: AiPreferences::default(), snapshot: ChatSnapshot { model_defaults: std::array::from_fn(|_| String::new()), navigation_revision: 0, revision: 1, flush_revision: 0, request_revision: 0, chats: Vec::new(), current: None, busy: false, loading: false, error: None, dirty: false, writable: true, status: String::new() }, loaded: Default::default(), unsaved: Default::default(), active: None, generation: 0, last_save: Instant::now(), discarded: false, io_busy: false, activity: ActivityLog::default() }
    }
    #[test]
    fn bootstrap_uses_ai_configuration_without_reading_system_preferences() {
        let temp = crate::test_support::TestDir::new(); let legacy = temp.path().join("preferences.toml");
        let store = crate::config::ai::AiSettingsStore::at(temp.path().join("ai.toml"),legacy.clone());
        let mut settings = AiPreferences { default_provider: AiProvider::Claude,..AiPreferences::default() };
        settings.claude.model = "separate-model".into(); store.save(&settings,&AiPreferences::default()).unwrap(); std::fs::write(&legacy,"broken system settings").unwrap();
        let service = ChatService::start(temp.path().join("state"),AiPreferences::default(),Some(store),ActivityLog::default()).unwrap();
        let until = Instant::now()+Duration::from_secs(4);
        loop {
            if let Some(snapshot) = service.poll() && !snapshot.loading {
                let chat = snapshot.current.unwrap(); assert_eq!(chat.provider,AiProvider::Claude); assert_eq!(chat.model,"separate-model"); assert_eq!(snapshot.model_defaults[1],"separate-model"); break;
            }
            assert!(Instant::now()<until); std::thread::yield_now();
        }
        drop(service);
    }

    #[test]
    fn missing_credentials_and_incompatible_files_never_consume_composer() {
        let temp = crate::test_support::TestDir::new();
        let mut actor = actor(temp.path().join("ai")); actor.new_chat(); actor.persist();
        let id = actor.current_id().unwrap();
        actor.loaded.get_mut(&id).unwrap().draft = "hello".into();
        let (tx, _) = mpsc::channel(8);
        actor.send(&id, false, &tx);
        assert!(actor.snapshot.error.as_ref().unwrap().contains("API key"));
        assert_eq!(actor.activity.read_since(None).unwrap().1[0].status,ActivityStatus::Failed);
        assert_eq!(actor.loaded[&id].draft, "hello"); assert!(actor.loaded[&id].messages.is_empty());
        actor.preferences.openai.api_key = crate::config::ai::ApiKey::new("offline-fake").unwrap();
        actor.loaded.get_mut(&id).unwrap().model = "manual-model".into();
        actor.loaded.get_mut(&id).unwrap().attachments.push(Attachment { id: "a-1".into(), name: "unknown.bin".into(), mime: "application/octet-stream".into(), size: 1, included: true, message: None, error: None });
        actor.send(&id, false, &tx);
        assert!(actor.snapshot.error.as_ref().unwrap().contains("unknown.bin"));
        assert_eq!(actor.loaded[&id].draft, "hello"); assert!(actor.active.is_none());
    }
    #[test]
    fn stale_scoped_draft_and_send_cannot_target_newly_selected_chat() {
        let temp = crate::test_support::TestDir::new();
        let mut actor = actor(temp.path().join("ai")); actor.new_chat(); actor.persist();
        let old = actor.current_id().unwrap();
        actor.loaded.get_mut(&old).unwrap().draft = "old composer".into();
        actor.new_chat(); let new = actor.current_id().unwrap();
        actor.loaded.get_mut(&new).unwrap().draft = "new composer".into();
        let (tx, _) = mpsc::channel(8);
        actor.command(ChatCommand::Scoped { chat: old.clone(), command: Box::new(ChatCommand::Draft("stale editor text".into())) }, &tx);
        assert_eq!(actor.loaded[&new].draft, "new composer");
        assert_eq!(actor.loaded[&old].draft, "old composer");
        actor.command(ChatCommand::Scoped { chat: old, command: Box::new(ChatCommand::Send) }, &tx);
        assert!(actor.active.is_none()); assert!(actor.loaded[&new].messages.is_empty());
        assert!(actor.snapshot.error.as_ref().unwrap().contains("changed"));
        actor.command(ChatCommand::Scoped { chat: new.clone(), command: Box::new(ChatCommand::Draft("correct editor".into())) }, &tx);
        assert_eq!(actor.loaded[&new].draft, "correct editor");
    }
    #[tokio::test(flavor = "current_thread")]
    async fn background_request_records_terminal_outcomes_once_without_private_text() {
        let temp = crate::test_support::TestDir::new();
        let mut actor = actor(temp.path().join("ai")); actor.new_chat(); actor.persist();
        let original = actor.current_id().unwrap();
        actor.loaded.get_mut(&original).unwrap().messages.push(ChatMessage {
            role: Role::Assistant,text: "private transcript".into(),provider: AiProvider::OpenAi,model: "manual".into(),status: MessageStatus::Streaming,
        });
        actor.new_chat();
        actor.active = Some(Active { chat: original.clone(),generation: 5,task: tokio::spawn(std::future::pending()) });
        actor.network(NetworkEvent { chat: original.clone(),generation: 4,event: provider::Update::Complete });
        actor.network(NetworkEvent { chat: original.clone(),generation: 5,event: provider::Update::Text(" extra private text".into()) });
        assert_eq!(actor.activity.unread(),0);
        actor.network(NetworkEvent { chat: original.clone(),generation: 5,event: provider::Update::Complete });
        actor.network(NetworkEvent { chat: original.clone(),generation: 5,event: provider::Update::Complete });
        assert_eq!(actor.activity.unread(),1);
        actor.active = Some(Active { chat: original.clone(),generation: 6,task: tokio::spawn(std::future::pending()) });
        actor.network(NetworkEvent { chat: original,generation: 6,event: provider::Update::Interrupted("private provider body".into()) });
        let entries = actor.activity.read_since(None).unwrap().1;
        assert_eq!(entries.iter().map(|entry| entry.status).collect::<Vec<_>>(),[ActivityStatus::Failed,ActivityStatus::Completed]);
        assert!(entries.iter().all(|entry| entry.source == ActivitySource::AiChat && !entry.detail.contains("private")));
    }
    #[tokio::test(flavor = "current_thread")]
    async fn switching_selection_keeps_origin_and_stop_ignores_late_events() {
        let temp = crate::test_support::TestDir::new();
        let mut actor = actor(temp.path().join("ai")); actor.new_chat(); actor.persist();
        let original = actor.current_id().unwrap();
        let message = ChatMessage { role: Role::Assistant, text: String::new(), provider: AiProvider::OpenAi, model: "manual".into(), status: MessageStatus::Streaming };
        actor.loaded.get_mut(&original).unwrap().messages.push(message);
        actor.active = Some(Active { chat: original.clone(), generation: 5, task: tokio::spawn(std::future::pending()) });
        actor.new_chat(); let selected = actor.current_id().unwrap();
        actor.network(NetworkEvent { chat: original.clone(), generation: 4, event: provider::Update::Text("stale generation".into()) });
        actor.network(NetworkEvent { chat: selected.clone(), generation: 5, event: provider::Update::Text("wrong chat".into()) });
        actor.network(NetworkEvent { chat: original.clone(), generation: 5, event: provider::Update::Text("partial".into()) });
        assert_eq!(actor.loaded[&original].messages[0].text, "partial"); assert!(actor.loaded[&selected].messages.is_empty());
        actor.stop();
        actor.network(NetworkEvent { chat: original.clone(), generation: 5, event: provider::Update::Text("late".into()) });
        assert_eq!(actor.loaded[&original].messages[0].status, MessageStatus::Stopped);
        assert_eq!(actor.loaded[&original].messages[0].text, "partial");
    }
    #[test]
    fn bounded_submit_and_revision_poll_are_nonblocking() {
        let shared = Arc::new(Shared { configuration: Mutex::new(None), snapshot: Mutex::new(ChatSnapshot { model_defaults: std::array::from_fn(|_| String::new()), navigation_revision: 0, revision: 1, flush_revision: 0, request_revision: 0, chats: Vec::new(), current: None, busy: false, loading: true, error: None, dirty: false, writable: false, status: String::new() }), shutdown: AtomicBool::new(false), wake: Notify::new() });
        let (tx, _rx) = mpsc::channel(2);
        let service = ChatService { tx, shared: shared.clone(), polled: AtomicU64::new(0) };
        assert!(service.poll().unwrap().loading); assert!(service.poll().is_none());
        service.submit(ChatCommand::Draft("one".into())).unwrap(); service.submit(ChatCommand::Draft("two".into())).unwrap();
        assert!(service.submit(ChatCommand::New).is_err());
        let preferences = AiPreferences { default_provider: AiProvider::Gemini,..AiPreferences::default() };
        service.submit(ChatCommand::Configure(preferences.clone())).unwrap();
        assert_eq!(shared.configuration.lock().unwrap().as_ref(),Some(&preferences),"saved settings bypass a full draft queue");
        assert!(service.submit(ChatCommand::Draft("x".repeat(MAX_DRAFT + 1))).is_err());
        let guard = shared.snapshot.lock().unwrap(); assert!(service.poll().is_none()); drop(guard);
    }
    #[test]
    fn queued_draft_survives_drop_and_reopen_without_network() {
        let temp = crate::test_support::TestDir::new(); let path = temp.path().join("ai");
        let service = ChatService::new_at(path.clone(), AiPreferences::default()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        let id = loop {
            if let Some(snapshot) = service.poll() && !snapshot.loading { break snapshot.current.unwrap().id; }
            assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(2));
        };
        service.submit(ChatCommand::Draft("queued ไทย draft".into())).unwrap(); drop(service);
        loop {
            let store = store::Store::open(path.clone()).unwrap();
            if store.writable { assert_eq!(store.load(&id).unwrap().draft, "queued ไทย draft"); break; }
            assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn flush_ack_follows_queued_draft_persistence() {
        let temp = crate::test_support::TestDir::new(); let path = temp.path().join("ai");
        let service = ChatService::new_at(path.clone(), AiPreferences::default()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        let before = loop {
            if let Some(snapshot) = service.poll() && !snapshot.loading { break snapshot; }
            assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(2));
        };
        service.submit(ChatCommand::Draft("saved before acknowledgement".into())).unwrap();
        service.submit(ChatCommand::Flush).unwrap();
        let after = loop {
            if let Some(snapshot) = service.poll() && snapshot.flush_revision > before.flush_revision { break snapshot; }
            assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(2));
        };
        assert!(!after.busy && !after.dirty);
        let store = store::Store::open(path).unwrap();
        assert_eq!(store.load(&after.current.unwrap().id).unwrap().draft, "saved before acknowledgement");
    }
    #[tokio::test(flavor = "current_thread")]
    async fn retry_replaces_incomplete_attempt_and_preserves_user_turn_source() {
        let temp = crate::test_support::TestDir::new();
        let mut actor = actor(temp.path().join("ai")); actor.new_chat(); actor.persist();
        let id = actor.current_id().unwrap();
        actor.preferences.openai.api_key = crate::config::ai::ApiKey::new("offline-fake").unwrap();
        actor.preferences.gemini.api_key = crate::config::ai::ApiKey::new("offline-fake").unwrap();
        actor.loaded.get_mut(&id).unwrap().model = "manual-openai".into();
        actor.loaded.get_mut(&id).unwrap().draft = "one user turn".into();
        let (tx, _) = mpsc::channel(8);
        // No await occurs between spawn and stop, so these futures never perform I/O.
        actor.send(&id, false, &tx); actor.stop(); actor.persist();
        assert_eq!(actor.snapshot.request_revision, 1);
        assert_eq!(actor.loaded[&id].messages.len(), 2);
        actor.loaded.get_mut(&id).unwrap().provider = AiProvider::Gemini;
        actor.loaded.get_mut(&id).unwrap().model = "manual-gemini".into();
        actor.send(&id, true, &tx); actor.stop(); actor.persist();
        assert_eq!(actor.snapshot.request_revision, 2);
        let messages = &actor.loaded[&id].messages;
        assert_eq!(messages.len(), 2); assert_eq!(messages[0].text, "one user turn");
        assert_eq!(messages[0].provider, AiProvider::OpenAi); assert_eq!(messages[1].provider, AiProvider::Gemini);
        assert_eq!(messages[1].model, "manual-gemini");
    }
    #[tokio::test(flavor = "current_thread")]
    async fn save_failure_keeps_retryable_turn_without_sending_or_replacing_disk() {
        let temp = crate::test_support::TestDir::new(); let root = temp.path().join("ai");
        let mut actor = actor(root.clone()); actor.new_chat(); actor.persist();
        let id = actor.current_id().unwrap();
        actor.preferences.openai.api_key = crate::config::ai::ApiKey::new("offline-fake").unwrap();
        actor.loaded.get_mut(&id).unwrap().model = "manual".into(); actor.loaded.get_mut(&id).unwrap().draft = "keep me".into();
        let summary = root.join("chats").join(&id).join("summary.json");
        std::fs::remove_file(&summary).unwrap(); std::os::unix::fs::symlink(temp.path().join("outside"), &summary).unwrap();
        let (tx, _) = mpsc::channel(8); actor.send(&id, false, &tx);
        assert!(actor.active.is_none()); assert!(actor.unsaved.contains(&id));
        assert_eq!(actor.loaded[&id].messages.last().unwrap().status, MessageStatus::Failed);
        assert!(actor.store.as_ref().unwrap().load(&id).unwrap().messages.is_empty());
        std::fs::remove_file(summary).unwrap(); actor.command(ChatCommand::Flush, &tx);
        assert!(actor.unsaved.is_empty()); assert_eq!(actor.snapshot.flush_revision, 1);
        assert_eq!(actor.store.as_ref().unwrap().load(&id).unwrap().messages[0].text, "keep me");
    }
}
