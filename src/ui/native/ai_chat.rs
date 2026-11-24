use crate::config::ai::{AiPreferences,AiProvider};
use crate::runtime::ai::{ChatCommand,ChatService,ChatSnapshot,Role,MessageStatus};
use appcui::prelude::*;
use appcui::dialogs::{Location,OpenFileDialogFlags};
use std::time::Duration;

fn wrap_lines(text: &str,width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for line in text.split('\n') {
        let glyphs = tattoy_wezterm_term::Line::from_text(line,&tattoy_wezterm_term::CellAttributes::default(),0,None);
        let (mut row,mut column) = (String::new(),0);
        for cell in glyphs.visible_cells() {
            let size = cell.width();
            if column+size > width && !row.is_empty() { lines.push(std::mem::take(&mut row)); column = 0; }
            if size <= width { row.push_str(cell.str()); column += size; }
        }
        lines.push(row);
    }
    lines
}
#[CustomControl(overwrite = OnPaint+OnKeyPressed+OnMouseEvent+OnResize)]
struct Transcript { text: String, lines: Vec<String>, first: usize, viewport: Size }
impl Transcript {
    fn new() -> Self { Self { base: ControlBase::new(layout!("l:0,t:4,r:0,b:7"),true),text: String::new(),lines: vec![],first: 0,viewport: Size::new(0,0) } }
    fn bottom(&self) -> usize { self.lines.len().saturating_sub(self.viewport.height as usize) }
    fn update(&mut self,text: String,reset: bool) {
        let follow = reset || self.first >= self.bottom(); self.text = text;
        self.lines = wrap_lines(&self.text,self.viewport.width.max(1) as usize);
        self.first = if follow { self.bottom() } else { self.first.min(self.bottom()) };
    }
    fn scroll(&mut self,delta: i32) { self.first = self.first.saturating_add_signed(delta as isize).min(self.bottom()); }
}
impl OnPaint for Transcript {
    fn on_paint(&self,surface: &mut Surface,theme: &Theme) {
        surface.clear(Character::with_attributes(' ',theme.editor.normal));
        for (y,text) in self.lines.iter().skip(self.first).take(self.viewport.height as usize).enumerate() {
            let line = tattoy_wezterm_term::Line::from_text(text,&tattoy_wezterm_term::CellAttributes::default(),0,None);
            let mut x = 0;
            for cell in line.visible_cells() {
                if x+cell.width() > self.viewport.width as usize { break; }
                surface.write_grapheme(x as i32,y as i32,cell.str(),cell.width(),theme.text.normal); x += cell.width();
            }
        }
    }
}
impl OnResize for Transcript {
    fn on_resize(&mut self,_: Size,size: Size) { self.viewport = size; self.update(self.text.clone(),false); }
}
impl OnKeyPressed for Transcript {
    fn on_key_pressed(&mut self,key: Key,_: char) -> EventProcessStatus {
        match key.code {
            KeyCode::Up => self.scroll(-1),KeyCode::Down => self.scroll(1),
            KeyCode::PageUp => self.scroll(-(self.viewport.height as i32).max(1)),KeyCode::PageDown => self.scroll((self.viewport.height as i32).max(1)),
            KeyCode::Home => self.first = 0,KeyCode::End => self.first = self.bottom(),_ => return EventProcessStatus::Ignored,
        }
        EventProcessStatus::Processed
    }
}
impl OnMouseEvent for Transcript {
    fn on_mouse_event(&mut self,event: &MouseEvent) -> EventProcessStatus {
        match event { MouseEvent::Wheel(MouseWheelDirection::Up) => self.scroll(-3),MouseEvent::Wheel(MouseWheelDirection::Down) => self.scroll(3),
            MouseEvent::Pressed(_) => { self.request_focus(); },_ => return EventProcessStatus::Ignored }
        EventProcessStatus::Processed
    }
}
#[ModalWindow(response = usize,events = ButtonEvents+WindowEvents)]
struct Choice { list: Handle<ListBox>, choose: Handle<Button> }
impl Choice {
    fn new(title: &str,labels: &[String]) -> Self {
        let mut dialog = Self { base: ModalWindow::new(title,layout!("a:c,w:62,h:16"),window::Flags::Sizeable),list: Handle::None,choose: Handle::None };
        let mut list = ListBox::new(layout!("l:1,r:1,t:0,b:3"),listbox::Flags::ScrollBars);
        for label in labels { list.add(label); }
        dialog.list = dialog.add(list); dialog.choose = dialog.add(Button::new("&Choose",layout!("l:1,b:0,w:12")));
        dialog.add(Button::new("&Cancel",layout!("r:1,b:0,w:12"))); dialog.base.request_focus_for_control(dialog.list); dialog
    }
    fn select(&mut self) { if let Some(index) = self.control(self.list).map(ListBox::index) && index != usize::MAX { self.exit_with(index); } }
}
impl ButtonEvents for Choice {
    fn on_pressed(&mut self,handle: Handle<Button>) -> EventProcessStatus { if handle == self.choose { self.select(); } else { self.exit(); } EventProcessStatus::Processed }
}
impl WindowEvents for Choice { fn on_accept(&mut self) { self.select(); } }

