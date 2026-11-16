use super::image_viewer::home_directory;
use crate::runtime::kanban::{self,BoardWorker,Completion,Document,OpenFiles,Operation,Priority,Task};
use appcui::dialogs::{Location,OpenFileDialogFlags,SaveFileDialogFlags};
use appcui::prelude::*;
use std::{path::{Path,PathBuf},time::Duration};

const COLUMN_WIDTH: usize = 26;
const TOP: usize = 3;
const CARD_HEIGHT: usize = 3;
#[derive(Clone,Copy,Debug,PartialEq)]
enum Action { FileMenu,TaskMenu,Columns,New,Open,Save,SaveAs,Rename,Add,Edit,Delete,Left,Right,Up,Down }
#[derive(Clone,Copy)]
struct BoardLayout { columns: usize, rows: usize, width: usize, height: usize }
impl BoardLayout {
    fn new(size: Size) -> Option<Self> {
        let (width,height) = (size.width as usize,size.height as usize);
        (width >= COLUMN_WIDTH && height >= 8).then(|| Self { columns: width / COLUMN_WIDTH,rows: (height-TOP-1)/CARD_HEIGHT,width,height })
    }
    fn hit(self,x: i32,y: i32) -> Option<(usize,Option<usize>)> {
        if x < 0 || y < 2 || x as usize >= self.columns*COLUMN_WIDTH || y as usize >= self.height-1 { return None; }
        let column = x as usize / COLUMN_WIDTH;
        if x as usize % COLUMN_WIDTH == COLUMN_WIDTH-1 { return None; }
        if y as usize >= TOP+self.rows*CARD_HEIGHT { return None; }
        let row = if y < TOP as i32 { None } else { Some((y as usize-TOP)/CARD_HEIGHT) };
        Some((column,row))
    }
}
struct Drag { task: u64, start: (i32,i32), moved: bool }
#[CustomControl(overwrite = OnPaint+OnKeyPressed+OnMouseEvent+OnFocus+OnResize)]
struct BoardControl {
    document: Document, column: usize, selected: Option<u64>, first_column: usize, offsets: Vec<usize>,
    viewport: Size, frozen: bool, action: Option<Action>, drag: Option<Drag>, status: String,
}
impl BoardControl {
    fn new() -> Self {
        Self { base: ControlBase::new(Layout::fill(),true),document: Document::default(),column: 0,selected: None,
            first_column: 0,offsets: vec![0;4],viewport: Size::new(0,0),frozen: false,action: None,drag: None,status: String::new() }
    }
    fn suspend(&mut self) { self.drag = None; }
    fn bound(&mut self) {
        let columns = &self.document.board.columns;
        self.column = self.column.min(columns.len()-1); self.offsets.resize(columns.len(),0);
        if self.selected.is_some_and(|id| self.document.locate(id).is_none()) { self.selected = None; }
        let rows = BoardLayout::new(self.viewport).map_or(1,|layout| layout.rows);
        for (offset,column) in self.offsets.iter_mut().zip(columns) { *offset = (*offset).min(column.tasks.len().saturating_sub(rows)); }
        let visible = BoardLayout::new(self.viewport).map_or(1,|layout| layout.columns);
        self.first_column = self.first_column.min(columns.len().saturating_sub(visible));
    }
    fn reveal(&mut self) {
        self.bound();
        if let Some(id) = self.selected && let Some((column,index)) = self.document.locate(id) {
            self.column = column;
            let rows = BoardLayout::new(self.viewport).map_or(1,|layout| layout.rows);
            self.offsets[column] = self.offsets[column].min(index).max(index.saturating_sub(rows-1));
        }
        let visible = BoardLayout::new(self.viewport).map_or(1,|layout| layout.columns);
        self.first_column = self.first_column.min(self.column).max(self.column.saturating_sub(visible-1));
    }
    fn queue(&mut self,action: Action) { if !self.frozen { self.suspend(); self.action = Some(action); } }
    fn selected_task(&self) -> Option<&Task> { let (c,t) = self.document.locate(self.selected?)?; self.document.board.columns[c].tasks.get(t) }
    fn select_column(&mut self,column: usize) {
        let index = self.selected.and_then(|id| self.document.locate(id)).map_or(0,|(_,index)| index);
        self.column = column.min(self.document.board.columns.len()-1);
        let tasks = &self.document.board.columns[self.column].tasks;
        self.selected = tasks.get(index.min(tasks.len().saturating_sub(1))).map(|task| task.id); self.reveal();
    }
    fn move_selected(&mut self,action: Action) {
        let Some(id) = self.selected else { return; }; let Some((column,index)) = self.document.locate(id) else { return; };
        let (target,position) = match action {
            Action::Left if column > 0 => (column-1,self.document.board.columns[column-1].tasks.len()),
            Action::Right if column+1 < self.document.board.columns.len() => (column+1,self.document.board.columns[column+1].tasks.len()),
            Action::Up if index > 0 => (column,index-1),
            Action::Down if index+1 < self.document.board.columns[column].tasks.len() => (column,index+1), _ => return,
        };
        if let Err(error) = self.document.move_task(id,target,position) { self.status = error; }
        self.reveal();
    }
    fn input(&mut self,key: Key) -> EventProcessStatus {
        if key.code == KeyCode::Escape { self.suspend(); return EventProcessStatus::Processed; }
        if self.frozen { return EventProcessStatus::Processed; }
        let action = if key == key!("Ctrl+N").into() { Some(Action::New) } else if key == key!("Ctrl+O").into() { Some(Action::Open) }
            else if key == key!("Ctrl+S").into() { Some(Action::Save) } else if key == key!("Ctrl+Shift+S").into() { Some(Action::SaveAs) }
            else if key == key!("Alt+F").into() { Some(Action::FileMenu) } else if key == key!("Alt+T").into() { Some(Action::TaskMenu) }
            else if key == key!("Alt+C").into() { Some(Action::Columns) } else { None };
        if let Some(action) = action { self.queue(action); return EventProcessStatus::Processed; }
        self.suspend();
        if BoardLayout::new(self.viewport).is_none() { return EventProcessStatus::Processed; }
        if key.modifier == KeyModifier::Shift {
            match key.code { KeyCode::Left => self.move_selected(Action::Left),KeyCode::Right => self.move_selected(Action::Right),
                KeyCode::Up => self.move_selected(Action::Up),KeyCode::Down => self.move_selected(Action::Down),_ => return EventProcessStatus::Ignored }
        } else if key.modifier == KeyModifier::None {
            match key.code {
                KeyCode::Left => self.select_column(self.column.saturating_sub(1)), KeyCode::Right => self.select_column(self.column+1),
                KeyCode::Up | KeyCode::Down => {
                    let tasks = &self.document.board.columns[self.column].tasks;
                    let old = self.selected.and_then(|id| tasks.iter().position(|task| task.id == id));
                    let index = match (old,key.code) { (Some(index),KeyCode::Up) => index.saturating_sub(1),(Some(index),_) => (index+1).min(tasks.len().saturating_sub(1)),_ => 0 };
                    self.selected = tasks.get(index).map(|task| task.id); self.reveal();
                }
                KeyCode::Enter => self.queue(Action::Edit), KeyCode::Insert => self.queue(Action::Add), KeyCode::Delete => self.queue(Action::Delete),
                _ => return EventProcessStatus::Ignored,
            }
        } else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
    fn point(&self,x: i32,y: i32) -> Option<(usize,Option<usize>)> {
        let layout = BoardLayout::new(self.viewport)?; let (column,row) = layout.hit(x,y)?; let column = column+self.first_column;
        (column < self.document.board.columns.len()).then(|| (column,row.map(|row| row+self.offsets[column])))
    }
    fn scroll(&mut self,direction: MouseWheelDirection,column: usize) {
        self.suspend(); self.bound();
        match direction {
            MouseWheelDirection::Left => self.first_column = self.first_column.saturating_sub(1),
            MouseWheelDirection::Right => self.first_column += 1,
            MouseWheelDirection::Up => self.offsets[column] = self.offsets[column].saturating_sub(2),
            MouseWheelDirection::Down => self.offsets[column] += 2,
        }
        self.bound();
    }
}
/// Use the terminal core's grapheme segmentation and widths with AppCUI's complete-grapheme writer.
fn write_label(surface: &mut Surface,x: i32,y: i32,width: usize,text: &str,attr: CharAttribute) {
    let line = tattoy_wezterm_term::Line::from_text(text,&tattoy_wezterm_term::CellAttributes::default(),0,None);
    let mut offset = 0;
    for cell in line.visible_cells() {
        let size = cell.width(); if offset+size > width { break; }
        surface.write_grapheme(x+offset as i32,y,cell.str(),size,attr); offset += size;
    }
}
impl OnPaint for BoardControl {
    fn on_paint(&self,surface: &mut Surface,theme: &Theme) {
        surface.clear(Character::with_attributes(' ',theme.window.normal));
        let Some(layout) = BoardLayout::new(self.viewport) else { write_label(surface,0,0,self.viewport.width as usize,"Resize window: needs 28 x 10",theme.text.warning); return; };
        surface.write_string(0,0,"[File] [Task] [Columns]",theme.text.enphasized_1,false);
        write_label(surface,23,0,layout.width.saturating_sub(23),&self.document.board.name,theme.text.normal);
        for (slot,column) in self.document.board.columns.iter().enumerate().skip(self.first_column).take(layout.columns) {
            let x = ((slot-self.first_column)*COLUMN_WIDTH) as i32;
            let attr = if slot == self.column { theme.text.enphasized_1 } else { theme.text.normal };
            write_label(surface,x,2,COLUMN_WIDTH-1,&format!("{} ({})",column.name,column.tasks.len()),attr);
            for (row,task) in column.tasks.iter().skip(self.offsets[slot]).take(layout.rows).enumerate() {
                let y = (TOP+row*CARD_HEIGHT) as i32;
                let attr = if self.selected == Some(task.id) { theme.text.enphasized_2 } else { theme.text.normal };
                write_label(surface,x,y,COLUMN_WIDTH-1,&format!("{}{}",if self.selected == Some(task.id) { "> " } else { "  " },task.title),attr);
                write_label(surface,x+2,y+1,COLUMN_WIDTH-3,&format!("{} {}",task.priority.label(),task.due),attr);
            }
            if column.tasks.is_empty() { surface.write_string(x+2,4,"(empty)",theme.text.normal,false); }
        }
        let text = if self.frozen { "Waiting for file operation / document decision".to_owned() }
            else if self.drag.is_some() { "Drag: release to move; Escape cancels".into() }
            else if !self.status.is_empty() { self.status.clone() }
            else { format!("Cols {}-{} / {} | Insert add; Enter edit; Shift+arrows move",self.first_column+1,(self.first_column+layout.columns).min(self.document.board.columns.len()),self.document.board.columns.len()) };
        write_label(surface,0,layout.height as i32-1,layout.width,&text,theme.text.normal);
    }
}
impl OnKeyPressed for BoardControl {
    fn on_key_pressed(&mut self,key: Key,_: char) -> EventProcessStatus { if self.has_focus() { self.input(key) } else { EventProcessStatus::Ignored } }
}
impl OnMouseEvent for BoardControl {
    fn on_mouse_event(&mut self,event: &MouseEvent) -> EventProcessStatus {
        if self.frozen { self.suspend(); return EventProcessStatus::Processed; }
        match event {
            MouseEvent::Pressed(data) if data.button == MouseButton::Left => {
                self.suspend();
                if data.y == 0 { self.queue(match data.x { 0..=5 => Action::FileMenu,7..=12 => Action::TaskMenu,14..=22 => Action::Columns,_ => return EventProcessStatus::Ignored }); }
                else if let Some((column,row)) = self.point(data.x,data.y) {
                    self.column = column; self.selected = row.and_then(|row| self.document.board.columns[column].tasks.get(row)).map(|task| task.id);
                    if let Some(task) = self.selected { self.drag = Some(Drag { task,start: (data.x,data.y),moved: false }); }
                }
            }
            MouseEvent::DoubleClick(data) if data.button == MouseButton::Left => {
                if let Some((column,Some(row))) = self.point(data.x,data.y) {
                    self.column = column; self.selected = self.document.board.columns[column].tasks.get(row).map(|task| task.id); self.queue(Action::Edit);
                }
            }
            MouseEvent::Drag(data) => if let Some(drag) = &mut self.drag { drag.moved |= drag.start != (data.x,data.y); },
            MouseEvent::Released(data) => {
                if let Some(drag) = self.drag.take() && drag.moved && data.button == MouseButton::Left
                    && let Some((column,Some(row))) = self.point(data.x,data.y) {
                    let count = self.document.board.columns[column].tasks.len();
                    let after = row < count && (data.y as usize-TOP)%CARD_HEIGHT >= CARD_HEIGHT/2;
                    let mut position = (row+usize::from(after)).min(count);
                    if let Some((source,index)) = self.document.locate(drag.task) && source == column && position > index { position -= 1; }
                    if let Err(error) = self.document.move_task(drag.task,column,position) { self.status = error; }
                    self.selected = Some(drag.task); self.reveal();
                }
            }
            MouseEvent::Leave => self.suspend(),
            MouseEvent::WheelAt(data) => {
                let column = self.point(data.x,data.y).map_or(self.column,|point| point.0);
                let direction = if data.modifier.contains(KeyModifier::Shift) { match data.direction { MouseWheelDirection::Up => MouseWheelDirection::Left,MouseWheelDirection::Down => MouseWheelDirection::Right,other => other } } else { data.direction };
                self.scroll(direction,column);
            }
            MouseEvent::Wheel(direction) => self.scroll(*direction,self.column),
            _ => return EventProcessStatus::Ignored,
        }
        EventProcessStatus::Processed
    }
}
impl OnFocus for BoardControl { fn on_lose_focus(&mut self) { self.suspend(); } }
impl OnResize for BoardControl { fn on_resize(&mut self,_: Size,size: Size) { self.suspend(); self.viewport = size; self.reveal(); } }

#[derive(Clone)]
struct Choice { index: usize, label: String }
impl listview::ListItem for Choice {
    fn columns_count() -> u16 { 1 }
    fn column(_: u16) -> appcui::prelude::Column { appcui::prelude::Column::new("Action / destination",58,TextAlignment::Left) }
    fn render_method(&self,_: u16) -> Option<listview::RenderMethod<'_>> { Some(listview::RenderMethod::Text(&self.label)) }
}
#[ModalWindow(response = usize,events = ListViewEvents<Choice>)]
struct ChoiceDialog { list: Handle<ListView<Choice>> }
impl ChoiceDialog {
    fn new(title: &str,choices: Vec<Choice>) -> Self {
        let mut dialog = Self { base: ModalWindow::new(title,layout!("a:c,w:64,h:18"),window::Flags::Sizeable),list: Handle::None };
        let mut list = ListView::new(layout!("l:1,r:1,t:0,b:2"),listview::Flags::ScrollBars);
        list.add_items(choices); dialog.list = dialog.add(list);
        dialog.add(Label::new("Enter selects; Escape cancels",layout!("l:1,r:1,b:0,h:1")));
        dialog.base.request_focus_for_control(dialog.list); dialog
    }
    fn actions(title: &str,actions: &[(Action,&str)]) -> Option<Action> {
        let choices = actions.iter().enumerate().map(|(index,(_,label))| Choice { index,label: (*label).into() }).collect();
        Self::new(title,choices).show().and_then(|index| actions.get(index).map(|pair| pair.0))
    }
}
impl ListViewEvents<Choice> for ChoiceDialog {
    fn on_item_action(&mut self,handle: Handle<ListView<Choice>>,index: usize) -> EventProcessStatus {
        if let Some(index) = self.control(handle).and_then(|list| list.item(index)).map(|choice| choice.index) { self.exit_with(index); }
        EventProcessStatus::Processed
    }
}
#[ModalWindow(response = String,events = ButtonEvents+WindowEvents)]
struct NameDialog { name: Handle<TextField>,apply: Handle<Button>,status: Handle<Label> }
impl NameDialog {
    fn new(title: &str,name: &str) -> Self {
        let mut dialog = Self { base: ModalWindow::new(title,layout!("a:c,w:60,h:9"),window::Flags::None),name: Handle::None,apply: Handle::None,status: Handle::None };
        dialog.name = dialog.add(TextField::new(name,layout!("l:1,r:1,t:1,h:1"),textfield::Flags::None));
        dialog.status = dialog.add(Label::new("Name required (up to 256 UTF-8 bytes)",layout!("l:1,r:1,t:3,h:1")));
        dialog.apply = dialog.add(Button::new("&Apply",layout!("a:b,w:14"))); dialog.base.request_focus_for_control(dialog.name); dialog
    }
    fn submit(&mut self) {
        let value = self.control(self.name).map(|field| field.text().trim().to_owned()).unwrap_or_default();
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            let handle = self.status; if let Some(status) = self.control_mut(handle) { status.set_caption("Enter a name of 1–256 bytes without control characters"); }
        } else { self.exit_with(value); }
    }
}
impl ButtonEvents for NameDialog { fn on_pressed(&mut self,_: Handle<Button>) -> EventProcessStatus { self.submit(); EventProcessStatus::Processed } }
impl WindowEvents for NameDialog { fn on_accept(&mut self) { self.submit(); } }
#[ModalWindow(response = Task,events = ButtonEvents)]
struct TaskDialog { task: Task,notes_initial: String,title: Handle<TextField>,notes: Handle<TextArea>,priority: Handle<ComboBox>,due: Handle<TextField>,status: Handle<Label>,apply: Handle<Button>,cancel: Handle<Button> }
impl TaskDialog {
    fn new(task: Task) -> Self {
        let mut dialog = Self { base: ModalWindow::new("Kanban task",layout!("a:c,w:70,h:22"),window::Flags::Sizeable),
            task: task.clone(),notes_initial: String::new(),title: Handle::None,notes: Handle::None,priority: Handle::None,due: Handle::None,status: Handle::None,apply: Handle::None,cancel: Handle::None };
        dialog.add(Label::new("Title (required)",layout!("l:1,t:0,w:30,h:1")));
        dialog.title = dialog.add(TextField::new(&task.title,layout!("l:1,r:1,t:1,h:1"),textfield::Flags::None));
        dialog.add(Label::new("Priority",layout!("l:1,t:3,w:12,h:1")));
        let mut priority = ComboBox::new(layout!("l:13,t:3,w:12,h:1"),combobox::Flags::None);
        for label in ["Low","Medium","High"] { priority.add(label); }
        priority.set_index(match task.priority { Priority::Low => 0,Priority::Medium => 1,Priority::High => 2 }); dialog.priority = dialog.add(priority);
        dialog.add(Label::new("Due YYYY-MM-DD",layout!("l:28,t:3,w:18,h:1")));
        dialog.due = dialog.add(TextField::new(&task.due,layout!("l:47,r:1,t:3,h:1"),textfield::Flags::None));
        dialog.add(Label::new("Notes (multiline; Tab moves to the next field)",layout!("l:1,r:1,t:5,h:1")));
        dialog.notes = dialog.add(TextArea::new(&task.notes,layout!("l:1,r:1,t:6,b:5"),textarea::Flags::ScrollBars));
        dialog.notes_initial = dialog.control(dialog.notes).map(|area| area.text().to_owned()).unwrap_or_default();
        dialog.status = dialog.add(Label::new("",layout!("l:1,r:1,b:2,h:2")));
        dialog.apply = dialog.add(Button::new("&Apply",layout!("l:14,b:0,w:14")));
        dialog.cancel = dialog.add(Button::new("&Cancel",layout!("r:14,b:0,w:14")));
        dialog.base.request_focus_for_control(dialog.title); dialog
    }
    fn submit(&mut self) {
        let mut task = self.task.clone();
        task.title = self.control(self.title).map(|field| field.text().trim().to_owned()).unwrap_or_default();
        let notes = self.control(self.notes).map(|area| area.text()).unwrap_or("");
        // TextArea normalizes tabs/CRLF and keeps a final newline. Leave unchanged
        // notes byte-for-byte intact; omit the editor's sentinel on changed text.
        if notes != self.notes_initial { task.notes = notes.strip_suffix('\n').unwrap_or(notes).to_owned(); }
        task.due = self.control(self.due).map(|field| field.text().trim().to_owned()).unwrap_or_default();
        task.priority = match self.control(self.priority).and_then(ComboBox::index) { Some(0) => Priority::Low,Some(2) => Priority::High,_ => Priority::Medium };
        match Document::default().add_task(0,task.clone()) {
            Ok(_) => self.exit_with(task),Err(error) => { let handle = self.status; if let Some(status) = self.control_mut(handle) { status.set_caption(&error); } }
        }
    }
}
impl ButtonEvents for TaskDialog {
    fn on_pressed(&mut self,handle: Handle<Button>) -> EventProcessStatus { if handle == self.cancel { self.exit(); } else if handle == self.apply { self.submit(); } EventProcessStatus::Processed }
}
#[ModalWindow(response = Document,events = ButtonEvents)]
struct ColumnsDialog { document: Document,columns: Handle<ComboBox>,buttons: Vec<Handle<Button>>,status: Handle<Label> }
impl ColumnsDialog {
    fn new(document: Document) -> Self {
        let mut dialog = Self { base: ModalWindow::new("Manage Kanban columns",layout!("a:c,w:70,h:16"),window::Flags::None),document,columns: Handle::None,buttons: Vec::new(),status: Handle::None };
        dialog.add(Label::new("Choose a column. Deleting a populated column moves its tasks.",layout!("l:1,r:1,t:0,h:2")));
        dialog.columns = dialog.add(ComboBox::new(layout!("l:2,r:2,t:3,h:1"),combobox::Flags::None));
        for (label,left,top,width) in [("&Add",2,6,12),("&Rename",16,6,16),("&Left",34,6,12),("&Right",48,6,12),("&Delete",2,9,14),("A&pply",24,12,14),("&Cancel",44,12,14)] {
            let button = dialog.add(Button::new(label,Layout::absolute(left,top,width,1))); dialog.buttons.push(button);
        }
        dialog.status = dialog.add(Label::new("Changes apply together; Escape cancels",layout!("l:2,r:2,t:11,h:1")));
        dialog.refresh(0); dialog.base.request_focus_for_control(dialog.columns); dialog
    }
    fn refresh(&mut self,index: usize) {
        let names = self.document.board.columns.iter().map(|column| format!("{} ({} tasks)",column.name,column.tasks.len())).collect::<Vec<_>>();
        let handle = self.columns; if let Some(columns) = self.control_mut(handle) { columns.clear(); for name in names { columns.add(&name); } columns.set_index(index.min(columns.count().saturating_sub(1) as usize) as u32); }
    }
    fn action(&mut self,action: usize) {
        let index = self.control(self.columns).and_then(ComboBox::index).unwrap_or(0) as usize;
        let mut next = index;
        let result = match action {
            0 => if let Some(name) = NameDialog::new("Add column","").show() { self.document.add_column(name).map(|_| { next = self.document.board.columns.len()-1; }) } else { Ok(()) },
            1 => if let Some(name) = NameDialog::new("Rename column",&self.document.board.columns[index].name).show() { self.document.edit(|board| { board.columns[index].name = name; Ok(()) }) } else { Ok(()) },
            2 | 3 => { next = if action == 2 { index.saturating_sub(1) } else { (index+1).min(self.document.board.columns.len()-1) };
                self.document.edit(|board| { board.columns.swap(index,next); Ok(()) }) },
            4 => {
                let destination = if self.document.board.columns[index].tasks.is_empty() { None } else {
                    let choices = self.document.board.columns.iter().enumerate().filter(|(i,_)| *i != index).map(|(index,column)| Choice { index,label: column.name.clone() }).collect();
                    let Some(destination) = ChoiceDialog::new("Move tasks to which column?",choices).show() else { return; }; Some(destination)
                };
                self.document.delete_column(index,destination)
            }
            5 => { self.exit_with(self.document.clone()); return; },_ => { self.exit(); return; }
        };
        if let Err(error) = result { let handle = self.status; if let Some(status) = self.control_mut(handle) { status.set_caption(&error); } }
        self.refresh(next);
    }
}
impl ButtonEvents for ColumnsDialog {
    fn on_pressed(&mut self,handle: Handle<Button>) -> EventProcessStatus { if let Some(index) = self.buttons.iter().position(|button| *button == handle) { self.action(index); } EventProcessStatus::Processed }
}

