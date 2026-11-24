use crate::config::{Shortcut, catalog, parse_shortcut_dir};
use anyhow::{Context, bail};
use appcui::prelude::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{atomic::{AtomicU64, Ordering}, mpsc};
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDir, UI_LOCK};

    #[test]
    fn ai_app_settings_are_reserved_but_existing_ai_named_launchers_remain_available() {
        let dir = TestDir::new(); let settings = dir.path().join("preferences.toml");
        assert!(new_destination(dir.path(),"ai.toml",&settings).is_ok());
        let ai = crate::config::ai::AiSettingsStore::path_next_to(&settings); fs::create_dir(ai.parent().unwrap()).unwrap();
        assert!(new_destination(ai.parent().unwrap(),"ai.toml",&settings).is_err());
        let equivalent = ai.parent().unwrap().join("../ai-chat");
        assert!(new_destination(&equivalent,"ai.toml",&settings).is_err());
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/shortcuts/helix.toml"));
        assert!(save_draft(dir.path(),&settings,&ai,source,None).is_err()); assert!(!ai.exists());
    }

    #[test]
    fn invalid_drafts_and_duplicate_names_preserve_existing_files() {
        let dir = TestDir::new();
        let path = dir.path().join("editor.toml");
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml"));
        fs::write(&path,source).unwrap();
        let settings = dir.path().join("preferences.toml");
        assert!(save_draft(dir.path(),&settings,&path,"invalid",Some(source)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(),source);
        assert!(save_draft(dir.path(),&settings,&dir.path().join("other.toml"),source,None).is_err());
        assert!(!dir.path().join("other.toml").exists());
        assert!(save_draft(dir.path(),&settings,&path,&source.replace("hx","nano"),Some("stale original")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(),source);
    }
    #[test]
    fn atomic_save_preserves_relative_toml_and_refuses_existing_new_destinations() {
        let dir = TestDir::new();
        let path = dir.path().join("editor.toml");
        let settings = dir.path().join("preferences.toml");
        let source = format!("id = \"editor\"\nworking_directory = \"project with spaces\"\n{}", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml")));
        save_draft(dir.path(),&settings,&path,&source,None).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(),source);
        assert!(save_draft(dir.path(),&settings,&path,&source,None).is_err());
        let changed = source.replace("hx","nano");
        save_draft(dir.path(),&settings,&path,&changed,Some(&source)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(),changed);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(),1);
    }
    #[test]
    fn new_filenames_cannot_escape_directory_or_replace_application_settings() {
        let dir = TestDir::new();
        for filename in ["../outside.toml", "sub/file.toml", "/tmp/file.toml", "invalid.txt", "", "."] {
            assert!(new_destination(dir.path(),filename,&dir.path().join("preferences.toml")).is_err(),"{filename}");
        }
        assert!(new_destination(dir.path(),"preferences.toml",&dir.path().join("preferences.toml")).is_err());
        assert!(new_destination(dir.path(),"文 editor.toml",&dir.path().join("preferences.toml")).is_ok());
    }
    fn poll(dialog: &mut LauncherManager) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while dialog.worker.is_some() {
            TimerEvents::on_update(dialog,0);
            assert!(std::time::Instant::now() < deadline,"launcher worker timed out");
            std::thread::yield_now();
        }
    }
    #[test]
    fn editor_saves_renamed_launcher_to_its_original_source_and_keeps_new_selection() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let path = dir.path().join("custom-name.toml");
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shortcuts/helix.toml"));
        fs::write(&path,source).unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").timers_count(8).build().unwrap();
        let mut dialog = LauncherManager::new(dir.path().into(),dir.path().join("preferences.toml"));
        poll(&mut dialog);
        let index = dialog.drafts.iter().position(|draft| draft.path.as_ref() == Some(&path)).unwrap();
        dialog.show_draft(index);
        let changed = source.replace("Text editor","My editor").replace("hx","nano");
        let editor = dialog.editor; dialog.control_mut(editor).unwrap().set_text(&changed);
        dialog.save();
        assert!(dialog.saving);
        assert!(matches!(WindowEvents::on_cancel(&mut dialog),ActionRequest::Deny));
        poll(&mut dialog);
        assert_eq!(fs::read_to_string(&path).unwrap(),changed);
        assert_eq!(dialog.drafts[dialog.selected.unwrap()].path.as_ref(),Some(&path));
        assert_eq!(dialog.drafts[dialog.selected.unwrap()].name,"My editor");
        drop(dialog); app.run();
    }
    #[test]
    fn diagnostics_modal_escape_cancels_and_never_creates_configuration() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new(); let missing = dir.path().join("missing");
        let app = App::debug(80,24,"Key.Pressed(Escape)").timers_count(8).build().unwrap();
        assert!(DiagnosticsDialog::new(missing.clone(),missing.join("preferences.toml")).show().is_none());
        app.run();
        assert!(!missing.exists());
    }
    #[test]
    fn manager_escape_cancels_without_starting_any_launcher() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = App::debug(80,24,"Key.Pressed(Escape)").timers_count(8).build().unwrap();
        assert!(LauncherManager::new(dir.path().into(),dir.path().join("preferences.toml")).show().is_none());
        app.run();
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }
}