#[derive(Clone,Copy,PartialEq)]
enum Pending { Close, Guard }
#[derive(Clone)]
enum Navigation { New, Select, Delete }
#[Window(events = [TimerEvents,WindowEvents,ButtonEvents,ComboBoxEvents,TextFieldEvents,ListBoxEvents])]
pub(crate) struct AiChatWindow {
    service: ChatService, snapshot: Option<ChatSnapshot>, preferences: AiPreferences, local_configuration: bool,
    transcript: Handle<Transcript>, composer: Handle<TextArea>, provider: Handle<ComboBox>, model: Handle<TextField>,
    sidebar: Handle<ListBox>, status: Handle<Label>, small: Handle<Label>,
    chats: Handle<Button>, new: Handle<Button>, delete: Handle<Button>, settings: Handle<Button>,
    send: Handle<Button>, stop: Handle<Button>, retry: Handle<Button>, attach: Handle<Button>, files: Handle<Button>, copy: Handle<Button>, route: Handle<Button>,
    last_draft: String, await_send: bool, request_after: u64, navigation: Option<Navigation>, navigation_token: u64, pending_route: Option<(AiProvider,String)>, flush_error: Option<String>,
    closing: bool, updating: bool, held: bool, route_edited: bool, pending: Option<Pending>, guard_result: Option<bool>, flush_wait: bool, flush_after: u64,
}
impl AiChatWindow {
    pub fn new() -> anyhow::Result<Self> { Self::with_service(ChatService::open()?,AiPreferences::default()) }
    pub(crate) fn with_service(service: ChatService,preferences: AiPreferences) -> anyhow::Result<Self> {
        let mut window = Self { base: Window::new("AI Chat",layout!("a:c,w:92%,h:90%"),window::Flags::Sizeable),service,snapshot: None,preferences,local_configuration: false,
            transcript: Handle::None,composer: Handle::None,provider: Handle::None,model: Handle::None,sidebar: Handle::None,status: Handle::None,small: Handle::None,
            chats: Handle::None,new: Handle::None,delete: Handle::None,settings: Handle::None,send: Handle::None,stop: Handle::None,retry: Handle::None,
            attach: Handle::None,files: Handle::None,copy: Handle::None,route: Handle::None,
            last_draft: String::new(),await_send: false,request_after: 0,navigation: None,navigation_token: 0,pending_route: None,flush_error: None,
            closing: false,updating: false,held: false,route_edited: false,pending: None,guard_result: None,flush_wait: false,flush_after: 0 };
        window.chats = window.add(Button::new("&Chats",layout!("l:0,t:0,w:10,h:1")));
        let mut new = Button::new("New",layout!("l:11,t:0,w:10,h:1")); new.set_hotkey(key!("Ctrl+N")); window.new = window.add(new);
        window.delete = window.add(Button::new("&Delete",layout!("l:22,t:0,w:10,h:1")));
        window.settings = window.add(Button::new("Settin&gs",layout!("r:0,t:0,w:13,h:1")));
        let mut provider = ComboBox::new(layout!("l:0,t:2,w:13,h:1"),combobox::Flags::None);
        for value in AiProvider::ALL { provider.add(value.label()); }
        window.provider = window.add(provider);
        window.model = window.add(TextField::new("",layout!("l:15,t:2,r:11,h:1"),textfield::Flags::None));
        window.route = window.add(Button::new("Use model",layout!("r:0,t:2,w:10,h:1")));
        window.sidebar = window.add(ListBox::new(layout!("l:0,t:4,w:22,b:7"),listbox::Flags::ScrollBars));
        window.transcript = window.add(Transcript::new());
        window.status = window.add(Label::new("Loading chats...",layout!("l:0,r:0,b:5,h:2")));
        window.composer = window.add(TextArea::new("\n",layout!("l:0,r:0,b:2,h:3"),textarea::Flags::ScrollBars));
        window.send = window.add(Button::new("&Send",layout!("l:0,b:0,w:9,h:1")));
        window.stop = window.add(Button::new("Stop",layout!("l:10,b:0,w:9,h:1"))); let stop = window.stop; if let Some(button) = window.control_mut(stop) { button.set_hotkey(key!("Alt+X")); }
        window.retry = window.add(Button::new("&Retry",layout!("l:20,b:0,w:9,h:1")));
        window.attach = window.add(Button::new("&Attach",layout!("l:30,b:0,w:10,h:1")));
        window.files = window.add(Button::new("&Files",layout!("l:41,b:0,w:9,h:1")));
        window.copy = window.add(Button::new("Copy",layout!("l:51,b:0,w:9,h:1")));
        window.small = window.add(Label::new("Resize AI Chat to at least 62 x 16",layout!("l:0,t:0,r:0,h:2")));
        window.timer().ok_or_else(|| anyhow::anyhow!("AI Chat timer unavailable"))?.start(Duration::from_millis(50));
        window.base.request_focus_for_control(window.composer); window.layout(); window.refresh_controls(); Ok(window)
    }
    pub fn is_closing(&self) -> bool { self.closing }
    pub fn configure(&mut self,ai: AiPreferences) { self.local_configuration = true; self.preferences = ai.clone(); self.command(ChatCommand::Configure(ai)); }
    pub fn constrain_to(&mut self,available: Size) { self.set_size_bounds(1,1,available.width.clamp(1,u16::MAX as u32) as u16,available.height.clamp(1,u16::MAX as u32) as u16); }
    pub fn set_guard_hold(&mut self,held: bool) { if held { self.capture_draft(); } self.held = held; self.refresh_controls(); }
    pub fn request_close(&mut self) { if !self.held && self.pending.is_none() { self.start_flush(Pending::Close); } }
    pub fn request_guard(&mut self) { self.start_flush(Pending::Guard); }
    pub fn take_guard_result(&mut self) -> Option<bool> { self.guard_result.take() }
    pub fn shutdown(&mut self) { self.closing = true; self.close(); }
    fn command(&mut self,command: ChatCommand) -> bool {
        let command = match command {
            ChatCommand::Navigate { .. } | ChatCommand::Configure(_) | ChatCommand::New | ChatCommand::Select(_) | ChatCommand::Stop | ChatCommand::Flush | ChatCommand::DiscardUnsaved => command,
            command => {
                let Some(chat) = self.snapshot.as_ref().and_then(|s| s.current.as_ref()).map(|c| c.id.clone()) else { return false; };
                ChatCommand::Scoped { chat,command: Box::new(command) }
            }
        };
        match self.service.submit(command) { Ok(()) => true,Err(error) => { let status = self.status; if let Some(label) = self.control_mut(status) { label.set_caption(&error); } false } }
    }
    fn start_flush(&mut self,pending: Pending) {
        if self.closing { return; }
        self.pending = Some(pending); self.flush_error = None; self.flush_wait = false;
        if !self.capture_draft() {
            self.flush_error = Some("The current draft could not be saved. Correct it after Cancel, retry, or explicitly discard it.".into());
        } else {
            self.flush_after = self.snapshot.as_ref().map_or(0,|s| s.flush_revision);
            if self.command(ChatCommand::Flush) { self.flush_wait = true; }
            else { self.flush_error = Some("AI Chat could not accept the save request.".into()); }
        }
        self.refresh_controls();
    }
    fn complete_pending(&mut self,approved: bool) {
        match self.pending.take() { Some(Pending::Close) if approved => self.shutdown(),Some(Pending::Guard) => self.guard_result = Some(approved),_ => {} }
        self.flush_wait = false; self.flush_error = None; self.refresh_controls();
    }
    fn layout(&mut self) {
        let size = self.client_size(); let tiny = size.width < 60 || size.height < 14; let wide = size.width >= 90;
        for handle in [self.chats,self.new,self.delete,self.settings,self.send,self.stop,self.retry,self.attach,self.files,self.copy,self.route] {
            if let Some(control) = self.control_mut(handle) { control.set_visible(!tiny); }
        }
        let h = self.provider; if let Some(c) = self.control_mut(h) { c.set_visible(!tiny); }
        let h = self.model; if let Some(c) = self.control_mut(h) { c.set_visible(!tiny); }
        let h = self.composer; if let Some(c) = self.control_mut(h) { c.set_visible(!tiny); }
        let h = self.status; if let Some(c) = self.control_mut(h) { c.set_visible(!tiny); }
        let h = self.sidebar; if let Some(c) = self.control_mut(h) { c.set_visible(!tiny && wide); }
        let h = self.transcript; if let Some(c) = self.control_mut(h) { c.set_visible(!tiny); c.set_position(if wide { 23 } else { 0 },4); c.set_size(size.width.saturating_sub(if wide {23} else {0}).max(1) as u16,size.height.saturating_sub(11).max(1) as u16); }
        let h = self.small; if let Some(c) = self.control_mut(h) { c.set_visible(tiny); }
    }
    fn refresh_controls(&mut self) {
        let ready = self.snapshot.as_ref().is_some_and(|s| !s.loading && s.writable) && !self.held && self.pending.is_none() && self.navigation.is_none();
        let current = self.snapshot.as_ref().is_some_and(|s| s.current.is_some()); let busy = self.snapshot.as_ref().is_some_and(|s| s.busy);
        for h in [self.new,self.delete,self.route,self.send,self.retry,self.attach,self.files] { let enabled = ready && (h == self.new || current) && (h == self.files || !busy); if let Some(c) = self.control_mut(h) { c.set_enabled(enabled); } }
        let h = self.stop; if let Some(c) = self.control_mut(h) { c.set_enabled(ready && busy); }
        let h = self.provider; if let Some(c) = self.control_mut(h) { c.set_enabled(ready && current && !busy); }
        let h = self.model; if let Some(c) = self.control_mut(h) { c.set_enabled(ready && current && !busy); }
        let h = self.composer; if let Some(c) = self.control_mut(h) { c.set_enabled(ready && current && !busy); }
        let enabled = !self.held && self.pending.is_none(); let h = self.settings; if let Some(c) = self.control_mut(h) { c.set_enabled(enabled); }
    }
    fn capture_draft(&mut self) -> bool {
        if self.navigation.is_some() { return false; }
        if self.await_send || !self.snapshot.as_ref().is_some_and(|s| s.current.is_some() && !s.loading) { return true; }
        let text = self.control(self.composer).map(|c| c.text().to_owned()).unwrap_or_default();
        if text != self.last_draft {
            if !self.command(ChatCommand::Draft(text.clone())) { return false; }
            self.last_draft = text;
        }
        true
    }
    fn navigate(&mut self,command: ChatCommand,navigation: Navigation) {
        if self.navigation.is_some() || !self.capture_draft() { return; }
        let command = match command {
            ChatCommand::Delete => { let Some(chat) = self.snapshot.as_ref().and_then(|s| s.current.as_ref()).map(|c| c.id.clone()) else { return; }; ChatCommand::Scoped { chat,command: Box::new(ChatCommand::Delete) } },
            command => command,
        };
        let token = self.navigation_token.wrapping_add(1).max(1);
        if self.command(ChatCommand::Navigate { token,command: Box::new(command) }) { self.navigation_token = token; self.navigation = Some(navigation); self.refresh_controls(); }
    }
    fn use_route(&mut self) -> bool {
        let provider = AiProvider::ALL[self.control(self.provider).and_then(ComboBox::index).unwrap_or(0).min(2) as usize];
        let model = self.control(self.model).map(|c| c.text().trim().to_owned()).unwrap_or_default();
        if model.len() > 256 || model.chars().any(char::is_control) { dialogs::error("Invalid model ID","Use at most 256 characters without control characters."); return false; }
        let sent = self.command(ChatCommand::Route { provider,model: model.clone() }); if sent { self.pending_route = Some((provider,model)); } sent
    }
    fn choose_chat(&mut self) {
        if !self.capture_draft() { return; }
        let Some(snapshot) = &self.snapshot else { return; }; let ids = snapshot.chats.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
        if let Some(index) = Choice::new("Saved chats",&snapshot.chats.iter().map(|c| c.title.clone()).collect::<Vec<_>>()).show() && let Some(id) = ids.get(index) { self.navigate(ChatCommand::Select(id.clone()),Navigation::Select); }
    }
    fn choose_files(&mut self) {
        let Some(chat) = self.snapshot.as_ref().and_then(|s| s.current.as_ref()) else { return; };
        let files = chat.attachments.clone();
        if files.is_empty() { dialogs::error("Attachments","Use Attach to select a file. Files are sent only when you press Send."); return; }
        let labels = files.iter().map(|a| format!("[{}] {} ({} bytes){}",if a.included { "included" } else { "excluded" },a.name,a.size,a.error.as_ref().map(|e| format!(" - {e}")).unwrap_or_default())).collect::<Vec<_>>();
        if let Some(index) = Choice::new("Files: choose to include / exclude",&labels).show() && let Some(file) = files.get(index) {
            let options = if file.message.is_none() { vec!["Toggle inclusion".into(),"Remove attachment".into()] } else { vec!["Toggle inclusion in future requests".into()] };
            match Choice::new(&file.name,&options).show() { Some(0) => { self.command(ChatCommand::ToggleAttachment(file.id.clone())); },Some(1) => { self.command(ChatCommand::RemoveAttachment(file.id.clone())); },_ => {} }
        }
    }
    fn refresh(&mut self,snapshot: ChatSnapshot) {
        self.updating = true;
        if self.navigation.is_some() && snapshot.navigation_revision == self.navigation_token { self.navigation = None; }
        if self.await_send && snapshot.request_revision > self.request_after && !snapshot.busy { self.await_send = false; }
        if self.pending_route.as_ref().is_some_and(|(provider,model)| snapshot.current.as_ref().is_some_and(|c| c.provider == *provider && c.model == *model)) { self.pending_route = None; self.route_edited = false; }
        let changed_chat = self.snapshot.as_ref().and_then(|s| s.current.as_ref()).map(|c| &c.id) != snapshot.current.as_ref().map(|c| &c.id);
        let summaries_changed = self.snapshot.as_ref().map(|s| s.chats.iter().map(|c| (&c.id,&c.title)).collect::<Vec<_>>()) != Some(snapshot.chats.iter().map(|c| (&c.id,&c.title)).collect());
        let sidebar = self.sidebar; if summaries_changed && let Some(control) = self.control_mut(sidebar) { control.clear(); for chat in &snapshot.chats { control.add(&chat.title); } }
        if changed_chat { self.pending_route = None; self.route_edited = false; }
        if let Some(chat) = &snapshot.current {
            if let Some(index) = snapshot.chats.iter().position(|c| c.id == chat.id) && let Some(list) = self.control_mut(sidebar) { list.set_index(index); }
            if changed_chat || (!self.route_edited && self.pending_route.is_none()) {
                let h = self.provider; if let Some(c) = self.control_mut(h) { c.set_index(AiProvider::ALL.iter().position(|p| *p == chat.provider).unwrap_or(0) as u32); }
                let h = self.model; if let Some(c) = self.control_mut(h) && c.text() != chat.model { c.set_text(&chat.model); }
            }
            let reset_composer = changed_chat || (chat.draft.is_empty() && self.snapshot.as_ref().and_then(|s| s.current.as_ref()).is_some_and(|old| old.messages.len() < chat.messages.len()));
            let h = self.composer;
            if reset_composer && let Some(c) = self.control_mut(h) && c.text() != chat.draft && !(chat.draft.is_empty() && c.text() == "\n") { c.set_text(if chat.draft.is_empty() { "\n" } else { &chat.draft }); }
            if reset_composer { self.last_draft = self.control(self.composer).map(|c| c.text().to_owned()).unwrap_or_default(); self.await_send = false; }
            if snapshot.error.is_some() && !snapshot.busy { self.await_send = false; }
            let mut text = String::new();
            for (index,message) in chat.messages.iter().enumerate() {
                text.push_str(&format!("{}{}\n{}\n",match message.role { Role::User => "You".to_owned(),Role::Assistant => format!("{} / {}",message.provider.label(),message.model) },if message.status == MessageStatus::Complete { String::new() } else { format!(" [{:?}]",message.status) },message.text));
                for file in chat.attachments.iter().filter(|a| a.message == Some(index)) { text.push_str(&format!("  File: {}{}\n",file.name,if file.included { "" } else { " (excluded)" })); }
                text.push('\n');
            }
            if text.is_empty() { text = "Start a conversation. Enter adds a line; Alt+S sends.\nSelected history and included files go to the provider shown above.\n".into(); }
            let h = self.transcript; if let Some(c) = self.control_mut(h) && (changed_chat || c.text != text) { c.update(text,changed_chat); }
            self.set_title(&format!("AI Chat — {}",chat.title.replace('&',"&&")));
        }
        let pending_files = snapshot.current.as_ref().map(|c| c.attachments.iter().filter(|a| a.message.is_none()).count()).unwrap_or(0);
        let status = format!("{}{}{}",snapshot.error.as_deref().unwrap_or(&snapshot.status),if pending_files > 0 { format!(" | {pending_files} pending file(s); Files manages them") } else { String::new() },if snapshot.dirty { " | UNSAVED" } else { "" });
        let h = self.status; if let Some(c) = self.control_mut(h) { c.set_caption(&status); }
        self.snapshot = Some(snapshot); self.updating = false; self.refresh_controls();
    }
}
impl ButtonEvents for AiChatWindow {
    fn on_pressed(&mut self,handle: Handle<Button>) -> EventProcessStatus {
        if self.held || self.pending.is_some() { return EventProcessStatus::Processed; }
        if handle == self.new { self.navigate(ChatCommand::New,Navigation::New); }
        else if handle == self.chats { self.choose_chat(); }
        else if handle == self.settings {
            match crate::config::ai::AiSettingsStore::new() {
                Ok(store) => if let Some(ai) = super::ai_settings::AiSettingsDialog::new(store).show() { self.configure(ai); },
                Err(error) => dialogs::error("AI Chat settings unavailable",&format!("{error:#}")),
            }
        }
        else if handle == self.route { self.use_route(); }
        else if handle == self.send || handle == self.retry { if self.await_send || self.navigation.is_some() { return EventProcessStatus::Processed; } let text = self.control(self.composer).map(|c| c.text().to_owned()).unwrap_or_default(); if self.command(ChatCommand::Draft(text)) && self.use_route() { self.request_after = self.snapshot.as_ref().map_or(0,|s| s.request_revision); self.await_send = self.command(if handle == self.send { ChatCommand::Send } else { ChatCommand::Retry }); } }
        else if handle == self.stop { self.command(ChatCommand::Stop); }
        else if handle == self.attach {
            if let Some(path) = dialogs::open("Attach any file (provider support varies)","",Location::Path(&super::image_viewer::home_directory()),None,OpenFileDialogFlags::Icons|OpenFileDialogFlags::CheckIfFileExists|OpenFileDialogFlags::PreserveBackground) { self.command(ChatCommand::Attach(path)); }
        } else if handle == self.files { self.choose_files(); }
        else if handle == self.delete {
            if Choice::new("Delete this chat and its local attachments?",&["Cancel".into(),"Delete chat".into()]).show() == Some(1) { self.navigate(ChatCommand::Delete,Navigation::Delete); }
        } else if handle == self.copy {
            if let Some(message) = self.snapshot.as_ref().and_then(|s| s.current.as_ref()).and_then(|c| c.messages.iter().rev().find(|m| m.role == Role::Assistant))
                && !appcui::system::Clipboard::set_internal_text(&message.text) { dialogs::error("Reply too large","The reply exceeds the internal clipboard limit of 256 KiB."); }
        } else { return EventProcessStatus::Ignored; }
        EventProcessStatus::Processed
    }
}
impl ComboBoxEvents for AiChatWindow {
    fn on_selection_changed(&mut self,handle: Handle<ComboBox>) -> EventProcessStatus {
        if handle != self.provider || self.updating { return EventProcessStatus::Ignored; }
        let provider = AiProvider::ALL[self.control(handle).and_then(ComboBox::index).unwrap_or(0).min(2) as usize];
        if self.snapshot.as_ref().and_then(|s| s.current.as_ref()).is_some_and(|c| c.provider == provider) { return EventProcessStatus::Ignored; }
        let model = if self.local_configuration { self.preferences.settings(provider).model.clone() }
            else { self.snapshot.as_ref().map(|s| s.model_defaults[AiProvider::ALL.iter().position(|p| *p == provider).unwrap_or(0)].clone()).unwrap_or_default() }; let h = self.model;
        if let Some(c) = self.control_mut(h) { c.set_text(&model); } self.use_route(); EventProcessStatus::Processed
    }
}
impl TextFieldEvents for AiChatWindow {
    fn on_text_changed(&mut self,handle: Handle<TextField>) -> EventProcessStatus {
        if handle == self.model && !self.updating { self.route_edited = true; return EventProcessStatus::Processed; }
        EventProcessStatus::Ignored
    }
}
impl ListBoxEvents for AiChatWindow {
    fn on_current_item_changed(&mut self,handle: Handle<ListBox>,index: usize) -> EventProcessStatus {
        if handle == self.sidebar && !self.updating && let Some(id) = self.snapshot.as_ref().and_then(|s| s.chats.get(index)).map(|c| c.id.clone()) { if self.snapshot.as_ref().and_then(|s| s.current.as_ref()).is_none_or(|c| c.id != id) { self.navigate(ChatCommand::Select(id.clone()),Navigation::Select); } return EventProcessStatus::Processed; }
        EventProcessStatus::Ignored
    }
}
impl WindowEvents for AiChatWindow {
    fn on_layout_changed(&mut self,_: Rect,_: Rect) { self.layout(); }
    fn on_cancel(&mut self) -> ActionRequest { if self.closing { ActionRequest::Allow } else { self.request_close(); ActionRequest::Deny } }
}
impl TimerEvents for AiChatWindow {
    fn on_update(&mut self,_: u64) -> EventProcessStatus {
        if self.closing { return EventProcessStatus::Ignored; }
        if self.pending.is_none() && !self.held { self.capture_draft(); }
        let changed = if let Some(snapshot) = self.service.poll() { self.refresh(snapshot); true } else { false };
        let done = self.flush_wait && self.snapshot.as_ref().is_some_and(|s| s.flush_revision > self.flush_after && !s.loading && !s.busy);
        if done {
            if self.snapshot.as_ref().is_some_and(|s| !s.dirty) { self.complete_pending(true); }
            else { self.flush_error = Some("Changes could not be saved to disk.".into()); self.flush_wait = false; }
        }
        if self.pending.is_some() && self.flush_error.is_some() && self.is_visible() {
            match Choice::new(self.flush_error.as_deref().unwrap_or("Chat was not saved"),&["Retry save".into(),"Discard unsaved changes".into(),"Cancel closing".into()]).show() {
                Some(0) => if let Some(pending) = self.pending { self.start_flush(pending); },
                Some(1) => if self.command(ChatCommand::DiscardUnsaved) { self.complete_pending(true); },
                _ => self.complete_pending(false),
            }
            return EventProcessStatus::Processed;
        }
        if changed || done { EventProcessStatus::Processed } else { EventProcessStatus::Ignored }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[track_caller]
    fn tick_until(window: &mut AiChatWindow, predicate: impl Fn(&AiChatWindow) -> bool) {
        let deadline = std::time::Instant::now()+Duration::from_secs(5);
        while !predicate(window) { TimerEvents::on_update(window,0); assert!(std::time::Instant::now()<deadline,"snapshot={:?}; composer={:?}",window.snapshot,window.control(window.composer).map(|c| c.text())); std::thread::yield_now(); }
    }
    #[test]
    fn review_regressions_navigation_route_retry_and_rejected_draft() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap(); let dir = crate::test_support::TestDir::new();
        let app = App::debug(80,24,"").timers_count(8).build().unwrap(); let preferences = AiPreferences::default();
        let service = ChatService::new_at(dir.path().join("ai"),preferences.clone()).unwrap();
        let mut window = AiChatWindow::with_service(service,preferences).unwrap();
        tick_until(&mut window,|w| w.snapshot.as_ref().is_some_and(|s| !s.loading && s.current.is_some()));
        let old_id = window.snapshot.as_ref().unwrap().current.as_ref().unwrap().id.clone();
        let composer = window.composer; window.control_mut(composer).unwrap().set_text("belongs only to A");
        let new = window.new; ButtonEvents::on_pressed(&mut window,new);
        assert!(window.navigation.is_some()); assert!(!window.capture_draft());
        tick_until(&mut window,|w| w.navigation.is_none() && w.snapshot.as_ref().and_then(|s| s.current.as_ref()).is_some_and(|c| c.id != old_id));
        assert!(window.snapshot.as_ref().unwrap().current.as_ref().unwrap().draft.trim().is_empty());
        window.navigate(ChatCommand::Select(old_id.clone()),Navigation::Select);
        tick_until(&mut window,|w| w.navigation.is_none() && w.snapshot.as_ref().and_then(|s| s.current.as_ref()).is_some_and(|c| c.id == old_id));
        assert_eq!(window.control(composer).unwrap().text(),"belongs only to A\n");

        let stale = window.snapshot.clone().unwrap();
        let model = window.model; window.control_mut(model).unwrap().set_text("desired-model");
        TextFieldEvents::on_text_changed(&mut window,model); assert!(window.use_route());
        window.refresh(stale); assert_eq!(window.control(model).unwrap().text(),"desired-model");
        tick_until(&mut window,|w| w.pending_route.is_none() && w.snapshot.as_ref().and_then(|s| s.current.as_ref()).is_some_and(|c| c.model == "desired-model"));

        let mut retry = window.snapshot.clone().unwrap();
        retry.current.as_mut().unwrap().messages.push(crate::runtime::ai::ChatMessage { role: Role::Assistant,text: "partial".into(),provider: AiProvider::OpenAi,model: "desired-model".into(),status: MessageStatus::Stopped });
        window.refresh(retry.clone()); window.await_send = true; window.request_after = retry.request_revision;
        retry.request_revision += 1; retry.current.as_mut().unwrap().messages.last_mut().unwrap().status = MessageStatus::Complete;
        window.refresh(retry); assert!(!window.await_send,"a completed Retry must release Send and draft capture");

        window.control_mut(composer).unwrap().set_text(&"x".repeat(65537));
        assert!(!window.capture_draft()); window.set_visible(false); window.request_guard();
        assert!(window.flush_error.is_some()); assert!(window.guard_result.is_none()); assert!(!window.flush_wait);
        TimerEvents::on_update(&mut window,0);
        assert!(window.guard_result.is_none(),"actor's clean disk state cannot approve a rejected local edit");
        assert!(window.control(composer).unwrap().text().len() > 65536);
        window.complete_pending(false); assert_eq!(window.take_guard_result(),Some(false));
        drop(window); app.run();
    }

