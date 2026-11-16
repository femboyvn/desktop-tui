use super::image_viewer::home_directory;
use crate::runtime::ascii_paint::{Cell, Completion, Document, Operation, PaintWorker, Rgb, ImageOptions, MAX_WIDTH, MAX_HEIGHT};
use appcui::dialogs::{Location, OpenFileDialogFlags, SaveFileDialogFlags};
use appcui::prelude::*;
use std::path::PathBuf;
use std::time::Duration;

const TOP: usize = 5;
const FOOTER: usize = 1;
const SWATCHES: [[u8; 3]; 12] = [
    [0,0,0], [255,255,255], [128,128,128], [255,0,0], [255,128,0], [255,255,0],
    [0,192,0], [0,255,255], [0,96,255], [128,0,255], [255,0,255], [255,128,192],
];
#[derive(Clone, Copy, Debug, PartialEq)]
enum PaintAction { New, Open, Save, SaveAs, Export, Undo, Redo, Clear, Brush, Eraser, Resize, Image }
const TOOLS: &[(i32, &str, PaintAction)] = &[
    (0,"[New]",PaintAction::New), (6,"[Open]",PaintAction::Open), (13,"[Save]",PaintAction::Save),
    (20,"[Save As]",PaintAction::SaveAs), (30,"[Export Text]",PaintAction::Export),
    (44,"[Resize]",PaintAction::Resize), (53,"[Image to ASCII]",PaintAction::Image),
];
const EDIT_TOOLS: &[(i32, &str, PaintAction)] = &[
    (0,"[Undo]",PaintAction::Undo), (8,"[Redo]",PaintAction::Redo), (16,"[Clear]",PaintAction::Clear),
    (25,"[Brush / RGB]",PaintAction::Brush), (40,"[Eraser]",PaintAction::Eraser),
];

#[CustomControl(overwrite = OnPaint+OnKeyPressed+OnMouseEvent+OnFocus+OnResize)]
struct PaintCanvas {
    document: Document, brush: Cell, eraser: bool, cursor: (usize, usize), offset: (usize, usize),
    viewport: Size, frozen: bool, action: Option<PaintAction>, status: String,
}
impl PaintCanvas {
    fn new() -> Self {
        let document = Document::default();
        let brush = Cell { ch: '#', ..document.blank() };
        Self { base: ControlBase::new(Layout::fill(), true), document, brush, eraser: false,
            cursor: (0,0), offset: (0,0), viewport: Size::new(0,0), frozen: false, action: None, status: String::new() }
    }
    fn extent(&self) -> (usize,usize) { (self.viewport.width as usize, (self.viewport.height as usize).saturating_sub(TOP + FOOTER)) }
    fn clamp_scroll(&mut self) {
        let (width,height) = self.extent();
        self.offset.0 = self.offset.0.min(self.document.width().saturating_sub(width));
        self.offset.1 = self.offset.1.min(self.document.height().saturating_sub(height));
    }
    fn reveal_cursor(&mut self) {
        let (width,height) = self.extent();
        if width == 0 || height == 0 { return; }
        self.offset.0 = self.offset.0.min(self.cursor.0).max(self.cursor.0.saturating_sub(width - 1));
        self.offset.1 = self.offset.1.min(self.cursor.1).max(self.cursor.1.saturating_sub(height - 1));
        self.clamp_scroll();
    }
    fn scroll(&mut self, dx: isize, dy: isize) {
        self.document.finish_stroke();
        self.offset.0 = self.offset.0.saturating_add_signed(dx);
        self.offset.1 = self.offset.1.saturating_add_signed(dy);
        self.clamp_scroll();
    }
    fn point(&self, x: i32, y: i32) -> Option<(usize,usize)> {
        if x < 0 || y < TOP as i32 || x >= self.viewport.width as i32 || y >= self.viewport.height.saturating_sub(FOOTER as u32) as i32 { return None; }
        let (x,y) = (x as usize + self.offset.0, y as usize - TOP + self.offset.1);
        (x < self.document.width() && y < self.document.height()).then_some((x,y))
    }
    fn footer_labels(&self) -> (String,usize,String) {
        let width = self.viewport.width as usize;
        let mut left = format!("[Size {}x{}]",self.document.width(),self.document.height());
        let mut right = format!("Cursor {},{}",self.cursor.0+1,self.cursor.1+1);
        if left.len()+right.len()+1 > width {
            left = format!("{}x{}",self.document.width(),self.document.height());
            right = format!("{},{}",self.cursor.0+1,self.cursor.1+1);
        }
        if right.len()+1 > width { right.clear(); }
        left.truncate(width.saturating_sub(right.len()+usize::from(!right.is_empty())));
        (left,width.saturating_sub(right.len()),right)
    }
    fn bound_cursor(&mut self) {
        self.cursor.0 = self.cursor.0.min(self.document.width()-1);
        self.cursor.1 = self.cursor.1.min(self.document.height()-1);
        self.reveal_cursor();
    }
    fn stamp(&mut self, erase: bool) {
        self.status.clear();
        self.document.finish_stroke();
        let cell = if erase || self.eraser { self.document.blank() } else { self.brush };
        self.document.begin_stroke(self.cursor.0,self.cursor.1,cell);
        self.document.finish_stroke();
    }
    fn replace(&mut self, document: Document) {
        self.document = document; self.cursor = (0,0); self.offset = (0,0); self.status.clear();
    }
    fn queue(&mut self, action: PaintAction) {
        self.document.finish_stroke();
        if !self.frozen { self.action = Some(action); }
    }
    fn input(&mut self, key: Key, character: char) -> EventProcessStatus {
        if self.frozen { return EventProcessStatus::Processed; }
        let action = match key.value() {
            key!("Ctrl+N") => Some(PaintAction::New), key!("Ctrl+O") => Some(PaintAction::Open),
            key!("Ctrl+S") => Some(PaintAction::Save), key!("Ctrl+Shift+S") => Some(PaintAction::SaveAs),
            key!("Ctrl+E") => Some(PaintAction::Export), key!("Ctrl+Z") => Some(PaintAction::Undo),
            key!("Ctrl+Y") => Some(PaintAction::Redo), key!("Ctrl+R") => Some(PaintAction::Resize),
            key!("Ctrl+I") => Some(PaintAction::Image), _ => None,
        };
        if let Some(action) = action { self.queue(action); return EventProcessStatus::Processed; }
        if key.modifier != KeyModifier::None && key.modifier != KeyModifier::Shift { return EventProcessStatus::Ignored; }
        self.document.finish_stroke();
        match key.code {
            KeyCode::Left => self.cursor.0 = self.cursor.0.saturating_sub(1),
            KeyCode::Right => self.cursor.0 = (self.cursor.0 + 1).min(self.document.width() - 1),
            KeyCode::Up => self.cursor.1 = self.cursor.1.saturating_sub(1),
            KeyCode::Down => self.cursor.1 = (self.cursor.1 + 1).min(self.document.height() - 1),
            KeyCode::Space | KeyCode::Enter => self.stamp(false),
            KeyCode::Delete | KeyCode::Backspace => self.stamp(true),
            KeyCode::PageUp => { self.scroll(0,-(self.extent().1.max(1) as isize)); return EventProcessStatus::Processed; }
            KeyCode::PageDown => { self.scroll(0,self.extent().1.max(1) as isize); return EventProcessStatus::Processed; }
            _ if (' '..='~').contains(&character) => { self.brush.ch = character; self.eraser = false; }
            _ => return EventProcessStatus::Ignored,
        }
        self.reveal_cursor();
        EventProcessStatus::Processed
    }
}
impl OnPaint for PaintCanvas {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        surface.clear(Character::with_attributes(' ',theme.window.normal));
        for (x,label,_) in TOOLS { surface.write_string(*x,0,label,theme.text.normal,false); }
        for (x,label,action) in EDIT_TOOLS {
            surface.write_string(*x,1,label,if *action == PaintAction::Eraser && self.eraser { theme.text.highlighted } else { theme.text.normal },false);
        }
        surface.write_string(0,2,"Palette L:FG R:BG",theme.text.normal,false);
        for (index,[r,g,b]) in SWATCHES.iter().enumerate() {
            surface.write_string(18 + index as i32 * 3,2,"   ",CharAttribute::new(Color::RGB(*r,*g,*b),Color::RGB(*r,*g,*b),CharFlags::None),false);
        }
        let [r,g,b] = self.brush.foreground.0;
        let [br,bg,bb] = self.brush.background.0;
        surface.write_string(0,3,"Preview:",theme.text.normal,false);
        surface.write_char(9,3,Character::new(self.brush.ch,Color::RGB(r,g,b),Color::RGB(br,bg,bb),CharFlags::None));
        surface.write_string(12,3,&format!("FG {}  BG {}  {}",self.brush.foreground.to_hex(),self.brush.background.to_hex(),if self.eraser { "Eraser" } else { "Brush" }),theme.text.normal,false);
        let status = if self.frozen { "Busy / waiting for decision".to_owned() } else if !self.status.is_empty() { self.status.clone() }
            else { format!("View {},{}  Arrows / Space / Del | PgUp/Dn",self.offset.0+1,self.offset.1+1) };
        surface.write_string(0,4,&status,theme.text.normal,false);
        let (width,height) = self.extent();
        for y in 0..height.min(self.document.height().saturating_sub(self.offset.1)) {
            for x in 0..width.min(self.document.width().saturating_sub(self.offset.0)) {
                if let Some(cell) = self.document.cell(x + self.offset.0,y + self.offset.1) {
                    let [r,g,b] = cell.foreground.0;
                    let [br,bg,bb] = cell.background.0;
                    surface.write_char(x as i32,(y+TOP) as i32,Character::new(cell.ch,Color::RGB(r,g,b),Color::RGB(br,bg,bb),CharFlags::None));
                }
            }
        }
        if self.viewport.height > 0 {
            let (left,right_x,right) = self.footer_labels();
            let y = self.viewport.height as i32-1;
            surface.fill_horizontal_line_with_size(0,y,self.viewport.width,Character::with_attributes(' ',theme.window.normal));
            surface.write_string(0,y,&left,theme.text.normal,false);
            surface.write_string(right_x as i32,y,&right,theme.text.normal,false);
        }
        if self.has_focus() && !self.frozen && self.cursor.0 >= self.offset.0 && self.cursor.1 >= self.offset.1 {
            let (x,y) = (self.cursor.0-self.offset.0,self.cursor.1-self.offset.1);
            if x < width && y < height { surface.set_cursor(x as i32,(y+TOP) as i32); }
        }
    }
}
impl OnKeyPressed for PaintCanvas {
    fn on_key_pressed(&mut self,key: Key,ch: char) -> EventProcessStatus {
        if !self.has_focus() { return EventProcessStatus::Ignored; }
        self.input(key,ch)
    }
}
impl OnMouseEvent for PaintCanvas {
    fn on_mouse_event(&mut self,event: &MouseEvent) -> EventProcessStatus {
        if self.frozen { return EventProcessStatus::Processed; }
        match event {
            MouseEvent::Pressed(data) => {
                self.request_focus();
                self.document.finish_stroke();
                if !matches!(data.button,MouseButton::Left | MouseButton::Right) { return EventProcessStatus::Ignored; }
                if data.x < 0 || data.y < 0 || data.x >= self.viewport.width as i32 || data.y >= self.viewport.height as i32 { return EventProcessStatus::Ignored; }
                if data.y == self.viewport.height as i32-1 {
                    if data.button == MouseButton::Left && data.x < self.footer_labels().0.len() as i32 { self.queue(PaintAction::Resize); }
                } else if data.y == 0 || data.y == 1 {
                    let tools = if data.y == 0 { TOOLS } else { EDIT_TOOLS };
                    if let Some((_,_,action)) = tools.iter().find(|(x,label,_)| data.x >= *x && data.x < *x + label.len() as i32) { self.queue(*action); }
                } else if data.y == 2 && (18..54).contains(&data.x) {
                    let rgb = Rgb(SWATCHES[(data.x as usize-18)/3]);
                    if data.button == MouseButton::Right { self.brush.background = rgb; } else { self.brush.foreground = rgb; }
                } else if let Some((x,y)) = self.point(data.x,data.y) {
                    self.cursor = (x,y);
                    let cell = if data.button == MouseButton::Right || self.eraser { self.document.blank() } else { self.brush };
                    self.document.begin_stroke(x,y,cell);
                }
            }
            MouseEvent::Drag(data) => if let Some((x,y)) = self.point(data.x,data.y) {
                self.cursor = (x,y); self.document.continue_stroke(x,y);
            },
            MouseEvent::Released(_) | MouseEvent::Leave => self.document.finish_stroke(),
            MouseEvent::Wheel(MouseWheelDirection::Up) => self.scroll(0,-3),
            MouseEvent::Wheel(MouseWheelDirection::Down) => self.scroll(0,3),
            MouseEvent::Wheel(MouseWheelDirection::Left) => self.scroll(-3,0),
            MouseEvent::Wheel(MouseWheelDirection::Right) => self.scroll(3,0),
            _ => return EventProcessStatus::Ignored,
        }
        EventProcessStatus::Processed
    }
}
impl OnFocus for PaintCanvas { fn on_lose_focus(&mut self) { self.document.finish_stroke(); } }
impl OnResize for PaintCanvas {
    fn on_resize(&mut self,_: Size,size: Size) { self.document.finish_stroke(); self.viewport = size; self.clamp_scroll(); }
}

