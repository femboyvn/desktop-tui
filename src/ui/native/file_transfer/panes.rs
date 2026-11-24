use super::{Command, Item, Kind, Side};
use appcui::prelude::*;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Default)]
struct Pane {
    path: Option<PathBuf>,
    entries: Vec<Item>,
    selected: usize, first: usize,
    marked: HashSet<OsString>,
}

impl Pane {
    fn list(&mut self, path: PathBuf, entries: Vec<Item>) {
        if self.path.as_ref() == Some(&path) {
            let selected = self.entries.get(self.selected).map(|item| &item.name);
            self.selected = entries.iter().position(|item| Some(&item.name) == selected)
                .unwrap_or_else(|| self.selected.min(entries.len().saturating_sub(1)));
            let names: HashSet<_> = entries.iter().map(|item| &item.name).collect();
            self.marked.retain(|name| names.contains(name));
        } else {
            self.selected = 0;
            self.first = 0;
            self.marked.clear();
        }
        self.path = Some(path);
        self.entries = entries;
    }

    fn names(&self) -> Vec<OsString> {
        if self.marked.is_empty() {
            self.entries.get(self.selected).map(|item| item.name.clone()).into_iter().collect()
        } else {
            self.entries.iter().filter(|item| self.marked.contains(&item.name))
                .map(|item| item.name.clone()).collect()
        }
    }

    fn move_selection(&mut self, delta: isize) {
        self.selected = self.selected.saturating_add_signed(delta).min(self.entries.len().saturating_sub(1));
    }
    fn reveal(&mut self, rows: usize) {
        let rows = rows.max(1);
        self.first = self.first.min(self.entries.len().saturating_sub(rows));
        self.first = self.first.clamp(self.selected.saturating_add(1).saturating_sub(rows), self.selected);
    }
}

pub(super) enum Action { Connect, Sites, Cancel, Details, Submit(Command) }

#[derive(Clone, Copy, PartialEq)]
enum Tool { Connection, Sites, Copy, Cancel, Refresh, Details }

struct ButtonArea { tool: Tool, label: &'static str, x: usize, y: usize }

#[CustomControl(overwrite = OnPaint+OnKeyPressed+OnMouseEvent+OnResize)]
pub(super) struct Panes {
    local: Pane, remote: Pane, active: Side, size: Size, pending: Option<Action>,
    pub(super) connected: bool, pub(super) connecting: bool, pub(super) busy: bool,
    pub(super) status: String, pub(super) error: bool,
}

impl Panes {
    pub(super) fn new() -> Self {
        Self {
            base: ControlBase::new(Layout::fill(),true),
            local: Pane::default(), remote: Pane::default(), active: Side::Local,
            size: Size::new(0,0), pending: None, connected: false, connecting: false, busy: true,
            status: "Choose Connect to open a remote folder.".into(), error: false,
        }
    }
    fn pane(&self, side: Side) -> &Pane { match side { Side::Local => &self.local, Side::Remote => &self.remote } }
    fn pane_mut(&mut self, side: Side) -> &mut Pane { match side { Side::Local => &mut self.local, Side::Remote => &mut self.remote } }
    pub(super) fn list(&mut self, side: Side, path: PathBuf, entries: Vec<Item>) { self.pane_mut(side).list(path,entries); self.reveal(); }
    pub(super) fn disconnected(&mut self) { self.connected = false; self.connecting = false; self.remote = Pane::default(); }
    pub(super) fn take_action(&mut self) -> Option<Action> { self.pending.take() }
    pub(super) fn notice(&mut self, message: String, error: bool) { self.status = message; self.error = error; }