#[derive(Debug)]
pub(crate) enum KanbanAction { New,Open(PathBuf) }
#[derive(Clone,Copy,PartialEq)]
enum Intent { Close,Guard }
struct Pending { intent: Intent,approved: bool }
#[Window(events = TimerEvents+WindowEvents)]
pub(crate) struct KanbanWindow {
    board: Handle<BoardControl>,worker: BoardWorker,path: Option<PathBuf>,requested_path: Option<PathBuf>,
    outgoing: Option<KanbanAction>,pending: Option<Pending>,guard_result: Option<bool>,guard_hold: bool,closing: bool,updating: bool,
}
impl KanbanWindow {
    pub fn new(path: Option<PathBuf>,files: OpenFiles) -> anyhow::Result<Self> {
        let mut window = Self { base: Window::new("Kanban — Untitled",layout!("a:c,w:108,h:30"),window::Flags::Sizeable),
            board: Handle::None,worker: BoardWorker::new(files),path: None,requested_path: path.clone(),outgoing: None,pending: None,guard_result: None,guard_hold: false,closing: false,updating: false };
        window.timer().ok_or_else(|| anyhow::anyhow!("Kanban timer unavailable"))?.start(Duration::from_millis(40));
        window.board = window.add(BoardControl::new()); window.base.request_focus_for_control(window.board);
        if let Some(path) = path { anyhow::ensure!(window.worker.submit(Operation::Open(path)),"Cannot start Kanban file worker"); }
        window.freeze(); Ok(window)
    }
    pub fn document_path(&self) -> Option<PathBuf> { self.path.clone() }
    pub fn matches_path(&self,path: &Path) -> bool { self.worker.owns(path) || self.requested_path.as_deref() == Some(path) }
    pub fn is_closing(&self) -> bool { self.closing }
    pub fn take_action(&mut self) -> Option<KanbanAction> { if self.guard_hold { None } else { self.outgoing.take() } }
    pub fn suspend(&mut self) { let handle = self.board; if let Some(board) = self.control_mut(handle) { board.suspend(); } }
    pub fn constrain_to(&mut self,available: Size) { self.suspend(); self.set_size_bounds(1,1,available.width.clamp(1,u16::MAX as u32) as u16,available.height.clamp(1,u16::MAX as u32) as u16); }
    pub fn shutdown(&mut self) { self.suspend(); self.closing = true; self.close(); }
    pub fn set_guard_hold(&mut self,held: bool) {
        self.guard_hold = held;
        // A global decision supersedes queued local continuations. In-flight I/O
        // still completes, but a later document must not close during another prompt.
        if held {
            if self.pending.as_ref().is_some_and(|pending| pending.intent != Intent::Guard) { self.pending = None; }
            self.outgoing = None;
        }
        self.suspend(); self.freeze();
    }
    pub fn request_close(&mut self) {
        if self.closing || self.guard_hold || self.pending.is_some() || self.guard_result.is_some() { return; }
        self.pending = Some(Pending { intent: Intent::Close,approved: false }); self.suspend(); self.freeze();
    }
    pub fn request_guard(&mut self) {
        if self.closing || self.guard_result.is_some() { return; }
        self.pending = Some(Pending { intent: Intent::Guard,approved: false }); self.suspend(); self.freeze();
    }
    pub fn take_guard_result(&mut self) -> Option<bool> { let result = self.guard_result.take(); self.freeze(); result }
    fn freeze(&mut self) {
        let frozen = self.worker.is_busy() || self.pending.is_some() || self.guard_result.is_some() || self.guard_hold;
        let handle = self.board; if let Some(board) = self.control_mut(handle) { board.frozen = frozen; if frozen { board.suspend(); board.action = None; } }
    }
    fn cancel_pending(&mut self) {
        if self.pending.take().is_some_and(|pending| pending.intent == Intent::Guard) { self.guard_result = Some(false); } self.freeze();
    }
    fn save(&mut self,save_as: bool) -> bool {
        if self.worker.is_busy() { return false; }
        let path = if save_as { None } else { self.path.clone() };
        let path = path.or_else(|| dialogs::save("Save Kanban board (.kanban)","Untitled.kanban",Location::Path(&home_directory()),None,
            SaveFileDialogFlags::Icons | SaveFileDialogFlags::ValidateOverwrite));
        let Some(path) = path else { return false; };
        if !kanban::is_project(&path) { dialogs::error("Save Kanban","Use the .kanban extension"); return false; }
        let Some(document) = self.control(self.board).map(|board| board.document.clone()) else { return false; };
        if !self.worker.submit(Operation::Save(document,path)) { return false; } self.freeze(); true
    }
    fn process_pending(&mut self) {
        if self.worker.is_busy() || self.pending.is_none() { return; }
        let dirty = self.control(self.board).is_some_and(|board| board.document.is_dirty());
        if dirty && self.pending.as_ref().is_some_and(|pending| !pending.approved) {
            use super::ascii_paint::{UnsavedChoice,UnsavedDialog};
            match UnsavedDialog::for_document("Unsaved Kanban board","Save this board before continuing?").show() {
                Some(UnsavedChoice::Save) => {
                    if let Some(pending) = &mut self.pending { pending.approved = true; }
                    if !self.save(false) { self.cancel_pending(); }
                    return;
                },
                Some(UnsavedChoice::Discard) => {},_ => { self.cancel_pending(); return; }
            }
        }
        if let Some(pending) = self.pending.take() { match pending.intent { Intent::Close => self.shutdown(),Intent::Guard => self.guard_result = Some(true) } }
        self.freeze();
    }
    fn action(&mut self,action: Action) {
        if self.worker.is_busy() || self.pending.is_some() || self.guard_hold || self.guard_result.is_some() { return; }
        self.suspend();
        let action = match action {
            Action::FileMenu => ChoiceDialog::actions("Kanban file",&[(Action::New,"New board (Ctrl+N)"),(Action::Open,"Open board (Ctrl+O)"),(Action::Save,"Save (Ctrl+S)"),(Action::SaveAs,"Save As (Ctrl+Shift+S)"),(Action::Rename,"Rename board")]),
            Action::TaskMenu => ChoiceDialog::actions("Kanban task",&[(Action::Add,"Add (Insert)"),(Action::Edit,"Edit (Enter)"),(Action::Delete,"Delete"),(Action::Left,"Move left (Shift+Left)"),(Action::Right,"Move right (Shift+Right)"),(Action::Up,"Move up (Shift+Up)"),(Action::Down,"Move down (Shift+Down)")]),
            other => Some(other),
        };
        let Some(action) = action else { return; }; let handle = self.board;
        match action {
            Action::New => self.outgoing = Some(KanbanAction::New),
            Action::Open => if let Some(path) = dialogs::open("Open Kanban board","",Location::Path(&home_directory()),None,
                OpenFileDialogFlags::Icons | OpenFileDialogFlags::CheckIfFileExists | OpenFileDialogFlags::PreserveBackground) { self.outgoing = Some(KanbanAction::Open(path)); },
            Action::Save => { self.save(false); },Action::SaveAs => { self.save(true); },
            Action::Rename => {
                if let Some(name) = self.control(handle).map(|board| board.document.board.name.clone()).and_then(|name| NameDialog::new("Rename board",&name).show())
                    && let Some(board) = self.control_mut(handle)
                    && let Err(error) = board.document.edit(|board| { board.name = name; Ok(()) }) { board.status = error; }
            }
            Action::Columns => {
                if let Some(document) = self.control(handle).map(|board| board.document.clone()).and_then(|doc| ColumnsDialog::new(doc).show())
                    && let Some(board) = self.control_mut(handle) { board.document = document; board.reveal(); }
            }
            Action::Add | Action::Edit => {
                let task = if action == Action::Add { Some(Task::draft("")) } else { self.control(handle).and_then(BoardControl::selected_task).cloned() };
                if let Some(task) = task.and_then(|task| TaskDialog::new(task).show()) && let Some(board) = self.control_mut(handle) {
                    let result = if action == Action::Add { board.document.add_task(board.column,task).map(|id| { board.selected = Some(id); }) }
                    else if let Some((column,index)) = board.document.locate(task.id) { board.document.edit(|board| { board.columns[column].tasks[index] = task; Ok(()) }) } else { Ok(()) };
                    if let Err(error) = result { board.status = error; } board.reveal();
                }
            }
            Action::Delete => {
                if let Some(task) = self.control(handle).and_then(BoardControl::selected_task).cloned()
                    && ChoiceDialog::new("Delete selected task?",vec![Choice { index: 0,label: "Cancel".into() },Choice { index: 1,label: format!("Delete {}",task.title) }]).show() == Some(1)
                    && let Some(board) = self.control_mut(handle) && let Some((column,index)) = board.document.locate(task.id) {
                    if let Err(error) = board.document.edit(|board| { board.columns[column].tasks.remove(index); Ok(()) }) { board.status = error; }
                    board.selected = None; board.reveal();
                }
            }
            Action::Left|Action::Right|Action::Up|Action::Down => if let Some(board) = self.control_mut(handle) { board.move_selected(action); },
            _ => {}
        }
        self.freeze();
    }
    fn tick(&mut self) {
        if self.updating { return; } self.updating = true;
        if let Some(result) = self.worker.poll() {
            match result {
                Ok(Completion::Opened(document,path)) => {
                    let handle = self.board; if let Some(board) = self.control_mut(handle) { board.document = document; board.selected = None; board.column = 0; board.reveal(); }
                    self.path = Some(path); self.requested_path = None;
                }
                Ok(Completion::Saved(path)) => { self.path = Some(path); let handle = self.board; if let Some(board) = self.control_mut(handle) { board.document.mark_saved(); board.status = "Board saved".into(); } }
                Err(error) => { self.requested_path = None; self.cancel_pending(); dialogs::error("Kanban file operation failed",&error); }
            }
        }
        self.process_pending(); self.freeze();
        let handle = self.board; let action = self.control_mut(handle).and_then(|board| board.action.take()); if let Some(action) = action { self.action(action); }
        if let Some((name,dirty)) = self.control(handle).map(|board| (board.document.board.name.clone(),board.document.is_dirty())) {
            self.set_title(&format!("Kanban — {name}{}",if dirty { " *" } else { "" }));
        }
        self.updating = false;
    }
}
impl TimerEvents for KanbanWindow {
    fn on_update(&mut self,_: u64) -> EventProcessStatus {
        if self.updating || self.closing { return EventProcessStatus::Ignored; }
        if !self.is_visible() { self.suspend(); return EventProcessStatus::Ignored; }
        if !self.has_focus() { self.suspend(); } self.tick(); EventProcessStatus::Processed
    }
}
impl WindowEvents for KanbanWindow {
    fn on_cancel(&mut self) -> ActionRequest { if self.closing { ActionRequest::Allow } else { self.request_close(); ActionRequest::Deny } }
    fn on_layout_changed(&mut self,_: Rect,_: Rect) { self.suspend(); }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_support::UI_LOCK;
    pub(crate) fn dirty(window: &mut KanbanWindow) { let h = window.board; window.control_mut(h).unwrap().document.add_task(0,Task::draft("Unsaved")).unwrap(); }
    pub(crate) fn is_dirty(window: &KanbanWindow) -> bool { window.control(window.board).unwrap().document.is_dirty() }
    pub(crate) fn frozen(window: &KanbanWindow) -> bool { window.control(window.board).unwrap().frozen }
    pub(crate) fn save_fixture(path: &Path) {
        let mut worker = BoardWorker::new(OpenFiles::default()); assert!(worker.submit(Operation::Save(Document::default(),path.to_owned())));
        let deadline = std::time::Instant::now()+Duration::from_secs(5);
        loop { if let Some(result) = worker.poll() { assert!(result.is_ok()); break; } assert!(std::time::Instant::now()<deadline); std::thread::yield_now(); }
    }
    fn control() -> BoardControl { let mut board = BoardControl::new(); board.viewport = Size::new(78,20); board }
    fn mouse(x: i32,y: i32) -> MouseEventData { MouseEventData { x,y,button: MouseButton::Left,modifier: KeyModifier::None } }
    #[test]
    fn kanban_keyboard_moves_reorders_and_reveals_selection() {
        let _lock = UI_LOCK.lock().unwrap(); let _app = App::debug(80,24,"").build().unwrap(); let mut board = control();
        let a = board.document.add_task(0,Task::draft("A")).unwrap(); let b = board.document.add_task(0,Task::draft("B")).unwrap();
        board.selected = Some(a); board.input(key!("Shift+Down").into());
        assert_eq!(board.document.board.columns[0].tasks.iter().map(|t| t.id).collect::<Vec<_>>(),[b,a]);
        board.input(key!("Shift+Right").into()); assert_eq!(board.document.locate(a),Some((1,0)));
        board.input(key!("Right").into()); assert_eq!(board.column,2); assert!(board.selected.is_none());
        board.input(key!("Insert").into()); assert_eq!(board.action,Some(Action::Add));
        for _ in 0..25 { board.document.add_task(3,Task::draft("Long list")).unwrap(); }
        board.column = 3; board.selected = Some(board.document.board.columns[3].tasks[24].id); board.reveal();
        assert!(board.first_column > 0); assert!(board.offsets[3] > 0);
    }
    #[test]
    fn kanban_drag_commits_only_valid_release_and_cancellation_preserves_board() {
        let _lock = UI_LOCK.lock().unwrap(); let _app = App::debug(80,24,"").build().unwrap();
        for cancel in 0..5 {
            let mut board = control(); board.document.add_task(0,Task::draft("Move")).unwrap(); let before = board.document.board.clone();
            board.on_mouse_event(&MouseEvent::Pressed(mouse(2,4))); board.on_mouse_event(&MouseEvent::Drag(mouse(28,4)));
            match cancel { 0 => { board.input(key!("Escape").into()); },1 => board.on_lose_focus(),2 => board.on_resize(board.viewport,Size::new(40,10)),3 => board.suspend(),_ => {} }
            board.on_mouse_event(&MouseEvent::Released(mouse(if cancel == 4 { -1 } else { 28 },4)));
            assert_eq!(board.document.board,before,"cancel {cancel}");
        }
        let mut board = control(); let id = board.document.add_task(0,Task::draft("Move")).unwrap();
        board.on_mouse_event(&MouseEvent::Pressed(mouse(2,4))); board.on_mouse_event(&MouseEvent::Drag(mouse(28,4)));
        board.on_mouse_event(&MouseEvent::Released(mouse(28,4))); assert_eq!(board.document.locate(id),Some((1,0)));
    }
    #[test]
    fn kanban_drag_reorders_same_column_at_lower_card_half() {
        let _lock = UI_LOCK.lock().unwrap(); let _app = App::debug(80,24,"").build().unwrap(); let mut board = control();
        let a = board.document.add_task(0,Task::draft("A")).unwrap(); let b = board.document.add_task(0,Task::draft("B")).unwrap();
        board.on_mouse_event(&MouseEvent::Pressed(mouse(2,4))); board.on_mouse_event(&MouseEvent::Drag(mouse(2,7))); board.on_mouse_event(&MouseEvent::Released(mouse(2,7)));
        assert_eq!(board.document.board.columns[0].tasks.iter().map(|task| task.id).collect::<Vec<_>>(),[b,a]);
    }
    #[test]
    fn kanban_labels_preserve_complete_graphemes_and_clip_wide_right_edge() {
        let attr = CharAttribute::with_color(Color::White,Color::Black); let mut surface = Surface::new(6,2);
        write_label(&mut surface,0,0,4,"中e\u{301}👩🏽‍💻",attr);
        assert_eq!(surface.cell_text(0,0).unwrap(),"中"); assert_eq!(surface.cell_width(0,0),Some(2));
        assert_eq!(surface.cell_text(2,0).unwrap(),"e\u{301}"); assert_eq!(surface.cell_text(3,0).unwrap()," ");
        write_label(&mut surface,0,1,5,"中e\u{301}👩🏽‍💻",attr); assert_eq!(surface.cell_text(3,1).unwrap(),"👩🏽‍💻"); assert_eq!(surface.cell_width(3,1),Some(2));
    }
    #[test]
    fn kanban_render_clips_unicode_and_tiny_surfaces() {
        let _lock = UI_LOCK.lock().unwrap(); let _app = App::debug(80,24,"").build().unwrap(); let mut board = control();
        board.document.add_task(0,Task::draft(&format!("{}中e\u{301}👩🏽‍💻", "a".repeat(22)))).unwrap();
        let theme = Theme::new(Themes::Light); let mut surface = Surface::new(78,20); board.on_paint(&mut surface,&theme);
        assert_eq!(surface.char(26,4).unwrap().code,' ');
        for (w,h) in [(1,1),(0,0),(20,5),(78,20)] { board.on_resize(board.viewport,Size::new(w,h)); board.on_paint(&mut Surface::new(w,h),&theme); }
        assert!(board.drag.is_none());
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::test_support::{TestDir,UI_LOCK};
    use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
    fn wait(window: &mut KanbanWindow) {
        let deadline = std::time::Instant::now()+Duration::from_secs(5);
        while window.worker.is_busy() { assert!(std::time::Instant::now()<deadline); window.tick(); std::thread::yield_now(); }
    }
    #[test]
    fn kanban_file_actions_keep_current_dirty_board_and_save_updates_state_only_on_success() {
        let _lock = UI_LOCK.lock().unwrap(); let dir = TestDir::new();
        let _app = App::debug(80,24,"").build().unwrap(); let mut window = KanbanWindow::new(None,OpenFiles::default()).unwrap();
        super::tests::dirty(&mut window); window.action(Action::New); assert!(matches!(window.take_action(),Some(KanbanAction::New))); assert!(super::tests::is_dirty(&window));
        let path = dir.path().join("board.kanban"); window.path = Some(path.clone());
        assert!(window.save(false)); assert!(super::tests::is_dirty(&window)); assert!(super::tests::frozen(&window));
        wait(&mut window); assert_eq!(window.document_path(),Some(path)); assert!(!super::tests::is_dirty(&window)); assert!(!super::tests::frozen(&window));
    }
    #[test]
    fn kanban_failed_save_aborts_guard_preserves_dirty_document_and_old_path() {
        let _lock = UI_LOCK.lock().unwrap(); let dir = TestDir::new();
        let _app = App::debug(80,24,"Key.Pressed(Alt+S)\nKey.Pressed(Enter)").build().unwrap(); let mut window = KanbanWindow::new(None,OpenFiles::default()).unwrap();
        super::tests::dirty(&mut window); let path = dir.path().join("missing/board.kanban"); window.path = Some(path.clone());
        window.request_guard(); window.tick(); wait(&mut window);
        assert_eq!(window.take_guard_result(),Some(false)); assert_eq!(window.document_path(),Some(path)); assert!(super::tests::is_dirty(&window)); assert!(!window.is_closing());
    }
    #[test]
    fn kanban_close_save_discard_cancel_and_wait_for_active_file_operation() {
        let _lock = UI_LOCK.lock().unwrap();
        for choice in [0,1,2] {
            let dir = TestDir::new(); let script = match choice { 0 => "Key.Pressed(Alt+S)",1 => "Key.Pressed(Alt+D)",_ => "Key.Pressed(Escape)" };
            let _app = App::debug(80,24,&format!("Paint.Enable(false)\n{script}\nPaint.Enable(false)")).build().unwrap(); let mut window = KanbanWindow::new(None,OpenFiles::default()).unwrap();
            super::tests::dirty(&mut window); window.path = Some(dir.path().join("board.kanban"));
            window.request_close(); window.tick(); wait(&mut window);
            assert_eq!(window.is_closing(),choice != 2,"choice {choice}, pending {}, busy {}",window.pending.is_some(),window.worker.is_busy()); assert_eq!(super::tests::is_dirty(&window),choice != 0);
        }
        let dir = TestDir::new(); let path = dir.path().join("opening.kanban"); super::tests::save_fixture(&path);
        let _app = App::debug(80,24,"").build().unwrap(); let mut window = KanbanWindow::new(Some(path.clone()),OpenFiles::default()).unwrap();
        window.request_guard(); wait(&mut window); assert_eq!(window.take_guard_result(),Some(true)); assert_eq!(window.document_path(),Some(path));
    }
    #[test]
    fn kanban_task_editor_validates_fields_and_retains_multiline_details() {
        let _lock = UI_LOCK.lock().unwrap(); let _app = App::debug(80,24,"Key.Pressed(Alt+A)\nPaint.Enable(false)").build().unwrap();
        let mut dialog = TaskDialog::new(Task::draft("")); dialog.submit(); assert!(!dialog.control(dialog.status).unwrap().caption().is_empty());
        let title = dialog.title; dialog.control_mut(title).unwrap().set_text("Plan 文");
        let notes = dialog.notes; dialog.control_mut(notes).unwrap().set_text("First\nSecond 👩🏽‍💻");
        let priority = dialog.priority; dialog.control_mut(priority).unwrap().set_index(2);
        let due = dialog.due; dialog.control_mut(due).unwrap().set_text("2026-02-29"); dialog.submit(); assert!(dialog.control(dialog.status).unwrap().caption().contains("YYYY-MM-DD"));
        dialog.control_mut(due).unwrap().set_text("2028-02-29"); let task = dialog.show().unwrap();
        assert_eq!(task.title,"Plan 文"); assert_eq!(task.notes,"First\nSecond 👩🏽‍💻"); assert_eq!(task.priority,Priority::High); assert_eq!(task.due,"2028-02-29");
    }
    #[test]
    fn kanban_columns_dialog_moves_populated_columns_and_cancel_keeps_original_document() {
        let _lock = UI_LOCK.lock().unwrap(); let _app = App::debug(80,24,"Key.Pressed(Enter)\nKey.Pressed(Escape)").build().unwrap();
        let mut document = Document::default(); let task = document.add_task(0,Task::draft("Keep")).unwrap(); let original = document.board.clone();
        let mut dialog = ColumnsDialog::new(document.clone()); dialog.action(4);
        assert_eq!(dialog.document.board.columns.len(),3); assert_eq!(dialog.document.locate(task),Some((0,0)));
        dialog.action(3); assert_eq!(dialog.document.locate(task),Some((1,0)));
        assert!(dialog.show().is_none()); assert_eq!(document.board,original);
    }
    #[test]
    fn kanban_desktop_hold_supersedes_queued_local_close_without_consuming_modal_input() {
        let _lock = UI_LOCK.lock().unwrap(); let _app = App::debug(80,24,"Key.Pressed(Alt+D)\nPaint.Enable(false)").build().unwrap();
        let mut window = KanbanWindow::new(None,OpenFiles::default()).unwrap(); super::tests::dirty(&mut window); window.request_close();
        window.set_guard_hold(true); window.tick(); assert!(!window.is_closing()); assert!(window.pending.is_none());
        window.set_guard_hold(false); window.tick(); assert!(!window.is_closing()); assert!(super::tests::is_dirty(&window));
    }
    #[test]
    fn kanban_editor_apply_preserves_unchanged_notes_exactly() {
        let _lock = UI_LOCK.lock().unwrap();
        for notes in ["","text","text\n","a\tb\r\n"] {
            let _app = App::debug(80,24,"Key.Pressed(Alt+A)\nPaint.Enable(false)").build().unwrap();
            let mut task = Task::draft("Unchanged"); task.notes = notes.into();
            let edited = TaskDialog::new(task.clone()).show().unwrap(); assert_eq!(edited,task);
        }
    }
    #[Desktop(events = DesktopEvents,overwrite = OnResize)]
    struct CloseProbe { board: Handle<KanbanWindow>,other: Handle<Window>,observed: Arc<AtomicUsize>,phase: u8 }
    impl DesktopEvents for CloseProbe {
        fn on_start(&mut self) {
            self.other = self.add_window(Window::new("Other app",layout!("a:c,w:20,h:8"),window::Flags::None));
            let mut window = KanbanWindow::new(None,OpenFiles::default()).unwrap(); super::tests::dirty(&mut window);
            window.set_size(60,20); window.set_position(0,1); self.board = self.add_window(window);
        }
    }
    impl OnResize for CloseProbe {
        fn on_resize(&mut self,_: Size,size: Size) {
            if size.width == 79 && self.phase == 0 {
                self.phase = 1; let handle = self.board; let window = self.window_mut(handle).unwrap();
                assert!(window.pending.as_ref().is_some_and(|pending| pending.intent == Intent::Close)); TimerEvents::on_update(window,0);
            } else if size.width == 78 && self.phase == 1 {
                self.phase = 2; assert!(self.windowt(self.board).is_none()); assert!(self.windowt(self.other).is_some()); self.observed.store(1,Ordering::Relaxed);
            }
        }
    }
    #[test]
    fn kanban_titlebar_close_removes_registered_board_after_discard_and_keeps_other_app() {
        let _lock = UI_LOCK.lock().unwrap(); let observed = Arc::new(AtomicUsize::new(0));
        let desktop = CloseProbe { base: Desktop::new(),board: Handle::None,other: Handle::None,observed: observed.clone(),phase: 0 };
        App::debug(80,24,"Paint.Enable(false)\nMouse.Click(57,1,left)\nResize(79,24)\nKey.Pressed(Alt+D)\nPaint.Enable(false)\nResize(78,24)").desktop(desktop).build().unwrap().run();
        assert_eq!(observed.load(Ordering::Relaxed),1);
    }
    #[Desktop(events = DesktopEvents,overwrite = OnResize)]
    struct ModalProbe { board: Handle<KanbanWindow>,observed: Arc<AtomicUsize> }
    impl DesktopEvents for ModalProbe {
        fn on_start(&mut self) {
            let mut window = KanbanWindow::new(None,OpenFiles::default()).unwrap(); super::tests::dirty(&mut window); window.request_guard();
            self.board = self.add_window(window); Window::update_control(self.board,|window| { TimerEvents::on_update(window,0); });
            let handle = self.board; let window = self.window_mut(handle).unwrap(); assert_eq!(window.take_guard_result(),Some(false)); assert!(super::tests::is_dirty(window)); assert!(!window.closing);
            self.observed.fetch_or(4,Ordering::Relaxed);
        }
    }
    impl OnResize for ModalProbe {
        fn on_resize(&mut self,_: Size,size: Size) {
            if size.width != 79 && size.width != 78 { return; }
            let handle = self.board; let window = self.window_mut(handle).unwrap(); let approved = size.width == 78;
            assert_eq!(window.pending.as_ref().map(|pending| pending.approved),Some(approved));
            assert!(matches!(TimerEvents::on_update(window,1),EventProcessStatus::Ignored));
            assert!(window.guard_result.is_none()); assert_eq!(window.pending.as_ref().map(|pending| pending.approved),Some(approved));
            self.observed.fetch_or(if approved { 2 } else { 1 },Ordering::Relaxed);
        }
    }
    #[test]
    fn kanban_nested_modal_timers_cannot_consume_guard_or_save_decisions_twice() {
        let _lock = UI_LOCK.lock().unwrap(); let observed = Arc::new(AtomicUsize::new(0));
        let desktop = ModalProbe { base: Desktop::new(),board: Handle::None,observed: observed.clone() };
        App::debug(80,24,"Paint.Enable(false)\nResize(79,24)\nKey.Pressed(Alt+S)\nResize(78,24)\nKey.Pressed(Escape)").desktop(desktop).build().unwrap().run();
        assert_eq!(observed.load(Ordering::Relaxed),7);
    }
}