#[derive(Clone)]
enum Intent { New, Open(PathBuf), Image(PathBuf), Close, Guard }
struct Pending { intent: Intent, approved: bool }

/// The desktop asks for a guard before touching any other windows or issuing power actions.
/// Results and dialogs are deferred while hidden (including Terminal Lock).
#[Window(events = TimerEvents+WindowEvents)]
pub(crate) struct PaintWindow {
    canvas: Handle<PaintCanvas>, worker: PaintWorker, path: Option<PathBuf>, saving_path: Option<PathBuf>,
    pending: Option<Pending>, guard_result: Option<bool>, closing: bool, updating: bool, guard_hold: bool,
}
impl PaintWindow {
    pub fn document_path(&self) -> Option<PathBuf> { self.path.clone() }
    pub fn new(path: Option<PathBuf>) -> anyhow::Result<Self> {
        let mut window = Self { base: Window::new("ASCII Paint",layout!("a:c,w:92,h:34"),window::Flags::Sizeable),
            canvas: Handle::None, worker: PaintWorker::new(), path: None, saving_path: None,
            pending: path.map(|path| Pending { intent: Intent::Open(path), approved: true }), guard_result: None, closing: false, updating: false, guard_hold: false };
        window.timer().ok_or_else(|| anyhow::anyhow!("ASCII Paint timer unavailable"))?.start(Duration::from_millis(40));
        window.canvas = window.add(PaintCanvas::new());
        window.base.request_focus_for_control(window.canvas);
        Ok(window)
    }
    pub fn is_closing(&self) -> bool { self.closing }
    pub fn set_guard_hold(&mut self,held: bool) {
        self.guard_hold = held;
        // Desktop-wide decisions supersede queued local close/replacement actions.
        // The file worker remains alive and its result is still applied normally.
        if held && self.pending.as_ref().is_some_and(|pending| !matches!(pending.intent,Intent::Guard)) { self.pending = None; }
        self.suspend(); self.freeze();
    }
    // Desktop Exit calls this only after the unsaved-work guard has approved.
    pub fn shutdown(&mut self) { self.closing = true; self.suspend(); self.close(); }
    pub fn suspend(&mut self) { let handle = self.canvas; if let Some(canvas) = self.control_mut(handle) { canvas.document.finish_stroke(); } }
    pub fn constrain_to(&mut self,available: Size) {
        self.suspend();
        let (width,height) = (available.width.clamp(1,u16::MAX as u32) as u16,available.height.clamp(1,u16::MAX as u32) as u16);
        let size = self.size(); let position = self.position();
        self.set_size_bounds(1,1,width,height);
        if size.width > 1 && size.height > 1 {
            self.set_position(position.x.clamp(0,i32::from(width)-size.width.min(u32::from(width)) as i32),
                position.y.clamp(0,i32::from(height)-size.height.min(u32::from(height)) as i32));
        }
    }
    fn request(&mut self,intent: Intent) {
        if self.closing || self.guard_hold || self.pending.is_some() || self.guard_result.is_some() { return; }
        self.suspend();
        self.pending = Some(Pending { intent, approved: false });
        self.freeze();
    }
    pub fn open(&mut self,path: PathBuf) { self.request(Intent::Open(path)); }
    pub fn request_close(&mut self) { self.request(Intent::Close); }
    pub fn request_guard(&mut self) {
        if self.closing || self.guard_result.is_some() { return; }
        self.suspend();
        // A desktop exit or power action supersedes a queued local replacement.
        self.pending = Some(Pending { intent: Intent::Guard, approved: false });
        self.freeze();
    }
    pub fn take_guard_result(&mut self) -> Option<bool> {
        let result = self.guard_result.take(); self.freeze(); result
    }
    fn freeze(&mut self) {
        let frozen = self.worker.is_busy() || self.pending.is_some() || self.guard_result.is_some() || self.guard_hold;
        let handle = self.canvas;
        if let Some(canvas) = self.control_mut(handle) { canvas.frozen = frozen; if frozen { canvas.action = None; } }
    }
    fn cancel_pending(&mut self) {
        if self.pending.take().is_some_and(|pending| matches!(pending.intent,Intent::Guard)) { self.guard_result = Some(false); }
        self.freeze();
    }
    fn save(&mut self,save_as: bool) -> bool {
        self.suspend();
        let path = if save_as { None } else { self.path.clone() };
        let path = path.or_else(|| dialogs::save("Save colored project (.asciipaint)","Untitled.asciipaint",Location::Path(&home_directory()),None,
            SaveFileDialogFlags::Icons | SaveFileDialogFlags::ValidateOverwrite));
        let Some(path) = path else { return false; };
        if !is_project(&path) { dialogs::error("Save project","Use the .asciipaint extension for colored projects."); return false; }
        let Some(canvas) = self.control(self.canvas) else { return false; };
        let document = canvas.document.clone();
        if self.worker.submit(Operation::Save(document,path.clone())) { self.saving_path = Some(path); self.freeze(); true } else { false }
    }
    fn process_pending(&mut self) {
        if self.worker.is_busy() || self.pending.is_none() { return; }
        let dirty = self.control(self.canvas).is_some_and(|canvas| canvas.document.is_dirty());
        if dirty && self.pending.as_ref().is_some_and(|pending| !pending.approved) {
            match UnsavedDialog::new().show() {
                Some(UnsavedChoice::Save) => {
                    if let Some(pending) = &mut self.pending { pending.approved = true; }
                    if !self.save(false) { self.cancel_pending(); }
                    return;
                }
                Some(UnsavedChoice::Discard) => {}
                _ => { self.cancel_pending(); return; }
            }
        }
        let Some(pending) = self.pending.take() else { return; };
        match pending.intent {
            Intent::New => if let Some(Dimensions { width,height }) = DimensionsDialog::new().show()
                && let Ok(document) = Document::new(width,height) {
                let handle = self.canvas;
                if let Some(canvas) = self.control_mut(handle) { canvas.replace(document); }
                self.path = None;
            },
            Intent::Open(path) => {
                // Assign the path only after a successful load; failures keep the old document intact.
                self.saving_path = Some(path.clone());
                if !self.worker.submit(Operation::Open(path)) { self.saving_path = None; }
            }
            Intent::Image(path) => {
                let dimensions = self.control(self.canvas).map(|canvas| (canvas.document.width(),canvas.document.height()));
                if let Some((width,height)) = dimensions && let Some(options) = ImageConversionDialog::new(width,height).show() {
                    self.saving_path = None;
                    self.worker.submit(Operation::ConvertImage(path,options));
                }
            }
            Intent::Close => { self.closing = true; self.close(); }
            Intent::Guard => self.guard_result = Some(true),
        }
        self.freeze();
    }
    fn action(&mut self,action: PaintAction) {
        if self.worker.is_busy() || self.pending.is_some() || self.guard_result.is_some() || self.guard_hold { return; }
        match action {
            PaintAction::New => self.request(Intent::New),
            PaintAction::Open => if let Some(path) = dialogs::open("Open Paint project or ASCII text", "",Location::Path(&home_directory()),None,
                OpenFileDialogFlags::Icons | OpenFileDialogFlags::CheckIfFileExists | OpenFileDialogFlags::PreserveBackground) { self.open(path); },
            PaintAction::Image => if let Some(path) = super::select_image() { self.request(Intent::Image(path)); },
            PaintAction::Resize => {
                let dimensions = self.control(self.canvas).map(|canvas| (canvas.document.width(),canvas.document.height()));
                if let Some((width,height)) = dimensions
                    && let Some(Dimensions { width,height }) = DimensionsDialog::with_size("Resize canvas (Undo restores cropped cells)",width,height).show() {
                    let handle = self.canvas;
                    if let Some(canvas) = self.control_mut(handle) {
                        match canvas.document.resize(width,height) {
                            Ok(_) => { canvas.status.clear(); canvas.bound_cursor(); }
                            Err(error) => dialogs::error("Cannot resize canvas",&error),
                        }
                    }
                }
            }
            PaintAction::Save => { self.save(false); }
            PaintAction::SaveAs => { self.save(true); }
            PaintAction::Export => if let Some(path) = dialogs::save("Export Text (colorless)","Untitled.txt",Location::Path(&home_directory()),None,
                SaveFileDialogFlags::Icons | SaveFileDialogFlags::ValidateOverwrite) {
                if !path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("txt") || extension.eq_ignore_ascii_case("asc")) {
                    dialogs::error("Export Text","Use .txt or .asc for colorless text export."); return;
                }
                if self.path.as_ref() == Some(&path) { dialogs::error("Export Text","Choose a different path from the colored project."); return; }
                if let Some(canvas) = self.control(self.canvas) { let document = canvas.document.clone(); self.worker.submit(Operation::Export(document,path)); }
            },
            PaintAction::Brush => {
                if let Some(brush) = self.control(self.canvas).map(|canvas| canvas.brush).and_then(|brush| BrushDialog::new(brush).show()) {
                    let handle = self.canvas;
                    if let Some(canvas) = self.control_mut(handle) { canvas.brush = brush; canvas.eraser = false; }
                }
            }
            _ => { let handle = self.canvas; if let Some(canvas) = self.control_mut(handle) {
                canvas.document.finish_stroke();
                match action { PaintAction::Undo => { canvas.document.undo(); }, PaintAction::Redo => { canvas.document.redo(); },
                    PaintAction::Clear => { canvas.document.clear(); }, PaintAction::Eraser => canvas.eraser = !canvas.eraser, _ => {} }
                canvas.status.clear(); canvas.bound_cursor();
            } }
        }
        self.freeze();
    }
    fn tick(&mut self) {
        // AppCUI runs a nested event loop for every modal, including file pickers
        // and errors. Keep pending decisions and save continuations owned by this tick.
        if self.updating { return; }
        self.updating = true;
        if let Some(result) = self.worker.poll() {
            match result {
                Ok(Completion::Opened(document)) => {
                    let path = self.saving_path.take();
                    self.path = path.filter(|path| is_project(path));
                    let handle = self.canvas;
                    if let Some(canvas) = self.control_mut(handle) { canvas.replace(*document); }
                }
                Ok(Completion::Saved) => {
                    self.path = self.saving_path.take();
                    let handle = self.canvas;
                    if let Some(canvas) = self.control_mut(handle) { canvas.document.mark_saved(); canvas.status = "Project saved".into(); }
                }
                Ok(Completion::Exported) => { let handle = self.canvas; if let Some(canvas) = self.control_mut(handle) { canvas.status = "Exported text without colors".into(); } }
                Err(error) => { self.saving_path = None; self.cancel_pending(); dialogs::error("ASCII Paint file operation failed",&error); }
            }
        }
        self.process_pending();
        self.freeze();
        let handle = self.canvas;
        let action = self.control_mut(handle).and_then(|canvas| canvas.action.take());
        if let Some(action) = action { self.action(action); }
        let dirty = self.control(self.canvas).is_some_and(|canvas| canvas.document.is_dirty());
        let name = self.path.as_ref().map(|path| path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
        self.set_title(&format!("ASCII Paint — {name}{}",if dirty { " *" } else { "" }));
        self.updating = false;
    }
}
fn is_project(path: &std::path::Path) -> bool { path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("asciipaint")) }
impl TimerEvents for PaintWindow {
    fn on_update(&mut self,_: u64) -> EventProcessStatus {
        if self.updating { return EventProcessStatus::Ignored; }
        if self.closing || !self.is_visible() { self.suspend(); return EventProcessStatus::Ignored; }
        if !self.has_focus() { self.suspend(); }
        self.tick();
        EventProcessStatus::Processed
    }
}
impl WindowEvents for PaintWindow {
    fn on_cancel(&mut self) -> ActionRequest {
        // Window::close calls on_cancel again after our pending close is approved.
        if self.closing { return ActionRequest::Allow; }
        self.request_close();
        ActionRequest::Deny
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum UnsavedChoice { Save, Discard }
#[ModalWindow(response: UnsavedChoice, events: ButtonEvents)]
pub(super) struct UnsavedDialog { save: Handle<Button>, discard: Handle<Button>, cancel: Handle<Button> }
impl UnsavedDialog {
    fn new() -> Self {
        Self::for_document("Unsaved ASCII Paint","Save the colored project before continuing?")
    }
    pub(super) fn for_document(title: &str,message: &str) -> Self {
        let mut dialog = Self { base: ModalWindow::new(title,layout!("a:c,w:56,h:9"),window::Flags::None),save: Handle::None,discard: Handle::None,cancel: Handle::None };
        dialog.add(Label::new(message,layout!("l:2,t:1,r:2,h:2")));
        dialog.save = dialog.add(button!("&Save,l:2,b:0,w:14"));
        dialog.discard = dialog.add(button!("&Discard,l:19,b:0,w:14"));
        dialog.cancel = dialog.add(button!("&Cancel,r:2,b:0,w:14"));
        dialog.base.request_focus_for_control(dialog.cancel);
        dialog
    }
}
impl ButtonEvents for UnsavedDialog {
    fn on_pressed(&mut self,handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.save { self.exit_with(UnsavedChoice::Save); }
        else if handle == self.discard { self.exit_with(UnsavedChoice::Discard); }
        else if handle == self.cancel { self.exit(); }
        else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
}

fn validate_brush(character: &str,foreground: &str,background: &str) -> Result<Cell,String> {
    let mut chars = character.chars();
    let ch = chars.next().filter(|ch| (' '..='~').contains(ch)).ok_or("Choose one printable ASCII character (space through ~).")?;
    if chars.next().is_some() { return Err("Choose exactly one printable ASCII character.".into()); }
    Ok(Cell { ch, foreground: Rgb::parse_hex(foreground)?, background: Rgb::parse_hex(background)? })
}
#[ModalWindow(response: Cell, events: ButtonEvents+WindowEvents)]
struct BrushDialog { character: Handle<TextField>, foreground: Handle<TextField>, background: Handle<TextField>, status: Handle<Label>, apply: Handle<Button>, cancel: Handle<Button> }
impl BrushDialog {
    fn new(brush: Cell) -> Self {
        let mut dialog = Self { base: ModalWindow::new("Brush and RGB colors",layout!("a:c,w:54,h:13"),window::Flags::None),
            character: Handle::None,foreground: Handle::None,background: Handle::None,status: Handle::None,apply: Handle::None,cancel: Handle::None };
        dialog.add(Label::new("Character (ASCII)",layout!("l:2,t:1,w:20,h:1")));
        dialog.character = dialog.add(TextField::new(&brush.ch.to_string(),layout!("l:25,t:1,w:10,h:1"),textfield::Flags::None));
        dialog.add(Label::new("Foreground #RRGGBB",layout!("l:2,t:3,w:22,h:1")));
        dialog.foreground = dialog.add(TextField::new(&brush.foreground.to_hex(),layout!("l:25,t:3,w:12,h:1"),textfield::Flags::None));
        dialog.add(Label::new("Background #RRGGBB",layout!("l:2,t:5,w:22,h:1")));
        dialog.background = dialog.add(TextField::new(&brush.background.to_hex(),layout!("l:25,t:5,w:12,h:1"),textfield::Flags::None));
        dialog.status = dialog.add(Label::new("",layout!("l:2,t:7,r:2,h:2")));
        dialog.apply = dialog.add(button!("&Apply,l:6,b:0,w:16"));
        dialog.cancel = dialog.add(button!("&Cancel,r:6,b:0,w:16"));
        dialog.base.request_focus_for_control(dialog.character);
        dialog
    }
    fn submit(&mut self) {
        let result = validate_brush(self.control(self.character).map_or("",TextField::text),self.control(self.foreground).map_or("",TextField::text),self.control(self.background).map_or("",TextField::text));
        match result { Ok(brush) => self.exit_with(brush), Err(error) => { let handle = self.status; if let Some(status) = self.control_mut(handle) { status.set_caption(&error); } } }
    }
}
impl ButtonEvents for BrushDialog {
    fn on_pressed(&mut self,handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.apply { self.submit(); } else if handle == self.cancel { self.exit(); } else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
}
impl WindowEvents for BrushDialog { fn on_accept(&mut self) { self.submit(); } }

struct Dimensions { width: usize, height: usize }
#[ModalWindow(response: Dimensions, events: ButtonEvents+WindowEvents)]
struct DimensionsDialog { width: Handle<TextField>, height: Handle<TextField>, status: Handle<Label>, create: Handle<Button>, cancel: Handle<Button> }
impl DimensionsDialog {
    fn new() -> Self { Self::with_size("New ASCII canvas",80,24) }
    fn with_size(title: &str,width: usize,height: usize) -> Self {
        let mut dialog = Self { base: ModalWindow::new(title,layout!("a:c,w:54,h:11"),window::Flags::None),
            width: Handle::None,height: Handle::None,status: Handle::None,create: Handle::None,cancel: Handle::None };
        dialog.add(Label::new("Columns (1-200)",layout!("l:2,t:1,w:22,h:1")));
        dialog.width = dialog.add(TextField::new(&width.to_string(),layout!("l:26,t:1,w:8,h:1"),textfield::Flags::None));
        dialog.add(Label::new("Rows (1-80)",layout!("l:2,t:3,w:22,h:1")));
        dialog.height = dialog.add(TextField::new(&height.to_string(),layout!("l:26,t:3,w:8,h:1"),textfield::Flags::None));
        dialog.status = dialog.add(Label::new("",layout!("l:2,t:5,r:2,h:2")));
        dialog.create = dialog.add(button!("&Apply,l:4,b:0,w:16")); dialog.cancel = dialog.add(button!("&Cancel,r:4,b:0,w:16"));
        dialog.base.request_focus_for_control(dialog.width); dialog
    }
    fn submit(&mut self) {
        let width = self.control(self.width).and_then(|field| field.text().parse::<usize>().ok());
        let height = self.control(self.height).and_then(|field| field.text().parse::<usize>().ok());
        if let (Some(width),Some(height)) = (width,height) && (1..=MAX_WIDTH).contains(&width) && (1..=MAX_HEIGHT).contains(&height) { self.exit_with(Dimensions { width,height }); }
        else { let handle = self.status; if let Some(status) = self.control_mut(handle) { status.set_caption("Use 1-200 columns and 1-80 rows."); } }
    }
}
impl ButtonEvents for DimensionsDialog {
    fn on_pressed(&mut self,handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.create { self.submit(); } else if handle == self.cancel { self.exit(); } else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
}
impl WindowEvents for DimensionsDialog { fn on_accept(&mut self) { self.submit(); } }

#[ModalWindow(response: ImageOptions, events: ButtonEvents+WindowEvents)]
struct ImageConversionDialog {
    width: Handle<TextField>, height: Handle<TextField>, color: Handle<CheckBox>,
    status: Handle<Label>, convert: Handle<Button>, cancel: Handle<Button>,
}
impl ImageConversionDialog {
    fn new(width: usize,height: usize) -> Self {
        let mut dialog = Self { base: ModalWindow::new("Convert image to ASCII",layout!("a:c,w:58,h:15"),window::Flags::None),
            width: Handle::None,height: Handle::None,color: Handle::None,status: Handle::None,convert: Handle::None,cancel: Handle::None };
        dialog.add(Label::new("Fit within these limits, preserving proportions.\nAccounts for tall terminal character cells.",layout!("l:2,t:0,r:2,h:2")));
        dialog.add(Label::new("Max columns (1-200)",layout!("l:2,t:3,w:24,h:1")));
        dialog.width = dialog.add(TextField::new(&width.to_string(),layout!("l:28,t:3,w:8,h:1"),textfield::Flags::None));
        dialog.add(Label::new("Max rows (1-80)",layout!("l:2,t:5,w:24,h:1")));
        dialog.height = dialog.add(TextField::new(&height.to_string(),layout!("l:28,t:5,w:8,h:1"),textfield::Flags::None));
        dialog.color = dialog.add(CheckBox::new("&Preserve image colors",layout!("l:2,t:7,r:2,h:1"),true));
        dialog.status = dialog.add(Label::new("",layout!("l:2,t:9,r:2,h:2")));
        dialog.convert = dialog.add(button!("Con&vert,l:5,b:0,w:18")); dialog.cancel = dialog.add(button!("&Cancel,r:5,b:0,w:18"));
        dialog.base.request_focus_for_control(dialog.width); dialog
    }
    fn submit(&mut self) {
        let width = self.control(self.width).and_then(|field| field.text().parse::<usize>().ok());
        let height = self.control(self.height).and_then(|field| field.text().parse::<usize>().ok());
        if let (Some(width),Some(height)) = (width,height) && (1..=MAX_WIDTH).contains(&width) && (1..=MAX_HEIGHT).contains(&height) {
            let color = self.control(self.color).is_some_and(CheckBox::is_checked);
            self.exit_with(ImageOptions { width,height,color });
        } else { let handle = self.status; if let Some(status) = self.control_mut(handle) { status.set_caption("Use 1-200 columns and 1-80 rows."); } }
    }
}
impl ButtonEvents for ImageConversionDialog {
    fn on_pressed(&mut self,handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.convert { self.submit(); } else if handle == self.cancel { self.exit(); } else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
}
impl WindowEvents for ImageConversionDialog { fn on_accept(&mut self) { self.submit(); } }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDir, UI_LOCK};
    use std::sync::{Arc, atomic::{AtomicBool,Ordering}};

    #[test]
    fn brush_rejects_non_ascii_multiple_characters_and_malformed_rgb() {
        assert!(validate_brush("é","#FFFFFF","#000000").is_err());
        assert!(validate_brush("ab","#FFFFFF","#000000").is_err());
        assert!(validate_brush("x","#FFF","#000000").is_err());
        assert!(validate_brush("x","#GG0000","#000000").is_err());
        let brush = validate_brush(" ","#12aB34","#560078").unwrap();
        assert_eq!(brush.ch,' '); assert_eq!(brush.foreground,Rgb([18,171,52]));
        assert_eq!(brush.background,Rgb([86,0,120]));
    }
    fn mouse(x: i32,y: i32,button: MouseButton) -> MouseEventData {
        MouseEventData { x,y,button,modifier: KeyModifier::None }
    }
    #[test]
    fn mouse_drag_focus_loss_erase_and_palette_keep_strokes_grouped() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"").build().unwrap();
        let mut canvas = PaintCanvas::new();
        canvas.viewport = Size::new(40,15);
        canvas.on_mouse_event(&MouseEvent::Pressed(mouse(0,5,MouseButton::Left)));
        canvas.on_mouse_event(&MouseEvent::Drag(mouse(7,5,MouseButton::Left)));
        canvas.on_lose_focus();
        for x in 0..8 { assert_eq!(canvas.document.cell(x,0).unwrap().ch,'#'); }
        assert!(canvas.document.undo());
        for x in 0..8 { assert_eq!(canvas.document.cell(x,0).unwrap().ch,' '); }
        assert!(canvas.document.redo());
        canvas.on_mouse_event(&MouseEvent::Pressed(mouse(3,5,MouseButton::Right)));
        canvas.on_mouse_event(&MouseEvent::Released(mouse(3,5,MouseButton::Right)));
        assert_eq!(canvas.document.cell(3,0),Some(canvas.document.blank()));
        canvas.on_mouse_event(&MouseEvent::Pressed(mouse(21,2,MouseButton::Right)));
        assert_eq!(canvas.brush.background,Rgb([255,255,255]));
        assert_eq!(canvas.document.cell(0,0).unwrap().background,Rgb([0,0,0]));
    }
    #[test]
    fn cursor_scroll_resize_and_busy_state_preserve_document() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"").build().unwrap();
        let mut canvas = PaintCanvas::new();
        canvas.viewport = Size::new(10,8);
        canvas.input(Key::None,'@'); canvas.input(key!("Space").into(),' ');
        for _ in 0..79 { canvas.input(key!("Right").into(),'\0'); }
        assert_eq!(canvas.cursor,(79,0)); assert_eq!(canvas.offset.0,70);
        for _ in 0..23 { canvas.input(key!("Down").into(),'\0'); }
        assert_eq!(canvas.offset.1,22);
        canvas.input(key!("Enter").into(),'\0');
        canvas.frozen = true;
        canvas.input(key!("Delete").into(),'\0');
        assert_eq!(canvas.document.cell(79,23).unwrap().ch,'@');
        canvas.on_resize(Size::new(10,8),Size::new(1,1));
        assert_eq!(canvas.document.width(),80); assert_eq!(canvas.document.height(),24);
        assert!(canvas.point(0,5).is_none());
        canvas.on_resize(Size::new(1,1),Size::new(100,50));
        assert_eq!(canvas.offset,(0,0));
        assert_eq!(canvas.document.cell(0,0).unwrap().ch,'@');
    }
    #[Window(events = WindowEvents)]
    struct InputProbe { canvas: Handle<PaintCanvas>, observed: Arc<AtomicBool> }
    impl WindowEvents for InputProbe {
        fn on_cancel(&mut self) -> ActionRequest {
            let canvas = self.control(self.canvas).unwrap();
            assert_eq!(canvas.document.cell(0,0).unwrap().ch,'@');
            assert_eq!(canvas.document.cell(1,0).unwrap().ch,'@');
            assert_eq!(canvas.document.cell(2,1).unwrap().ch,'@');
            assert_eq!(canvas.document.cell(7,1).unwrap().ch,'@');
            assert_eq!(canvas.document.width(),80);
            self.observed.store(true,Ordering::Relaxed); ActionRequest::Allow
        }
    }
    #[test]
    fn appcui_delivers_mouse_and_keyboard_after_actual_layout() {
        let _lock = UI_LOCK.lock().unwrap();
        let script = "Paint.Enable(false)\nKey.TypeText('@')\nKey.Pressed(Space)\nKey.Pressed(Right)\nKey.Pressed(Enter)\nMouse.Drag(3,8,8,8)\nKey.Pressed(Escape)";
        let mut app = App::debug(80,24,script).build().unwrap();
        let observed = Arc::new(AtomicBool::new(false));
        let mut window = InputProbe { base: Window::new("Paint input",layout!("l:0,t:1,w:36,h:16"),window::Flags::Sizeable),canvas: Handle::None,observed: observed.clone() };
        window.canvas = window.add(PaintCanvas::new()); window.base.request_focus_for_control(window.canvas);
        app.add_window(window); app.run(); assert!(observed.load(Ordering::Relaxed));
    }
    #[test]
    fn hidden_guard_defers_prompt_and_default_cancel_keeps_artwork() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"Key.Pressed(Escape)").build().unwrap();
        let mut window = PaintWindow::new(None).unwrap();
        let handle = window.canvas;
        window.control_mut(handle).unwrap().stamp(false);
        window.request_guard(); window.set_visible(false);
        assert!(matches!(TimerEvents::on_update(&mut window,0),EventProcessStatus::Ignored));
        assert!(window.take_guard_result().is_none());
        window.set_visible(true); window.tick();
        assert_eq!(window.take_guard_result(),Some(false)); assert!(!window.is_closing());
        assert_eq!(window.control(handle).unwrap().document.cell(0,0).unwrap().ch,'#');
    }
    #[test]
    fn a_save_must_complete_before_guard_approval() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"").build().unwrap();
        let dir = TestDir::new();
        let mut window = PaintWindow::new(None).unwrap();
        let handle = window.canvas;
        window.control_mut(handle).unwrap().stamp(false);
        window.path = Some(dir.path().join("work.asciipaint"));
        window.request_guard();
        window.pending.as_mut().unwrap().approved = true;
        assert!(window.save(false));
        window.process_pending(); assert!(window.guard_result.is_none());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while window.worker.is_busy() && std::time::Instant::now() < deadline {
            // Poll on the UI thread without sleeping in production callbacks.
            window.tick(); std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(window.take_guard_result(),Some(true));
        assert!(!window.control(handle).unwrap().document.is_dirty());
        assert!(dir.path().join("work.asciipaint").is_file());
    }
    #[test]
    fn unsaved_dialog_discard_and_cancel_have_distinct_results() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"Key.Pressed(Alt+D)\nKey.Pressed(Escape)").build().unwrap();
        assert_eq!(UnsavedDialog::new().show(),Some(UnsavedChoice::Discard));
        assert_eq!(UnsavedDialog::new().show(),None);
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::test_support::{TestDir,UI_LOCK};
    use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};

    #[Desktop(events = DesktopEvents, overwrite = OnResize)]
    struct ModalTimerProbe { paint: Handle<PaintWindow>, intent: Intent, observed: Arc<AtomicUsize> }
    impl DesktopEvents for ModalTimerProbe {
        fn on_start(&mut self) {
            let other = self.add_window(Window::new("Other application",layout!("a:c,w:20,h:8"),window::Flags::None));
            let mut paint = PaintWindow::new(None).unwrap();
            let canvas = paint.canvas;
            paint.control_mut(canvas).unwrap().stamp(false);
            let guarding = matches!(self.intent,Intent::Guard);
            paint.request(self.intent.clone());
            self.paint = self.add_window(paint);
            Window::update_control(self.paint,|paint| { TimerEvents::on_update(paint,0); });
            let handle = self.paint;
            let paint = self.window_mut(handle).unwrap();
            assert!(!paint.is_closing());
            assert!(paint.pending.is_none());
            assert_eq!(paint.take_guard_result(),guarding.then_some(false));
            assert!(paint.control(canvas).unwrap().document.is_dirty());
            assert_eq!(paint.control(canvas).unwrap().document.cell(0,0).unwrap().ch,'#');
            assert!(self.windowt(other).is_some());
            self.observed.fetch_or(4,Ordering::Relaxed);
        }
    }
    impl OnResize for ModalTimerProbe {
        fn on_resize(&mut self,_: Size,size: Size) {
            if !matches!(size.width,78 | 79) { return; }
            let approved = size.width == 78;
            let handle = self.paint;
            let paint = self.window_mut(handle).unwrap();
            assert_eq!(paint.pending.as_ref().unwrap().approved,approved);
            // The debug backend has no real timer events. Inject one while the
            // scripted resize is being dispatched inside the actual modal loop.
            let status = TimerEvents::on_update(paint,1);
            assert!(!paint.is_closing(),"nested timer closed Paint while its picker was open");
            assert!(paint.guard_result.is_none(),"nested timer resolved the desktop guard");
            assert_eq!(paint.pending.as_ref().map(|pending| pending.approved),Some(approved),"nested timer consumed the pending action");
            assert!(!paint.worker.is_busy());
            assert!(matches!(status,EventProcessStatus::Ignored));
            self.observed.fetch_or(if approved { 2 } else { 1 },Ordering::Relaxed);
        }
    }
    fn modal_timer_cancel_preserves_document(intent: Intent) {
        let observed = Arc::new(AtomicUsize::new(0));
        let desktop = ModalTimerProbe { base: Desktop::new(),paint: Handle::None,intent,observed: observed.clone() };
        let script = "Paint.Enable(false)\nResize(79,24)\nKey.Pressed(Alt+S)\nResize(78,24)\nKey.Pressed(Escape)";
        App::debug(80,24,script).desktop(desktop).build().unwrap().run();
        assert_eq!(observed.load(Ordering::Relaxed),7,"both modals must reject the nested timer and cancel cleanly");
    }
    #[test]
    fn guard_timer_reentry_during_unsaved_and_save_modals_preserves_dirty_document() {
        let _lock = UI_LOCK.lock().unwrap();
        modal_timer_cancel_preserves_document(Intent::Guard);
    }
    #[test]
    fn close_timer_reentry_during_unsaved_and_save_modals_preserves_dirty_document() {
        let _lock = UI_LOCK.lock().unwrap();
        modal_timer_cancel_preserves_document(Intent::Close);
    }
    #[test]
    fn failed_save_cancels_guard_and_keeps_artwork_dirty() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"Key.Pressed(Escape)").build().unwrap();
        let dir = TestDir::new(); let mut window = PaintWindow::new(None).unwrap();
        let handle = window.canvas;
        window.control_mut(handle).unwrap().stamp(false);
        let path = dir.path().join("missing").join("work.asciipaint");
        window.path = Some(path.clone()); window.request_guard();
        window.pending.as_mut().unwrap().approved = true;
        assert!(window.save(false));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while window.worker.is_busy() && std::time::Instant::now() < deadline {
            window.tick(); std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(window.take_guard_result(),Some(false));
        assert!(window.control(handle).unwrap().document.is_dirty());
        assert_eq!(window.control(handle).unwrap().document.cell(0,0).unwrap().ch,'#');
        assert_eq!(window.path,Some(path)); assert!(!window.is_closing());
    }
    #[test]
    fn new_open_and_close_cancel_preserve_the_same_document() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"Key.Pressed(Escape)\nKey.Pressed(Escape)\nKey.Pressed(Escape)").build().unwrap();
        let mut window = PaintWindow::new(None).unwrap(); let handle = window.canvas;
        window.control_mut(handle).unwrap().stamp(false);
        window.request(Intent::New); window.tick();
        window.open(PathBuf::from("missing.asciipaint")); window.tick();
        window.request_close(); window.tick();
        assert!(window.pending.is_none()); assert!(!window.is_closing());
        assert_eq!(window.control(handle).unwrap().document.cell(0,0).unwrap().ch,'#');
        assert!(window.control(handle).unwrap().document.is_dirty());
    }
    #[test]
    fn canvas_paints_original_rgb_after_theme_changes_and_on_tiny_surfaces() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"").build().unwrap();
        let mut canvas = PaintCanvas::new(); canvas.viewport = Size::new(8,8);
        canvas.brush = validate_brush("@","#123456","#789ABC").unwrap(); canvas.stamp(false);
        for mut theme in [Theme::new(Themes::Default),Theme::new(Themes::Light)] {
            theme.window.normal.background = Color::Red;
            let mut surface = Surface::new(8,8); canvas.on_paint(&mut surface,&theme);
            let cell = surface.char(0,5).unwrap();
            assert_eq!(cell.code,'@'); assert_eq!(cell.foreground,Color::RGB(0x12,0x34,0x56));
            assert_eq!(cell.background,Color::RGB(0x78,0x9A,0xBC));
            canvas.on_resize(Size::new(8,8),Size::new(1,1));
            canvas.on_paint(&mut Surface::new(1,1),&theme);
            canvas.on_resize(Size::new(1,1),Size::new(8,8));
        }
    }
}

