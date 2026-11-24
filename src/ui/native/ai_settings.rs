use crate::config::ai::{AiPreferences, AiProvider, ApiKey, AiSettingsStore};
use appcui::prelude::*;
use zeroize::{Zeroize, Zeroizing};

#[CustomControl(overwrite = OnPaint+OnKeyPressed)]
struct ApiKeyInput { value: Zeroizing<String>, selected: bool }
impl ApiKeyInput {
    fn new(layout: Layout) -> Self { Self { base: ControlBase::new(layout,true),value: Zeroizing::new(String::new()),selected: false } }
    fn insert(&mut self, text: &str) {
        let text = text.trim();
        if !text.bytes().all(|b| (33..=126).contains(&b)) { return; }
        let length = if self.selected { 0 } else { self.value.len() };
        if length+text.len() > 4096 { return; }
        if self.selected { self.value.zeroize(); self.selected = false; }
        self.value.push_str(text);
    }
}
impl OnPaint for ApiKeyInput {
    fn on_paint(&self,surface: &mut Surface,theme: &Theme) {
        let attr = if self.has_focus() { theme.editor.focused } else { theme.editor.normal };
        surface.clear(Character::with_attributes(' ',attr));
        let width = surface.size().width;
        let count = (self.value.len() as u32).min(width.saturating_sub(1));
        surface.fill_horizontal_line_with_size(0,0,count,Character::with_attributes('*',attr));
        if self.has_focus() && width > 0 { surface.set_cursor(count.min(width-1) as i32,0); }
    }
}
impl OnKeyPressed for ApiKeyInput {
    fn on_paste(&mut self,text: &str) -> EventProcessStatus { self.insert(text); EventProcessStatus::Processed }
    fn on_key_pressed(&mut self,key: Key,ch: char) -> EventProcessStatus {
        match key.value() {
            key!("Ctrl+A") => self.selected = true,
            key!("Ctrl+V") => { if let Some(text) = appcui::system::Clipboard::internal_text() { self.insert(&text); } },
            key!("Ctrl+U") => { self.value.zeroize(); self.selected = false; },
            key!("Back") | key!("Delete") => { if self.selected { self.value.zeroize(); self.selected = false; } else { self.value.pop(); } },
            _ if !key.modifier.contains_one(KeyModifier::Alt|KeyModifier::Ctrl) && !ch.is_control() => self.insert(&ch.to_string()),
            _ => return EventProcessStatus::Ignored,
        }
        EventProcessStatus::Processed
    }
}
struct AiSettingsFields {
    original: AiPreferences, preferred: Handle<ComboBox>, models: [Handle<TextField>;3],
    actions: [Handle<ComboBox>;3], keys: [Handle<ApiKeyInput>;3],
}
impl AiSettingsFields {
    fn add_to(window: &mut Window, ai: &AiPreferences) -> Self {
        let mut view = Self { original: ai.clone(),preferred: Handle::None,
            models: [Handle::None;3],actions: [Handle::None;3],keys: [Handle::None;3] };
        window.add(Label::new("New chats use",layout!("l:1,t:0,w:15,h:1")));
        let mut preferred = ComboBox::new(layout!("l:17,t:0,r:1,h:1"),combobox::Flags::None);
        for provider in AiProvider::ALL { preferred.add(provider.label()); }
        preferred.set_index(AiProvider::ALL.iter().position(|p| *p == ai.default_provider).unwrap_or(0) as u32);
        view.preferred = window.add(preferred);
        for (index,provider) in AiProvider::ALL.into_iter().enumerate() {
            let y = 2+index as i32*3;
            window.add(Label::new(&format!("{} model",provider.label()),LayoutBuilder::new().x(1).y(y).width(15).height(1).build()));
            view.models[index] = window.add(TextField::new(&ai.settings(provider).model,LayoutBuilder::new().left_anchor(17).top_anchor(y).right_anchor(1).height(1).build(),textfield::Flags::None));
            let mut action = ComboBox::new(LayoutBuilder::new().x(1).y(y+1).width(22).height(1).build(),combobox::Flags::None);
            action.add(if ai.settings(provider).api_key.is_empty() { "No key (keep)" } else { "Saved key (keep)" });
            action.add("Replace key"); action.add("Remove key"); action.set_index(0);
            view.actions[index] = window.add(action);
            let mut input = ApiKeyInput::new(LayoutBuilder::new().left_anchor(25).top_anchor(y+1).right_anchor(1).height(1).build()); input.set_enabled(false);
            view.keys[index] = window.add(input);
        }
        view
    }
    fn changed(&self,window: &mut Window,handle: Handle<ComboBox>) -> bool {
        let Some(index) = self.actions.iter().position(|action| *action == handle) else { return false; };
        let enabled = window.control(handle).and_then(ComboBox::index) == Some(1);
        let key = self.keys[index];
        if let Some(input) = window.control_mut(key) { input.value.zeroize(); input.set_enabled(enabled); }
        true
    }
    fn set_enabled(&self,window: &mut Window,enabled: bool) {
        if let Some(control) = window.control_mut(self.preferred) { control.set_enabled(enabled); }
        for index in 0..3 {
            if let Some(control) = window.control_mut(self.models[index]) { control.set_enabled(enabled); }
            if let Some(control) = window.control_mut(self.actions[index]) { control.set_enabled(enabled); }
            let replace = window.control(self.actions[index]).and_then(ComboBox::index) == Some(1);
            if let Some(control) = window.control_mut(self.keys[index]) { control.set_enabled(enabled && replace); }
        }
    }
    fn draft(&self,window: &Window) -> Result<AiPreferences,String> {
        let mut ai = self.original.clone();
        ai.default_provider = AiProvider::ALL[window.control(self.preferred).and_then(ComboBox::index).unwrap_or(0).min(2) as usize];
        for (index,provider) in AiProvider::ALL.into_iter().enumerate() {
            let settings = ai.settings_mut(provider);
            let model = window.control(self.models[index]).map(|field| field.text().trim()).unwrap_or("");
            if model.len() > 256 || model.chars().any(char::is_control) { return Err("Model IDs must be at most 256 characters without control characters.".into()); }
            settings.model = model.to_owned();
            match window.control(self.actions[index]).and_then(ComboBox::index).unwrap_or(0) {
                1 => { settings.api_key = ApiKey::new(window.control(self.keys[index]).map(|input| input.value.as_str()).unwrap_or(""))?;
                    if settings.api_key.is_empty() { return Err(format!("Enter the replacement {} API key.",provider.label())); } },
                2 => settings.api_key = ApiKey::default(), _ => {},
            }
        }
        Ok(ai)
    }
}

