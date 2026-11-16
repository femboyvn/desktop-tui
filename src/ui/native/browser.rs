use super::image_viewer::home_directory;
use crate::runtime::files::{self, Entry, FileKind, Listing};
use crate::runtime::latest::LatestWorker;
use appcui::prelude::*;
use std::path::PathBuf;
use std::collections::VecDeque;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BrowserAction { Open(PathBuf), OpenWith(PathBuf), OpenTerminal(PathBuf) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewMode { Grid, Details }
enum EntryRequest { Open(Entry), OpenWith(PathBuf), Terminal(Option<Entry>), Information(Entry), Refresh, Hidden, Mutate(Mutation,Option<Entry>), Favorite, Favorites }

#[CustomControl(events = MenuEvents, overwrite = OnPaint+OnKeyPressed+OnMouseEvent+OnResize,
    commands = Open+OpenWith+Terminal+Information+Refresh+Hidden+Grid+Details+NewFile+NewFolder+Rename+Copy+Move+Trash+Favorite+Favorites)]
struct BrowserEntries {
    entries: Vec<Entry>, selected: Option<usize>, offset: usize, viewport: Size, mode: ViewMode,
    request: Option<EntryRequest>, menu: Handle<Menu>, menu_target: Option<Option<Entry>>, loading: bool, hidden: bool,
}
impl BrowserEntries {
    fn new() -> Self {
        Self { base: ControlBase::new(layout!("l:0,r:0,t:5,b:1"),true), entries: Vec::new(), selected: None,
            offset: 0, viewport: Size::new(0,0), mode: ViewMode::Grid, request: None,
            menu: Handle::None, menu_target: None, loading: false, hidden: false }
    }
    fn columns(&self) -> usize { if self.mode == ViewMode::Grid { (self.viewport.width as usize / 16).max(1) } else { 1 } }
    fn row_height(&self) -> usize { if self.mode == ViewMode::Grid { 5 } else { 1 } }
    fn header(&self) -> usize { usize::from(self.mode == ViewMode::Details) }
    fn visible_rows(&self) -> usize { (self.viewport.height as usize).saturating_sub(self.header()).div_ceil(self.row_height()).max(1) }
    fn page_rows(&self) -> usize { ((self.viewport.height as usize).saturating_sub(self.header()) / self.row_height()).max(1) }
    fn clamp_scroll(&mut self) { self.offset = self.offset.min(self.entries.len().div_ceil(self.columns()).saturating_sub(self.page_rows())); }
    fn reveal(&mut self) {
        if let Some(selected) = self.selected {
            let row = selected / self.columns();
            self.offset = self.offset.min(row).max(row.saturating_sub(self.page_rows()-1));
        }
        self.clamp_scroll();
    }
    fn set_mode(&mut self, mode: ViewMode) { self.mode = mode; self.reveal(); }
    fn clear(&mut self) { self.entries.clear(); self.selected = None; self.offset = 0; self.invalidate(); }
    fn add_items(&mut self, entries: Vec<Entry>) {
        self.entries.extend(entries);
        if self.selected.is_none() && !self.entries.is_empty() { self.selected = Some(0); }
        self.clamp_scroll();
    }
    fn selected_entry(&self) -> Option<Entry> { self.selected.and_then(|i| self.entries.get(i)).cloned() }
    fn scroll(&mut self, rows: isize) { self.offset = self.offset.saturating_add_signed(rows); self.clamp_scroll(); }
    fn point(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < self.header() as i32 || x >= self.viewport.width as i32 || y >= self.viewport.height as i32 { return None; }
        let column = x as usize / (self.viewport.width as usize / self.columns()).max(1);
        if column >= self.columns() { return None; }
        let row = (y as usize-self.header()) / self.row_height() + self.offset;
        let index = row * self.columns() + column;
        (index < self.entries.len()).then_some(index)
    }
    fn input(&mut self, key: Key) -> EventProcessStatus {
        if self.loading { return EventProcessStatus::Ignored; }
        if key.value() == key!("Shift+F10") {
            self.reveal();
            let index = self.selected.unwrap_or(0);
            let x = index % self.columns() * (self.viewport.width as usize / self.columns()).max(1);
            let y = (index/self.columns()).saturating_sub(self.offset) * self.row_height() + self.header();
            self.show_context(self.selected_entry(),x as i32,y as i32);
            return EventProcessStatus::Processed;
        }
        if key.value() == key!("Alt+Enter") {
            self.release_menu();
            self.request = self.selected_entry().map(EntryRequest::Information);
            return if self.request.is_some() { EventProcessStatus::Processed } else { EventProcessStatus::Ignored };
        }
        let mutation = if key.value() == key!("Ctrl+N") { Some(Mutation::NewFile) }
            else if key.value() == key!("Ctrl+Shift+N") { Some(Mutation::NewFolder) }
            else if key.value() == key!("F2") { Some(Mutation::Rename) }
            else if key.value() == key!("F5") { Some(Mutation::Copy) }
            else if key.value() == key!("F6") { Some(Mutation::Move) }
            else if key.value() == key!("Delete") { Some(Mutation::Trash) } else { None };
        if let Some(mutation) = mutation { self.release_menu(); self.request = Some(EntryRequest::Mutate(mutation,self.selected_entry())); return EventProcessStatus::Processed; }
        if key.modifier != KeyModifier::None { return EventProcessStatus::Ignored; }
        let selected = self.selected.unwrap_or(0);
        let step = self.columns();
        let index = match key.code {
            KeyCode::Left => selected.saturating_sub(1), KeyCode::Right => selected.saturating_add(1),
            KeyCode::Up => selected.saturating_sub(step), KeyCode::Down => selected.saturating_add(step),
            KeyCode::Home => 0, KeyCode::End => self.entries.len().saturating_sub(1),
            KeyCode::PageUp => selected.saturating_sub(step*self.page_rows()),
            KeyCode::PageDown => selected.saturating_add(step*self.page_rows()),
            KeyCode::Enter => { self.request = self.selected_entry().map(EntryRequest::Open); return EventProcessStatus::Processed; }
            _ => return EventProcessStatus::Ignored,
        };
        self.selected = (!self.entries.is_empty()).then_some(index.min(self.entries.len().saturating_sub(1)));
        self.reveal(); EventProcessStatus::Processed
    }
    fn release_menu(&mut self) {
        let menu = std::mem::replace(&mut self.menu,Handle::None);
        self.menu_target = None;
        if !menu.is_none() { self.unregister_menu(menu); }
    }
    fn invalidate(&mut self) { self.release_menu(); self.request = None; }
    fn context_at(&mut self, x: i32, y: i32) {
        if self.loading { return; }
        let index = self.point(x,y);
        if let Some(index) = index { self.selected = Some(index); }
        let target = index.and_then(|i| self.entries.get(i)).cloned();
        self.show_context(target,x,y);
    }
    fn show_context(&mut self, target: Option<Entry>, x: i32, y: i32) {
        self.release_menu();
        let mut menu = Menu::new();
        use browserentries::Commands;
        if let Some(entry) = &target {
            if entry.kind != FileKind::Other { menu.add(menu::Command::new("&Open",Key::None,Commands::Open)); }
            if entry.kind == FileKind::File { menu.add(menu::Command::new("Open &with…",Key::None,Commands::OpenWith)); }
        }
        menu.add(menu::Command::new(if target.is_some() { "Open in &terminal" } else { "Open &terminal here" },Key::None,Commands::Terminal));
        if target.is_some() { menu.add(menu::Command::new("File &information…",key!("Alt+Enter"),Commands::Information)); }
        if target.is_some() {
            for (caption,command) in [("&Rename…",Commands::Rename),("&Copy to…",Commands::Copy),("&Move to…",Commands::Move),("Move to &trash…",Commands::Trash)] { menu.add(menu::Command::new(caption,Key::None,command)); }
        }
        menu.add(menu::Command::new("New &file…",key!("Ctrl+N"),Commands::NewFile));
        menu.add(menu::Command::new("New f&older…",key!("Ctrl+Shift+N"),Commands::NewFolder));
        menu.add(menu::Command::new("Add/remove current f&avorite",Key::None,Commands::Favorite));
        menu.add(menu::Command::new("Fa&vorite folders…",Key::None,Commands::Favorites));
        menu.add(menu::Separator::new());
        menu.add(menu::Command::new("&Refresh",Key::None,Commands::Refresh));
        menu.add(menu::Command::new(if self.hidden { "&Hidden: on" } else { "&Hidden: off" },Key::None,Commands::Hidden));
        menu.add(menu::Command::new(if self.mode == ViewMode::Grid { "&Grid (selected)" } else { "&Grid" },Key::None,Commands::Grid));
        menu.add(menu::Command::new(if self.mode == ViewMode::Details { "&Details (selected)" } else { "&Details" },Key::None,Commands::Details));
        self.menu_target = Some(target);
        self.menu = self.register_menu(menu);
        self.show_menu(self.menu,x,y,None);
    }
}
impl MenuEvents for BrowserEntries {
    fn on_command(&mut self, menu: Handle<Menu>, _: Handle<menu::Command>, command: browserentries::Commands) {
        if menu != self.menu || self.menu.is_none() { return; }
        let Some(target) = self.menu_target.take() else { return; };
        use browserentries::Commands;
        match command {
            Commands::Open => self.request = target.filter(|e| e.kind != FileKind::Other).map(EntryRequest::Open),
            Commands::OpenWith => self.request = target.filter(|e| e.kind == FileKind::File).map(|e| EntryRequest::OpenWith(e.path)),
            Commands::Terminal => self.request = Some(EntryRequest::Terminal(target)),
            Commands::Information => self.request = target.map(EntryRequest::Information),
            Commands::Refresh => self.request = Some(EntryRequest::Refresh),
            Commands::Hidden => self.request = Some(EntryRequest::Hidden),
            Commands::Grid => self.set_mode(ViewMode::Grid), Commands::Details => self.set_mode(ViewMode::Details),
            Commands::NewFile => self.request = Some(EntryRequest::Mutate(Mutation::NewFile,None)),
            Commands::NewFolder => self.request = Some(EntryRequest::Mutate(Mutation::NewFolder,None)),
            Commands::Rename => self.request = target.map(|entry| EntryRequest::Mutate(Mutation::Rename,Some(entry))),
            Commands::Copy => self.request = target.map(|entry| EntryRequest::Mutate(Mutation::Copy,Some(entry))),
            Commands::Move => self.request = target.map(|entry| EntryRequest::Mutate(Mutation::Move,Some(entry))),
            Commands::Trash => self.request = target.map(|entry| EntryRequest::Mutate(Mutation::Trash,Some(entry))),
            Commands::Favorite => self.request = Some(EntryRequest::Favorite), Commands::Favorites => self.request = Some(EntryRequest::Favorites),
        }
        self.release_menu();
    }
}
fn clipped(text: &str, width: usize) -> String {
    text.chars().take(width).map(|ch| if ch.is_control() { '�' } else { ch }).collect()
}
impl OnPaint for BrowserEntries {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        surface.clear(Character::with_attributes(' ',theme.window.normal));
        let width = self.viewport.width as usize;
        if width == 0 || self.viewport.height == 0 { return; }
        let columns = self.columns();
        let tile_width = (width/columns).max(1);
        if self.mode == ViewMode::Details {
            surface.write_string(0,0,&clipped("Name                           Type       Bytes        Modified",width),theme.text.highlighted,false);
        }
        let start = self.offset * columns;
        let end = self.entries.len().min(start.saturating_add(self.visible_rows()*columns));
        for index in start..end {
            let entry = &self.entries[index];
            let x = ((index % columns)*tile_width) as i32;
            let y = ((index/columns-self.offset)*self.row_height()+self.header()) as i32;
            let attr = if self.selected == Some(index) { if self.has_focus() { theme.list_current_item.focus } else { theme.list_current_item.normal } } else { theme.text.normal };
            for row in 0..self.row_height() { surface.fill_horizontal_line_with_size(x,y+row as i32,tile_width as u32,Character::with_attributes(' ',attr)); }
            if self.mode == ViewMode::Grid {
                let icon = match entry.kind { FileKind::Directory => [" __  ","|  | ","|__| "], FileKind::Other => [" ___ ","| ? |","|___|"], FileKind::File => [" ___ ","|   |","|___|"] };
                let label = if entry.kind == FileKind::Directory { "DIR" } else if files::is_image(&entry.path) { "IMG" } else if files::is_audio(&entry.path) { "AUD" } else if files::is_pdf(&entry.path) { "PDF" } else { "   " };
                for (row,line) in icon.iter().enumerate() { surface.write_string(x,y+row as i32,&clipped(line,tile_width),attr,false); }
                if tile_width >= 5 { surface.write_string(x+1,y+1,label,attr,false); }
                surface.write_string(x,y+3,&clipped(&entry.name,tile_width.saturating_sub(1)),attr,false);
            } else {
                let kind = match entry.kind { FileKind::Directory => "Folder", FileKind::File => "File", FileKind::Other => "Other" };
                for (x,text,limit) in [(0,entry.name.as_str(),30),(31,kind,10),(42,entry.size.as_str(),12),(55,entry.modified.as_str(),18)] {
                    if x < width { surface.write_string(x as i32,y,&clipped(text,limit.min(width-x)),attr,false); }
                }
            }
        }
        if self.entries.is_empty() { surface.write_string(0,self.header() as i32,&clipped(if self.loading { "Loading…" } else { "Empty folder" },width),theme.text.normal,false); }
    }
}
impl OnKeyPressed for BrowserEntries {
    fn on_key_pressed(&mut self, key: Key, _: char) -> EventProcessStatus { if self.has_focus() { self.input(key) } else { EventProcessStatus::Ignored } }
}
impl OnMouseEvent for BrowserEntries {
    fn on_mouse_event(&mut self, event: &MouseEvent) -> EventProcessStatus {
        if self.loading { return EventProcessStatus::Processed; }
        match event {
            MouseEvent::Pressed(data) | MouseEvent::DoubleClick(data) => {
                self.request_focus();
                if data.button == MouseButton::Right { self.context_at(data.x,data.y); }
                else if data.button == MouseButton::Left {
                    self.release_menu(); self.selected = self.point(data.x,data.y);
                    if matches!(event,MouseEvent::DoubleClick(_)) { self.request = self.selected_entry().map(EntryRequest::Open); }
                } else { return EventProcessStatus::Ignored; }
            }
            MouseEvent::Wheel(MouseWheelDirection::Up) => self.scroll(-3),
            MouseEvent::Wheel(MouseWheelDirection::Down) => self.scroll(3),
            _ => return EventProcessStatus::Ignored,
        }
        EventProcessStatus::Processed
    }
}
impl OnResize for BrowserEntries {
    fn on_resize(&mut self, _: Size, size: Size) { self.viewport = size; self.release_menu(); self.reveal(); }
}