#[cfg(test)]
mod followup_tests {
    use super::*;
    use crate::test_support::UI_LOCK;
    #[test]
    fn desktop_hold_supersedes_paint_close_while_another_document_is_guarded() {
        let _lock = UI_LOCK.lock().unwrap(); let _app = App::debug(80,24,"Key.Pressed(Alt+D)\nPaint.Enable(false)").build().unwrap();
        let mut window = PaintWindow::new(None).unwrap(); let handle = window.canvas; window.control_mut(handle).unwrap().stamp(false);
        window.request_close(); window.set_guard_hold(true); window.tick(); assert!(!window.is_closing()); assert!(window.pending.is_none());
        window.set_guard_hold(false); window.tick(); assert!(!window.is_closing()); assert!(window.control(handle).unwrap().document.is_dirty());
    }
    #[test]
    fn short_canvas_footer_clears_toolbar_status_and_palette_fragments() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"").build().unwrap();
        let mut canvas = PaintCanvas::new(); canvas.cursor = (7,3);
        let theme = Theme::new(Themes::Default);
        let expected = format!("[Size 80x24]{}Cursor 8,4"," ".repeat(38));
        let mut rows = Vec::new();
        for height in 1..=5 {
            canvas.viewport = Size::new(60,height);
            let mut surface = Surface::new(60,height); canvas.on_paint(&mut surface,&theme);
            rows.push((0..60).map(|x| surface.char(x,height as i32-1).unwrap().code).collect::<String>());
        }
        assert_eq!(rows,vec![expected;5]);
        for height in 1..=5 {
            canvas.viewport = Size::new(60,height);
            let mut surface = Surface::new(60,height); canvas.on_paint(&mut surface,&theme);
            for x in 12..50 {
                let cell = surface.char(x,height as i32-1).unwrap();
                assert_eq!(cell.foreground,theme.window.normal.foreground);
                assert_eq!(cell.background,theme.window.normal.background);
            }
        }
    }
    #[test]
    fn footer_click_and_shortcuts_queue_canvas_actions_without_painting() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"").build().unwrap();
        let mut canvas = PaintCanvas::new(); canvas.viewport = Size::new(60,12);
        let data = MouseEventData { x: 2,y: 11,button: MouseButton::Left,modifier: KeyModifier::None };
        canvas.on_mouse_event(&MouseEvent::Pressed(data));
        assert_eq!(canvas.action.take(),Some(PaintAction::Resize)); assert!(!canvas.document.is_dirty());
        canvas.input(key!("Ctrl+I").into(),'\0'); assert_eq!(canvas.action.take(),Some(PaintAction::Image));
        canvas.input(key!("Ctrl+R").into(),'\0'); assert_eq!(canvas.action.take(),Some(PaintAction::Resize));
        for width in 0..35 {
            canvas.on_resize(canvas.viewport,Size::new(width,8));
            let (left,x,right) = canvas.footer_labels();
            assert!(left.len() <= x); assert!(x+right.len() <= width as usize);
            canvas.on_paint(&mut Surface::new(width,8),&Theme::new(Themes::Light));
            assert!(canvas.point(0,7).is_none());
        }
    }
    #[test]
    fn resize_dialog_keeps_artwork_and_undo_restores_cropped_edges() {
        let _lock = UI_LOCK.lock().unwrap();
        let script = "Paint.Enable(false)\nKey.Pressed(Ctrl+A)\nKey.TypeText('5')\nKey.Pressed(Tab)\nKey.Pressed(Ctrl+A)\nKey.TypeText('3')\nKey.Pressed(Enter)";
        let _app = App::debug(80,24,script).build().unwrap();
        let mut window = PaintWindow::new(None).unwrap(); let handle = window.canvas;
        let canvas = window.control_mut(handle).unwrap();
        canvas.viewport = Size::new(40,15); canvas.cursor = (79,23); canvas.stamp(false); canvas.document.mark_saved();
        canvas.action = Some(PaintAction::Resize); window.tick();
        let canvas = window.control(handle).unwrap();
        assert_eq!((canvas.document.width(),canvas.document.height()),(5,3));
        assert_eq!(canvas.cursor,(4,2)); assert!(canvas.document.is_dirty());
        window.action(PaintAction::Undo);
        let canvas = window.control(handle).unwrap();
        assert_eq!((canvas.document.width(),canvas.document.height()),(80,24));
        assert_eq!(canvas.document.cell(79,23).unwrap().ch,'#'); assert!(!canvas.document.is_dirty());
        window.action(PaintAction::Redo);
        assert_eq!(window.control(handle).unwrap().cursor,(4,2));
    }
    #[test]
    fn conversion_dialog_validates_limits_and_monochrome_option() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"Paint.Enable(false)\nKey.Pressed(Alt+P)\nKey.Pressed(Alt+V)\nPaint.Enable(false)").build().unwrap();
        let mut dialog = ImageConversionDialog::new(200,80);
        let width = dialog.width; dialog.control_mut(width).unwrap().set_text("201");
        dialog.submit(); assert!(!dialog.control(dialog.status).unwrap().caption().is_empty());
        dialog.control_mut(width).unwrap().set_text("200");
        let options = dialog.show().unwrap();
        assert_eq!((options.width,options.height),(200,80)); assert!(!options.color);
    }
    #[test]
    fn image_conversion_replaces_only_on_success_and_stays_untitled_dirty() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = crate::test_support::TestDir::new();
        let path = dir.path().join("image 文.png");
        ::image::RgbaImage::from_pixel(8,8,::image::Rgba([255,0,0,255])).save(&path).unwrap();
        let _app = App::debug(80,24,"Paint.Enable(false)\nKey.Pressed(Enter)").build().unwrap();
        let mut window = PaintWindow::new(None).unwrap(); let handle = window.canvas;
        window.path = Some(dir.path().join("previous.asciipaint"));
        window.request(Intent::Image(path)); window.tick();
        assert!(window.worker.is_busy()); assert!(window.control(handle).unwrap().frozen);
        assert_eq!(window.control(handle).unwrap().document.width(),80);
        let deadline = std::time::Instant::now()+Duration::from_secs(5);
        while window.worker.is_busy() {
            assert!(std::time::Instant::now()<deadline); window.tick(); std::thread::sleep(Duration::from_millis(1));
        }
        let canvas = window.control(handle).unwrap();
        assert_eq!((canvas.document.width(),canvas.document.height()),(48,24));
        assert!(canvas.document.is_dirty()); assert!(window.path.is_none());
        assert_eq!(canvas.document.cell(0,0).unwrap().foreground,Rgb([255,0,0]));
    }
    #[test]
    fn canceled_conversion_keeps_saved_path_and_dirty_artwork() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"Key.Pressed(Escape)").build().unwrap();
        let mut window = PaintWindow::new(None).unwrap(); let handle = window.canvas;
        window.control_mut(handle).unwrap().stamp(false);
        window.path = Some(PathBuf::from("previous.asciipaint"));
        window.request(Intent::Image(PathBuf::from("unused.png"))); window.tick();
        assert!(window.pending.is_none()); assert!(!window.worker.is_busy());
        assert_eq!(window.path,Some(PathBuf::from("previous.asciipaint")));
        assert_eq!(window.control(handle).unwrap().document.cell(0,0).unwrap().ch,'#');
        assert!(window.control(handle).unwrap().document.is_dirty());
    }
    #[test]
    fn failed_image_conversion_preserves_the_existing_document_and_path() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"Paint.Enable(false)\nKey.Pressed(Alt+D)\nPaint.Enable(false)\nPaint.Enable(false)\nKey.Pressed(Enter)\nKey.Pressed(Escape)").build().unwrap();
        let dir = crate::test_support::TestDir::new();
        let mut window = PaintWindow::new(None).unwrap(); let handle = window.canvas;
        window.control_mut(handle).unwrap().stamp(false);
        let path = dir.path().join("previous.asciipaint"); window.path = Some(path.clone());
        window.request(Intent::Image(dir.path().join("missing.png"))); window.tick();
        assert!(window.worker.is_busy(),"conversion must actually run before checking its error path");
        let deadline = std::time::Instant::now()+Duration::from_secs(5);
        while window.worker.is_busy() {
            assert!(std::time::Instant::now()<deadline); window.tick(); std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(window.path,Some(path)); assert!(!window.is_closing());
        assert_eq!(window.control(handle).unwrap().document.cell(0,0).unwrap().ch,'#');
        assert!(window.control(handle).unwrap().document.is_dirty());
    }
    #[test]
    fn bottom_status_row_is_reserved_from_the_drawing_area() {
        let _lock = UI_LOCK.lock().unwrap();
        let _app = App::debug(80,24,"").build().unwrap();
        let mut canvas = PaintCanvas::new(); canvas.viewport = Size::new(60,12); canvas.cursor = (7,3);
        assert!(canvas.point(0,11).is_none(),"footer must not accept drawing input");
        let mut surface = Surface::new(60,12); canvas.on_paint(&mut surface,&Theme::new(Themes::Default));
        let row: String = (0..60).map(|x| surface.char(x,11).unwrap().code).collect();
        assert!(row.starts_with("[Size 80x24]")); assert!(row.ends_with("Cursor 8,4"));
    }
}

