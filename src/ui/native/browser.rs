use super::image_viewer::home_directory;
use crate::runtime::files::{self, Entry, FileKind, Listing};
use crate::runtime::latest::LatestWorker;
use crate::runtime::activity::{ActivityLog, Source as ActivitySource, Status as ActivityStatus};
use appcui::prelude::*;
use std::path::{Path, PathBuf};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BrowserAction { Open(PathBuf), OpenWith(PathBuf), OpenTerminal(PathBuf) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewMode { Grid, Details }
enum EntryRequest {
    Open(Entry), OpenWith(PathBuf), Terminal(Option<Entry>), Information(Entry),
    Refresh, Hidden, Mutate(Mutation,Option<Entry>), Batch(Mutation,Vec<Entry>),
    Favorite, Favorites, Find, TrashView, Restore(Vec<PathBuf>), TrashInfo(Entry), Notice(&'static str),
}
const SELECTION_LIMIT: usize = 1_000;

#[CustomControl(events = MenuEvents, overwrite = OnPaint+OnKeyPressed+OnMouseEvent+OnResize,
    commands = Open+OpenWith+Terminal+Information+Refresh+Hidden+Grid+Details+NewFile+NewFolder+Rename+Copy+Move+Trash+Favorite+Favorites+Find+TrashView+Restore)]