    #[test]
    fn chat_window_preserves_latest_draft_across_background_updates_and_flush_guard() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap(); let dir = crate::test_support::TestDir::new();
        let app = App::debug(80,24,"").timers_count(8).build().unwrap();
        let preferences = AiPreferences::default();
        let service = ChatService::new_at(dir.path().join("ai"),preferences.clone()).unwrap();
        let mut window = AiChatWindow::with_service(service,preferences).unwrap();
        tick_until(&mut window,|w| w.snapshot.as_ref().is_some_and(|s| !s.loading && s.current.is_some()));
        let composer = window.composer; window.control_mut(composer).unwrap().set_text("latest ไทย draft");
        window.capture_draft();
        tick_until(&mut window,|w| w.snapshot.as_ref().and_then(|s| s.current.as_ref()).is_some_and(|c| c.draft == "latest ไทย draft\n"));
        assert_eq!(window.control(composer).unwrap().text(),"latest ไทย draft\n");
        let field = window.model; window.control_mut(field).unwrap().set_text("new-model");
        TextFieldEvents::on_text_changed(&mut window,field); window.use_route();
        tick_until(&mut window,|w| w.snapshot.as_ref().and_then(|s| s.current.as_ref()).is_some_and(|c| c.model == "new-model"));
        window.control_mut(composer).unwrap().set_text("last edit before close");
        window.request_guard();
        assert!(window.guard_result.is_none());
        tick_until(&mut window,|w| w.guard_result.is_some());
        assert_eq!(window.take_guard_result(),Some(true));
        assert_eq!(window.snapshot.as_ref().unwrap().current.as_ref().unwrap().draft,"last edit before close\n");
        assert!(!window.snapshot.as_ref().unwrap().dirty);
        drop(window); app.run();
    }

    #[test]
    fn transcript_never_renders_raw_escape_controls() {
        let lines = wrap_lines("hello \x1b[2J \x07",80);
        assert!(lines.iter().all(|line| !line.chars().any(char::is_control)));
    }
    #[test]
    fn wraps_complete_graphemes_and_preserves_scrollback_during_streaming() {
        assert_eq!(wrap_lines("e\u{301}界abc",3),["e\u{301}界","abc"]);
        let mut transcript = Transcript::new(); transcript.viewport = Size::new(8,2);
        transcript.update("a\nb\nc\nd".into(),true); assert_eq!(transcript.first,2);
        transcript.scroll(-1); transcript.update("a\nb\nc\nd\ne".into(),false); assert_eq!(transcript.first,1);
        transcript.first = transcript.bottom(); transcript.update("a\nb\nc\nd\ne\nf".into(),false); assert_eq!(transcript.first,4);
    }
}
