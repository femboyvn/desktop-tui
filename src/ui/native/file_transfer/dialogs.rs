use super::{Answer, PromptKind, Protocol, Site};
use crate::ui::locking::SecretInput;
use appcui::prelude::*;
use zeroize::Zeroizing;

const PROTOCOLS: [Protocol;3] = [Protocol::Sftp, Protocol::Scp, Protocol::Ftp];

#[ModalWindow(response = Site, events = ButtonEvents+WindowEvents)]
struct SiteEditor {
    named: bool, name: Handle<TextField>, host: Handle<TextField>, port: Handle<TextField>,
    username: Handle<TextField>, protocol: Handle<ComboBox>, local: Handle<TextField>, remote: Handle<TextField>,
    submit: Handle<Button>, status: Handle<Label>,
}
impl SiteEditor {
    fn new(site: &Site, named: bool) -> Self {
        let title = if named { "File Transfer — Site settings" } else { "File Transfer — Quick connect" };
        let mut dialog = Self {
            base: ModalWindow::new(title,layout!("a:c,w:92%,h:95%"),window::Flags::Sizeable),
            named, name: Handle::None, host: Handle::None, port: Handle::None, username: Handle::None,
            protocol: Handle::None, local: Handle::None, remote: Handle::None, submit: Handle::None,status: Handle::None,
        };
        dialog.set_size_bounds(1,1,82,15);
        let row = i32::from(named);
        if named { dialog.name = dialog.field("Site name",&site.name,0); }
        dialog.host = dialog.field("Host",&site.host,row+1);
        dialog.add(Label::new("Protocol",Layout::absolute(1,row,13,1)));
        let mut protocols = ComboBox::new(LayoutBuilder::new().left_anchor(15).top_anchor(row).right_anchor(1).height(1).build(),combobox::Flags::None);
        for protocol in PROTOCOLS { protocols.add(protocol.label()); }
        protocols.set_index(PROTOCOLS.iter().position(|p| *p == site.protocol).unwrap_or(0) as u32);
        dialog.protocol = dialog.add(protocols);
        dialog.port = dialog.field("Port", &site.port.map(|p| p.to_string()).unwrap_or_default(),row+2);
        dialog.username = dialog.field("Username",&site.username,row+3);
        dialog.local = dialog.field("Local folder",&site.local_folder.to_string_lossy(),row+4);
        dialog.remote = dialog.field("Remote folder",&site.remote_folder.to_string_lossy(),row+5);
        dialog.status = dialog.add(Label::new("Blank port uses the protocol default. FTP sends credentials and files in clear text.",layout!("l:1,r:1,t:7,b:1")));
        dialog.submit = dialog.add(Button::with_type(if named { "&Apply" } else { "&Connect" },layout!("l:1,b:0,w:11"),button::Type::Flat));
        dialog.add(Button::with_type("Cancel",layout!("r:1,b:0,w:10"),button::Type::Flat));
        dialog.base.request_focus_for_control(if named { dialog.name } else { dialog.host });
        dialog
    }
    fn field(&mut self, label: &str, value: &str, row: i32) -> Handle<TextField> {
        self.add(Label::new(label,Layout::absolute(1,row,13,1)));
        self.add(TextField::new(value,LayoutBuilder::new().left_anchor(15).top_anchor(row).right_anchor(1).height(1).build(),textfield::Flags::None))
    }
    fn text(&self, handle: Handle<TextField>) -> &str { self.control(handle).map(TextField::text).unwrap_or("") }
    fn draft(&self) -> anyhow::Result<Site> {
        let name = self.text(self.name).trim().to_owned();
        anyhow::ensure!(!self.named || !name.is_empty(),"Enter a site name.");
        let port = self.text(self.port).trim();
        let port = if port.is_empty() { None } else {
            let n = port.parse::<u16>().ok().filter(|n| *n != 0)
                .ok_or_else(|| anyhow::anyhow!("Enter a port from 1 through 65535, or leave it blank."))?;
            Some(n)
        };
        let index = self.control(self.protocol).and_then(ComboBox::index).unwrap_or(0) as usize;
        let site = Site { name, protocol: PROTOCOLS[index], port, host: self.text(self.host).trim().into(),
            username: self.text(self.username).trim().into(), local_folder: self.text(self.local).into(), remote_folder: self.text(self.remote).into() };
        site.validate()?;
        Ok(site)
    }
    fn submit(&mut self) {
        match self.draft() {
            Ok(site) => self.exit_with(site),
            Err(error) => { let handle = self.status; if let Some(status) = self.control_mut(handle) { status.set_caption(&error.to_string()); } }
        }
    }
}
impl ButtonEvents for SiteEditor {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.submit { self.submit(); } else { self.exit(); }
        EventProcessStatus::Processed
    }
}
impl WindowEvents for SiteEditor { fn on_accept(&mut self) { self.submit(); } }