    fn toolbar(&self) -> Vec<ButtonArea> {
        let connection = if self.connected || self.connecting { "[F2 Disconnect]" } else { "[F2 Connect]" };
        let copy = if self.active == Side::Local { "[F5 Upload]" } else { "[F5 Download]" };
        let mut x = 0;
        let mut y = 0;
        [(Tool::Connection,connection), (Tool::Sites,"[F3 Sites]"), (Tool::Copy,copy),
         (Tool::Cancel,"[F8 Cancel]"), (Tool::Refresh,"[Ctrl+R Refresh]"), (Tool::Details,"[F1 Info]")]
            .into_iter().map(|(tool,label)| {
                let width = match tool { Tool::Connection => 15, Tool::Copy => 13, _ => label.len() };
                if x > 0 && x + width > self.size.width as usize { x = 0; y += 1; }
                let button = ButtonArea { tool, label, x, y };
                x += width + 1;
                button
            }).collect()
    }
    fn header_row(&self) -> usize { self.toolbar().last().map_or(0,|button| button.y + 1) }
    fn rows(&self) -> usize { (self.size.height as usize).saturating_sub(self.header_row() + 3) }
    fn reveal(&mut self) {
        let rows = self.rows();
        self.local.reveal(rows); self.remote.reveal(rows);
    }
    fn columns(&self) -> Vec<(Side,usize,usize)> {
        let width = self.size.width as usize;
        if width < 56 { return vec![(self.active,0,width)]; }
        let middle = width / 2;
        vec![(Side::Local,0,middle),(Side::Remote,middle+1,width-middle-1)]
    }
    fn tool(&mut self, tool: Tool) {
        if self.pending.is_some() { return; }
        let action = match tool {
            Tool::Connection if self.connected || self.connecting => Action::Submit(Command::Disconnect),
            Tool::Cancel if self.busy => Action::Cancel,
            Tool::Details => Action::Details,
            _ if self.busy => return,
            Tool::Connection => Action::Connect,
            Tool::Sites => Action::Sites,
            Tool::Cancel => return,
            Tool::Copy => {
                let names = self.pane(self.active).names();
                if !self.connected || self.local.path.is_none() || self.remote.path.is_none() {
                    self.notice("Connect and open both folders before copying.".into(),true);
                    return;
                }
                if names.is_empty() { self.notice("Choose a file or folder to copy.".into(),true); return; }
                Action::Submit(Command::Copy { from: self.active, names })
            }
            Tool::Refresh => {
                let Some(path) = &self.pane(self.active).path else { return; };
                Action::Submit(Command::List(self.active,path.clone()))
            }
        };
        self.pending = Some(action);
    }
    fn navigate(&mut self, parent: bool) {
        if self.busy || self.pending.is_some() { return; }
        let pane = self.pane(self.active);
        let Some(current) = &pane.path else { return; };
        let path = if parent {
            let Some(parent) = current.parent() else { return; };
            parent.to_owned()
        } else {
            let Some(entry) = pane.entries.get(pane.selected) else { return; };
            if entry.kind != Kind::Directory { return; }
            current.join(&entry.name)
        };
        self.pending = Some(Action::Submit(Command::List(self.active,path)));
    }
    fn toggle_mark(&mut self) {
        let pane = self.pane_mut(self.active);
        if let Some(item) = pane.entries.get(pane.selected) && !pane.marked.remove(&item.name) {
            pane.marked.insert(item.name.clone());
        }
    }
    fn select_at(&mut self, x: usize, y: usize) -> bool {
        let Some((side,_,_)) = self.columns().into_iter().find(|(_,left,width)| x >= *left && x < left+width) else { return false; };
        self.active = side;
        let Some(row) = y.checked_sub(self.header_row()+1) else { return false; };
        let rows = self.rows();
        if row >= rows { return false; }
        let pane = self.pane_mut(side);
        let index = pane.first+row;
        if index >= pane.entries.len() { return false; }
        pane.selected = index;
        true
    }
}

// Paint only whole terminal graphemes within each pane, including when paths contain wide glyphs.
fn line(surface: &mut Surface, x: usize, y: usize, width: usize, text: &str, attr: CharAttribute) {
    let text = text.replace(char::is_control," ");
    let cells = tattoy_wezterm_term::Line::from_text(&text,&tattoy_wezterm_term::CellAttributes::default(),0,None);
    let mut column = 0;
    for cell in cells.visible_cells() {
        if column + cell.width() > width { break; }
        surface.write_grapheme((x+column) as i32,y as i32,cell.str(),cell.width(),attr);
        column += cell.width();
    }
}

impl OnPaint for Panes {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        let width = self.size.width as usize;
        let height = self.size.height as usize;
        surface.clear(Character::with_attributes(' ',theme.window.normal));
        if width == 0 || height == 0 { return; }
        for button in self.toolbar() {
            line(surface,button.x,button.y,width.saturating_sub(button.x),button.label,theme.text.enphasized_1);
        }
        let header_row = self.header_row();
        let rows = self.rows();
        for (side,left,width) in self.columns() {
            let pane = self.pane(side);
            let name = if side == Side::Local { "Local" } else { "Remote" };
            let header = format!("{} {name} {}",if side == self.active { "▶" } else { " " },
                pane.path.as_deref().map(|dir| dir.to_string_lossy()).unwrap_or_default());
            line(surface,left,header_row,width,&header,theme.text.enphasized_1);
            if rows == 0 { continue; }
            if pane.entries.is_empty() {
                let message = if pane.path.is_some() { "(empty)" } else if side == Side::Remote && !self.connected { "(disconnected)" } else { "(loading)" };
                line(surface,left,header_row+1,width,message,theme.text.inactive);
            }
            let start = pane.first;
            for (row,item) in pane.entries.iter().skip(start).take(rows).enumerate() {
                let selected = pane.selected == start+row && side == self.active;
                let attr = if selected { theme.editor.pressed_or_selected } else { theme.text.normal };
                let prefix = if pane.marked.contains(&item.name) { "[x]" } else { "[ ]" };
                let suffix = match item.kind { Kind::Directory => "/".to_owned(), Kind::File => item.size.map(|bytes| format!("  {bytes} B")).unwrap_or_default(), Kind::Other => "  (special)".into() };
                let label = format!("{prefix} {}{suffix}",item.name.to_string_lossy());
                let y = header_row+1+row;
                if selected { surface.fill_horizontal_line_with_size(left as i32,y as i32,width as u32,Character::with_attributes(' ',attr)); }
                line(surface,left,y,width,&label,attr);
            }
        }
        if height >= header_row + 3 {
            let hint = if width < 56 { "Tab pane  Space mark  Enter open" } else { "Tab pane   Space mark   Enter open folder   Backspace parent" };
            line(surface,0,height-2,width,hint,theme.text.inactive);
            line(surface,0,height-1,width,&self.status,if self.error { theme.text.error } else { theme.text.normal });
        }
    }
}
impl OnResize for Panes {
    fn on_resize(&mut self, _: Size, size: Size) { self.size = size; self.reveal(); }
}
impl OnKeyPressed for Panes {
    fn on_key_pressed(&mut self, key: Key, _: char) -> EventProcessStatus {
        let rows = self.rows().max(1) as isize;
        match key.value() {
            key!("F1") => self.tool(Tool::Details),
            key!("F2") => self.tool(Tool::Connection),
            key!("F3") => self.tool(Tool::Sites),
            key!("F5") => self.tool(Tool::Copy),
            key!("F8") => self.tool(Tool::Cancel),
            key!("Ctrl+R") => self.tool(Tool::Refresh),
            key!("Tab") | key!("Shift+Tab") => self.active = if self.active == Side::Local { Side::Remote } else { Side::Local },
            key!("Left") => self.active = Side::Local,
            key!("Right") => self.active = Side::Remote,
            key!("Up") => self.pane_mut(self.active).move_selection(-1),
            key!("Down") => self.pane_mut(self.active).move_selection(1),
            key!("PageUp") => self.pane_mut(self.active).move_selection(-rows),
            key!("PageDown") => self.pane_mut(self.active).move_selection(rows),
            key!("Home") => self.pane_mut(self.active).selected = 0,
            key!("End") => self.pane_mut(self.active).move_selection(isize::MAX),
            key!("Enter") => self.navigate(false),
            key!("Back") => self.navigate(true),
            key!("Space") => self.toggle_mark(),
            _ => return EventProcessStatus::Ignored,
        }
        self.reveal();
        EventProcessStatus::Processed
    }
}
impl OnMouseEvent for Panes {
    fn on_mouse_event(&mut self, event: &MouseEvent) -> EventProcessStatus {
        match event {
            MouseEvent::Pressed(data) | MouseEvent::DoubleClick(data) if data.button == MouseButton::Left && data.x >= 0 && data.y >= 0 => {
                self.request_focus();
                let (x,y) = (data.x as usize,data.y as usize);
                if let Some(button) = self.toolbar().into_iter().find(|b| b.y == y && x >= b.x && x < b.x+b.label.len()) {
                    self.tool(button.tool);
                } else if y+1 == self.size.height as usize { self.tool(Tool::Details); }
                else if self.select_at(x,y) && matches!(event,MouseEvent::DoubleClick(_)) { self.navigate(false); }
            }
            MouseEvent::WheelAt(data) => {
                if let Some((side,_,_)) = self.columns().into_iter().find(|(_,left,width)| data.x >= *left as i32 && data.x < (left+width) as i32) {
                    self.active = side;
                    match data.direction { MouseWheelDirection::Up => self.pane_mut(side).move_selection(-3),
                        MouseWheelDirection::Down => self.pane_mut(side).move_selection(3), _ => return EventProcessStatus::Ignored }
                }
            }
            _ => return EventProcessStatus::Ignored,
        }
        self.reveal();
        EventProcessStatus::Processed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::UI_LOCK;
    use std::sync::{Arc, atomic::{AtomicUsize,Ordering}};

    fn item(name: &str, kind: Kind) -> Item { Item { name: name.into(), kind, size: None } }
    fn panes() -> Panes {
        let mut view = Panes::new();
        view.connected = true;
        view.busy = false;
        view.list(Side::Local,"/home/sam".into(),vec![item("reports",Kind::Directory),item("notes",Kind::File),item("table",Kind::File)]);
        view.list(Side::Remote,"/srv".into(),vec![item("incoming",Kind::Directory),item("result",Kind::File)]);
        view
    }
    #[Desktop(events = DesktopEvents, overwrite = OnResize)]
    struct KeyProbe { window: Handle<Window>, view: Handle<Panes>, observed: Arc<AtomicUsize> }
    impl DesktopEvents for KeyProbe {
        fn on_start(&mut self) {
            let mut window = Window::new("File Transfer",layout!("d:f"),window::Flags::None);
            self.view = window.add(panes());
            window.request_focus_for_control(self.view);
            self.window = self.add_window(window);
        }
    }
    impl OnResize for KeyProbe {
        fn on_resize(&mut self, _: Size, size: Size) {
            let (side,names,bit) = match size.width {
                79 => (Side::Local,vec![OsString::from("reports"),"notes".into()],1),
                38 => (Side::Remote,vec![OsString::from("result")],2),
                _ => return,
            };
            if self.observed.fetch_or(bit,Ordering::Relaxed) & bit != 0 { return; }
            let handle = self.view;
            let window = self.window; let view = self.window_mut(window).unwrap().control_mut(handle).unwrap();
            assert!(view.has_focus());
            let Some(Action::Submit(Command::Copy { from,names: paths })) = view.take_action() else {
                panic!("No copy command at width {}. Active {:?}, status {}",size.width,view.active,view.status);
            };
            assert_eq!(from,side); assert_eq!(paths,names);
        }
    }
    #[test]
    fn tab_and_f5_route_marked_upload_and_selected_download_through_focused_control() {
        let _lock = UI_LOCK.lock().unwrap();
        let observed = Arc::new(AtomicUsize::new(0));
        let desktop = KeyProbe { base: Desktop::new(), window: Handle::None, view: Handle::None, observed: observed.clone() };
        App::debug(80,24,"Paint.Enable(false)\nKey.Pressed(Space)\nKey.Pressed(Down)\nKey.Pressed(Space)\nKey.Pressed(Down)\nKey.Pressed(F5)\nResize(79,24)\nResize(40,12)\nKey.Pressed(Tab)\nKey.Pressed(Down)\nKey.Pressed(F5)\nResize(38,12)")
            .desktop(desktop).build().unwrap().run();
        assert_eq!(observed.load(Ordering::Relaxed),3);
    }
    #[test]
    fn mouse_selects_remote_folder_and_toolbar_respects_busy_and_direction() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(100,24,"").build().unwrap();
        let mut view = panes();
        view.on_resize(Size::default(),Size::new(98,21));
        let click = |x,y| MouseEventData { x, y, button: MouseButton::Left, modifier: KeyModifier::None };
        let y = (view.header_row()+1) as i32;
        view.on_mouse_event(&MouseEvent::DoubleClick(click(56,y)));
        assert!(matches!(view.take_action(),Some(Action::Submit(Command::List(Side::Remote,path))) if path == std::path::Path::new("/srv/incoming")));
        view.on_key_pressed(key!("Back").into(),'\0');
        assert!(matches!(view.take_action(),Some(Action::Submit(Command::List(Side::Remote,path))) if path == std::path::Path::new("/")));
        let click_tool = |view: &mut Panes,tool: Tool| {
            let area = view.toolbar().into_iter().find(|area| area.tool == tool).unwrap();
            view.on_mouse_event(&MouseEvent::Pressed(click(area.x as i32,area.y as i32)));
        };
        click_tool(&mut view,Tool::Copy);
        assert!(matches!(view.take_action(),Some(Action::Submit(Command::Copy { from: Side::Remote,.. }))));
        click_tool(&mut view,Tool::Sites);
        assert!(matches!(view.take_action(),Some(Action::Sites)));
        view.busy = true;
        click_tool(&mut view,Tool::Copy);
        assert!(view.take_action().is_none());
        click_tool(&mut view,Tool::Cancel);
        assert!(matches!(view.take_action(),Some(Action::Cancel)));
        click_tool(&mut view,Tool::Connection);
        assert!(matches!(view.take_action(),Some(Action::Submit(Command::Disconnect))));
        view.disconnected(); view.busy = false;
        click_tool(&mut view,Tool::Connection);
        assert!(matches!(view.take_action(),Some(Action::Connect)));
        app.run();
    }
    #[test]
    fn narrow_panes_show_active_directory_and_refresh_keeps_marks_by_name() {
        let mut view = panes();
        view.on_resize(Size::default(),Size::new(37,12));
        view.on_key_pressed(key!("Space").into(),' ');
        view.list(Side::Local,"/home/sam".into(),vec![item("first",Kind::File),item("reports",Kind::Directory)]);
        assert_eq!(view.local.names(),vec![OsString::from("reports")]);
        assert_eq!(view.local.selected,1);
        for (key,header,unseen) in [(key!("Right"),"Remote /srv","Local /home"),(key!("Left"),"Local /home/sam","Remote /srv")] {
            view.on_key_pressed(key.into(),'\0');
            let mut surface = Surface::new(37,12); view.on_paint(&mut surface,&Theme::default());
            let row: String = (0..37).map(|x| surface.char(x,view.header_row() as i32).unwrap().code).collect();
            assert!(row.contains(header),"{row}"); assert!(!row.contains(unseen));
        }
        for (w,h) in [(0,0),(1,1),(20,4)] {
            view.on_resize(view.size,Size::new(w,h)); view.on_paint(&mut Surface::new(w,h),&Theme::default());
        }
    }
}