#[ModalWindow(response = bool, events = ButtonEvents)]
struct FileInformationDialog { information: Handle<TextArea>, close_button: Handle<Button> }
impl FileInformationDialog {
    fn new(entry: &Entry) -> Self {
        let display = |value: &str| value.chars().map(|ch| if ch.is_control() { '�' } else { ch }).collect::<String>();
        let kind = match entry.kind { FileKind::Directory => "Folder", FileKind::File => "File", FileKind::Other => "Other" };
        let size = if entry.kind == FileKind::File && entry.size != "—" { format!("{} bytes",entry.size) } else { "—".to_owned() };
        let text = format!("Name: {}\n\nFull path: {}\n\nType: {kind}\n\nSize: {size}\n\nModified: {}",
            display(&entry.name),display(&entry.path.to_string_lossy()),display(&entry.modified));
        let mut dialog = Self {
            base: ModalWindow::new("File information",layout!("a:c,w:74,h:18"),window::Flags::Sizeable),
            information: Handle::None, close_button: Handle::None,
        };
        // The directory worker already collected this metadata; opening the dialog does no filesystem I/O.
        dialog.information = dialog.add(TextArea::new(&text,layout!("l:1,r:1,t:1,b:3"),textarea::Flags::ReadOnly | textarea::Flags::ScrollBars));
        dialog.close_button = dialog.add(Button::new("&Close",layout!("a:b,w:14")));
        dialog.base.request_focus_for_control(dialog.close_button);
        dialog
    }
}
impl ButtonEvents for FileInformationDialog {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle != self.close_button { return EventProcessStatus::Ignored; }
        self.exit_with(true); EventProcessStatus::Processed
    }
}