struct BrowserEntries {
    entries: Vec<Entry>, selected: Option<usize>, offset: usize, viewport: Size, mode: ViewMode,
    request: Option<EntryRequest>, menu: Handle<Menu>, menu_target: Option<Option<Entry>>, loading: bool, hidden: bool,
    marked: BTreeSet<usize>, anchor: Option<usize>, trash: bool, search: bool,
}
impl BrowserEntries {
    fn new() -> Self {
        Self { base: ControlBase::new(layout!("l:0,r:0,t:5,b:1"),true), entries: Vec::new(), selected: None,
            offset: 0, viewport: Size::new(0,0), mode: ViewMode::Grid, request: None,
            menu: Handle::None, menu_target: None, loading: false, hidden: false,
            marked: BTreeSet::new(), anchor: None, trash: false, search: false }
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
    fn clear(&mut self) { self.entries.clear(); self.selected = None; self.clear_marks(); self.offset = 0; self.invalidate(); }
    fn add_items(&mut self, entries: Vec<Entry>) {
        self.entries.extend(entries);
        if self.selected.is_none() && !self.entries.is_empty() { self.selected = Some(0); }
        self.clamp_scroll();
    }
    fn selected_entry(&self) -> Option<Entry> { self.selected.and_then(|i| self.entries.get(i)).cloned() }
    fn clear_marks(&mut self) { self.marked.clear(); self.anchor = None; }
    fn toggle_mark(&mut self, index: usize) {
        if !self.marked.remove(&index) {
            if self.marked.len() == SELECTION_LIMIT { self.selection_limit(); return; }
            self.marked.insert(index);
        }
        self.anchor = Some(index);
    }
    fn selection_limit(&mut self) {
        self.request = Some(EntryRequest::Notice("Select up to 1000 items at a time"));
    }
    fn mark_range(&mut self, from: usize, to: usize) {
        if from.abs_diff(to) >= SELECTION_LIMIT { self.selection_limit(); return; }
        self.anchor = Some(from);
        self.marked = (from.min(to)..=from.max(to)).collect();
    }
    fn targets(&self, entry: Option<Entry>) -> Vec<PathBuf> {
        if self.marked.is_empty() { return entry.into_iter().map(|entry| entry.path).collect(); }
        self.marked.iter().filter_map(|&index| self.entries.get(index)).map(|entry| entry.path.clone()).collect()
    }
    fn mutation_request(&self, mutation: Mutation, entry: Option<Entry>) -> EntryRequest {
        if !matches!(mutation,Mutation::Copy | Mutation::Move | Mutation::Trash) || self.marked.is_empty() {
            return EntryRequest::Mutate(mutation,entry);
        }
        if self.marked.len() == 1 {
            return EntryRequest::Mutate(mutation,self.marked.first().and_then(|index| self.entries.get(*index)).cloned());
        }
        let marked = self.marked.iter().filter_map(|&index| self.entries.get(index)).cloned().collect();
        EntryRequest::Batch(mutation,marked)
    }
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
        match key.value() {
            key!("Space") => {
                if let Some(index) = self.selected { self.toggle_mark(index); }
                return EventProcessStatus::Processed;
            }
            key!("Ctrl+A") => {
                if !self.entries.is_empty() { self.mark_range(0,self.entries.len()-1); }
                return EventProcessStatus::Processed;
            }
            key!("Escape") if !self.marked.is_empty() => {
                self.clear_marks(); return EventProcessStatus::Processed;
            }
            key!("Enter") if self.trash => {
                self.request = Some(EntryRequest::Restore(self.targets(self.selected_entry())));
                return EventProcessStatus::Processed;
            }
            _ => {}
        }
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
            self.request = self.selected_entry().map(if self.trash { EntryRequest::TrashInfo } else { EntryRequest::Information });
            return if self.request.is_some() { EventProcessStatus::Processed } else { EventProcessStatus::Ignored };
        }
        let mutation = if key.value() == key!("Ctrl+N") { Some(Mutation::NewFile) }
            else if key.value() == key!("Ctrl+Shift+N") { Some(Mutation::NewFolder) }
            else if key.value() == key!("F2") { Some(Mutation::Rename) }
            else if key.value() == key!("F5") { Some(Mutation::Copy) }
            else if key.value() == key!("F6") { Some(Mutation::Move) }
            else if key.value() == key!("Delete") { Some(Mutation::Trash) } else { None };
        if let Some(mutation) = mutation && !self.trash {
            self.release_menu(); self.request = Some(self.mutation_request(mutation,self.selected_entry())); return EventProcessStatus::Processed;
        }
        if key.modifier != KeyModifier::None && key.modifier != KeyModifier::Shift { return EventProcessStatus::Ignored; }
        let selected = self.selected.unwrap_or(0);
        let step = self.columns();
        let index = match key.code {
            KeyCode::Left => selected.saturating_sub(1), KeyCode::Right => selected.saturating_add(1),
            KeyCode::Up => selected.saturating_sub(step), KeyCode::Down => selected.saturating_add(step),
            KeyCode::Home => 0, KeyCode::End => self.entries.len().saturating_sub(1),
            KeyCode::PageUp => selected.saturating_sub(step*self.page_rows()),
            KeyCode::PageDown => selected.saturating_add(step*self.page_rows()),
            KeyCode::Enter if key.modifier == KeyModifier::None => {
                self.request = self.selected_entry().map(EntryRequest::Open); return EventProcessStatus::Processed;
            }
            _ => return EventProcessStatus::Ignored,
        };
        self.selected = (!self.entries.is_empty()).then_some(index.min(self.entries.len().saturating_sub(1)));
        if key.modifier == KeyModifier::Shift && let Some(index) = self.selected {
            self.mark_range(self.anchor.unwrap_or(selected),index);
        } else { self.anchor = self.selected; }
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
        if let Some(index) = index {
            if !self.marked.contains(&index) { self.clear_marks(); }
            self.selected = Some(index);
        }
        let target = index.and_then(|i| self.entries.get(i)).cloned();
        self.show_context(target,x,y);
    }
    fn show_context(&mut self, target: Option<Entry>, x: i32, y: i32) {
        self.release_menu();
        let mut menu = Menu::new();
        use browserentries::Commands;
        if self.trash {
            if target.is_some() || !self.marked.is_empty() {
                let caption = if self.marked.len() > 1 { "Rest&ore marked items…" } else { "Rest&ore to original location…" };
                menu.add(menu::Command::new(caption,Key::None,Commands::Restore));
            }
            if target.is_some() { menu.add(menu::Command::new("Original &location…",key!("Alt+Enter"),Commands::Information)); }
            menu.add(menu::Command::new("Close &Trash",Key::None,Commands::TrashView));
        } else {
            if let Some(entry) = &target {
                if entry.kind != FileKind::Other { menu.add(menu::Command::new("&Open",Key::None,Commands::Open)); }
                if entry.kind == FileKind::File { menu.add(menu::Command::new("Open &with…",Key::None,Commands::OpenWith)); }
            }
            menu.add(menu::Command::new(if target.is_some() { "Open in &terminal" } else { "Open &terminal here" },Key::None,Commands::Terminal));
            if target.is_some() {
                menu.add(menu::Command::new("File &information…",key!("Alt+Enter"),Commands::Information));
                menu.add(menu::Command::new("&Rename…",Key::None,Commands::Rename));
            }
            if target.is_some() || !self.marked.is_empty() {
                let actions = if self.marked.len() > 1 {
                    [("&Copy marked items…",Commands::Copy),("&Move marked items…",Commands::Move),("&Trash marked items…",Commands::Trash)]
                } else { [("&Copy to…",Commands::Copy),("&Move to…",Commands::Move),("Move to &trash…",Commands::Trash)] };
                for (caption,command) in actions {
                    menu.add(menu::Command::new(caption,Key::None,command));
                }
            }
            menu.add(menu::Command::new("New &file…",key!("Ctrl+N"),Commands::NewFile));
            menu.add(menu::Command::new("New f&older…",key!("Ctrl+Shift+N"),Commands::NewFolder));
            menu.add(menu::Command::new("Add/remove current f&avorite",Key::None,Commands::Favorite));
            menu.add(menu::Command::new("Fa&vorite folders…",Key::None,Commands::Favorites));
            menu.add(menu::Command::new("Find in &subfolders…",key!("Ctrl+F"),Commands::Find));
            menu.add(menu::Command::new("View Tr&ash",Key::None,Commands::TrashView));
        }
        menu.add(menu::Separator::new());
        menu.add(menu::Command::new("&Refresh",Key::None,Commands::Refresh));
        if !self.trash { menu.add(menu::Command::new(if self.hidden { "&Hidden: on" } else { "&Hidden: off" },Key::None,Commands::Hidden)); }
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
            Commands::Information => self.request = target.map(if self.trash { EntryRequest::TrashInfo } else { EntryRequest::Information }),
            Commands::Refresh => self.request = Some(EntryRequest::Refresh),
            Commands::Hidden => self.request = Some(EntryRequest::Hidden),
            Commands::Grid => self.set_mode(ViewMode::Grid), Commands::Details => self.set_mode(ViewMode::Details),
            Commands::NewFile => self.request = Some(EntryRequest::Mutate(Mutation::NewFile,None)),
            Commands::NewFolder => self.request = Some(EntryRequest::Mutate(Mutation::NewFolder,None)),
            Commands::Rename => self.request = target.map(|entry| EntryRequest::Mutate(Mutation::Rename,Some(entry))),
            Commands::Copy => self.request = Some(self.mutation_request(Mutation::Copy,target)),
            Commands::Move => self.request = Some(self.mutation_request(Mutation::Move,target)),
            Commands::Trash => self.request = Some(self.mutation_request(Mutation::Trash,target)),
            Commands::Favorite => self.request = Some(EntryRequest::Favorite), Commands::Favorites => self.request = Some(EntryRequest::Favorites),
            Commands::Find => self.request = Some(EntryRequest::Find),
            Commands::TrashView => self.request = Some(EntryRequest::TrashView),
            Commands::Restore => self.request = Some(EntryRequest::Restore(self.targets(target))),
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
            let header = if self.trash { "Original name / folder         Type       Bytes        Deleted" }
                else if self.search { "Name / relative folder         Type       Bytes        Modified" }
                else { "Name                           Type       Bytes        Modified" };
            surface.write_string(0,0,&clipped(header,width),theme.text.highlighted,false);
        }
        let start = self.offset * columns;
        let end = self.entries.len().min(start.saturating_add(self.visible_rows()*columns));
        for index in start..end {
            let entry = &self.entries[index];
            let x = ((index % columns)*tile_width) as i32;
            let y = ((index/columns-self.offset)*self.row_height()+self.header()) as i32;
            let marked = self.marked.contains(&index);
            let attr = if self.selected == Some(index) { if self.has_focus() { theme.list_current_item.focus } else { theme.list_current_item.normal } } else { theme.text.normal };
            for row in 0..self.row_height() { surface.fill_horizontal_line_with_size(x,y+row as i32,tile_width as u32,Character::with_attributes(' ',attr)); }
            if self.mode == ViewMode::Grid {
                let icon = match entry.kind { FileKind::Directory => [" __  ","|  | ","|__| "], FileKind::Other => [" ___ ","| ? |","|___|"], FileKind::File => [" ___ ","|   |","|___|"] };
                let label = if entry.kind == FileKind::Directory { "DIR" } else if files::is_image(&entry.path) { "IMG" } else if files::is_audio(&entry.path) { "AUD" } else if files::is_pdf(&entry.path) { "PDF" } else { "   " };
                for (row,line) in icon.iter().enumerate() { surface.write_string(x,y+row as i32,&clipped(line,tile_width),attr,false); }
                if tile_width >= 5 { surface.write_string(x+1,y+1,label,attr,false); }
                surface.write_string(x,y+3,&clipped(&entry.name,tile_width.saturating_sub(1)),attr,false);
                if marked { surface.write_string(x+tile_width as i32-1,y,"*",attr,false); }
            } else {
                let kind = match entry.kind { FileKind::Directory => "Folder", FileKind::File => "File", FileKind::Other => "Other" };
                if marked { surface.write_string(0,y,"*",attr,false); }
                let inset = if marked { 2 } else { 0 };
                for (x,text,limit) in [(inset,entry.name.as_str(),30-inset),(31,kind,10),(42,entry.size.as_str(),12),(55,entry.modified.as_str(),20)] {
                    if x < width { surface.write_string(x as i32,y,&clipped(text,limit.min(width-x)),attr,false); }
                }
            }
        }
        if self.entries.is_empty() {
            let message = if self.loading { "Loading…" } else if self.trash { "No Trash items" }
                else if self.search { "No filename matches" } else { "Empty folder" };
            surface.write_string(0,self.header() as i32,&clipped(message,width),theme.text.normal,false);
        }
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
                    self.release_menu();
                    let clicked = self.point(data.x,data.y);
                    if data.modifier.contains(KeyModifier::Shift) {
                        if let Some(index) = clicked {
                            self.mark_range(self.anchor.or(self.selected).unwrap_or(index),index);
                            self.selected = clicked;
                        }
                    } else if data.modifier.contains(KeyModifier::Ctrl) {
                        if let Some(index) = clicked {
                            if !matches!(event,MouseEvent::DoubleClick(_)) {
                                if self.marked.is_empty() && let Some(previous) = self.selected && previous != index {
                                    self.marked.insert(previous);
                                }
                                self.toggle_mark(index);
                            }
                            self.selected = clicked;
                        }
                    } else {
                        self.clear_marks(); self.selected = clicked; self.anchor = clicked;
                        if matches!(event,MouseEvent::DoubleClick(_)) {
                            self.request = if self.trash { Some(EntryRequest::Restore(self.targets(self.selected_entry()))) }
                                else { self.selected_entry().map(EntryRequest::Open) };
                        }
                    }
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
        Self::with_labels("File information","Full path","Modified",entry)
    }
    fn with_labels(title: &str, path_label: &str, date_label: &str, entry: &Entry) -> Self {
        let display = |value: &str| value.chars().map(|ch| if ch.is_control() { '�' } else { ch }).collect::<String>();
        let kind = match entry.kind { FileKind::Directory => "Folder", FileKind::File => "File", FileKind::Other => "Other" };
        let size = if entry.kind == FileKind::File && entry.size != "—" { format!("{} bytes",entry.size) } else { "—".to_owned() };
        let text = format!("Name: {}\n\n{path_label}: {}\n\nType: {kind}\n\nSize: {size}\n\n{date_label}: {}",
            display(&entry.name),display(&entry.path.to_string_lossy()),display(&entry.modified));
        let mut dialog = Self {
            base: ModalWindow::new(title,layout!("a:c,w:74,h:18"),window::Flags::Sizeable),
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
        Self::build(title,original_folder,Some(original_name),favorites)
    }
    fn folder(title: &str, path: &Path, favorites: &[PathBuf]) -> Self {
        Self::build(title,path.to_owned(),None,favorites)
    }
    fn build(title: &str, original_folder: PathBuf, name: Option<PathBuf>, favorites: &[PathBuf]) -> Self {
        let mut dialog = Self { base: ModalWindow::new(title,layout!("a:c,w:76,h:13"),window::Flags::None),
            folder: Handle::None,name: Handle::None,original_folder,original_name: name.clone().unwrap_or_default(),favorites: favorites.to_vec(),choose_favorite: Handle::None,apply: Handle::None,cancel: Handle::None,status: Handle::None };
        dialog.add(Label::new("Destination folder (absolute or relative to the browser)",layout!("l:2,r:2,t:1,h:1")));
        let text = dialog.original_folder.to_string_lossy().into_owned(); dialog.folder = dialog.add(TextField::new(&text,layout!("l:2,r:2,t:2,h:1"),textfield::Flags::None));
        if let Some(name) = name {
            dialog.add(Label::new("Filename (unchanged names retain exact OS bytes)",layout!("l:2,r:2,t:4,h:1")));
            dialog.name = dialog.add(TextField::new(&name.to_string_lossy(),layout!("l:2,r:2,t:5,h:1"),textfield::Flags::None));
        } else {
            dialog.add(Label::new("Selected items keep their original filenames.",layout!("l:2,r:2,t:4,h:1")));
        }
        dialog.status = dialog.add(Label::new("Existing files require confirmation; folders are never merged.",layout!("l:2,r:2,t:7,h:2")));
        dialog.choose_favorite = dialog.add(Button::new("&Favorites…",layout!("l:2,b:0,w:18")));
        dialog.apply = dialog.add(Button::new("&Apply",layout!("l:28,b:0,w:18"))); dialog.cancel = dialog.add(Button::new("&Cancel",layout!("r:2,b:0,w:18")));
        dialog.base.request_focus_for_control(dialog.folder); dialog
    }
    fn destination(&self) -> anyhow::Result<PathBuf> {
        let folder = self.control(self.folder).map(|field| field.text()).unwrap_or("");
        anyhow::ensure!(!folder.is_empty() && !folder.contains('\0'),"Choose a nonempty destination folder");
        let folder = dialog_path(&self.original_folder,folder);
        let Some(name) = self.control(self.name).map(|field| field.text()) else { return Ok(folder); };
        anyhow::ensure!(!name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0'),"Enter one filename without / or NUL");
        Ok(folder.join(dialog_path(&self.original_name,name)))
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

#[derive(Clone,Default,PartialEq,Eq)]
enum BrowserPage { #[default] Folder, Search(String), Trash }

struct ListingRequest {
    path: PathBuf,
    page: BrowserPage,
    hidden: bool,
    trash_root: Option<PathBuf>,
    canceled: Arc<AtomicBool>,
}

struct BrowserListing {
    listing: Listing,
    page: BrowserPage,
    trash_items: HashMap<PathBuf,files::TrashItem>,
    status: Option<String>,
}

fn load_listing(request: ListingRequest) -> anyhow::Result<BrowserListing> {
    let ListingRequest { path,page,hidden,trash_root,canceled } = request;
    let mut trash_items = HashMap::new();
    let (listing,status) = match &page {
        BrowserPage::Folder => (files::list_directory(&path,hidden)?,None),
        BrowserPage::Search(query) => {
            let (listing,status) = files::search_filenames(&path,query,hidden,&canceled)?;
            (listing,Some(status))
        }
        BrowserPage::Trash => {
            let (items,status) = files::list_trash(trash_root,&canceled)?;
            let entries = items.into_iter().map(|(entry,item)| { trash_items.insert(entry.path.clone(),item); entry }).collect();
            (Listing { path,entries },Some(status))
        }
    };
    Ok(BrowserListing { listing,page,trash_items,status })
}

struct Batch {
    pending: VecDeque<files::Operation>,
    finished: usize,
    total: usize,
}

struct OperationNotice { message: String, dialog: Option<&'static str>, refresh: bool }

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
    close_pending: bool,
    filter_field: Handle<TextField>, favorite_button: Handle<Button>, favorites_button: Handle<Button>, cancel_operation: Handle<Button>, status: Handle<Label>,
    find: Handle<Button>, trash: Handle<Button>, page_label: Handle<Label>,
    saved: files::BrowserState, state_worker: Option<files::StateWorker>, state_ready: bool, restore_location: bool,
    operation: files::OperationWorker,
    batch: Option<Batch>,
    deferred_outcome: Option<files::Outcome>,
    notice: Option<OperationNotice>,
    activity: ActivityLog,
    worker: LatestWorker<ListingRequest, Result<BrowserListing, String>>,
    page: BrowserPage, pending_listing: Option<(PathBuf,BrowserPage)>, announce_listing: bool, cancel_listing: Arc<AtomicBool>,
    trash_root: Option<PathBuf>, trash_items: HashMap<PathBuf,files::TrashItem>,
    action: Option<BrowserAction>,
    pending_entries: VecDeque<Entry>,
}
impl BrowserWindow {
    pub fn new(activity: ActivityLog) -> anyhow::Result<Self> { Self::with_user_state(home_directory(),true,activity) }
    /// Open an explicit location while loading saved view/filter/favorites in the background.
    pub fn new_at(path: PathBuf, activity: ActivityLog) -> anyhow::Result<Self> { Self::with_user_state(path,false,activity) }
    pub fn current_path(&self) -> Option<PathBuf> { (!self.is_closing()).then(|| self.path.clone()) }
    fn with_user_state(path: PathBuf, restore: bool, activity: ActivityLog) -> anyhow::Result<Self> {
        // Headless tests must never write host user state; persistence tests inject isolated paths.
        let state_path = if cfg!(test) { Ok(None) } else { files::default_state_path().map(Some) };
        let mut window = Self::with_state(path,state_path.as_ref().ok().cloned().flatten(),restore)?;
        window.activity = activity;
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
        let worker = LatestWorker::new(|request| load_listing(request).map_err(|error| format!("{error:#}")))?;
        let mut window = Self {
            base: Window::new("File Browser — Enter Open | Alt+U Parent | Alt+R Refresh | Alt+H Hidden", layout!("a:c,w:90%,h:80%"), window::Flags::Sizeable),
            path, path_field: Handle::None, listing: Handle::None, up: Handle::None, refresh: Handle::None, hidden_button: Handle::None, mode_button: Handle::None,
            hidden: false, closing: false, close_pending: false, worker, action: None, pending_entries: VecDeque::new(),
            filter_field: Handle::None, favorite_button: Handle::None, favorites_button: Handle::None, cancel_operation: Handle::None, status: Handle::None,
            saved: files::BrowserState::default(), state_worker, state_ready, restore_location, operation: files::OperationWorker::new()?,
            find: Handle::None, trash: Handle::None, page_label: Handle::None, batch: None, deferred_outcome: None,
            notice: None, activity: ActivityLog::default(),
            page: BrowserPage::Folder, pending_listing: None, announce_listing: true, cancel_listing: Arc::new(AtomicBool::new(false)),
            trash_root: None, trash_items: HashMap::new(),
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
        window.find = window.add(Button::with_type("&Find…",layout!("x:0,y:4,w:11"),button::Type::Flat));
        window.trash = window.add(Button::with_type("&Trash",layout!("x:12,y:4,w:13"),button::Type::Flat));
        window.page_label = window.add(Label::new("",layout!("l:26,r:0,t:4,h:1")));
        window.status = window.add(Label::new("Space Mark | F2 Rename | F5 Copy | F6 Move | Delete Trash",layout!("l:0,r:0,b:0,h:1")));
        window.listing = window.add(BrowserEntries::new());
        // Keep directories first: user sorting is deliberately disabled for this view.
        window.timer().ok_or_else(|| anyhow::anyhow!("File Browser timer unavailable"))?.start(Duration::from_millis(40));
        window.navigate(window.path.clone());
        Ok(window)
    }
    fn navigate(&mut self, path: PathBuf) {
        let target = if path.is_absolute() { path } else { self.path.join(path) };
        self.submit_listing(target,BrowserPage::Folder,true);
    }
    fn submit_listing(&mut self, path: PathBuf, page: BrowserPage, announce: bool) {
        self.cancel_listing.store(true,Ordering::Relaxed);
        self.cancel_listing = Arc::new(AtomicBool::new(false));
        self.pending_entries.clear();
        self.action = None;
        let handle = self.listing;
        if let Some(list) = self.control_mut(handle) { list.invalidate(); list.loading = true; }
        self.announce_listing = announce;
        self.pending_listing = Some((path.clone(),page.clone()));
        self.worker.submit(ListingRequest { path,page,hidden: self.hidden,trash_root: self.trash_root.clone(),canceled: Arc::clone(&self.cancel_listing) });
    }
    fn parent(&mut self) {
        if self.requested_location().1 != &BrowserPage::Folder || self.page != BrowserPage::Folder {
            self.navigate(self.path.clone());
        } else if let Some(parent) = self.path.parent() { self.navigate(parent.to_owned()); }
    }
    fn requested_location(&self) -> (&PathBuf,&BrowserPage) {
        self.pending_listing.as_ref().map(|(path,page)| (path,page)).unwrap_or((&self.path,&self.page))
    }
    fn reload(&mut self, announce: bool) {
        let (path,page) = self.requested_location();
        self.submit_listing(path.clone(),page.clone(),announce);
    }
    fn refresh(&mut self) { self.reload(true); }
    fn refresh_after_operation(&mut self) { self.reload(self.pending_listing.is_some() && self.announce_listing); }
    pub(crate) fn find_shortcut(&mut self) -> EventProcessStatus {
        if self.page == BrowserPage::Trash || self.is_closing() { return EventProcessStatus::Ignored; }
        self.show_search();
        EventProcessStatus::Processed
    }
    fn show_search(&mut self) {
        let initial = match &self.page { BrowserPage::Search(query) => Some(query.clone()), _ => None };
        if let Some(query) = dialogs::input::<String>("Find filenames","Search the current folder and its subfolders",initial,None) {
            if query.trim().is_empty() || query.len() > 512 { self.set_status("Enter a filename query of 1–512 bytes"); return; }
            self.set_status("Searching subfolders…");
            self.submit_listing(self.path.clone(),BrowserPage::Search(query),true);
        }
    }
    fn toggle_trash(&mut self) {
        if self.requested_location().1 == &BrowserPage::Trash { self.navigate(self.path.clone()); }
        else {
            self.set_status("Loading Trash…");
            self.submit_listing(self.path.clone(),BrowserPage::Trash,true);
        }
    }
    fn toggle_hidden(&mut self) {
        self.hidden = !self.hidden;
        let button = self.hidden_button;
        let hidden = self.hidden;
        if let Some(button) = self.control_mut(button) { button.set_caption(if hidden { "&Hidden: on" } else { "&Hidden: off" }); }
        let handle = self.listing;
        if let Some(list) = self.control_mut(handle) { list.hidden = hidden; }
        self.save_view(); self.refresh();
    }
    pub fn take_action(&mut self) -> Option<BrowserAction> { if self.is_visible() && !self.is_closing() { self.action.take() } else { None } }
    pub fn is_closing(&self) -> bool { self.closing || self.close_pending }
    pub fn shutdown(&mut self) { self.cleanup(); self.close(); }
    fn cleanup(&mut self) {
        self.operation.cancel();
        self.cancel_listing.store(true,Ordering::Relaxed);
        self.batch = None; self.deferred_outcome = None; self.notice = None;
        self.closing = true; self.close_pending = false; self.worker.close(); self.action = None; self.pending_entries.clear();
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
            Some(EntryRequest::Batch(mutation,paths)) => self.mutate_batch(mutation,paths),
            Some(EntryRequest::Find) => self.show_search(),
            Some(EntryRequest::TrashView) => self.toggle_trash(),
            Some(EntryRequest::Restore(paths)) => self.restore_items(paths),
            Some(EntryRequest::TrashInfo(mut entry)) => {
                if let Some(item) = self.trash_items.get(&entry.path) {
                    entry.path = item.original_path().to_owned();
                    entry.name = entry.path.file_name().unwrap_or(entry.path.as_os_str()).to_string_lossy().into_owned();
                    FileInformationDialog::with_labels("Trash information","Original path","Deleted",&entry).show();
                }
            }
            Some(EntryRequest::Notice(message)) => self.set_status(message),
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
        match self.operation.submit(operation) {
            Ok(()) => self.set_status("Working… Cancel operation stops before publication"),
            Err(error) => self.operation_notice(ActivityStatus::Failed,format!("{error:#}"),None,false),
        }
    }
    fn begin_batch(&mut self, operations: VecDeque<files::Operation>) {
        if operations.is_empty() { return; }
        self.batch = Some(Batch { total: operations.len(), finished: 0, pending: operations });
        self.start_next_operation();
    }
    fn start_next_operation(&mut self) {
        let next = self.batch.as_mut().and_then(|batch| batch.pending.pop_front());
        match next {
            Some(operation) => {
                if let Err(error) = self.operation.submit(operation) { self.deferred_outcome = Some(files::Outcome::Failed(format!("{error:#}"))); }
            }
            None => self.finish_batch(ActivityStatus::Completed,None),
        }
    }
    fn operation_busy(&self) -> bool {
        self.waiting_for_operation() || self.notice.is_some()
    }
    fn waiting_for_operation(&self) -> bool {
        self.batch.is_some() || self.operation.busy() || self.deferred_outcome.is_some()
    }
    fn finish_batch(&mut self, status: ActivityStatus, reason: Option<&str>) {
        let Some(batch) = self.batch.take() else { return; };
        let summary = format!("{} of {} items completed",batch.finished,batch.total);
        let message = reason.map_or_else(|| summary.clone(),|reason| format!("{summary}. {reason}"));
        self.operation_notice(status,message,reason.map(|_| "Batch stopped"),true);
    }
    fn operation_notice(&mut self, status: ActivityStatus, message: String, dialog: Option<&'static str>, refresh: bool) {
        self.activity.record(ActivitySource::Files,status,&message);
        self.notice = Some(OperationNotice { message,dialog,refresh });
        self.present_operation_notice();
    }
    fn present_operation_notice(&mut self) -> bool {
        if !self.can_present() { return false; }
        let Some(notice) = self.notice.take() else { return false; };
        if notice.refresh { self.refresh_after_operation(); }
        self.set_status(&notice.message);
        if let Some(title) = notice.dialog { dialogs::error(title,&notice.message); }
        true
    }
    fn can_present(&self) -> bool {
        !self.is_closing() && self.is_visible() && (self.handle().is_none() || self.has_input_focus())
    }
    fn mutate_batch(&mut self, mutation: Mutation, entries: Vec<Entry>) {
        if self.operation_busy() { self.set_status("Another file operation is running; cancel it or wait"); return; }
        if entries.iter().any(|entry| entry.identity.is_none()) {
            self.set_status("A marked item is unavailable. Refresh and select it again"); return;
        }
        let selected = entries.iter().map(|entry| entry.path.as_path()).collect::<BTreeSet<_>>();
        if selected.iter().any(|path| path.ancestors().skip(1).any(|parent| selected.contains(parent))) {
            self.set_status("Select a folder or its contents for a batch, separately"); return;
        }
        let operations = if mutation == Mutation::Trash {
            if !dialogs::validate("Move to Trash?",&format!("Move {} selected items to the user Trash?",entries.len())) { return; }
            entries.into_iter().map(|entry| files::Operation::Trash { source: entry.path,root: self.trash_root.clone(),expected_source: entry.identity }).collect()
        } else {
            let names = entries.iter().filter_map(|entry| entry.path.file_name()).collect::<BTreeSet<_>>();
            if names.len() != entries.len() { self.set_status("Selected items must have distinct filenames"); return; }
            let title = if mutation == Mutation::Copy { "Copy selected items" } else { "Move selected items" };
            let Some(folder) = DestinationDialog::folder(title,&self.path,&self.saved.favorites).show() else { return; };
            let folder = if folder.is_absolute() { folder } else { self.path.join(folder) };
            entries.into_iter().filter_map(|entry| {
                let destination = folder.join(entry.path.file_name()?);
                Some(files::Operation::Transfer { source: entry.path,destination,moving: mutation == Mutation::Move,approval: None,expected_source: entry.identity })
            }).collect()
        };
        self.begin_batch(operations);
    }
    fn restore_items(&mut self, paths: Vec<PathBuf>) {
        if self.operation_busy() { self.set_status("Another file operation is running; cancel it or wait"); return; }
        let items = paths.iter().filter_map(|path| self.trash_items.get(path).cloned()).collect::<Vec<_>>();
        let question = match items.as_slice() {
            [] => return,
            [item] => format!("Restore to {}?",item.original_path().display()),
            _ => format!("Restore {} items to their original locations?",items.len()),
        };
        if !dialogs::validate("Restore from Trash?",&question) { return; }
        if let [item] = items.as_slice() { self.start_operation(files::Operation::Restore(item.clone())); }
        else { self.begin_batch(items.into_iter().map(files::Operation::Restore).collect()); }
    }
    fn poll_operation(&mut self) -> bool {
        use files::Outcome;
        if self.present_operation_notice() { return true; }
        let canceled = self.operation.is_canceled();
        if !canceled && !self.can_present() && matches!(self.deferred_outcome,Some(Outcome::Confirm(_))) { return false; }
        if let Some(outcome) = self.deferred_outcome.take().or_else(|| self.operation.poll()) {
            match outcome {
                Outcome::Complete(message) => {
                    if let Some(batch) = &mut self.batch {
                        batch.finished += 1;
                        if canceled && !batch.pending.is_empty() { self.finish_batch(ActivityStatus::Canceled,Some("Canceled. Published work was retained.")); }
                        else { self.start_next_operation(); }
                    } else { self.operation_notice(ActivityStatus::Completed,message,None,true); }
                }
                Outcome::Confirm(operation) => {
                    if !canceled && !self.can_present() { self.deferred_outcome = Some(Outcome::Confirm(operation)); return false; }
                    let destination = match &operation { files::Operation::Transfer { destination, .. } => destination.display().to_string(), _ => String::new() };
                    if !canceled && dialogs::validate("Replace existing file?",&format!("Replace {destination}? The source and destination will be checked again before publication.")) {
                        if let Err(error) = self.operation.submit(operation) { self.report_operation_issue(ActivityStatus::Failed,format!("{error:#}")); }
                    } else if self.batch.is_some() { self.finish_batch(ActivityStatus::Canceled,Some("Replacement canceled. Published work was retained.")); }
                    else { self.operation_notice(ActivityStatus::Canceled,"Replacement canceled; source and destination preserved".into(),None,false); }
                }
                Outcome::Warning(message) => self.report_operation_issue(ActivityStatus::Attention,message),
                Outcome::Failed(message) => self.report_operation_issue(ActivityStatus::Failed,message),
                Outcome::Canceled(message) => self.report_operation_issue(ActivityStatus::Canceled,message),
            }
            return true;
        }
        if !self.is_visible() || !self.operation.busy() { return false; }
        let (entries,bytes) = self.operation.progress();
        let progress = self.batch.as_ref().map(|batch| format!("Item {}/{} | ",batch.finished+1,batch.total)).unwrap_or_default();
        self.set_status(&format!("{progress}Working: {entries} entries, {bytes} bytes | Cancel operation to stop"));
        true
    }
    fn report_operation_issue(&mut self, status: ActivityStatus, message: String) {
        if self.batch.is_some() { self.finish_batch(status,Some(&message)); }
        else {
            let published = status == ActivityStatus::Attention;
            let title = if published { "File operation needs attention" } else { "File operation stopped" };
            self.operation_notice(status,message,Some(title),published);
        }
    }
    fn mutate(&mut self, mutation: Mutation, entry: Option<Entry>) {
        if self.operation_busy() { self.set_status("Another file operation is running; cancel it or wait"); return; }
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
                self.start_operation(files::Operation::Trash { source: entry.path, root: self.trash_root.clone(), expected_source: entry.identity });
            }
            return;
        }
        let destination = if mutation == Mutation::Rename {
            let parent = entry.path.parent().unwrap_or(&self.path).to_owned();
            let original = entry.path.file_name().map(PathBuf::from).unwrap_or_default();
            let initial = original.to_string_lossy().into_owned();
            dialogs::input::<String>("Rename entry","New filename (unchanged opaque names retain exact bytes)",Some(initial),None).and_then(|text| {
                if text.is_empty() || text == "." || text == ".." || text.contains('/') || text.contains('\0') { self.set_status("Enter one filename without / or NUL"); None }
                else { Some(parent.join(dialog_path(&original,&text))) }
            })
        } else { DestinationDialog::new(if mutation == Mutation::Copy { "Copy to folder and filename" } else { "Move to folder and filename" },&entry.path,&self.saved.favorites).show() };
        if let Some(destination) = destination {
            let destination = if destination.is_absolute() { destination } else { self.path.join(destination) };
            self.start_operation(files::Operation::Transfer { source: entry.path, destination, moving: mutation != Mutation::Copy, approval: None, expected_source: entry.identity });
        }
    }
    fn present_listing(&mut self, result: BrowserListing) {
        let BrowserListing { listing,page,trash_items,status } = result;
        let page_changed = self.page != page;
        self.path = listing.path;
        self.page = page;
        self.trash_items = trash_items;
        let path = self.path.to_string_lossy().into_owned();
        let field = self.path_field;
        if let Some(field) = self.control_mut(field) { field.set_text(&path); }
        let folder = self.page == BrowserPage::Folder;
        let trash = self.page == BrowserPage::Trash;
        let page_text = match &self.page {
            BrowserPage::Folder => String::new(),
            BrowserPage::Search(query) => format!("{} | Find: {}",status.as_deref().unwrap_or("Search"),clipped(query,80)),
            BrowserPage::Trash => format!("{} | Enter Restore",status.as_deref().unwrap_or("Trash")),
        };
        let label = self.page_label;
        if let Some(label) = self.control_mut(label) { label.set_caption(&page_text.replace('&',"&&")); }
        let button = self.up;
        if let Some(button) = self.control_mut(button) { button.set_caption(if folder { "&Up" } else { "Back (&U)" }); }
        let button = self.trash;
        if let Some(button) = self.control_mut(button) { button.set_caption(if trash { "Close &Trash" } else { "&Trash" }); }
        let filter = self.filter_field;
        if let Some(filter) = self.control_mut(filter) { filter.set_enabled(folder); }
        for button in [self.find,self.hidden_button,self.favorite_button,self.favorites_button] {
            if let Some(button) = self.control_mut(button) { button.set_enabled(!trash); }
        }
        let handle = self.listing;
        let focus = self.can_present();
        if let Some(list) = self.control_mut(handle) {
            list.clear();
            list.loading = !listing.entries.is_empty();
            list.trash = trash;
            list.search = !trash && !folder;
            if focus { list.request_focus(); }
        }
        self.pending_entries = listing.entries.into();
        if self.announce_listing && !self.operation_busy() {
            if let Some(status) = status { self.set_status(&status); }
            else if page_changed { self.set_status("Space Mark | F2 Rename | F5 Copy | F6 Move | Delete Trash"); }
        }
        if folder { self.save_view(); }
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
        else if handle == self.find { self.show_search(); }
        else if handle == self.trash { self.toggle_trash(); }
        else if handle == self.cancel_operation && self.operation_busy() {
            self.operation.cancel(); self.set_status("Cancellation requested; already published work is retained");
        }
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
        if self.closing { return EventProcessStatus::Ignored; }
        if self.close_pending || !self.is_visible() {
            self.poll_operation();
            if self.close_pending && !self.waiting_for_operation() { self.shutdown(); }
            return EventProcessStatus::Ignored;
        }
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
                    if self.page == BrowserPage::Folder && self.requested_location().1 == &BrowserPage::Folder {
                        self.navigate(target);
                    } else { self.refresh(); }
                }
                Ok(None) => {},
                Err(error) => { self.set_status(&format!("Browser state: {error}")); if !self.state_ready { self.state_worker = None; self.state_ready = true; } }
            }
            changed = true;
        }
        changed |= self.poll_operation();
        match self.worker.poll() {
            Some(Ok(result)) => {
                self.pending_listing = None;
                self.present_listing(result);
                changed = true;
            }
            Some(Err(error)) => {
                let title = match self.pending_listing.take().map(|(_,page)| page) {
                    Some(BrowserPage::Search(_)) => "Cannot search subfolders",
                    Some(BrowserPage::Trash) => "Cannot read Trash",
                    _ => "Cannot read directory",
                };
                let handle = self.listing;
                if let Some(list) = self.control_mut(handle) { list.loading = false; }
                self.set_status(&error);
                if self.can_present() { dialogs::error(title,&format!("{error}\nUse the path field, Up, or Refresh to try again.")); }
                changed = true;
            }
            None => {}
        }
        if !self.pending_entries.is_empty() {
            // Large directories are inserted incrementally to keep callbacks short.
            let filter = if self.page == BrowserPage::Folder { self.saved.filter.to_lowercase() } else { String::new() };
            let batch = self.pending_entries.drain(..self.pending_entries.len().min(256)).filter(|entry| filter.is_empty() || entry.name.to_lowercase().contains(&filter)).collect();
            let handle = self.listing;
            let loading = !self.pending_entries.is_empty();
            if let Some(list) = self.control_mut(handle) { list.add_items(batch); list.loading = loading; }
            changed = true;
        }
        if changed { EventProcessStatus::Processed } else { EventProcessStatus::Ignored }
    }
}
impl WindowEvents for BrowserWindow {
    fn on_cancel(&mut self) -> ActionRequest {
        if self.closing { return ActionRequest::Allow; }
        if self.waiting_for_operation() {
            self.close_pending = true;
            self.set_visible(false);
            self.operation.cancel();
            return ActionRequest::Deny;
        }
        self.cleanup();
        ActionRequest::Allow
    }
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
    fn hidden_browser_observes_single_transfer_completion() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = App::debug(80,24,"Paint.Enable(false)\nKey.Pressed(Enter)").build().unwrap();
        let source = dir.path().join("source");
        let destination = dir.path().join("copy");
        std::fs::write(&source,b"copy me").unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        window.set_visible(false);
        window.start_operation(files::Operation::Transfer {
            source,destination: destination.clone(),moving: false,approval: None,expected_source: None,
        });
        pump_until(&mut window,|w| w.activity.unread() == 1);
        assert!(window.notice.is_some());
        assert_eq!(std::fs::read(destination).unwrap(),b"copy me");
        let (revision,entries) = window.activity.read_since(None).unwrap();
        assert_eq!(entries[0].status,ActivityStatus::Completed);
        window.set_visible(true);
        window.poll_operation();
        assert!(window.activity.read_since(Some(revision)).is_none(),"revealing an outcome must not record it twice");