enum SiteChoice { Connect(Site), Save(Vec<Site>) }
#[derive(Clone,Copy)]
enum SiteButton { New, Edit, Delete, Save, Cancel, Connect, Quick }

#[ModalWindow(response = SiteChoice, events = ButtonEvents+WindowEvents)]
struct SiteList {
    sites: Vec<Site>, managing: bool, list: Handle<ListBox>, actions: Vec<(Handle<Button>,SiteButton)>,
}
impl SiteList {
    fn new(sites: &[Site], managing: bool) -> Self {
        let mut dialog = Self {
            base: ModalWindow::new(if managing { "File Transfer — Saved sites" } else { "File Transfer — Connect" },
                layout!("a:c,w:90%,h:65%"),window::Flags::Sizeable),
            sites: sites.to_vec(), managing, list: Handle::None, actions: Vec::new(),
        };
        let mut list = ListBox::new(layout!("l:1,r:1,t:0,b:4"),listbox::Flags::ScrollBars);
        list.set_empty_message("No saved sites");
        dialog.list = dialog.add(list);
        if managing {
            dialog.button(SiteButton::New,"&New",layout!("l:1,b:2,w:8"));
            dialog.button(SiteButton::Edit,"&Edit",layout!("l:10,b:2,w:8"));
            dialog.button(SiteButton::Delete,"&Delete",layout!("l:19,b:2,w:10"));
            dialog.button(SiteButton::Save,"&Save",layout!("l:1,b:0,w:10"));
        } else {
            dialog.button(SiteButton::Quick,"&Quick connect",layout!("l:1,b:2,w:17"));
            dialog.button(SiteButton::Connect,"&Connect",layout!("l:1,b:0,w:11"));
        }
        dialog.button(SiteButton::Cancel,"Cancel",layout!("r:1,b:0,w:10"));
        dialog.refresh(0);
        dialog.base.request_focus_for_control(dialog.list);
        dialog
    }
    fn button(&mut self, action: SiteButton, label: &str, layout: Layout) {
        let button = self.add(Button::new(label,layout));
        self.actions.push((button,action));
    }
    fn refresh(&mut self, selected: usize) {
        let handle = self.list;
        // The fields may contain opaque remote paths. The list keeps site records by index.
        let labels: Vec<_> = self.sites.iter().map(|s| format!("{}  ·  {}  {}",s.name,s.protocol.label(),s.host)).collect();
        if let Some(list) = self.control_mut(handle) {
            list.clear();
            for label in &labels { list.add(label); }
            if !labels.is_empty() { list.set_index(selected.min(labels.len()-1)); }
        }
        let has_sites = !labels.is_empty();
        for &(handle,action) in &self.actions {
            if matches!(action,SiteButton::Edit | SiteButton::Delete | SiteButton::Connect)
                && let Some(control) = self.base.control_mut(handle) { control.set_enabled(has_sites); }
        }
    }
    fn activate(&mut self) {
        self.act(if self.managing { SiteButton::Edit } else { SiteButton::Connect });
    }
    fn act(&mut self, action: SiteButton) {
        let index = self.control(self.list).map(ListBox::index).unwrap_or(usize::MAX);
        match action {
            SiteButton::New | SiteButton::Edit => {
                let creating = matches!(action,SiteButton::New);
                let original = if creating { Site::default() }
                    else if let Some(site) = self.sites.get(index) { site.clone() } else { return; };
                if let Some(site) = SiteEditor::new(&original,true).show() {
                    if self.sites.iter().enumerate().any(|(i,s)| (creating || i != index) && s.name == site.name) {
                        appcui::dialogs::error("Site name in use","Choose a different name for this site.");
                        return;
                    }
                    if creating { self.sites.push(site); self.refresh(self.sites.len()-1); }
                    else { self.sites[index] = site; self.refresh(index); }
                }
            }
            SiteButton::Delete => {
                if let Some(site) = self.sites.get(index)
                    && appcui::dialogs::validate("Delete saved site?",&format!("Delete {} from saved sites?",site.name)) {
                    self.sites.remove(index);
                    self.refresh(index);
                }
            }
            SiteButton::Save => {
                let sites = std::mem::take(&mut self.sites);
                self.exit_with(SiteChoice::Save(sites));
            }
            SiteButton::Connect => { if let Some(site) = self.sites.get(index).cloned() { self.exit_with(SiteChoice::Connect(site)); } }
            SiteButton::Quick => { if let Some(site) = SiteEditor::new(&Site::default(),false).show() { self.exit_with(SiteChoice::Connect(site)); } }
            SiteButton::Cancel => self.exit(),
        }
    }
}
impl ButtonEvents for SiteList {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if let Some(&(_,action)) = self.actions.iter().find(|(button,_)| *button == handle) { self.act(action); }
        EventProcessStatus::Processed
    }
}
impl WindowEvents for SiteList { fn on_accept(&mut self) { self.activate(); } }