fn dialog_path(original: &std::path::Path, text: &str) -> PathBuf {
    if text == original.to_string_lossy() { original.to_owned() } else { PathBuf::from(text) }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Mutation { NewFile, NewFolder, Rename, Copy, Move, Trash }
struct Favorite { path: PathBuf, label: String }
impl listview::ListItem for Favorite {
    fn columns_count() -> u16 { 1 }
    fn column(_: u16) -> Column { Column::new("Favorite folders",66,TextAlignment::Left) }
    fn render_method(&self, _: u16) -> Option<listview::RenderMethod<'_>> { Some(listview::RenderMethod::Text(&self.label)) }
}
#[derive(Debug,PartialEq,Eq)]
enum FavoriteChoice { Open(PathBuf), Remove(PathBuf) }
#[ModalWindow(response = FavoriteChoice, events = ListViewEvents<Favorite>+ButtonEvents)]
struct FavoritesDialog { listing: Handle<ListView<Favorite>>, remove: Handle<Button>, cancel: Handle<Button> }
impl FavoritesDialog {
    fn new(paths: &[PathBuf]) -> Self {
        let mut dialog = Self { base: ModalWindow::new("Favorites — Enter to open",layout!("a:c,w:74,h:18"),window::Flags::Sizeable),listing: Handle::None,remove: Handle::None,cancel: Handle::None };
        let mut list = ListView::new(layout!("l:1,r:1,t:1,b:3"),listview::Flags::ScrollBars); list.add_items(paths.iter().cloned().map(|path| Favorite { label: path.to_string_lossy().into_owned(), path }).collect());
        dialog.listing = dialog.add(list);
        dialog.remove = dialog.add(Button::new("&Remove",layout!("l:12,b:0,w:18")));
        dialog.cancel = dialog.add(Button::new("&Cancel",layout!("r:12,b:0,w:18"))); dialog.base.request_focus_for_control(dialog.listing); dialog
    }
    fn picker(paths: &[PathBuf]) -> Self {
        let mut dialog = Self::new(paths); let handle = dialog.remove;
        if let Some(button) = dialog.control_mut(handle) { button.set_enabled(false); }
        dialog
    }
}
impl ListViewEvents<Favorite> for FavoritesDialog {
    fn on_item_action(&mut self, handle: Handle<ListView<Favorite>>, index: usize) -> EventProcessStatus {
        if let Some(path) = self.control(handle).and_then(|list| list.item(index)).map(|favorite| favorite.path.clone()) { self.exit_with(FavoriteChoice::Open(path)); }
        EventProcessStatus::Processed
    }
}
impl ButtonEvents for FavoritesDialog {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.remove {
            if let Some(path) = self.control(self.listing).and_then(|list| list.current_item()).map(|favorite| favorite.path.clone()) { self.exit_with(FavoriteChoice::Remove(path)); }
        } else if handle == self.cancel { self.exit(); } else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
}

#[ModalWindow(response = PathBuf, events = ButtonEvents+WindowEvents)]
struct DestinationDialog {
    folder: Handle<TextField>, name: Handle<TextField>, original_folder: PathBuf, original_name: PathBuf,
    favorites: Vec<PathBuf>, choose_favorite: Handle<Button>, apply: Handle<Button>, cancel: Handle<Button>, status: Handle<Label>,
}
impl DestinationDialog {
    fn new(title: &str, source: &std::path::Path, favorites: &[PathBuf]) -> Self {
        let original_folder = source.parent().unwrap_or(std::path::Path::new("/")).to_owned();
        let original_name = source.file_name().map(PathBuf::from).unwrap_or_default();
        let mut dialog = Self { base: ModalWindow::new(title,layout!("a:c,w:76,h:13"),window::Flags::None),
            folder: Handle::None,name: Handle::None,original_folder,original_name,favorites: favorites.to_vec(),choose_favorite: Handle::None,apply: Handle::None,cancel: Handle::None,status: Handle::None };
        dialog.add(Label::new("Destination folder (absolute or relative to the browser)",layout!("l:2,r:2,t:1,h:1")));
        let text = dialog.original_folder.to_string_lossy().into_owned(); dialog.folder = dialog.add(TextField::new(&text,layout!("l:2,r:2,t:2,h:1"),textfield::Flags::None));
        dialog.add(Label::new("Filename (unchanged names retain exact OS bytes)",layout!("l:2,r:2,t:4,h:1")));
        let text = dialog.original_name.to_string_lossy().into_owned(); dialog.name = dialog.add(TextField::new(&text,layout!("l:2,r:2,t:5,h:1"),textfield::Flags::None));
        dialog.status = dialog.add(Label::new("Existing files require confirmation; folders are never merged.",layout!("l:2,r:2,t:7,h:2")));
        dialog.choose_favorite = dialog.add(Button::new("&Favorites…",layout!("l:2,b:0,w:18")));
        dialog.apply = dialog.add(Button::new("&Apply",layout!("l:28,b:0,w:18"))); dialog.cancel = dialog.add(Button::new("&Cancel",layout!("r:2,b:0,w:18")));
        dialog.base.request_focus_for_control(dialog.folder); dialog
    }
    fn destination(&self) -> anyhow::Result<PathBuf> {
        let folder = self.control(self.folder).map(|field| field.text()).unwrap_or(""); let name = self.control(self.name).map(|field| field.text()).unwrap_or("");
        anyhow::ensure!(!folder.is_empty() && !folder.contains('\0'),"Choose a nonempty destination folder");
        anyhow::ensure!(!name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0'),"Enter one filename without / or NUL");
        Ok(dialog_path(&self.original_folder,folder).join(dialog_path(&self.original_name,name)))
    }
    fn submit(&mut self) { match self.destination() { Ok(path) => self.exit_with(path), Err(error) => { let handle = self.status; if let Some(status) = self.control_mut(handle) { status.set_caption(&error.to_string()); } } } }
}
impl WindowEvents for DestinationDialog { fn on_accept(&mut self) { self.submit(); } }
impl ButtonEvents for DestinationDialog {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.apply { self.submit(); } else if handle == self.cancel { self.exit(); }
        else if handle == self.choose_favorite {
            if let Some(FavoriteChoice::Open(path)) = FavoritesDialog::picker(&self.favorites).show() {
                let text = path.to_string_lossy().into_owned(); self.original_folder = path; let handle = self.folder; if let Some(field) = self.control_mut(handle) { field.set_text(&text); }
            }
        } else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
}


#[Window(events = TimerEvents+WindowEvents+TextFieldEvents+ButtonEvents)]
pub(crate) struct BrowserWindow {
    path: PathBuf,
    path_field: Handle<TextField>,
    listing: Handle<BrowserEntries>,
    up: Handle<Button>,
    refresh: Handle<Button>,
    hidden_button: Handle<Button>,
    mode_button: Handle<Button>,
    hidden: bool,
    closing: bool,
    filter_field: Handle<TextField>, favorite_button: Handle<Button>, favorites_button: Handle<Button>, cancel_operation: Handle<Button>, status: Handle<Label>,
    saved: files::BrowserState, state_worker: Option<files::StateWorker>, state_ready: bool, restore_location: bool,
    operation: files::OperationWorker,
    worker: LatestWorker<(PathBuf, bool), Result<Listing, String>>,
    action: Option<BrowserAction>,
    pending_entries: VecDeque<Entry>,
}
impl BrowserWindow {
    pub fn new() -> anyhow::Result<Self> { Self::with_user_state(home_directory(),true) }
    /// Open an explicit location while loading saved view/filter/favorites in the background.
    pub fn new_at(path: PathBuf) -> anyhow::Result<Self> { Self::with_user_state(path,false) }
    pub fn current_path(&self) -> Option<PathBuf> { (!self.closing).then(|| self.path.clone()) }
    fn with_user_state(path: PathBuf, restore: bool) -> anyhow::Result<Self> {
        // Headless tests must never write host user state; persistence tests inject isolated paths.
        let state_path = if cfg!(test) { Ok(None) } else { files::default_state_path().map(Some) };
        let mut window = Self::with_state(path,state_path.as_ref().ok().cloned().flatten(),restore)?;
        if let Err(error) = state_path { window.set_status(&format!("Browser state unavailable: {error:#}")); }
        Ok(window)
    }
    #[cfg(test)]
    fn at(path: PathBuf) -> anyhow::Result<Self> { Self::with_state(path,None,false) }
    fn with_state(path: PathBuf, state_path: Option<PathBuf>, restore_location: bool) -> anyhow::Result<Self> {
        let path = if path.is_absolute() { path } else { std::env::current_dir()?.join(path) };
        let mut state_worker = state_path.map(files::StateWorker::new).transpose()?;
        if let Some(worker) = &mut state_worker { worker.submit(files::StateJob::Load); }
        let state_ready = state_worker.is_none();
        let worker = LatestWorker::new(|(path, hidden): (PathBuf, bool)| files::list_directory(&path, hidden).map_err(|e| format!("{e:#}")))?;
        let mut window = Self {
            base: Window::new("File Browser — Enter Open | Alt+U Parent | Alt+R Refresh | Alt+H Hidden", layout!("a:c,w:90%,h:80%"), window::Flags::Sizeable),
            path, path_field: Handle::None, listing: Handle::None, up: Handle::None, refresh: Handle::None, hidden_button: Handle::None, mode_button: Handle::None,
            hidden: false, closing: false, worker, action: None, pending_entries: VecDeque::new(),
            filter_field: Handle::None, favorite_button: Handle::None, favorites_button: Handle::None, cancel_operation: Handle::None, status: Handle::None,
            saved: files::BrowserState::default(), state_worker, state_ready, restore_location, operation: files::OperationWorker::new()?,
        };
        let path_text = window.path.to_string_lossy().into_owned();
        window.path_field = window.add(TextField::new(&path_text, layout!("l:0,r:0,t:0,h:1"), textfield::Flags::ProcessEnter));
        window.up = window.add(Button::with_type("&Up", layout!("x:0,y:1,w:10"), button::Type::Flat));
        window.refresh = window.add(Button::with_type("&Refresh", layout!("x:11,y:1,w:12"), button::Type::Flat));
        window.hidden_button = window.add(Button::with_type("&Hidden: off", layout!("x:24,y:1,w:16"), button::Type::Flat));
        window.mode_button = window.add(Button::with_type("&View: Grid", layout!("x:41,y:1,w:17"), button::Type::Flat));
        window.favorite_button = window.add(Button::with_type("Add/remove favorite",layout!("x:0,y:2,w:23"),button::Type::Flat));
        window.favorites_button = window.add(Button::with_type("Favorites…",layout!("x:24,y:2,w:17"),button::Type::Flat));
        window.cancel_operation = window.add(Button::with_type("Cancel operation",layout!("x:42,y:2,w:21"),button::Type::Flat));
        window.add(Label::new("Filter (Enter):",layout!("x:0,y:3,w:15,h:1")));
        window.filter_field = window.add(TextField::new("",layout!("l:15,r:0,t:3,h:1"),textfield::Flags::ProcessEnter));
        window.status = window.add(Label::new("F2 Rename | F5 Copy | F6 Move | Delete Trash | Ctrl+N File | Ctrl+Shift+N Folder",layout!("l:0,r:0,b:0,h:1")));
        window.listing = window.add(BrowserEntries::new());
        // Keep directories first: user sorting is deliberately disabled for this view.
        window.timer().ok_or_else(|| anyhow::anyhow!("File Browser timer unavailable"))?.start(Duration::from_millis(40));
        window.navigate(window.path.clone());
        Ok(window)
    }
    fn navigate(&mut self, path: PathBuf) {
        let target = if path.is_absolute() { path } else { self.path.join(path) };
        self.pending_entries.clear();
        self.action = None;
        let handle = self.listing;
        if let Some(list) = self.control_mut(handle) { list.invalidate(); list.loading = true; }
        self.worker.submit((target, self.hidden));
    }
    fn parent(&mut self) { if let Some(parent) = self.path.parent() { self.navigate(parent.to_owned()); } }
    fn refresh(&mut self) { self.navigate(self.path.clone()); }
    fn toggle_hidden(&mut self) {
        self.hidden = !self.hidden;
        let button = self.hidden_button;
        let hidden = self.hidden;
        if let Some(button) = self.control_mut(button) { button.set_caption(if hidden { "&Hidden: on" } else { "&Hidden: off" }); }
        let handle = self.listing;
        if let Some(list) = self.control_mut(handle) { list.hidden = hidden; }
        self.save_view(); self.refresh();
    }
    pub fn take_action(&mut self) -> Option<BrowserAction> { if self.is_visible() && !self.closing { self.action.take() } else { None } }
    pub fn is_closing(&self) -> bool { self.closing }
    pub fn shutdown(&mut self) { self.cleanup(); self.close(); }
    fn cleanup(&mut self) {
        self.operation.cancel();
        self.closing = true; self.worker.close(); self.action = None; self.pending_entries.clear();
        let handle = self.listing;
        if let Some(list) = self.control_mut(handle) { list.invalidate(); }
    }
    fn open_entry(&mut self, entry: Entry) {
        match entry.kind {
            FileKind::Directory => self.navigate(entry.path),
            FileKind::File => self.action = Some(BrowserAction::Open(entry.path)),
            FileKind::Other => {}
        }
    }
    fn poll_request(&mut self) -> bool {
        let handle = self.listing;
        let request = self.control_mut(handle).and_then(|list| list.request.take());
        let mut changed = request.is_some();
        match request {
            Some(EntryRequest::Open(entry)) => self.open_entry(entry),
            Some(EntryRequest::OpenWith(path)) => self.action = Some(BrowserAction::OpenWith(path)),
            Some(EntryRequest::Terminal(entry)) => {
                let path = entry.map(|entry| if entry.kind == FileKind::Directory { entry.path } else { entry.path.parent().unwrap_or(&self.path).to_owned() }).unwrap_or_else(|| self.path.clone());
                self.action = Some(BrowserAction::OpenTerminal(path));
            }
            Some(EntryRequest::Information(entry)) => { FileInformationDialog::new(&entry).show(); }
            Some(EntryRequest::Refresh) => self.refresh(), Some(EntryRequest::Hidden) => self.toggle_hidden(),
            Some(EntryRequest::Mutate(mutation,entry)) => self.mutate(mutation,entry),
            Some(EntryRequest::Favorite) => self.toggle_favorite(), Some(EntryRequest::Favorites) => self.show_favorites(), None => {}
        }
        let mode = self.control(handle).map(|list| list.mode);
        let button = self.mode_button;
        if let Some(button) = self.control_mut(button) {
            let caption = if mode == Some(ViewMode::Details) { "View: Details" } else { "View: Grid" };
            if button.caption() != caption { button.set_caption(&format!("&{caption}")); changed = true; }
        }
        if mode.is_some_and(|mode| (mode == ViewMode::Details) != self.saved.details) { self.save_view(); }
        changed
    }
    fn set_status(&mut self, text: &str) { let handle = self.status; if let Some(status) = self.control_mut(handle) { status.set_caption(text); } }
    fn save_view(&mut self) {
        if !self.state_ready { return; }
        self.saved.location = Some(self.path.clone()); self.saved.hidden = self.hidden;
        self.saved.details = self.control(self.listing).is_some_and(|list| list.mode == ViewMode::Details);
        if let Some(worker) = &mut self.state_worker { worker.submit(files::StateJob::Save(self.saved.clone())); }
    }
    fn toggle_favorite(&mut self) {
        if !self.state_ready { self.set_status("Favorites are still loading"); return; }
        if let Some(index) = self.saved.favorites.iter().position(|path| *path == self.path) { self.saved.favorites.remove(index); self.set_status("Favorite removed"); }
        else if self.saved.favorites.len() < 64 { self.saved.favorites.push(self.path.clone()); self.set_status("Current folder added to favorites"); }
        else { self.set_status("Favorite limit reached (64); remove a favorite first"); return; }
        self.save_view();
    }
    fn show_favorites(&mut self) {
        if !self.state_ready { self.set_status("Favorites are still loading"); return; }
        match FavoritesDialog::new(&self.saved.favorites).show() {
            Some(FavoriteChoice::Open(path)) => self.navigate(path),
            Some(FavoriteChoice::Remove(path)) => { self.saved.favorites.retain(|favorite| *favorite != path); self.save_view(); self.set_status("Favorite removed"); }
            None => {}
        }
    }
    fn start_operation(&mut self, operation: files::Operation) {
        match self.operation.submit(operation) { Ok(()) => self.set_status("Working… Cancel operation stops before publication"), Err(error) => self.set_status(&format!("{error:#}")) }
    }
    fn mutate(&mut self, mutation: Mutation, entry: Option<Entry>) {
        if self.operation.busy() { self.set_status("Another file operation is running; cancel it or wait"); return; }
        if matches!(mutation,Mutation::NewFile | Mutation::NewFolder) {
            let title = if mutation == Mutation::NewFolder { "Create folder" } else { "Create empty file" };
            if let Some(name) = dialogs::input::<String>(title,"Name in the current folder (spaces are preserved)",None,None) {
                if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') { self.set_status("Enter one nonempty filename without / or NUL"); return; }
                self.start_operation(files::Operation::Create { path: self.path.join(name), directory: mutation == Mutation::NewFolder });
            }
            return;
        }
        let Some(entry) = entry else { self.set_status("Select an entry first"); return; };
        if mutation == Mutation::Trash {
            if dialogs::validate("Move to user trash?",&format!("Move {} to the user trash? Restoration metadata will be retained. No permanent deletion fallback is used.",entry.path.display())) {
                self.start_operation(files::Operation::Trash { source: entry.path, root: None });
            }
            return;
        }
        let destination = if mutation == Mutation::Rename {
            let original = entry.path.file_name().map(PathBuf::from).unwrap_or_default();
            let initial = original.to_string_lossy().into_owned();
            dialogs::input::<String>("Rename entry","New filename (unchanged opaque names retain exact bytes)",Some(initial),None).and_then(|text| {
                if text.is_empty() || text == "." || text == ".." || text.contains('/') || text.contains('\0') { self.set_status("Enter one filename without / or NUL"); None }
                else { Some(self.path.join(dialog_path(&original,&text))) }
            })
        } else { DestinationDialog::new(if mutation == Mutation::Copy { "Copy to folder and filename" } else { "Move to folder and filename" },&entry.path,&self.saved.favorites).show() };
        if let Some(destination) = destination {
            let destination = if destination.is_absolute() { destination } else { self.path.join(destination) };
            self.start_operation(files::Operation::Transfer { source: entry.path, destination, moving: mutation != Mutation::Copy, approval: None });
        }
    }
}
impl TextFieldEvents for BrowserWindow {
    fn on_validate(&mut self, handle: Handle<TextField>, text: &str) -> EventProcessStatus {
        if handle == self.filter_field {
            if !self.state_ready { self.set_status("Browser preferences are still loading; apply the filter again"); return EventProcessStatus::Processed; }
            if text.len() > 512 { self.set_status("Filename filter is limited to 512 bytes"); return EventProcessStatus::Processed; }
            self.saved.filter = text.to_owned(); self.save_view(); self.refresh(); return EventProcessStatus::Processed;
        }
        // Unedited display text can be lossy for non-UTF-8 directories; retain the original OS path.
        let path = if text == self.path.to_string_lossy() { self.path.clone() } else { PathBuf::from(text) };
        self.navigate(path);
        EventProcessStatus::Processed
    }
}
impl ButtonEvents for BrowserWindow {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.up { self.parent(); }
        else if handle == self.refresh { self.refresh(); }
        else if handle == self.hidden_button { self.toggle_hidden(); }
        else if handle == self.favorite_button { self.toggle_favorite(); }
        else if handle == self.favorites_button { self.show_favorites(); }
        else if handle == self.cancel_operation { self.operation.cancel(); self.set_status("Cancellation requested; already published work is retained"); }
        else if handle == self.mode_button {
            let handle = self.listing;
            if let Some(list) = self.control_mut(handle) { list.set_mode(if list.mode == ViewMode::Grid { ViewMode::Details } else { ViewMode::Grid }); list.request_focus(); }
            self.poll_request();
        }
        else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
}
impl TimerEvents for BrowserWindow {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        if self.closing || !self.is_visible() { return EventProcessStatus::Ignored; }
        let mut changed = self.poll_request();
        if let Some(result) = self.state_worker.as_mut().and_then(|worker| worker.poll()) {
            match result {
                Ok(Some(saved)) => {
                    self.saved = saved; self.hidden = self.saved.hidden; self.state_ready = true;
                    let handle = self.listing; let mode = if self.saved.details { ViewMode::Details } else { ViewMode::Grid }; let hidden = self.hidden;
                    if let Some(list) = self.control_mut(handle) { list.set_mode(mode); list.hidden = hidden; }
                    let handle = self.filter_field; let filter = self.saved.filter.clone(); if let Some(field) = self.control_mut(handle) { field.set_text(&filter); }
                    let handle = self.hidden_button; if let Some(button) = self.control_mut(handle) { button.set_caption(if hidden { "&Hidden: on" } else { "&Hidden: off" }); }
                    let target = if self.restore_location { self.saved.location.clone().unwrap_or_else(|| self.path.clone()) } else { self.path.clone() };
                    self.navigate(target);
                }
                Ok(None) => {},
                Err(error) => { self.set_status(&format!("Browser state: {error}")); if !self.state_ready { self.state_worker = None; self.state_ready = true; } }
            }
            changed = true;
        }
        if let Some(outcome) = self.operation.poll() {
            match outcome {
                files::Outcome::Complete(message) => { self.set_status(&message); self.refresh(); }
                files::Outcome::Failed(error) => { self.set_status(&error); dialogs::error("File operation stopped",&error); }
                files::Outcome::Confirm(operation) => {
                    let destination = match &operation { files::Operation::Transfer { destination, .. } => destination.display().to_string(), _ => String::new() };
                    if dialogs::validate("Replace existing file?",&format!("Replace {destination}? The source and destination will be checked again before publication.")) { self.start_operation(operation); }
                    else { self.set_status("Replacement canceled; source and destination preserved"); }
                }
            }
            changed = true;
        } else if self.operation.busy() {
            let (entries,bytes) = self.operation.progress(); self.set_status(&format!("Working: {entries} entries, {bytes} bytes | Cancel operation to stop")); changed = true;
        }
        match self.worker.poll() {
            Some(Ok(listing)) => {
                self.path = listing.path;
                let field = self.path_field;
                let text = self.path.to_string_lossy().into_owned();
                if let Some(field) = self.control_mut(field) { field.set_text(&text); }
                let handle = self.listing;
                if let Some(list) = self.control_mut(handle) { list.clear(); list.loading = false; list.request_focus(); }
                self.pending_entries = listing.entries.into();
                self.save_view();
                changed = true;
            }
            Some(Err(error)) => {
                let handle = self.listing;
                if let Some(list) = self.control_mut(handle) { list.loading = false; }
                dialogs::error("Cannot read directory", &format!("{error}\nUse the path field, Up, or Refresh to try again."));
                changed = true;
            }
            None => {}
        }
        if !self.pending_entries.is_empty() {
            // Large directories are inserted incrementally to keep callbacks short.
            let filter = self.saved.filter.to_lowercase();
            let batch = self.pending_entries.drain(..self.pending_entries.len().min(256)).filter(|entry| filter.is_empty() || entry.name.to_lowercase().contains(&filter)).collect();
            let handle = self.listing;
            if let Some(list) = self.control_mut(handle) { list.add_items(batch); }
            changed = true;
        }
        if changed { EventProcessStatus::Processed } else { EventProcessStatus::Ignored }
    }
}
impl WindowEvents for BrowserWindow {
    fn on_cancel(&mut self) -> ActionRequest { self.cleanup(); ActionRequest::Allow }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDir, UI_LOCK};
    fn pump_until(window: &mut BrowserWindow, ready: impl Fn(&BrowserWindow) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !ready(window) {
            TimerEvents::on_update(window, 0);
            assert!(std::time::Instant::now() < deadline, "directory result did not arrive");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn stale_favorite_can_be_removed_without_navigation_and_releases_a_full_favorite_list() {
        let _lock = UI_LOCK.lock().unwrap(); let dir = TestDir::new();
        let app = App::debug(100,32,"Paint.Enable(false)\nKey.Pressed(Tab)\nKey.Pressed(Enter)").build().unwrap();
        let store = dir.path().join("state/browser.toml");
        let favorites = (0..64).map(|index| dir.path().join(format!("missing-{index}"))).collect::<Vec<_>>();
        let removed = favorites[0].clone(); std::fs::create_dir(&removed).unwrap();
        files::save_state(&store,&files::BrowserState { favorites, ..files::BrowserState::default() }).unwrap();
        std::fs::remove_dir(&removed).unwrap();
        let mut window = BrowserWindow::with_state(dir.path().to_owned(),Some(store.clone()),false).unwrap();
        pump_until(&mut window,|w| w.state_ready && !w.control(w.listing).unwrap().loading);
        window.show_favorites();
        assert_eq!(window.saved.favorites.len(),63); assert!(!window.saved.favorites.contains(&removed));
        assert_eq!(window.current_path(),Some(dir.path().to_owned()));
        let deadline = std::time::Instant::now()+Duration::from_secs(3);
        while files::load_state(&store).unwrap().favorites.contains(&removed) { assert!(std::time::Instant::now()<deadline); std::thread::yield_now(); }
        window.toggle_favorite(); assert_eq!(window.saved.favorites.len(),64); assert!(window.saved.favorites.contains(&dir.path().to_owned()));
        window.shutdown(); app.run();
    }
    #[test]
    fn create_rename_and_cancel_dialogs_reach_native_worker_without_blocking_navigation() {
        let _lock = UI_LOCK.lock().unwrap(); let dir = TestDir::new();
        let app = App::debug(100,32,"Paint.Enable(false)\nKey.TypeText('created 文 file')\nKey.Pressed(Enter)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        window.mutate(Mutation::NewFile,None);
        pump_until(&mut window,|w| !w.operation.busy());
        assert!(dir.path().join("created 文 file").exists()); window.shutdown(); app.run();
        let app = App::debug(100,32,"Paint.Enable(false)\nKey.Pressed(Ctrl+A)\nKey.TypeText('renamed 文 file')\nKey.Pressed(Enter)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        let entry = files::list_directory(dir.path(),false).unwrap().entries.remove(0);
        window.mutate(Mutation::Rename,Some(entry)); pump_until(&mut window,|w| !w.operation.busy());
        assert!(!dir.path().join("created 文 file").exists()); assert!(dir.path().join("renamed 文 file").exists()); window.shutdown(); app.run();
        let app = App::debug(100,32,"Paint.Enable(false)\nKey.Pressed(Escape)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap(); window.mutate(Mutation::NewFolder,None);
        assert!(!window.operation.busy()); assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(),1); window.shutdown(); app.run();
    }
    #[test]
    fn favorite_chooser_returns_exact_opaque_folder_and_filter_and_favorites_save_atomically() {
        use std::os::unix::ffi::OsStringExt;
        let _lock = UI_LOCK.lock().unwrap(); let dir = TestDir::new();
        let opaque = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/favorite \xff".to_vec()));
        let app = App::debug(100,32,"Paint.Enable(false)\nKey.Pressed(Enter)").build().unwrap();
        assert_eq!(FavoritesDialog::new(std::slice::from_ref(&opaque)).show(),Some(FavoriteChoice::Open(opaque))); app.run();
        let store = dir.path().join("state/browser.toml");
        let app = App::debug(100,32,"Paint.Enable(false)").build().unwrap();
        let mut window = BrowserWindow::with_state(dir.path().to_owned(),Some(store.clone()),false).unwrap();
        pump_until(&mut window,|w| w.state_ready && !w.control(w.listing).unwrap().loading);
        window.toggle_favorite(); let handle = window.filter_field;
        TextFieldEvents::on_validate(&mut window,handle," 文 ");
        let deadline = std::time::Instant::now()+Duration::from_secs(3);
        loop { TimerEvents::on_update(&mut window,0); if files::load_state(&store).is_ok_and(|state| state.filter == " 文 " && state.favorites == vec![dir.path().to_owned()]) { break; } assert!(std::time::Instant::now()<deadline); std::thread::yield_now(); }
        window.toggle_favorite(); assert!(window.saved.favorites.is_empty()); window.shutdown(); app.run();
    }
    #[test]
    fn typing_filter_and_pressing_enter_routes_validation_through_the_real_control() {
        let _lock = UI_LOCK.lock().unwrap(); let dir = TestDir::new();
        let store = dir.path().join("state/browser.toml");
        let mut app = App::debug(100,32,"Paint.Enable(false)\nKey.Pressed(Tab,9)\nKey.TypeText('smoke')\nKey.Pressed(Enter)").build().unwrap();
        let mut window = BrowserWindow::with_state(dir.path().to_owned(),Some(store.clone()),false).unwrap();
        pump_until(&mut window,|w| w.state_ready && !w.control(w.listing).unwrap().loading);
        app.add_window(window); app.run(); files::finish_shutdown();
        assert_eq!(files::load_state(&store).unwrap().filter,"smoke");
    }
    #[test]
    fn destination_dialog_retains_opaque_filename_when_only_folder_changes_and_escape_cancels() {
        use std::os::unix::ffi::OsStringExt;
        let _lock = UI_LOCK.lock().unwrap(); let app = App::debug(100,32,"Paint.Enable(false)").build().unwrap();
        let source = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/opaque \xff".to_vec()));
        let mut dialog = DestinationDialog::new("Copy",&source,&[]);
        let folder = dialog.folder; dialog.control_mut(folder).unwrap().set_text("/tmp/new 文 folder");
        assert_eq!(dialog.destination().unwrap(),PathBuf::from("/tmp/new 文 folder").join(source.file_name().unwrap()));
        let name = dialog.name; dialog.control_mut(name).unwrap().set_text("../escape"); assert!(dialog.destination().is_err());
        app.run();
        let app = App::debug(100,32,"Key.Pressed(Escape)").build().unwrap(); assert_eq!(DestinationDialog::new("Move",&source,&[]).show(),None); app.run();
    }
    #[test]
    fn filter_keeps_exact_paths_and_persisted_preferences_restore_from_an_isolated_store() {
        let _lock = UI_LOCK.lock().unwrap(); let dir = TestDir::new();
        std::fs::write(dir.path().join("文 target.txt"),b"keep").unwrap();
        std::fs::write(dir.path().join("other.txt"),b"other").unwrap();
        let state_path = dir.path().join("state/browser.toml");
        files::save_state(&state_path,&files::BrowserState { location: Some(dir.path().to_owned()), details: true, hidden: true, filter: "target".into(), favorites: vec![dir.path().to_owned()] }).unwrap();
        let app = App::debug(100,32,"Paint.Enable(false)").build().unwrap();
        let mut window = BrowserWindow::with_state(dir.path().to_owned(),Some(state_path),true).unwrap();
        pump_until(&mut window,|w| w.state_ready && w.control(w.listing).unwrap().entries.len() == 1 && !w.control(w.listing).unwrap().loading);
        assert_eq!(window.control(window.listing).unwrap().mode,ViewMode::Details);
        assert_eq!(window.control(window.listing).unwrap().entries[0].path,dir.path().join("文 target.txt"));
        assert_eq!(window.current_path(),Some(dir.path().to_owned())); assert_eq!(window.saved.favorites,vec![dir.path().to_owned()]);
        window.shutdown(); app.run();
    }
    #[test]
    fn native_mutation_requests_preserve_target_path_in_both_views() {
        let _lock = UI_LOCK.lock().unwrap(); let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new(); list.viewport = Size::new(48,15); list.add_items((0..2).map(|index| Entry { path: PathBuf::from(format!("/tmp/{index}")),name: index.to_string(),kind: FileKind::File,size: "1".into(),modified: "—".into() }).collect());
        for mode in [ViewMode::Grid,ViewMode::Details] {
            list.set_mode(mode); list.selected = Some(1); list.input(key!("F2").into());
            assert!(matches!(list.request.take(),Some(EntryRequest::Mutate(Mutation::Rename,Some(entry))) if entry.path == std::path::Path::new("/tmp/1")));
            list.context_at(0,if mode == ViewMode::Grid { 0 } else { 1 }); let menu = list.menu;
            MenuEvents::on_command(&mut list,menu,Handle::None,browserentries::Commands::Copy);
            assert!(matches!(list.request.take(),Some(EntryRequest::Mutate(Mutation::Copy,Some(entry))) if entry.path == std::path::Path::new("/tmp/0")));
        }
        list.release_menu(); app.run();
    }
    #[test]
    fn keyboard_information_releases_context_menu_before_dialog_and_single_escape() {
        let _lock = UI_LOCK.lock().unwrap(); let dir = TestDir::new();
        std::fs::write(dir.path().join("file.txt"),b"hello").unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)\nKey.Pressed(Escape)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        pump_until(&mut window,|w| w.control(w.listing).unwrap().entries.len() == 1);
        let handle = window.listing; let list = window.control_mut(handle).unwrap();
        list.viewport = Size::new(48,15);
        OnMouseEvent::on_mouse_event(list,&MouseEvent::Pressed(MouseEventData { x: 0,y: 0,button: MouseButton::Right,modifier: KeyModifier::None }));
        assert!(!list.menu.is_none());
        list.input(key!("Alt+Enter").into());
        assert!(list.menu.is_none(),"the context menu must be removed before the modal opens");
        assert!(list.menu_target.is_none());
        assert!(matches!(list.request.as_ref(),Some(EntryRequest::Information(entry)) if entry.path == dir.path().join("file.txt")));
        TimerEvents::on_update(&mut window,0);
        assert!(!window.is_closing()); assert!(window.take_action().is_none());
        assert_eq!(window.control(handle).unwrap().selected,Some(0));
        window.shutdown(); app.run();
    }
    #[test]
    fn browser_information_request_opens_a_focusable_dialog_and_preserves_selection() {
        let _lock = UI_LOCK.lock().unwrap(); let dir = TestDir::new();
        std::fs::write(dir.path().join("file.txt"),b"hello").unwrap();
        for mode in [ViewMode::Grid,ViewMode::Details] {
            let app = App::debug(80,24,"Paint.Enable(false)\nKey.Pressed(Tab)\nKey.Pressed(Right,6)\nKey.Pressed(Shift+Right,8)\nKey.Pressed(Ctrl+C)\nCheckClipboardText('file.txt')\nKey.Pressed(Escape)").build().unwrap();
            let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
            pump_until(&mut window,|w| w.control(w.listing).unwrap().entries.len() == 1);
            let handle = window.listing; let list = window.control_mut(handle).unwrap();
            list.set_mode(mode); list.input(key!("Alt+Enter").into());
            TimerEvents::on_update(&mut window,0);
            assert_eq!(appcui::system::Clipboard::text().as_deref(),Some("file.txt"));
            assert!(window.take_action().is_none());
            assert_eq!(window.control(handle).unwrap().selected,Some(0));
            assert_eq!(std::fs::read(dir.path().join("file.txt")).unwrap(),b"hello");
            window.shutdown(); app.run();
        }
    }
    #[test]
    fn file_information_dialog_shows_metadata_preserves_paths_and_closes() {
        use std::os::unix::ffi::OsStringExt;
        let _lock = UI_LOCK.lock().unwrap();
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/long name \xff <FILE_PATH>".to_vec()));
        let mut entry = Entry { path: path.clone(), name: "long name � <FILE_PATH>".into(), kind: FileKind::File,
            size: "42".into(), modified: "2026-10-31 00:30".into() };
        for (kind,label,size) in [(FileKind::File,"File","42 bytes"),(FileKind::Directory,"Folder","—"),(FileKind::Other,"Other","—")] {
            entry.kind = kind;
            let app = App::debug(80,24,"Key.Pressed(Enter)").build().unwrap();
            let mut dialog = FileInformationDialog::new(&entry);
            let handle = dialog.information;
            let text = dialog.control(handle).unwrap().text().to_owned();
            assert!(text.contains("long name � <FILE_PATH>"));
            assert!(text.contains("/tmp/long name � <FILE_PATH>"));
            assert!(text.contains(&format!("Type: {label}")));
            assert!(text.contains(&format!("Size: {size}")));
            assert!(text.contains("2026-10-31 00:30"));
            OnKeyPressed::on_key_pressed(dialog.control_mut(handle).unwrap(),key!("A").into(),'a');
            assert_eq!(dialog.control(handle).unwrap().text(),text);
            assert_eq!(dialog.show(),Some(true));
            assert_eq!(entry.path,path); app.run();
        }
        let app = App::debug(80,24,"Key.Pressed(Tab)\nKey.Pressed(PageDown)\nKey.Pressed(Escape)").build().unwrap();
        entry.name = "文".repeat(300);
        assert_eq!(FileInformationDialog::new(&entry).show(),None); app.run();
    }
    #[test]
    fn timer_repaints_view_caption_after_context_menu_changes_layout() {
        let _lock = UI_LOCK.lock().unwrap(); let dir = TestDir::new();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        pump_until(&mut window,|w| !w.control(w.listing).unwrap().loading);
        let handle = window.listing; let list = window.control_mut(handle).unwrap(); list.set_mode(ViewMode::Details);
        assert!(matches!(TimerEvents::on_update(&mut window,0),EventProcessStatus::Processed));
        assert_eq!(window.control(window.mode_button).unwrap().caption(),"View: Details");
        window.shutdown(); app.run();
    }
    #[test]
    fn terminal_targets_menu_kinds_navigation_and_close_invalidate_pending_actions() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new(); let child = dir.path().join("child"); std::fs::create_dir(&child).unwrap();
        std::fs::write(dir.path().join("file"),b"x").unwrap();
        // A broken symlink is a special entry and may only open a terminal at its parent.
        std::os::unix::fs::symlink("missing",dir.path().join("special")).unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        pump_until(&mut window,|w| w.control(w.listing).unwrap().entries.len() == 3);
        let handle = window.listing;
        for mode in [ViewMode::Grid,ViewMode::Details] {
            let list = window.control_mut(handle).unwrap(); list.viewport = Size::new(64,20); list.set_mode(mode);
            for index in 0..3 {
                let entry = window.control(handle).unwrap().entries[index].clone();
                let (x,y) = if mode == ViewMode::Grid { (index as i32*16,0) } else { (0,index as i32+1) };
                let list = window.control_mut(handle).unwrap(); list.context_at(x,y);
                let menu = list.menu;
                MenuEvents::on_command(list,menu,Handle::None,browserentries::Commands::Terminal);
                window.poll_request();
                let expected = if entry.kind == FileKind::Directory { child.clone() } else { dir.path().to_owned() };
                assert_eq!(window.take_action(),Some(BrowserAction::OpenTerminal(expected)));
            }
            let list = window.control_mut(handle).unwrap(); list.context_at(63,19);
            let menu = list.menu; MenuEvents::on_command(list,menu,Handle::None,browserentries::Commands::Terminal);
            window.poll_request(); assert_eq!(window.take_action(),Some(BrowserAction::OpenTerminal(dir.path().to_owned())));
        }
        let list = window.control_mut(handle).unwrap(); list.context_at(0,1); let old_menu = list.menu;
        window.navigate(child.clone());
        let list = window.control_mut(handle).unwrap();
        assert!(list.menu.is_none()); assert!(list.menu_target.is_none());
        MenuEvents::on_command(list,old_menu,Handle::None,browserentries::Commands::Open);
        assert!(list.request.is_none()); assert!(window.take_action().is_none());
        pump_until(&mut window,|w| w.path == child);
        window.action = Some(BrowserAction::OpenTerminal(child));
        window.set_visible(false); assert!(window.take_action().is_none());
        window.set_visible(true); assert!(window.take_action().is_some());
        let list = window.control_mut(handle).unwrap(); list.context_at(1,2);
        window.shutdown();
        assert!(window.control(handle).unwrap().menu.is_none()); assert!(window.take_action().is_none());
        app.run();
    }
    #[test]
    fn hidden_browser_defers_failed_results_without_opening_a_dialog() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().join("missing secret path")).unwrap();
        window.set_visible(false);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            assert!(matches!(TimerEvents::on_update(&mut window, 0), EventProcessStatus::Ignored));
            if let Some(result) = window.worker.poll() { assert!(result.is_err()); break; }
            assert!(std::time::Instant::now() < deadline, "hidden window consumed its pending result");
            std::thread::sleep(Duration::from_millis(2));
        }
        window.shutdown();
        app.run();
    }
    #[test]
    fn navigates_parent_refreshes_toggles_hidden_and_emits_original_file_paths() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let child = dir.path().join("child 文 folder");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(dir.path().join("image.PNG"), b"image").unwrap();
        std::fs::write(dir.path().join(".hidden"), b"hidden").unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        pump_until(&mut window, |window| window.control(window.listing).unwrap().entries.len() == 2);
        let listing = window.listing;
        let entry = window.control(listing).unwrap().entries[0].clone();
        window.open_entry(entry);
        pump_until(&mut window, |window| window.path == child);
        window.parent();
        pump_until(&mut window, |window| window.path == dir.path());
        window.toggle_hidden();
        pump_until(&mut window, |window| window.control(window.listing).unwrap().entries.len() == 3);
        std::fs::write(dir.path().join("new.txt"), b"new").unwrap();
        window.refresh();
        pump_until(&mut window, |window| window.control(window.listing).unwrap().entries.len() == 4);
        let list = window.control(window.listing).unwrap();
        let image_index = (0..list.entries.len()).find(|index| list.entries[*index].name == "image.PNG").unwrap();
        let entry = window.control(listing).unwrap().entries[image_index].clone();
        window.open_entry(entry);
        assert_eq!(window.take_action(), Some(BrowserAction::Open(dir.path().join("image.PNG"))));
        window.shutdown();
        app.run();
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;
    use crate::test_support::UI_LOCK;
    fn entries() -> Vec<Entry> {
        (0..23).map(|i| Entry { path: PathBuf::from(format!("/tmp/{i}")), name: format!("{i} 文 long filename"), kind: FileKind::File, size: "42".into(), modified: "today".into() }).collect()
    }
    #[test]
    fn information_actions_keep_the_clicked_or_selected_entry_in_both_views() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new(); list.viewport = Size::new(48,15); list.add_items(entries()[..2].to_vec());
        for mode in [ViewMode::Grid,ViewMode::Details] {
            list.set_mode(mode);
            let (x,y) = if mode == ViewMode::Grid { (16,0) } else { (0,2) };
            list.context_at(x,y);
            let menu = list.menu;
            MenuEvents::on_command(&mut list,menu,Handle::None,browserentries::Commands::Information);
            assert!(matches!(list.request.take(),Some(EntryRequest::Information(entry)) if entry.path == std::path::Path::new("/tmp/1")));
            assert!(list.menu.is_none());
            list.input(key!("Alt+Enter").into());
            assert!(matches!(list.request.take(),Some(EntryRequest::Information(entry)) if entry.path == std::path::Path::new("/tmp/1")));
            list.selected = None; list.input(key!("Alt+Enter").into()); assert!(list.request.is_none());
        }
        list.release_menu(); app.run();
    }
    #[test]
    fn loading_keeps_focus_and_hotkeys_available_without_opening_stale_entries() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new(); list.add_items(entries()); list.loading = true;
        for key in [key!("Tab"),key!("Shift+Tab"),key!("Alt+U"),key!("Alt+R"),key!("Alt+H"),key!("Alt+V"),key!("Alt+F4")] {
            assert!(matches!(list.input(key.into()),EventProcessStatus::Ignored),"key {key} was swallowed during loading");
        }
        list.input(key!("Enter").into()); assert!(list.request.is_none());
        list.release_menu(); app.run();
    }
    #[test]
    fn keyboard_context_without_selection_targets_current_folder_even_after_scrolling() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new(); list.viewport = Size::new(48,10); list.add_items(entries());
        for mode in [ViewMode::Grid,ViewMode::Details] {
            list.set_mode(mode); list.scroll(100);
            let (x,y) = if mode == ViewMode::Grid { (47,9) } else { (47,0) };
            OnMouseEvent::on_mouse_event(&mut list,&MouseEvent::Pressed(MouseEventData { x,y,button: MouseButton::Left,modifier: KeyModifier::None }));
            assert!(list.selected.is_none()); assert!(list.offset > 0);
            list.input(key!("Shift+F10").into());
            let menu = list.menu; MenuEvents::on_command(&mut list,menu,Handle::None,browserentries::Commands::Terminal);
            assert!(matches!(list.request.take(),Some(EntryRequest::Terminal(None))));
            assert!(list.selected.is_none());
        }
        list.release_menu(); app.run();
    }
    #[test]
    fn escape_without_open_popup_reaches_window_close_handler() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new(); list.add_items(entries());
        assert!(matches!(list.input(key!("Escape").into()),EventProcessStatus::Ignored));
        list.release_menu(); app.run();
    }
    #[test]
    fn grid_navigation_scroll_and_mode_switch_keep_selection_visible() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new();
        list.viewport = Size::new(48,10);
        list.add_items(entries());
        assert_eq!(list.columns(),3);
        list.input(key!("Right").into()); list.input(key!("Down").into());
        assert_eq!(list.selected,Some(4));
        list.input(key!("PageDown").into());
        assert_eq!(list.selected,Some(10));
        assert_eq!(list.point(16,5),Some(10));
        list.input(key!("End").into());
        assert_eq!(list.selected,Some(22));
        list.set_mode(ViewMode::Details);
        assert_eq!(list.selected,Some(22));
        assert_eq!(list.point(0,9),Some(22));
        list.set_mode(ViewMode::Grid);
        assert_eq!(list.point(16,5),Some(22));
        list.scroll(-100);
        assert_eq!(list.point(0,0),Some(0));
        list.input(key!("Home").into());
        assert_eq!(list.selected,Some(0));
        list.release_menu(); app.run();
    }
    #[test]
    fn context_targets_clicked_entry_and_blank_space_in_each_mode() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new(); list.viewport = Size::new(48,15); list.add_items(entries()[..2].to_vec());
        for mode in [ViewMode::Grid,ViewMode::Details] {
            list.set_mode(mode);
            let (x,y) = if mode == ViewMode::Grid { (16,0) } else { (0,2) };
            list.context_at(x,y);
            assert_eq!(list.selected,Some(1));
            let menu = list.menu;
            MenuEvents::on_command(&mut list,menu,Handle::None,browserentries::Commands::OpenWith);
            assert!(matches!(list.request.take(),Some(EntryRequest::OpenWith(path)) if path == std::path::Path::new("/tmp/1")));
            list.context_at(47,14);
            let menu = list.menu;
            MenuEvents::on_command(&mut list,menu,Handle::None,browserentries::Commands::Terminal);
            assert!(matches!(list.request.take(),Some(EntryRequest::Terminal(None))));
        }
        list.release_menu(); app.run();
    }
    #[test]
    fn tiny_surfaces_and_clipped_names_do_not_change_original_paths() {
        use std::os::unix::ffi::OsStringExt;
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new(); let mut values = entries();
        values[0].path = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/long <FILE_PATH> \xff".to_vec()));
        values[0].name = "文".repeat(500);
        let path = values[0].path.clone(); list.add_items(values);
        for mode in [ViewMode::Grid,ViewMode::Details] {
            list.set_mode(mode);
            for size in [Size::new(0,0),Size::new(1,1),Size::new(2,2),Size::new(15,4)] {
                OnResize::on_resize(&mut list,Size::new(0,0),size); list.input(key!("End").into()); list.input(key!("Home").into());
                let mut surface = Surface::new(size.width,size.height);
                OnPaint::on_paint(&list,&mut surface,&Theme::default());
                list.input(key!("Enter").into());
                assert!(matches!(list.request.take(),Some(EntryRequest::Open(entry)) if entry.path == path));
            }
        }
        list.release_menu(); app.run();
    }
    #[test]
    fn mouse_selection_double_click_wheel_and_keyboard_menu_use_same_entries() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new(); list.viewport = Size::new(48,10); list.add_items(entries());
        let click = |x,y,button| MouseEventData { x,y,button,modifier: KeyModifier::None };
        OnMouseEvent::on_mouse_event(&mut list,&MouseEvent::Pressed(click(17,1,MouseButton::Left)));
        assert_eq!(list.selected,Some(1)); assert!(list.request.is_none());
        OnMouseEvent::on_mouse_event(&mut list,&MouseEvent::DoubleClick(click(17,1,MouseButton::Left)));
        assert!(matches!(list.request.take(),Some(EntryRequest::Open(entry)) if entry.path == std::path::Path::new("/tmp/1")));
        OnMouseEvent::on_mouse_event(&mut list,&MouseEvent::Wheel(MouseWheelDirection::Down));
        assert_eq!(list.point(0,0),Some(9));
        OnMouseEvent::on_mouse_event(&mut list,&MouseEvent::Pressed(click(33,1,MouseButton::Right)));
        assert_eq!(list.selected,Some(11)); assert!(list.request.is_none());
        let old_menu = list.menu;
        list.release_menu();
        assert!(list.menu.is_none()); assert!(list.menu_target.is_none());
        MenuEvents::on_command(&mut list,old_menu,Handle::None,browserentries::Commands::Open);
        assert!(list.request.is_none());
        list.input(key!("Shift+F10").into());
        assert!(matches!(list.menu_target.as_ref(),Some(Some(entry)) if entry.path == std::path::Path::new("/tmp/11")));
        list.invalidate(); app.run();
    }
}