#[cfg(test)]
mod close_tests {
    use super::*;
    use crate::test_support::{TestDir,UI_LOCK};
    use std::sync::{Arc,atomic::{AtomicBool,Ordering}};

    #[derive(Clone,Copy,PartialEq)]
    enum CloseCase { Clean, Discard, Cancel, Save }
    #[Desktop(events = DesktopEvents, overwrite = OnResize)]
    struct CloseProbe {
        paint: Handle<PaintWindow>, other: Handle<Window>, case: CloseCase,
        path: PathBuf, observed: Arc<AtomicBool>, phases: u8,
    }
    impl DesktopEvents for CloseProbe {
        fn on_start(&mut self) {
            self.other = self.add_window(Window::new("Other application",layout!("a:c,w:20,h:8"),window::Flags::None));
            let mut paint = PaintWindow::new(None).unwrap();
            paint.set_size(60,20); paint.set_position(0,1);
            if self.case != CloseCase::Clean {
                let canvas = paint.canvas; paint.control_mut(canvas).unwrap().stamp(false);
            }
            if self.case == CloseCase::Save { paint.path = Some(self.path.clone()); }
            self.paint = self.add_window(paint);
        }
    }
    impl OnResize for CloseProbe {
        fn on_resize(&mut self,_: Size,size: Size) {
            let phase = match size.width { 79 => 1,78 => 2,77 => 4,_ => return };
            if self.phases & phase != 0 { return; }
            self.phases |= phase;
            let handle = self.paint;
            match size.width {
                79 => {
                    if let Some(paint) = self.window_mut(handle) {
                        assert!(paint.is_closing() || matches!(paint.pending.as_ref().map(|pending| &pending.intent),Some(Intent::Close)),"click must reach the titlebar X");
                        // Inject the timer update so the test does not depend on timing.
                        TimerEvents::on_update(paint,0);
                    } else { assert!(self.case != CloseCase::Cancel); }
                }
                78 => {
                    if let Some(paint) = self.window_mut(handle) {
                        let deadline = std::time::Instant::now()+Duration::from_secs(5);
                        while paint.worker.is_busy() {
                            assert!(std::time::Instant::now()<deadline,"save did not complete");
                            TimerEvents::on_update(paint,0);
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                }
                77 => {
                    if self.case == CloseCase::Cancel {
                        let paint = self.windowt(handle).unwrap();
                        assert!(!paint.is_closing());
                        let canvas = paint.control(paint.canvas).unwrap();
                        assert!(canvas.document.is_dirty()); assert_eq!(canvas.document.cell(0,0).unwrap().ch,'#');
                    } else {
                        assert!(self.windowt(handle).is_none(),"approved X close must remove the registered Paint window");
                    }
                    assert!(self.windowt(self.other).is_some(),"closing Paint must leave other windows running");
                    self.observed.store(true,Ordering::Relaxed);
                }
                _ => {}
            }
        }
    }
    #[test]
    fn titlebar_x_removes_clean_discarded_and_saved_windows_and_cancel_keeps_artwork() {
        let _lock = UI_LOCK.lock().unwrap();
        for case in [CloseCase::Clean,CloseCase::Discard,CloseCase::Cancel,CloseCase::Save] {
            let dir = TestDir::new(); let path = dir.path().join("saved.asciipaint");
            let observed = Arc::new(AtomicBool::new(false));
            let desktop = CloseProbe { base: Desktop::new(),paint: Handle::None,other: Handle::None,case,path: path.clone(),observed: observed.clone(),phases: 0 };
            let choice = match case { CloseCase::Clean => "",CloseCase::Discard => "Key.Pressed(Alt+D)\n",CloseCase::Cancel => "Key.Pressed(Escape)\n",CloseCase::Save => "Key.Pressed(Alt+S)\n" };
            let script = format!("Paint.Enable(false)\nMouse.Click(57,1,left)\nResize(79,24)\n{choice}Paint.Enable(false)\nResize(78,24)\nResize(77,24)");
            App::debug(80,24,&script).desktop(desktop).build().unwrap().run();
            assert!(observed.load(Ordering::Relaxed),"registered-window assertions must run");
            if case == CloseCase::Save {
                let saved: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
                assert_eq!(saved["cells"][0]["ch"].as_str(),Some("#"));
            }
        }
    }
}