fn destination_identity(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        path.parent().and_then(|parent| parent.canonicalize().ok())
            .zip(path.file_name()).map(|(parent,name)| parent.join(name)).unwrap_or_else(|| path.to_owned())
    })
}
fn reserved_settings(path: &Path,settings: &Path) -> bool {
    let path = destination_identity(path);
    [settings.to_owned(),crate::config::ai::AiSettingsStore::path_next_to(settings)].iter().any(|reserved| path == destination_identity(reserved))
}

fn new_destination(directory: &Path, filename: &str, settings: &Path) -> anyhow::Result<PathBuf> {
    let name = Path::new(filename);
    if filename.is_empty() || name.file_name() != Some(name.as_os_str()) || name.extension() != Some(std::ffi::OsStr::new("toml")) {
        bail!("Enter a filename ending in .toml, without a directory path");
    }
    let path = directory.join(name);
    if reserved_settings(&path,settings) {
        bail!("The application settings file cannot be used as a launcher");
    }
    Ok(path)
}

/// Validate the complete candidate catalog before atomically replacing any source.
/// An existing draft must still match the loaded source; a new draft never overwrites.
fn save_draft(directory: &Path, settings: &Path, path: &Path, text: &str, original: Option<&str>) -> anyhow::Result<()> {
    if reserved_settings(path,settings) { bail!("Application settings cannot be replaced with a launcher"); }
    let mut draft = Shortcut::parse_draft(text)?;
    draft.source_path = Some(path.to_owned());
    let mut shortcuts = parse_shortcut_dir(directory.to_owned(),settings)?;
    shortcuts.retain(|shortcut| shortcut.source_path.as_deref() != Some(path));
    shortcuts.push(draft);
    Shortcut::validate_catalog(&catalog::with_starters(shortcuts))?;
    match original {
        Some(source) if fs::read_to_string(path).with_context(|| format!("Cannot re-read {}",path.display()))? != source => bail!("This launcher changed on disk; reopen it before saving"),
        None if path.exists() => bail!("A launcher file already exists at {}; choose another filename",path.display()),
        _ => {}
    }
    let parent = path.parent().context("Launcher has no parent directory")?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = parent.join(format!(".launcher-{}-{}.tmp",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
    let result = (|| -> anyhow::Result<()> {
        let mode = fs::metadata(path).map(|metadata| metadata.permissions().mode() & 0o777).unwrap_or(0o600);
        let mut output = OpenOptions::new().write(true).create_new(true).mode(mode).open(&temporary)?;
        output.write_all(text.as_bytes())?;
        output.sync_all()?;
        if original.is_some() { fs::rename(&temporary,path)?; }
        else {
            // hard_link is an atomic no-replace operation on the same filesystem.
            fs::hard_link(&temporary,path)?;
            fs::remove_file(&temporary)?;
        }
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result.with_context(|| format!("Cannot save launcher {}",path.display()))
}

struct LauncherDraft { name: String, path: Option<PathBuf>, text: String }
struct Loaded { drafts: Vec<LauncherDraft> }
fn load(directory: &Path, settings: &Path) -> anyhow::Result<Loaded> {
    let shortcuts = parse_shortcut_dir(directory.to_owned(),settings)?;
    let mut drafts = Vec::new();
    for shortcut in catalog::with_starters(shortcuts.clone()) {
        let text = match shortcut.source_path.as_deref() {
            Some(path) => fs::read_to_string(path).with_context(|| format!("Cannot read {}",path.display()))?,
            None => toml::to_string_pretty(&shortcut)?,
        };
        drafts.push(LauncherDraft { name: shortcut.name, path: shortcut.source_path, text });
    }
    Ok(Loaded { drafts })
}

enum WorkerResult { Loaded(Loaded), Saved(Loaded, PathBuf), Reloaded(Vec<Shortcut>) }

type LauncherCatalog = Vec<Shortcut>;

#[ModalWindow(response = LauncherCatalog, events = ButtonEvents+TimerEvents+WindowEvents)]
pub(super) struct LauncherManager {
    shortcut_dir: PathBuf, settings_path: PathBuf,
    listing: Handle<ListBox>, editor: Handle<TextArea>, filename: Handle<TextField>, status: Handle<Label>,
    new_button: Handle<Button>, edit_button: Handle<Button>, save_button: Handle<Button>, reload_button: Handle<Button>, close_button: Handle<Button>,
    drafts: Vec<LauncherDraft>, selected: Option<usize>, original: String,
    worker: Option<mpsc::Receiver<Result<WorkerResult,String>>>, saving: bool,
}
impl LauncherManager {
    pub fn new(shortcut_dir: PathBuf, settings_path: PathBuf) -> Self {
        let shortcut_dir = if shortcut_dir.is_absolute() { shortcut_dir } else { std::env::current_dir().unwrap_or_default().join(shortcut_dir) };
        let mut dialog = Self {
            base: ModalWindow::new("Launcher manager",layout!("a:c,w:78,h:23"),window::Flags::Sizeable),
            shortcut_dir, settings_path, listing: Handle::None, editor: Handle::None, filename: Handle::None, status: Handle::None,
            new_button: Handle::None, edit_button: Handle::None, save_button: Handle::None, reload_button: Handle::None, close_button: Handle::None,
            drafts: Vec::new(), selected: None, original: String::new(), worker: None, saving: false,
        };
        dialog.listing = dialog.add(ListBox::new(layout!("l:1,t:1,w:23,b:5"),listbox::Flags::ScrollBars));
        dialog.add(Label::new("TOML: executable command and args array",layout!("l:25,r:1,t:0,h:1")));
        dialog.editor = dialog.add(TextArea::new("",layout!("l:25,r:1,t:1,b:5"),textarea::Flags::ScrollBars));
        dialog.add(Label::new("Filename",layout!("l:1,b:4,w:10,h:1")));
        dialog.filename = dialog.add(TextField::new("new-launcher.toml",layout!("l:12,r:1,b:4,h:1"),textfield::Flags::None));
        dialog.status = dialog.add(Label::new("Loading launchers…",layout!("l:1,r:1,b:3,h:2")));
        dialog.new_button = dialog.add(Button::new("&New",layout!("l:1,b:0,w:10")));
        dialog.edit_button = dialog.add(Button::new("&Edit",layout!("l:12,b:0,w:10")));
        dialog.save_button = dialog.add(Button::new("&Save",layout!("l:23,b:0,w:10")));
        dialog.reload_button = dialog.add(Button::new("&Reload",layout!("l:34,b:0,w:12")));
        dialog.close_button = dialog.add(Button::new("&Close",layout!("r:1,b:0,w:12")));
        if let Some(timer) = dialog.timer() { timer.start(Duration::from_millis(50)); dialog.begin_load(false); }
        else { dialog.set_busy(true); dialog.status("Launcher manager timer unavailable; close and retry."); }
        dialog
    }
    fn status(&mut self, text: &str) { let handle = self.status; if let Some(label) = self.control_mut(handle) { label.set_caption(text); } }
    fn set_busy(&mut self, busy: bool) {
        for handle in [self.new_button,self.edit_button,self.save_button,self.reload_button] {
            if let Some(button) = self.control_mut(handle) { button.set_enabled(!busy); }
        }
        let handle = self.editor; if let Some(editor) = self.control_mut(handle) { editor.set_enabled(!busy); }
        let handle = self.listing; if let Some(list) = self.control_mut(handle) { list.set_enabled(!busy); }
        let existing = self.selected.and_then(|index| self.drafts.get(index)).is_some_and(|draft| draft.path.is_some());
        let handle = self.filename; if let Some(field) = self.control_mut(handle) { field.set_enabled(!busy && !existing); }
    }
    fn start(&mut self, task: impl FnOnce() -> anyhow::Result<WorkerResult> + Send + 'static) {
        self.set_busy(true);
        let (sender,receiver) = mpsc::sync_channel(1);
        match std::thread::Builder::new().name("launcher-config".into()).spawn(move || {
            let result = task().map_err(|error| format!("{error:#}"));
            let _ = sender.send(result);
        }) {
            Ok(_) => self.worker = Some(receiver),
            Err(error) => { self.saving = false; self.set_busy(false); self.status(&format!("Cannot start launcher worker: {error}")); }
        }
    }
    fn begin_load(&mut self, reload: bool) {
        if self.worker.is_some() { return; }
        let directory = self.shortcut_dir.clone(); let settings = self.settings_path.clone();
        self.status(if reload { "Reloading launchers…" } else { "Loading launchers…" });
        self.start(move || {
            if reload { Ok(WorkerResult::Reloaded(parse_shortcut_dir(directory,&settings)?)) }
            else { Ok(WorkerResult::Loaded(load(&directory,&settings)?)) }
        });
    }
    fn show_draft(&mut self, index: usize) {
        let Some(draft) = self.drafts.get(index) else { return; };
        self.selected = Some(index); self.original = draft.text.clone();
        let text = draft.text.clone(); let path = draft.path.clone();
        let filename = path.as_deref().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("starter-{}.toml",index));
        let handle = self.editor; if let Some(editor) = self.control_mut(handle) { editor.set_text(&text); }
        let handle = self.filename; if let Some(field) = self.control_mut(handle) { field.set_text(&filename); field.set_enabled(path.is_none()); }
        self.status(&path.map(|path| format!("Editing {}",path.display())).unwrap_or_else(|| "Starter override: Save creates a launcher file. Reload applies saved changes.".into()));
    }
    fn dirty(&self) -> bool { self.control(self.editor).is_some_and(|editor| editor.text() != self.original.replace('\t',"    ").replace("\r\n","\n")) }
    fn confirm_discard(&mut self) -> bool { !self.dirty() || DiscardDraft::new().show() == Some(true) }
    fn new_draft(&mut self) {
        if !self.confirm_discard() { return; }
        let Some(mut shortcut) = catalog::with_starters(vec![]).into_iter().next() else { return; };
        shortcut.name = "New launcher".into(); shortcut.category = Some("User".into());
        let text = match toml::to_string_pretty(&shortcut) { Ok(text) => text, Err(error) => { self.status(&error.to_string()); return; } };
        self.selected = None; self.original = text.clone();
        let handle = self.editor; if let Some(editor) = self.control_mut(handle) { editor.set_text(&text); }
        let handle = self.filename; if let Some(field) = self.control_mut(handle) { field.set_enabled(true); field.set_text("new-launcher.toml"); }
        self.status("New launcher: edit name, command and args; Save then Reload.");
        self.base.request_focus_for_control(self.editor);
    }
    fn save(&mut self) {
        if self.worker.is_some() { return; }
        let text = self.control(self.editor).map(|editor| editor.text().to_owned()).unwrap_or_default();
        let existing = self.selected.and_then(|index| self.drafts.get(index)).and_then(|draft| draft.path.clone());
        let filename = self.control(self.filename).map(|field| field.text().to_owned()).unwrap_or_default();
        let path = match existing.clone().map(Ok).unwrap_or_else(|| new_destination(&self.shortcut_dir,&filename,&self.settings_path)) {
            Ok(path) => path, Err(error) => { self.status(&format!("{error:#}")); return; }
        };
        let directory = self.shortcut_dir.clone(); let settings = self.settings_path.clone();
        let original = existing.map(|_| self.original.clone());
        self.saving = true; self.status("Saving launcher…");
        self.start(move || { save_draft(&directory,&settings,&path,&text,original.as_deref())?; Ok(WorkerResult::Saved(load(&directory,&settings)?,path)) });
    }
}
impl ButtonEvents for LauncherManager {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if self.worker.is_some() {
            if handle == self.close_button && !self.saving { self.exit(); }
            return EventProcessStatus::Processed;
        }
        if handle == self.new_button { self.new_draft(); }
        else if handle == self.edit_button {
            if self.confirm_discard() && let Some(index) = self.control(self.listing).map(ListBox::index) { self.show_draft(index); }
        } else if handle == self.save_button { self.save(); }
        else if handle == self.reload_button { if self.confirm_discard() { self.begin_load(true); } }
        else if handle == self.close_button && self.confirm_discard() { self.exit(); }
        EventProcessStatus::Processed
    }
}
impl WindowEvents for LauncherManager {
    fn on_cancel(&mut self) -> ActionRequest {
        if self.saving { return ActionRequest::Deny; }
        if self.confirm_discard() { ActionRequest::Allow } else { ActionRequest::Deny }
    }
}
impl TimerEvents for LauncherManager {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        let result = self.worker.as_ref().map(|receiver| receiver.try_recv());
        let result = match result {
            Some(Ok(result)) => result,
            Some(Err(mpsc::TryRecvError::Disconnected)) => Err("Launcher worker stopped unexpectedly".into()),
            _ => return EventProcessStatus::Ignored,
        };
        self.worker = None; self.saving = false; self.set_busy(false);
        match result {
            Err(error) => self.status(&error),
            Ok(WorkerResult::Reloaded(shortcuts)) => self.exit_with(shortcuts),
            Ok(result) => {
                let (loaded,saved_path) = match result { WorkerResult::Loaded(loaded) => (loaded,None), WorkerResult::Saved(loaded,path) => (loaded,Some(path)), _ => return EventProcessStatus::Processed };
                self.drafts = loaded.drafts;
                let names = self.drafts.iter().map(|draft| draft.name.clone()).collect::<Vec<_>>();
                let handle = self.listing; if let Some(list) = self.control_mut(handle) { list.clear(); for name in names { list.add(&name); } list.set_index(0); }
                let index = saved_path.as_ref().and_then(|path| self.drafts.iter().position(|draft| draft.path.as_ref() == Some(path))).unwrap_or(0);
                self.show_draft(index);
                if saved_path.is_some() { self.status("Saved. Reload applies launcher changes to the desktop."); }
            }
        }
        EventProcessStatus::Processed
    }
}

#[ModalWindow(response = bool, events = ButtonEvents)]
struct DiscardDraft { discard: Handle<Button>, keep: Handle<Button> }
impl DiscardDraft {
    fn new() -> Self {
        let mut dialog = Self { base: ModalWindow::new("Unsaved launcher draft",layout!("a:c,w:56,h:7"),window::Flags::None),discard: Handle::None,keep: Handle::None };
        dialog.add(Label::new("Discard the unsaved changes?",layout!("l:2,r:2,t:1,h:1")));
        dialog.discard = dialog.add(Button::new("&Discard",layout!("l:4,b:0,w:18")));
        dialog.keep = dialog.add(Button::new("&Keep editing",layout!("r:4,b:0,w:18")));
        dialog.base.request_focus_for_control(dialog.keep); dialog
    }
}
impl ButtonEvents for DiscardDraft {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus { self.exit_with(handle == self.discard); EventProcessStatus::Processed }
}

#[ModalWindow(response = bool, events = ButtonEvents+TimerEvents)]
pub(super) struct DiagnosticsDialog { details: Handle<TextArea>, close_button: Handle<Button>, worker: mpsc::Receiver<String> }
impl DiagnosticsDialog {
    pub fn new(shortcut_dir: PathBuf, settings_path: PathBuf) -> Self {
        let (sender,receiver) = mpsc::sync_channel(1);
        let spawn = std::thread::Builder::new().name("desktop-diagnostics".into()).spawn(move || {
            let _ = sender.send(crate::platform::diagnostics::report(&shortcut_dir,&settings_path));
        });
        let message = spawn.err().map(|error| format!("Cannot start diagnostics: {error}")).unwrap_or_else(|| "Checking configuration and optional tools…".into());
        let mut dialog = Self { base: ModalWindow::new("Desktop diagnostics",layout!("a:c,w:78,h:22"),window::Flags::Sizeable),details: Handle::None,close_button: Handle::None,worker: receiver };
        dialog.details = dialog.add(TextArea::new(&message,layout!("l:1,r:1,t:0,b:3"),textarea::Flags::ReadOnly | textarea::Flags::ScrollBars));
        dialog.close_button = dialog.add(Button::new("&Close",layout!("a:b,w:14")));
        if let Some(timer) = dialog.timer() { timer.start(Duration::from_millis(50)); }
        dialog
    }
}
impl TimerEvents for DiagnosticsDialog {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        if let Ok(text) = self.worker.try_recv() {
            let handle = self.details; if let Some(details) = self.control_mut(handle) { details.set_text(&text); }
            EventProcessStatus::Processed
        } else { EventProcessStatus::Ignored }
    }
}
impl ButtonEvents for DiagnosticsDialog {
    fn on_pressed(&mut self, _: Handle<Button>) -> EventProcessStatus { self.exit(); EventProcessStatus::Processed }
}
