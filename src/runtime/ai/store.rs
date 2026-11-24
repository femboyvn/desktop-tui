//! Private, versioned chat storage. The exclusive lease also gates API generation.
use super::*;
use anyhow::{Context, Result, ensure};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    fn conversation() -> Conversation {
        Conversation { id: "c-123".into(), title: "test".into(), provider: AiProvider::OpenAi, model: "manual".into(), messages: vec![ChatMessage { role: Role::Assistant, text: "partial".into(), provider: AiProvider::OpenAi, model: "manual".into(), status: MessageStatus::Streaming }], draft: "preserved".into(), attachments: Vec::new() }
    }
    #[test]
    fn private_atomic_and_second_instance_read_only_and_reopen_interrupted() {
        let temp = crate::test_support::TestDir::new();
        let first = Store::open(temp.path().join("ai")).unwrap();
        let second = Store::open(temp.path().join("ai")).unwrap();
        assert!(first.writable); assert!(!second.writable);
        first.save(&conversation()).unwrap();
        assert_eq!(fs::metadata(temp.path().join("ai/chats/c-123/conversation.json")).unwrap().permissions().mode() & 0o777, 0o600);
        let loaded = second.load("c-123").unwrap();
        assert_eq!(loaded.messages[0].status, MessageStatus::Interrupted);
        assert_eq!(loaded.draft, "preserved");
        assert!(second.save(&conversation()).is_err());
        let mut invalid = conversation(); invalid.draft = "x".repeat(MAX_DRAFT + 1);
        assert!(first.save(&invalid).is_err());
        assert_eq!(first.load("c-123").unwrap().draft, "preserved");
    }
    #[test]
    fn corrupt_chat_and_unsafe_ids_and_storage_symlinks_are_rejected() {
        let temp = crate::test_support::TestDir::new();
        let store = Store::open(temp.path().join("ai")).unwrap();
        store.save(&conversation()).unwrap();
        assert!(store.load("../outside").is_err());
        fs::write(temp.path().join("ai/chats/c-123/conversation.json"), b"broken").unwrap();
        assert!(store.load("c-123").is_err());
        assert!(store.list().unwrap().iter().any(|chat| chat.id == "c-123"));
        std::os::unix::fs::symlink(temp.path().join("ai"), temp.path().join("link")).unwrap();
        assert!(Store::open(temp.path().join("link")).is_err());
    }
    #[test]
    fn file_snapshot_ignores_original_changes_and_rejects_special_files() {
        let temp = crate::test_support::TestDir::new();
        let source = temp.path().join("code with spaces.unknown");
        fs::write(&source, "hello ไทย").unwrap();
        let store = Store::open(temp.path().join("ai")).unwrap();
        store.save(&conversation()).unwrap();
        let attachment = store.import("c-123", &source).unwrap();
        fs::write(&source, "changed").unwrap();
        assert_eq!(store.bytes("c-123", &attachment).unwrap(), "hello ไทย".as_bytes());
        assert_eq!(attachment.mime, "text/plain");
        assert!(store.import("c-123", Path::new("/dev/null")).is_err());
    }
    #[test]
    fn file_transcript_caps_sniffing_and_corrupt_attachment_bounds() {
        let temp = crate::test_support::TestDir::new();
        let store = Store::open(temp.path().join("ai")).unwrap();
        let mut chat = conversation(); store.save(&chat).unwrap();
        let oversized = temp.path().join("oversize");
        File::create(&oversized).unwrap().set_len(MAX_FILE + 1).unwrap();
        assert!(store.import("c-123", &oversized).is_err());
        for _ in 0..5 { chat.messages.push(ChatMessage { text: "x".repeat(MAX_RESPONSE), ..chat.messages[0].clone() }); }
        assert!(store.save(&chat).is_err()); assert_eq!(store.load("c-123").unwrap().messages.len(), 1);
        assert_eq!(sniff(b"%PDF-1.5\n", Path::new("without-extension")), "application/pdf");
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n", Path::new("image.txt")), "image/png");
        assert_eq!(sniff(b"\xff\x00\x01", Path::new("fake.pdf")), "application/octet-stream");
        assert_eq!(sniff(b"hello code", Path::new("unknown.bin")), "text/plain");
        assert_eq!(sniff(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1", Path::new("legacy.XLS")), "application/vnd.ms-excel");
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00AVI ", Path::new("no-extension")), "video/avi");
        assert_eq!(sniff(b"\x00\x00\x00\x18ftypqt  ", Path::new("clip.bin")), "video/mov");
        assert_eq!(sniff(b"\x00\x00\x01\xba", Path::new("movie")), "video/mpeg");
        File::create(temp.path().join("ai/chats/c-123/conversation.json")).unwrap().set_len(MAX_TRANSCRIPT + 1).unwrap();
        assert!(store.load("c-123").is_err());
    }
}

pub(super) struct Store { root: PathBuf, _lease: File, pub writable: bool }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved { version: u8, conversation: Conversation }
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
pub(super) fn id(prefix: &str) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{prefix}-{now:x}-{:x}-{:x}", std::process::id(), NEXT_ID.fetch_add(1, Ordering::Relaxed))
}
fn valid_id(id: &str) -> bool { !id.is_empty() && id.len() <= 128 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') }
fn no_symlinks(path: &Path) -> Result<()> {
    let mut parent = PathBuf::new();
    for component in path.components() {
        ensure!(!matches!(component, std::path::Component::ParentDir), "Unsafe AI storage path");
        parent.push(component);
        match fs::symlink_metadata(&parent) {
            Ok(metadata) => ensure!(!metadata.file_type().is_symlink(), "AI storage path cannot contain symlinks"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error).context("Cannot inspect AI storage"),
        }
    }
    Ok(())
}
fn private_dir(path: &Path) -> Result<()> {
    no_symlinks(path)?;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    let metadata = fs::symlink_metadata(path)?;
    ensure!(metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() }, "AI directory must be owned by this user");
    ensure!(metadata.permissions().mode() & 0o777 == 0o700, "AI directory must have private 0700 permissions");
    Ok(())
}
fn read_private(path: &Path, cap: u64) -> Result<Vec<u8>> {
    no_symlinks(path)?;
    let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path).context("Cannot read AI state")?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() } && metadata.nlink() == 1, "AI state must be an owned regular file");
    ensure!(metadata.len() <= cap, "AI state exceeds its size limit");
    let mut bytes = Vec::new(); file.take(cap + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= cap, "AI state exceeds its size limit");
    Ok(bytes)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    no_symlinks(path)?;
    let parent = path.parent().context("Invalid AI state path")?;
    private_dir(parent)?;
    if let Ok(metadata) = fs::symlink_metadata(path) { ensure!(metadata.is_file() && metadata.nlink() == 1 && metadata.uid() == unsafe { libc::geteuid() }, "AI destination must be an owned regular file"); }
    let temporary = parent.join(id("write"));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&temporary)?;
        file.write_all(bytes)?; file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temporary); }
    result
}
impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        private_dir(&root)?;
        private_dir(&root.join("chats"))?;
        let lease_path = root.join("writer.lock"); no_symlinks(&lease_path)?;
        let lease = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(lease_path)?;
        let metadata = lease.metadata()?;
        ensure!(metadata.is_file() && metadata.nlink() == 1 && metadata.uid() == unsafe { libc::geteuid() }, "Invalid AI writer lock");
        let writable = unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if !writable { let error = std::io::Error::last_os_error(); ensure!(error.raw_os_error() == Some(libc::EWOULDBLOCK), "Cannot acquire AI writer lock"); }
        Ok(Self { root, _lease: lease, writable })
    }
    fn directory(&self, chat: &str) -> Result<PathBuf> {
        ensure!(valid_id(chat), "Unsafe chat identity");
        let path = self.root.join("chats").join(chat); no_symlinks(&path)?; Ok(path)
    }
    pub fn list(&self) -> Result<Vec<ChatSummary>> {
        no_symlinks(&self.root.join("chats"))?;
        let mut chats = Vec::new();
        for entry in fs::read_dir(self.root.join("chats"))? {
            let entry = entry?;
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else { continue; };
            if !valid_id(&id) || !entry.file_type()?.is_dir() { continue; }
            ensure!(chats.len() < 4096, "AI history exceeds 4096 chats");
            let summary = read_private(&entry.path().join("summary.json"), 16 * 1024).ok().and_then(|bytes| serde_json::from_slice::<ChatSummary>(&bytes).ok()).filter(|summary| summary.id == id && summary.title.len() <= 1024);
            chats.push(summary.unwrap_or(ChatSummary { title: format!("Chat {id}"), id }));
        }
        chats.sort_by(|a, b| b.id.cmp(&a.id)); Ok(chats)
    }
    pub fn load(&self, id: &str) -> Result<Conversation> {
        let bytes = read_private(&self.directory(id)?.join("conversation.json"), MAX_TRANSCRIPT)?;
        let saved: Saved = serde_json::from_slice(&bytes).context("Chat transcript is corrupt")?;
        ensure!(saved.version == 1 && saved.conversation.id == id, "Unsupported chat version or identity");
        validate(&saved.conversation)?;
        let mut conversation = saved.conversation;
        for message in &mut conversation.messages { if message.status == MessageStatus::Streaming { message.status = MessageStatus::Interrupted; } }
        Ok(conversation)
    }
    pub fn save(&self, conversation: &Conversation) -> Result<()> {
        ensure!(self.writable, "AI Chat is read-only: another instance owns history");
        validate(conversation)?;
        let bytes = serde_json::to_vec(&Saved { version: 1, conversation: conversation.clone() })?;
        ensure!(bytes.len() as u64 <= MAX_TRANSCRIPT, "Chat transcript exceeds 4 MiB");
        let directory = self.directory(&conversation.id)?;
        let summary = ChatSummary { id: conversation.id.clone(), title: conversation.title.clone() };
        // A summary is a cache; commit it first so a transcript failure preserves
        // the previously durable conversation in full.
        atomic_write(&directory.join("summary.json"), &serde_json::to_vec(&summary)?)?;
        atomic_write(&directory.join("conversation.json"), &bytes)?;
        Ok(())
    }
    pub fn import(&self, chat: &str, source: &Path) -> Result<Attachment> {
        ensure!(self.writable, "AI Chat is read-only");
        // O_NONBLOCK prevents a selected FIFO from blocking before metadata validation.
        // A user-selected symlink is allowed; only its regular-file target is read.
        let file = OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(source).context("Cannot open attachment")?;
        let metadata = file.metadata()?;
        ensure!(metadata.is_file(), "Only regular files can be attached");
        ensure!(metadata.len() <= MAX_FILE, "Attachment exceeds 10 MiB");
        let mut bytes = Vec::new(); file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= MAX_FILE, "Attachment exceeds 10 MiB");
        let name = source.file_name().context("Attachment has no filename")?.to_string_lossy().chars().take(1024).collect::<String>();
        let mime = sniff(&bytes, source).to_owned();
        let id = id("a");
        let destination = self.directory(chat)?.join("attachments").join(&id);
        atomic_write(&destination, &bytes)?;
        Ok(Attachment { id, name, mime, size: bytes.len() as u64, included: true, message: None, error: None })
    }
    pub fn bytes(&self, chat: &str, attachment: &Attachment) -> Result<Vec<u8>> {
        ensure!(valid_id(&attachment.id), "Unsafe attachment identity");
        let bytes = read_private(&self.directory(chat)?.join("attachments").join(&attachment.id), MAX_FILE)?;
        ensure!(bytes.len() as u64 == attachment.size, "Attachment snapshot size changed");
        Ok(bytes)
    }
    pub fn remove_attachment(&self, chat: &str, id: &str) -> Result<()> {
        ensure!(self.writable && valid_id(id), "Cannot remove attachment");
        let path = self.directory(chat)?.join("attachments").join(id); no_symlinks(&path)?;
        match fs::remove_file(path) { Ok(()) => Ok(()), Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()), Err(error) => Err(error.into()) }
    }
    pub fn delete(&self, chat: &str) -> Result<()> {
        ensure!(self.writable, "AI Chat is read-only");
        let directory = self.directory(chat)?;
        // Never traverse unrecognized directories or a symlink during deletion.
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            if entry.file_name() == "attachments" {
                ensure!(entry.file_type()?.is_dir(), "Invalid attachment directory");
                for file in fs::read_dir(entry.path())? { let file = file?; ensure!(file.file_type()?.is_file(), "Invalid stored attachment"); fs::remove_file(file.path())?; }
                fs::remove_dir(entry.path())?;
            } else { ensure!(entry.file_type()?.is_file(), "Invalid chat directory contents"); fs::remove_file(entry.path())?; }
        }
        fs::remove_dir(directory)?; Ok(())
    }
}
fn validate(conversation: &Conversation) -> Result<()> {
    ensure!(valid_id(&conversation.id) && conversation.title.len() <= 1024 && conversation.model.len() <= 256, "Invalid chat identity/title/model");
    ensure!(conversation.draft.len() <= MAX_DRAFT && conversation.messages.len() <= 4096 && conversation.attachments.len() <= 1024, "Chat exceeds document limits");
    let mut identities = std::collections::BTreeSet::new();
    for attachment in &conversation.attachments {
        ensure!(valid_id(&attachment.id) && identities.insert(&attachment.id) && attachment.name.len() <= 4096 && attachment.mime.len() <= 256 && attachment.size <= MAX_FILE && attachment.error.as_ref().is_none_or(|text| text.len() <= 4096), "Invalid attachment metadata");
        if let Some(index) = attachment.message { ensure!(conversation.messages.get(index).is_some_and(|message| message.role == Role::User), "Attachment refers to an invalid message"); }
    }
    for message in &conversation.messages { ensure!(message.text.len() <= MAX_RESPONSE && message.model.len() <= 256, "Message exceeds chat limits"); }
    Ok(())
}
fn sniff(bytes: &[u8], path: &Path) -> &'static str {
    if bytes.starts_with(b"%PDF-") { return "application/pdf"; }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") { return "image/png"; }
    if bytes.starts_with(b"\xff\xd8\xff") { return "image/jpeg"; }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") { return "image/gif"; }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") { return "image/webp"; }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") { return "audio/wav"; }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"AVI ") { return "video/avi"; }
    if bytes.starts_with(b"FORM") && matches!(bytes.get(8..12), Some(b"AIFF" | b"AIFC")) { return "audio/aiff"; }
    if bytes.starts_with(b"fLaC") { return "audio/flac"; }
    if bytes.starts_with(b"OggS") { return "audio/ogg"; }
    if bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xf6 == 0xf0 { return "audio/aac"; }
    if bytes.starts_with(b"ID3") || bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0 { return "audio/mpeg"; }
    if bytes.get(4..8) == Some(b"ftyp") {
        let brand = bytes.get(8..12).unwrap_or_default();
        if matches!(brand, b"heic" | b"heix" | b"hevc" | b"hevx") { return "image/heic"; }
        if matches!(brand, b"mif1" | b"msf1") { return "image/heif"; }
        if matches!(brand, b"avif" | b"avis") { return "application/octet-stream"; }
        if brand == b"qt  " { return "video/mov"; }
        if brand.starts_with(b"3gp") { return "video/3gpp"; }
        return if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("m4a")) { "audio/m4a" } else { "video/mp4" };
    }
    if bytes.starts_with(b"\x1a\x45\xdf\xa3") { return "video/webm"; }
    if bytes.starts_with(b"\x00\x00\x01\xba") || bytes.starts_with(b"\x00\x00\x01\xb3") { return "video/mpeg"; }
    if bytes.starts_with(b"FLV") { return "video/x-flv"; }
    if bytes.starts_with(b"PK\x03\x04") {
        if bytes.windows(5).any(|window| window == b"word/") { return "application/vnd.openxmlformats-officedocument.wordprocessingml.document"; }
        if bytes.windows(4).any(|window| window == b"ppt/") { return "application/vnd.openxmlformats-officedocument.presentationml.presentation"; }
        if bytes.windows(3).any(|window| window == b"xl/") { return "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"; }
    }
    if bytes.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") {
        let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or("").to_ascii_lowercase();
        return match extension.as_str() { "doc" => "application/msword", "xls" => "application/vnd.ms-excel", "ppt" => "application/vnd.ms-powerpoint", _ => "application/octet-stream" };
    }
    if std::str::from_utf8(bytes).is_ok() && !bytes.iter().any(|&byte| byte == 0 || byte < 0x09 || byte > 0x0d && byte < 0x20) { return "text/plain"; }
    "application/octet-stream"
}