pub(super) fn manage(sites: &[Site]) -> Option<Vec<Site>> {
    match SiteList::new(sites,true).show()? { SiteChoice::Save(sites) => Some(sites), _ => None }
}
pub(super) fn connect(sites: &[Site]) -> Option<Site> {
    if sites.is_empty() { return SiteEditor::new(&Site::default(),false).show(); }
    match SiteList::new(sites,false).show()? { SiteChoice::Connect(site) => Some(site), _ => None }
}

#[ModalWindow(response = bool, events = ButtonEvents)]
struct StatusDialog {}
impl ButtonEvents for StatusDialog {
    fn on_pressed(&mut self,_: Handle<Button>) -> EventProcessStatus { self.exit(); EventProcessStatus::Processed }
}
pub(super) fn show_status(message: &str) {
    let mut dialog = StatusDialog { base: ModalWindow::new("File Transfer — Status",layout!("a:c,w:90%,h:80%"),window::Flags::Sizeable) };
    let details = dialog.add(TextArea::new(message,layout!("l:1,r:1,t:0,b:2"),textarea::Flags::ReadOnly | textarea::Flags::ScrollBars));
    dialog.add(Button::new("&Close",layout!("r:1,b:0,w:10")));
    dialog.base.request_focus_for_control(details);
    dialog.show();
}

