use serde::{Deserialize, Serialize};
use std::{collections::{HashMap,HashSet}, io::{Read,Write}, os::unix::fs::OpenOptionsExt, path::{Path,PathBuf},
    sync::{Arc,Mutex,mpsc,atomic::{AtomicU64,Ordering}}};

pub(crate) const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_COLUMNS: usize = 64;
const MAX_TASKS: usize = 4096;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Serialize,Deserialize)]
pub(crate) enum Priority { Low, #[default] Medium, High }
impl Priority { pub fn label(self) -> &'static str { match self { Self::Low => "Low",Self::Medium => "Medium",Self::High => "High" } } }
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Task { pub id: u64, pub title: String, pub notes: String, pub priority: Priority, pub due: String }
impl Task {
    pub fn draft(title: &str) -> Self { Self { id: 0,title: title.into(),notes: String::new(),priority: Priority::Medium,due: String::new() } }
}
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Column { pub id: u64, pub name: String, pub tasks: Vec<Task> }
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Board { version: u32, pub name: String, pub columns: Vec<Column>, next_id: u64 }
#[derive(Clone,Debug)]
pub(crate) struct Document { pub board: Board, saved: Board }
impl Default for Document {
    fn default() -> Self {
        let board = Board { version: 1,name: "Untitled".into(),next_id: 5,columns: ["To do","In progress","Review","Done"].into_iter().enumerate()
            .map(|(i,name)| Column { id: i as u64+1,name: name.into(),tasks: Vec::new() }).collect() };
        Self { saved: board.clone(),board }
    }
}
impl Document {
    pub fn is_dirty(&self) -> bool { self.board != self.saved }
    pub fn mark_saved(&mut self) { self.saved = self.board.clone(); }
    /// Apply validated edits transactionally. Failed edits never alter the board or saved state.
    pub fn edit<T>(&mut self, change: impl FnOnce(&mut Board) -> Result<T,String>) -> Result<T,String> {
        let mut next = self.board.clone(); let result = change(&mut next)?; validate(&next)?; self.board = next; Ok(result)
    }
    pub fn add_column(&mut self, name: String) -> Result<u64,String> {
        self.edit(|board| { let id = board.allocate()?; board.columns.push(Column { id,name,tasks: Vec::new() }); Ok(id) })
    }
    pub fn add_task(&mut self, column: usize, mut task: Task) -> Result<u64,String> {
        self.edit(|board| { let id = board.allocate()?; task.id = id;
            board.columns.get_mut(column).ok_or("Column no longer exists")?.tasks.push(task); Ok(id) })
    }
    pub fn locate(&self, id: u64) -> Option<(usize,usize)> {
        self.board.columns.iter().enumerate().find_map(|(c,column)| column.tasks.iter().position(|task| task.id == id).map(|t| (c,t)))
    }
    /// Position is the insertion index in the destination after removing the task.
    pub fn move_task(&mut self, id: u64, column: usize, position: usize) -> Result<(),String> {
        let (source,index) = self.locate(id).ok_or("Task no longer exists")?;
        self.edit(|board| {
            if column >= board.columns.len() { return Err("Column no longer exists".into()); }
            let task = board.columns[source].tasks.remove(index);
            let destination = &mut board.columns[column].tasks;
            if position > destination.len() { return Err("Invalid task position".into()); }
            destination.insert(position,task); Ok(())
        })
    }
    pub fn delete_column(&mut self, index: usize, destination: Option<usize>) -> Result<(),String> {
        self.edit(|board| {
            if board.columns.len() <= 1 || index >= board.columns.len() { return Err("Keep at least one column".into()); }
            if !board.columns[index].tasks.is_empty() {
                let target = destination.filter(|target| *target != index && *target < board.columns.len()).ok_or("Choose another column for these tasks")?;
                let tasks = std::mem::take(&mut board.columns[index].tasks); board.columns[target].tasks.extend(tasks);
            }
            board.columns.remove(index); Ok(())
        })
    }
}
impl Board {
    fn allocate(&mut self) -> Result<u64,String> { let id = self.next_id; self.next_id = id.checked_add(1).ok_or("Board ID limit reached")?; Ok(id) }
}
fn valid_name(value: &str, max: usize) -> bool { !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control) }
pub(crate) fn validate_date(value: &str) -> Result<(),String> {
    if value.is_empty() { return Ok(()); }
    let bytes = value.as_bytes();
    if bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-'
        && bytes.iter().enumerate().all(|(i,b)| i == 4 || i == 7 || b.is_ascii_digit()) && &value[..4] != "0000"
        && chrono::NaiveDate::parse_from_str(value,"%Y-%m-%d").is_ok() { Ok(()) }
    else { Err("Due date must be a valid YYYY-MM-DD date, or empty".into()) }
}
fn validate(board: &Board) -> Result<(),String> {
    if board.version != 1 { return Err("Unsupported Kanban file version".into()); }
    if !valid_name(&board.name,256) || board.columns.is_empty() || board.columns.len() > MAX_COLUMNS { return Err("Use a board name and 1–64 columns".into()); }
    let mut ids = HashSet::new(); let mut count = 0;
    for column in &board.columns {
        if column.id == 0 || !ids.insert(column.id) || !valid_name(&column.name,256) { return Err("Invalid column name or duplicate ID".into()); }
        count += column.tasks.len();
        for task in &column.tasks {
            if task.id == 0 || !ids.insert(task.id) || !valid_name(&task.title,1024) || task.notes.len() > 65536
                || task.notes.chars().any(|ch| ch.is_control() && ch != '\n' && ch != '\r' && ch != '\t') { return Err("Invalid task title, notes, or duplicate ID".into()); }
            validate_date(&task.due)?;
        }
    }
    if count > MAX_TASKS || ids.iter().any(|id| *id >= board.next_id) { return Err("Too many tasks or invalid next ID".into()); }
    Ok(())
}
pub(crate) fn is_project(path: &Path) -> bool { path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("kanban")) }
fn load(path: &Path) -> Result<Document,String> {
    if !is_project(path) { return Err("Choose a .kanban file".into()); }
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 { return Err("Choose a regular Kanban file of at most 2 MiB".into()); }
    let mut text = String::new(); file.take(MAX_FILE_BYTES as u64+1).read_to_string(&mut text).map_err(|e| e.to_string())?;
    if text.len() > MAX_FILE_BYTES { return Err("Kanban file exceeds 2 MiB".into()); }
    let board: Board = toml::from_str(&text).map_err(|e| e.to_string())?; validate(&board)?;
    Ok(Document { saved: board.clone(),board })
}
fn save(doc: &Document, path: &Path) -> Result<(),String> {
    if !is_project(path) { return Err("Use the .kanban extension".into()); }
    validate(&doc.board)?;
    let text = toml::to_string(&doc.board).map_err(|e| e.to_string())?;
    if text.len() > MAX_FILE_BYTES { return Err("Kanban file exceeds 2 MiB".into()); }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => return Err("Destination must be a regular file, not a symlink or directory".into()),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()), _ => {}
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let temporary = parent.join(format!(".kanban-{}-{}.tmp",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary).map_err(|e| e.to_string())?;
    let result = (|| -> std::io::Result<()> { file.write_all(text.as_bytes())?; file.sync_all()?; std::fs::rename(&temporary,path) })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result.map_err(|e| format!("Cannot save {}: {e}",path.display()))
}
/// Resolve aliases, including a not-yet-existing file in an existing directory.
pub(crate) fn canonical_path(path: &Path) -> Result<PathBuf,String> {
    if path.exists() { return std::fs::canonicalize(path).map_err(|e| e.to_string()); }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    Ok(std::fs::canonicalize(parent).map_err(|e| e.to_string())?.join(path.file_name().ok_or("Choose a file name")?))
}
pub(crate) type OpenFiles = Arc<Mutex<HashMap<PathBuf,u64>>>;
struct FileLease { owner: u64, files: OpenFiles }
impl Drop for FileLease { fn drop(&mut self) { if let Ok(mut files) = self.files.lock() { files.retain(|_,owner| *owner != self.owner); } } }
pub(crate) enum Operation { Open(PathBuf), Save(Document,PathBuf) }
pub(crate) enum Completion { Opened(Document,PathBuf), Saved(PathBuf) }
pub(crate) struct BoardWorker { receiver: Option<mpsc::Receiver<Result<Completion,String>>>, lease: Arc<FileLease> }
impl BoardWorker {
    pub fn new(files: OpenFiles) -> Self {
        static OWNER: AtomicU64 = AtomicU64::new(1);
        Self { receiver: None,lease: Arc::new(FileLease { owner: OWNER.fetch_add(1,Ordering::Relaxed),files }) }
    }
    pub fn owns(&self, path: &Path) -> bool { self.lease.files.lock().is_ok_and(|files| files.get(path) == Some(&self.lease.owner)) }
    pub fn is_busy(&self) -> bool { self.receiver.is_some() }
    pub fn submit(&mut self, operation: Operation) -> bool {
        if self.is_busy() { return false; }
        // Desktop resolves Open paths before submission. Reserve immediately so a
        // concurrent Save As cannot win the interval before the reader starts.
        let reservation = if let Operation::Open(path) = &operation {
            let Ok(mut files) = self.lease.files.lock() else { return false; };
            if files.get(path).is_some_and(|owner| *owner != self.lease.owner) { return false; }
            if files.insert(path.clone(),self.lease.owner).is_none() { Some(path.clone()) } else { None }
        } else { None };
        let rollback = reservation.clone();
        let lease = self.lease.clone(); let (sender,receiver) = mpsc::sync_channel(1);
        if std::thread::Builder::new().name("kanban-io".into()).spawn(move || {
            let result = (|| {
                let path = match &operation { Operation::Open(path)|Operation::Save(_,path) => path };
                let identity = canonical_path(path)?;
                let existing = {
                    let mut files = lease.files.lock().map_err(|_| "Open board registry unavailable")?;
                    if files.get(&identity).is_some_and(|owner| *owner != lease.owner) { return Err("This file belongs to another open Kanban board".into()); }
                    files.insert(identity.clone(),lease.owner).is_some()
                };
                let result = match operation { Operation::Open(path) => load(&path).map(|doc| Completion::Opened(doc,identity.clone())),
                    Operation::Save(doc,path) => save(&doc,&path).map(|_| Completion::Saved(identity.clone())) };
                if let Ok(mut files) = lease.files.lock() {
                    if result.is_ok() { files.retain(|path,owner| *owner != lease.owner || *path == identity); }
                    else if !existing { files.remove(&identity); }
                }
                result
            })();
            if result.is_err() && let Some(path) = reservation && let Ok(mut files) = lease.files.lock() { files.remove(&path); }
            let _ = sender.send(result);
        }).is_err() {
            if let Some(path) = rollback && let Ok(mut files) = self.lease.files.lock() { files.remove(&path); }
            return false;
        }
        self.receiver = Some(receiver); true
    }
    pub fn poll(&mut self) -> Option<Result<Completion,String>> {
        match self.receiver.as_ref()?.try_recv() {
            Ok(result) => { self.receiver = None; Some(result) },
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => { self.receiver = None; Some(Err("Kanban worker stopped unexpectedly".into())) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    #[test]
    fn kanban_defaults_and_manual_order_preserve_identity() {
        let mut doc = Document::default();
        assert_eq!(doc.board.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),["To do","In progress","Review","Done"]);
        let first = doc.add_task(0,Task::draft("First")).unwrap();
        let second = doc.add_task(0,Task::draft("Second")).unwrap();
        doc.move_task(first,1,0).unwrap(); doc.move_task(second,1,0).unwrap();
        assert_eq!(doc.board.columns[1].tasks.iter().map(|t| t.id).collect::<Vec<_>>(),[second,first]);
        doc.edit(|board| { board.columns[1].name = "Custom".into(); Ok(()) }).unwrap();
        assert_eq!(doc.board.columns[1].tasks[1].id,first);
        assert!(doc.is_dirty()); doc.mark_saved(); assert!(!doc.is_dirty());
    }
    #[test]
    fn kanban_column_deletion_requires_destination_and_preserves_tasks() {
        let mut doc = Document::default(); let task = doc.add_task(0,Task::draft("Keep me")).unwrap();
        let before = doc.board.clone();
        assert!(doc.delete_column(0,None).is_err()); assert_eq!(doc.board,before);
        assert!(doc.delete_column(0,Some(0)).is_err()); assert_eq!(doc.board,before);
        doc.delete_column(0,Some(2)).unwrap(); assert_eq!(doc.board.columns[1].tasks[0].id,task);
        doc.delete_column(2,None).unwrap(); doc.delete_column(0,None).unwrap();
        assert!(doc.delete_column(0,None).is_err());
    }
    #[test]
    fn kanban_dates_and_titles_are_validated_transactionally() {
        for value in ["2024-02-29","2000-02-29","2026-12-31",""] { assert!(validate_date(value).is_ok(),"{value}"); }
        for value in ["2025-02-29","1900-02-29","2026-04-31","2026-00-12","0000-01-01","2026-1-01","text"] { assert!(validate_date(value).is_err(),"{value}"); }
        let mut doc = Document::default(); let before = doc.board.clone();
        assert!(doc.add_task(0,Task::draft(" \n ")).is_err()); assert_eq!(doc.board,before);
        assert!(doc.move_task(999,0,0).is_err()); assert_eq!(doc.board,before);
    }
    #[test]
    fn kanban_round_trip_keeps_custom_columns_details_and_saved_state() {
        let dir = TestDir::new(); let path = dir.path().join("文.kanban");
        let mut doc = Document::default();
        doc.edit(|board| { board.name = "Release 文".into(); board.columns.reverse(); board.columns[0].name = "Shipped".into(); Ok(()) }).unwrap();
        let mut task = Task::draft("Ship 👩🏽‍💻"); task.notes = "First\nSecond".into(); task.priority = Priority::High; task.due = "2028-02-29".into();
        doc.add_task(0,task).unwrap(); save(&doc,&path).unwrap(); let loaded = load(&path).unwrap();
        assert_eq!(loaded.board,doc.board); assert!(!loaded.is_dirty());
        doc.delete_column(1,None).unwrap(); assert_eq!(load(&path).unwrap().board.columns.len(),4);
    }
    #[test]
    fn kanban_files_reject_malformed_versions_limits_and_failed_saves() {
        let dir = TestDir::new(); let path = dir.path().join("bad.kanban");
        for text in ["bad = [", "version = 2\nname = 'Future'\nnext_id = 1\ncolumns = []"] {
            std::fs::write(&path,text).unwrap(); assert!(load(&path).is_err());
        }
        std::fs::write(&path,vec![b' ';MAX_FILE_BYTES+1]).unwrap(); assert!(load(&path).is_err());
        assert!(load(dir.path()).is_err());
        let doc = Document::default(); assert!(save(&doc,&dir.path().join("missing/a.kanban")).is_err());
        assert!(save(&doc,dir.path()).is_err()); assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(),1);
    }
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    use crate::test_support::TestDir;
    fn wait(worker: &mut BoardWorker) -> Result<Completion,String> {
        let deadline = std::time::Instant::now()+std::time::Duration::from_secs(5);
        loop { if let Some(result) = worker.poll() { return result; } assert!(std::time::Instant::now()<deadline); std::thread::yield_now(); }
    }
    #[test]
    fn kanban_open_reserves_path_before_worker_and_conflicting_save_preserves_file() {
        let dir = TestDir::new(); let path = dir.path().join("existing.kanban"); save(&Document::default(),&path).unwrap();
        let files = OpenFiles::default(); let mut opening = BoardWorker::new(files.clone());
        assert!(opening.submit(Operation::Open(path.clone())));
        assert!(opening.owns(&path),"an opening board owns its destination before background I/O starts");
        let mut other = BoardWorker::new(files.clone()); let mut changed = Document::default(); changed.add_task(0,Task::draft("Overwrite")).unwrap();
        assert!(other.submit(Operation::Save(changed,path.clone()))); assert!(wait(&mut other).is_err());
        assert!(load(&path).unwrap().board.columns[0].tasks.is_empty()); assert!(wait(&mut opening).is_ok());
        drop(opening); assert!(files.lock().unwrap().is_empty());
    }
    #[test]
    fn kanban_save_as_failure_preserves_previous_lease_and_success_releases_old_path() {
        let dir = TestDir::new(); let a = dir.path().join("a.kanban"); let b = dir.path().join("b.kanban");
        let mut worker = BoardWorker::new(OpenFiles::default());
        assert!(worker.submit(Operation::Save(Document::default(),a.clone()))); assert!(wait(&mut worker).is_ok());
        assert!(worker.submit(Operation::Save(Document::default(),dir.path().join("absent/c.kanban")))); assert!(wait(&mut worker).is_err()); assert!(worker.owns(&a));
        assert!(worker.submit(Operation::Save(Document::default(),b.clone()))); assert!(wait(&mut worker).is_ok()); assert!(!worker.owns(&a)); assert!(worker.owns(&b));
    }
    #[test]
    fn kanban_loader_rejects_duplicate_ids_and_fifo_and_saves_reject_symlinks() {
        let dir = TestDir::new(); let path = dir.path().join("bad.kanban"); let mut doc = Document::default();
        doc.board.columns[1].id = doc.board.columns[0].id; std::fs::write(&path,toml::to_string(&doc.board).unwrap()).unwrap(); assert!(load(&path).is_err());
        let link = dir.path().join("link.kanban"); std::os::unix::fs::symlink(&path,&link).unwrap(); let before = std::fs::read(&path).unwrap();
        assert!(save(&Document::default(),&link).is_err()); assert_eq!(std::fs::read(&path).unwrap(),before);
        let fifo = dir.path().join("pipe.kanban"); let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(),0o600) },0); assert!(load(&fifo).is_err());
    }
}