enum Job { Load(AiSettingsStore), Save(AiSettingsStore,Box<(AiPreferences,AiPreferences)>) }
enum Completion { Loaded(AiPreferences), Saved(AiPreferences) }
#[ModalWindow(response = AiPreferences,events = ButtonEvents+ComboBoxEvents+TimerEvents+WindowEvents)]
pub(crate) struct AiSettingsDialog {
    store: AiSettingsStore, fields: Option<AiSettingsFields>, original: AiPreferences,
    save: Handle<Button>, cancel: Handle<Button>, status: Handle<Label>, saving: bool,
    worker: Option<crate::runtime::latest::LatestWorker<Job,Result<Completion,String>>>,
}
impl AiSettingsDialog {
    pub(crate) fn new(store: AiSettingsStore) -> Self {
        let mut dialog = Self { base: ModalWindow::new("AI Chat Settings",layout!("a:c,w:66,h:18"),window::Flags::None),
            store,fields: None,original: AiPreferences::default(),save: Handle::None,cancel: Handle::None,status: Handle::None,saving: false,worker: None };
        dialog.status = dialog.add(Label::new("Loading AI Chat settings...",layout!("l:1,r:1,b:2,h:2")));
        dialog.save = dialog.add(Button::new("&Save",layout!("r:1,b:0,w:12")));
        dialog.cancel = dialog.add(Button::new("&Cancel",layout!("r:14,b:0,w:12")));
        dialog.start(Job::Load(dialog.store.clone())); dialog
    }
    fn start(&mut self,job: Job) {
        self.saving = matches!(job,Job::Save(..));
        match crate::runtime::latest::LatestWorker::new(|job| match job {
            Job::Load(store) => store.load().map(Completion::Loaded).map_err(|error| format!("{error:#}")),
            Job::Save(store,values) => { let (draft,expected) = *values; store.save(&draft,&expected).map(|()| Completion::Saved(draft)).map_err(|error| format!("{error:#}")) },
        }) {
            Ok(mut worker) => {
                if let Some(timer) = self.timer() { timer.start(std::time::Duration::from_millis(40)); worker.submit(job); self.worker = Some(worker); }
                else { self.saving = false; self.set_status("AI settings timer unavailable"); }
            }
            Err(error) => { self.saving = false; self.set_status(&format!("Cannot start AI settings worker: {error}")); }
        }
        self.buttons();
    }
    fn set_status(&mut self,text: &str) { let handle = self.status; if let Some(label) = self.control_mut(handle) { label.set_caption(text); } }
    fn buttons(&mut self) {
        let ready = self.worker.is_none() && self.fields.is_some();
        if let Some(fields) = &self.fields { fields.set_enabled(&mut self.base,ready); }
        let handle = self.save;
        if let Some(button) = self.control_mut(handle) { button.set_enabled(ready); }
        let enabled = !self.saving; let handle = self.cancel; if let Some(button) = self.control_mut(handle) { button.set_enabled(enabled); }
    }
    fn save_draft(&mut self) {
        if self.worker.is_some() { return; }
        match self.fields.as_ref().map(|fields| fields.draft(&self.base)) {
            Some(Ok(draft)) => { self.set_status("Saving AI Chat settings..."); self.start(Job::Save(self.store.clone(),Box::new((draft,self.original.clone())))); },
            Some(Err(error)) => self.set_status(&error),None => {},
        }
    }
}
impl ButtonEvents for AiSettingsDialog {
    fn on_pressed(&mut self,handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.save { self.save_draft(); }
        else if handle == self.cancel && !self.saving { self.exit(); }
        EventProcessStatus::Processed
    }
}
impl ComboBoxEvents for AiSettingsDialog {
    fn on_selection_changed(&mut self,handle: Handle<ComboBox>) -> EventProcessStatus {
        if self.fields.as_ref().is_some_and(|fields| fields.changed(&mut self.base,handle)) { EventProcessStatus::Processed } else { EventProcessStatus::Ignored }
    }
}
impl TimerEvents for AiSettingsDialog {
    fn on_update(&mut self,_: u64) -> EventProcessStatus {
        let Some(result) = self.worker.as_mut().and_then(crate::runtime::latest::LatestWorker::poll) else { return EventProcessStatus::Ignored; };
        self.worker = None; self.saving = false; if let Some(timer) = self.timer() { timer.pause(); }
        match result {
            Ok(Completion::Loaded(ai)) => {
                self.fields = Some(AiSettingsFields::add_to(&mut self.base,&ai)); self.original = ai;
                self.set_status("Keys saved only for AI Chat in ai.toml (owner-only).\nChoose Replace key, then paste. Cancel discards edits.");
            }
            Ok(Completion::Saved(ai)) => self.exit_with(ai),
            Err(error) => {
                if self.fields.is_none() { self.fields = Some(AiSettingsFields::add_to(&mut self.base,&AiPreferences::default())); }
                self.set_status(&error);
            },
        }
        self.buttons(); EventProcessStatus::Processed
    }
}
impl WindowEvents for AiSettingsDialog {
    fn on_accept(&mut self) { self.save_draft(); }
    fn on_cancel(&mut self) -> ActionRequest { if self.saving { ActionRequest::Deny } else { ActionRequest::Allow } }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait(dialog: &mut AiSettingsDialog) {
        let until = std::time::Instant::now()+std::time::Duration::from_secs(4);
        while dialog.worker.is_some() { TimerEvents::on_update(dialog,0); assert!(std::time::Instant::now()<until); std::thread::yield_now(); }
    }
    #[test]
    fn own_dialog_saves_and_cancels_without_system_preferences() {
        let _lock = crate::test_support::UI_LOCK.lock().unwrap(); let dir = crate::test_support::TestDir::new();
        let path = dir.path().join("ai.toml"); let system = dir.path().join("preferences.toml");
        let store = AiSettingsStore::at(path.clone(),system.clone()); store.save(&AiPreferences::default(),&AiPreferences::default()).unwrap();
        std::fs::write(&system,"broken unrelated system settings").unwrap(); let original = std::fs::read(&system).unwrap();
        let app = App::debug(80,24,"").timers_count(8).build().unwrap();
        let mut dialog = AiSettingsDialog::new(store.clone()); wait(&mut dialog);
        assert_eq!(dialog.title(),"AI Chat Settings");
        let fields = dialog.fields.as_ref().unwrap(); let model = fields.models[0]; let action = fields.actions[0]; let key = fields.keys[0];
        dialog.control_mut(model).unwrap().set_text("chat-only-model"); dialog.control_mut(action).unwrap().set_index(1);
        ComboBoxEvents::on_selection_changed(&mut dialog,action); dialog.control_mut(key).unwrap().on_paste("fake-independent-key");
        let save = dialog.save; ButtonEvents::on_pressed(&mut dialog,save); wait(&mut dialog);
        assert_eq!(store.load().unwrap().openai.model,"chat-only-model"); assert_eq!(store.load().unwrap().openai.api_key.expose(),"fake-independent-key");
        assert_eq!(std::fs::read(&system).unwrap(),original); drop(dialog); app.run();
        let before = std::fs::read(&path).unwrap(); let app = App::debug(80,24,"Key.Pressed(Escape)").timers_count(8).build().unwrap();
        let mut dialog = AiSettingsDialog::new(store); wait(&mut dialog); let model = dialog.fields.as_ref().unwrap().models[0]; dialog.control_mut(model).unwrap().set_text("cancelled-model");
        assert!(dialog.show().is_none()); app.run(); assert_eq!(std::fs::read(&path).unwrap(),before); assert_eq!(std::fs::read(system).unwrap(),original);
    }

    #[test]
    fn pasted_keys_are_bounded_masked_and_clearable() {
        let mut input = ApiKeyInput::new(layout!("x:0,y:0,w:12,h:1"));
        input.on_paste(" fake-api-secret\n");
        let mut surface = Surface::new(12,1); input.on_paint(&mut surface,&Theme::default());
        let text: String = (0..12).map(|x| surface.char(x,0).unwrap().code).collect();
        assert!(text.contains("***")); assert!(!text.contains("fake"));
        input.on_key_pressed(key!("Ctrl+A").into(),'\0'); input.on_paste("replacement");
        assert_eq!(input.value.as_str(),"replacement"); input.on_paste(&"x".repeat(4097)); assert_eq!(input.value.as_str(),"replacement");
        input.on_key_pressed(key!("Ctrl+U").into(),'\0'); assert!(input.value.is_empty());
    }
}