#[ModalWindow(response = Answer, events = ButtonEvents+WindowEvents)]
struct TransferPrompt {
    secret: Handle<SecretInput>, text: Handle<TextField>, submit: Handle<Button>, skip: Handle<Button>,
}
impl TransferPrompt {
    fn new(message: &str, kind: PromptKind) -> Self {
        let title = match kind { PromptKind::Secret => "File Transfer — Authentication", PromptKind::Text => "File Transfer — Connection prompt",
            PromptKind::Conflict => "File Transfer — Destination exists" };
        let mut dialog = Self {
            base: ModalWindow::new(title,layout!("a:c,w:90%,h:90%"),window::Flags::Sizeable),
            secret: Handle::None, text: Handle::None, submit: Handle::None, skip: Handle::None,
        };
        dialog.set_size_bounds(1,1,88,18);
        let message = TextArea::new(message,layout!("l:1,r:1,t:0,b:5"),textarea::Flags::ReadOnly | textarea::Flags::ScrollBars);
        dialog.add(message);
        let cancel = dialog.add(Button::new("&Cancel",layout!("r:1,b:0,w:9")));
        match kind {
            PromptKind::Secret => dialog.secret = dialog.add(SecretInput::new(layout!("l:1,r:1,b:3,h:1"))),
            PromptKind::Text => dialog.text = dialog.add(TextField::new("",layout!("l:1,r:1,b:3,h:1"),textfield::Flags::None)),
            PromptKind::Conflict => dialog.skip = dialog.add(Button::new("&Skip",layout!("l:1,b:2,w:10"))),
        }
        let submit = if matches!(kind,PromptKind::Conflict) { "&Replace" } else { "&Send" };
        dialog.submit = dialog.add(Button::new(submit,layout!("l:1,b:0,w:11")));
        match kind {
            PromptKind::Secret => dialog.base.request_focus_for_control(dialog.secret),
            PromptKind::Text => dialog.base.request_focus_for_control(dialog.text),
            PromptKind::Conflict => dialog.base.request_focus_for_control(cancel),
        }
        dialog
    }
    fn submit(&mut self) {
        let secret = self.secret;
        let answer = if let Some(input) = self.control_mut(secret) {
            Answer::Text(std::mem::take(&mut input.value))
        } else if let Some(input) = self.control(self.text) {
            Answer::Text(Zeroizing::new(input.text().to_owned()))
        } else { Answer::Replace };
        self.exit_with(answer);
    }
}
impl ButtonEvents for TransferPrompt {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.submit { self.submit(); } else if handle == self.skip { self.exit_with(Answer::Skip); } else { self.exit(); }
        EventProcessStatus::Processed
    }
}
impl WindowEvents for TransferPrompt {
    fn on_accept(&mut self) {
        if !self.secret.is_none() || !self.text.is_none() { self.submit(); }
    }
}
pub(super) fn prompt(message: &str, kind: PromptKind) -> Answer {
    TransferPrompt::new(message,kind).show().unwrap_or(Answer::Cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::UI_LOCK;

    #[test]
    fn editor_validates_fields_and_returns_profile_without_login_secrets() {
        let _lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80,24,"").build().unwrap();
        let mut editor = SiteEditor::new(&Site::default(),true);
        assert!(editor.draft().is_err());
        let fields = [(editor.name,"Lab"),(editor.host,"lab.example"),(editor.username,"sam"),
            (editor.port,"66222"),(editor.local,"/tmp/work folder"),(editor.remote,"/remote folder")];
        for (field,value) in fields { editor.control_mut(field).unwrap().set_text(value); }
        assert!(editor.draft().is_err());
        let port = editor.port; editor.control_mut(port).unwrap().set_text("2222");
        let protocol = editor.protocol; editor.control_mut(protocol).unwrap().set_index(1);
        let draft = editor.draft().unwrap();
        assert_eq!(draft.name,"Lab"); assert_eq!(draft.protocol,Protocol::Scp); assert_eq!(draft.port,Some(2222));
        assert_eq!(draft.remote_folder,std::path::Path::new("/remote folder"));
        assert_eq!(draft.local_folder,std::path::Path::new("/tmp/work folder"));
        drop(editor);
        app.run();
    }
    #[test]
    fn saved_sites_create_rename_and_delete_with_explicit_save() {
        let _lock = UI_LOCK.lock().unwrap();
        let save = "Key.Pressed(Alt+S)\nPaint.Enable(false)\nPaint.Enable(false)";
        let options = "\nPaint.Enable(false)\nPaint.Enable(false)\n";
        let create = format!("Key.Pressed(Alt+N){options}Key.TypeText('Test site')\nKey.Pressed(Tab)\nKey.TypeText('test.example')\nKey.Pressed(Alt+A){options}{save}");
        let app = App::debug(40,12,&create).build().unwrap();
        let sites = manage(&[]).unwrap();
        assert_eq!(sites.len(),1); assert_eq!(sites[0].name,"Test site"); assert_eq!(sites[0].host,"test.example");
        app.run();

        let rename = format!("Key.Pressed(Alt+E){options}Key.Pressed(Ctrl+A)\nKey.TypeText('Renamed')\nKey.Pressed(Alt+A){options}{save}");
        let app = App::debug(40,12,&rename).build().unwrap();
        let sites = manage(&sites).unwrap();
        assert_eq!(sites[0].name,"Renamed"); assert_eq!(sites[0].host,"test.example"); app.run();

        let delete = format!("Key.Pressed(Alt+D){options}Key.Pressed(Alt+Y){options}{save}");
        let app = App::debug(40,12,&delete).build().unwrap();
        assert!(manage(&sites).unwrap().is_empty()); app.run();
    }
    #[test]
    fn saved_site_picker_and_prompt_choices_follow_keyboard_input() {
        let _lock = UI_LOCK.lock().unwrap();
        let sites = [Site { name:"First".into(),host:"first.example".into(),..Site::default() },
            Site { name:"Second".into(),host:"second.example".into(),..Site::default() }];
        let app = App::debug(80,24,"Key.Pressed(Down)\nKey.Pressed(Enter)").build().unwrap();
        assert_eq!(connect(&sites),Some(sites[1].clone())); app.run();
        let app = App::debug(80,24,"Key.Pressed(Escape)").build().unwrap();
        assert!(manage(&sites).is_none()); app.run();
        for (kind,script,expected) in [
            (PromptKind::Secret,"Key.TypeText('fixture-only')\nKey.Pressed(Enter)","fixture-only"),
            (PromptKind::Text,"Key.TypeText('no')\nKey.Pressed(Enter)","no"),
            (PromptKind::Conflict,"Key.Pressed(Alt+S)","skip"),
            (PromptKind::Conflict,"Key.Pressed(Escape)","cancel"),
            (PromptKind::Conflict,"Key.Pressed(Enter)","cancel"),
            (PromptKind::Conflict,"Key.Pressed(Alt+R)","replace"),
        ] {
            let app = App::debug(80,24,&format!("{script}\nPaint.Enable(false)\nPaint.Enable(false)\nKey.Pressed(Escape)")).build().unwrap();
            let answer = prompt("Fixture prompt",kind);
            let actual = match &answer { Answer::Text(value) => value.as_str(), Answer::Skip => "skip", Answer::Replace => "replace", Answer::Cancel => "cancel" };
            assert_eq!(actual,expected); app.run();
        }
    }
}