        window.set_visible(false);
        window.start_operation(files::Operation::Transfer {
            source: dir.path().join("missing"),destination: dir.path().join("target"),moving: false,approval: None,expected_source: None,
        });
        pump_until(&mut window,|w| w.activity.unread() == 1);
        let entries = window.activity.read_since(None).unwrap().1;
        assert_eq!(entries.len(),2);
        assert_eq!(entries[0].status,ActivityStatus::Failed);
        assert!(window.notice.as_ref().unwrap().dialog.is_some());
        window.shutdown(); app.run();
    }
    #[test]
    fn closing_browser_records_a_published_transfer_and_preserves_the_copy() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let source = dir.path().join("source");
        let destination = dir.path().join("copy");
        std::fs::write(&source,b"keep").unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        window.start_operation(files::Operation::Transfer {
            source,destination: destination.clone(),moving: false,approval: None,expected_source: None,
        });
        let deadline = std::time::Instant::now()+Duration::from_secs(3);
        while !destination.exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(WindowEvents::on_cancel(&mut window) == ActionRequest::Deny);
        assert!(window.is_closing() && !window.is_visible());
        pump_until(&mut window,|w| w.closing);
        let entries = window.activity.read_since(None).unwrap().1;
        assert_eq!(entries.len(),1);
        assert_eq!(entries[0].status,ActivityStatus::Completed);
        assert_eq!(std::fs::read(destination).unwrap(),b"keep");
        app.run();
    }
    #[test]
    fn canceled_replacement_is_recorded_without_refocusing_the_browser() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let source = dir.path().join("source");
        let destination = dir.path().join("copy");
        std::fs::write(&source,b"new").unwrap();
        std::fs::write(&destination,b"keep").unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        window.set_visible(false);
        window.start_operation(files::Operation::Transfer {
            source,destination: destination.clone(),moving: true,approval: None,expected_source: None,
        });
        pump_until(&mut window,|w| w.deferred_outcome.is_some());
        assert_eq!(window.activity.unread(),0);
        window.operation.cancel();
        pump_until(&mut window,|w| w.activity.unread() == 1);
        assert_eq!(window.activity.read_since(None).unwrap().1[0].status,ActivityStatus::Canceled);
        assert_eq!(std::fs::read(destination).unwrap(),b"keep");
        window.shutdown(); app.run();
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
    fn marked_entries_survive_keyboard_navigation_and_batch_through_the_context_menu() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new();
        list.viewport = Size::new(48,15);
        list.set_mode(ViewMode::Details);
        list.add_items(grid_tests::entries());
        assert!(list.input(key!("Space").into()) == EventProcessStatus::Processed);
        list.input(key!("Down").into());
        list.input(key!("Space").into());
        list.context_at(0,2);
        let menu = list.menu;
        MenuEvents::on_command(&mut list,menu,Handle::None,browserentries::Commands::Trash);
        assert!(matches!(list.request.take(),Some(EntryRequest::Batch(Mutation::Trash,entries))
            if entries.iter().map(|entry| entry.path.as_path()).eq([Path::new("/tmp/0"),Path::new("/tmp/1")])));
        list.clear_marks();
        list.selected = Some(0);
        OnMouseEvent::on_mouse_event(&mut list,&MouseEvent::Pressed(MouseEventData {
            x: 0,y: 3,button: MouseButton::Left,modifier: KeyModifier::Ctrl,
        }));
        list.input(key!("F5").into());
        assert!(matches!(list.request.take(),Some(EntryRequest::Batch(Mutation::Copy,entries))
            if entries.iter().map(|entry| entry.path.as_path()).eq([Path::new("/tmp/0"),Path::new("/tmp/2")])));
        list.input(key!("Shift+Down").into());
        list.input(key!("F6").into());
        assert!(matches!(list.request.take(),Some(EntryRequest::Batch(Mutation::Move,entries))
            if entries.iter().map(|entry| entry.path.as_path()).eq([Path::new("/tmp/2"),Path::new("/tmp/3")])));
        list.release_menu(); app.run();
    }
    #[test]
    fn find_shortcut_searches_subfolders_and_back_restores_the_folder_filter() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let match_path = nested.join("Result.txt");
        std::fs::write(&match_path,b"match").unwrap();
        std::fs::write(dir.path().join("local.txt"),b"local").unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)\nKey.TypeText('RESULT')\nKey.Pressed(Enter)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        let listing = window.listing;
        let filter = window.filter_field;
        TextFieldEvents::on_validate(&mut window,filter,"local");
        pump_until(&mut window,|w| w.control(listing).unwrap().entries.len() == 1);
        assert!(window.find_shortcut() == EventProcessStatus::Processed);
        pump_until(&mut window,|w| w.control(listing).unwrap().search && w.control(listing).unwrap().entries.len() == 1);
        assert_eq!(window.control(listing).unwrap().entries[0].path,match_path);
        window.control_mut(listing).unwrap().input(key!("Enter").into());
        window.poll_request();
        assert_eq!(window.take_action(),Some(BrowserAction::Open(match_path)));
        let up = window.up;
        ButtonEvents::on_pressed(&mut window,up);
        pump_until(&mut window,|w| w.pending_listing.is_none() && w.control(listing).unwrap().entries.len() == 1);
        assert_eq!(window.control(listing).unwrap().entries[0].path,dir.path().join("local.txt"));
        assert_eq!(window.saved.filter,"local");
        window.shutdown(); app.run();
    }
    #[test]
    fn renaming_a_search_result_keeps_it_in_its_original_parent() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let source = nested.join("result.txt");
        let replacement = nested.join("renamed.txt");
        std::fs::write(&source,b"nested").unwrap();
        std::fs::write(dir.path().join("renamed.txt"),b"root").unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)\nKey.Pressed(Ctrl+A)\nKey.TypeText('renamed.txt')\nKey.Pressed(Enter)").build().unwrap();
        let mut window = BrowserWindow::at(dir.path().to_owned()).unwrap();
        let listing = window.listing;
        window.submit_listing(dir.path().to_owned(),BrowserPage::Search("result".into()),true);
        pump_until(&mut window,|w| w.control(listing).unwrap().search && w.control(listing).unwrap().entries.len() == 1);
        window.control_mut(listing).unwrap().input(key!("F2").into());
        pump_until(&mut window,|w| !source.exists() && w.pending_listing.is_none() && w.control(listing).unwrap().entries.is_empty());
        assert_eq!(std::fs::read(&replacement).unwrap(),b"nested");
        assert_eq!(std::fs::read(dir.path().join("renamed.txt")).unwrap(),b"root");
        window.shutdown(); app.run();
    }
    #[test]
    fn batch_trash_and_restore_run_from_the_listing_and_refresh_the_view() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let a = project.join("a.txt");
        let b = project.join("b.txt");
        std::fs::write(&a,b"a").unwrap();
        std::fs::write(&b,b"b").unwrap();
        let app = App::debug(80,24,"Paint.Enable(false)\nKey.Pressed(Alt+Y,2)").build().unwrap();
        let mut window = BrowserWindow::at(project.clone()).unwrap();
        window.trash_root = Some(dir.path().join("Trash"));
        let listing = window.listing;
        pump_until(&mut window,|w| w.control(listing).unwrap().entries.len() == 2);
        let list = window.control_mut(listing).unwrap();
        list.input(key!("Ctrl+A").into());
        list.input(key!("Delete").into());
        window.poll_request();
        assert!(window.operation.busy(),"batch trash was not accepted");
        window.set_visible(false);
        pump_until(&mut window,|w| w.notice.is_some() && !a.exists() && !b.exists());
        let entries = window.activity.read_since(None).unwrap().1;
        assert_eq!(entries.len(),1,"a batch should publish one result");
        assert_eq!(entries[0].detail,"2 of 2 items completed");
        window.set_visible(true);
        pump_until(&mut window,|w| w.batch.is_none() && w.pending_listing.is_none() && w.control(listing).unwrap().entries.is_empty());
        assert!(!a.exists() && !b.exists());
        window.shutdown(); app.run();

        let app = App::debug(80,24,"Paint.Enable(false)\nKey.Pressed(Alt+Y,2)").build().unwrap();
        let mut window = BrowserWindow::at(project).unwrap();
        window.trash_root = Some(dir.path().join("Trash"));
        let listing = window.listing;
        let trash = window.trash;
        ButtonEvents::on_pressed(&mut window,trash);
        pump_until(&mut window,|w| w.control(listing).unwrap().trash && w.control(listing).unwrap().entries.len() == 2);
        let list = window.control_mut(listing).unwrap();
        list.input(key!("Ctrl+A").into());
        list.input(key!("Enter").into());
        window.poll_request();
        assert!(window.operation.busy(),"restore was not accepted");
        pump_until(&mut window,|w| w.batch.is_none() && w.pending_listing.is_none() && w.control(listing).unwrap().entries.is_empty());
        assert_eq!(std::fs::read(&a).unwrap(),b"a");
        assert_eq!(std::fs::read(&b).unwrap(),b"b");
        assert!(window.control(listing).unwrap().trash);
        window.shutdown(); app.run();
    }
    #[test]
    fn native_mutation_requests_preserve_target_path_in_both_views() {
        let _lock = UI_LOCK.lock().unwrap(); let app = App::debug(80,24,"Paint.Enable(false)").build().unwrap();
        let mut list = BrowserEntries::new(); list.viewport = Size::new(48,15); list.add_items((0..2).map(|index| Entry { path: PathBuf::from(format!("/tmp/{index}")),name: index.to_string(),kind: FileKind::File,size: "1".into(),modified: "—".into(),identity: None }).collect());
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
            size: "42".into(), modified: "2026-10-31 00:30".into(), identity: None };
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
    pub(super) fn entries() -> Vec<Entry> {
        (0..23).map(|i| Entry { path: PathBuf::from(format!("/tmp/{i}")), name: format!("{i} 文 long filename"), kind: FileKind::File, size: "42".into(), modified: "today".into(), identity: None }).collect()
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
