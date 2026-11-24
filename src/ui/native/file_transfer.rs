mod dialogs;
mod panes;

use crate::runtime::transfer::{Answer, Command, Event, Item, Kind, PromptKind, Protocol, Side, Site, TransferService};
use appcui::prelude::*;
use panes::{Action, Panes};
use std::time::Duration;

#[Window(events = TimerEvents+WindowEvents)]
pub(crate) struct TransferWindow {
    service: TransferService, panes: Handle<Panes>, sites: Vec<Site>,
    prompt: Option<(u64,String,PromptKind)>, saving_sites: bool, listing: bool, canceling: bool, updating: bool, closing: bool,
}
impl TransferWindow {
    pub(crate) fn new() -> anyhow::Result<Self> { Self::with_service(TransferService::new()?) }
    fn with_service(service: TransferService) -> anyhow::Result<Self> {
        let mut window = Self {
            base: Window::new("File Transfer",layout!("a:c,w:90%,h:85%"),window::Flags::Sizeable),
            service, panes: Handle::None, sites: Vec::new(), prompt: None,
            saving_sites: false, listing: false, canceling: false, updating: false, closing: false,
        };
        window.timer().ok_or_else(|| anyhow::anyhow!("File Transfer timer unavailable"))?.start(Duration::from_millis(50));
        window.panes = window.add(Panes::new());
        window.base.request_focus_for_control(window.panes);
        Ok(window)
    }
    pub(crate) fn constrain_to(&mut self, available: Size) {
        self.set_size_bounds(1,1,available.width.clamp(1,u16::MAX as u32) as u16,available.height.clamp(1,u16::MAX as u32) as u16);
    }
    pub(crate) fn suspend(&mut self) {
        let handle = self.panes;
        if let Some(panes) = self.control_mut(handle) { panes.take_action(); }
    }
    pub(crate) fn is_closing(&self) -> bool { self.closing }
    fn stop(&mut self) { self.closing = true; self.prompt = None; self.service.shutdown(); }
    pub(crate) fn shutdown(&mut self) { self.stop(); self.close(); }

    fn submit(&mut self, cmd: Command) {
        let connecting = matches!(&cmd,Command::Connect(_));
        let disconnecting = matches!(&cmd,Command::Disconnect);
        let status = match &cmd {
            Command::Connect(site) => format!("Connecting to {}…",site.host),
            Command::Disconnect => "Disconnecting…".into(),
            Command::SaveSites(_) => "Saving sites…".into(),
            Command::List(_,path) => format!("Opening {}…",path.display()),
            Command::Copy { from: Side::Local,.. } => "Uploading…".into(),
            Command::Copy { from: Side::Remote,.. } => "Downloading…".into(),
        };
        let saving_sites = matches!(&cmd,Command::SaveSites(_));
        let listing = matches!(&cmd,Command::List(..));
        let result = self.service.submit(cmd);
        if result.is_ok() { self.saving_sites = saving_sites; self.listing = listing; }
        if result.is_ok() && disconnecting { self.canceling = true; self.prompt = None; }
        let handle = self.panes;
        if let Some(panes) = self.control_mut(handle) {
            match result {
                Ok(()) => { panes.connecting |= connecting; panes.busy = true; panes.notice(status,false); }
                Err(error) => panes.notice(error.to_string(),true),
            }
        }
    }
    fn event(&mut self, event: Event) {
        let handle = self.panes;
        match event {
            Event::Sites(sites) => {
                self.sites = sites;
                if self.saving_sites && let Some(panes) = self.control_mut(handle) { panes.notice("Sites saved.".into(),false); }
            }
            Event::Prompt { id,message,kind } => {
                if !self.canceling { self.prompt = Some((id,message,kind)); }
            }
            Event::Busy(busy) => {
                if !busy { self.canceling = false; self.saving_sites = false; self.listing = false; self.prompt = None; }
                if let Some(panes) = self.control_mut(handle) { panes.busy = busy; if !busy { panes.connecting = false; } }
            }
            Event::Listing(side,path,items) => {
                let listing = self.listing;
                if let Some(panes) = self.control_mut(handle) {
                    if listing { panes.notice(format!("{} entries in {}",items.len(),path.display()),false); }
                    panes.list(side,path,items);
                }
            }
            Event::Connected(label) => {
                self.set_title(&format!("File Transfer — {label}"));
                if let Some(panes) = self.control_mut(handle) { panes.connected = true; panes.connecting = false; panes.notice(format!("Connected to {label}."),false); }
            }
            Event::Disconnected => {
                self.set_title("File Transfer");
                if let Some(panes) = self.control_mut(handle) { panes.disconnected(); panes.notice("Disconnected.".into(),false); }
            }
            Event::Status(message) | Event::Done(message) => {
                if let Some(panes) = self.control_mut(handle) { panes.notice(message,false); }
            }
            Event::Error(message) => {
                if let Some(panes) = self.control_mut(handle) { panes.notice(message,true); }
            }
            Event::Progress { file,files,bytes } => {
                if let Some(panes) = self.control_mut(handle) { panes.notice(format!("{files} files · {bytes} B · {file}"),false); }
            }
        }
    }
    fn action(&mut self, action: Action) {
        match action {
            Action::Submit(cmd) => self.submit(cmd),
            Action::Connect => { if let Some(site) = dialogs::connect(&self.sites) { self.submit(Command::Connect(site)); } }
            Action::Sites => {
                if let Some(sites) = dialogs::manage(&self.sites) && sites != self.sites { self.submit(Command::SaveSites(sites)); }
            }
            Action::Cancel => {
                self.canceling = true; self.prompt = None; self.service.cancel();
                let handle = self.panes;
                if let Some(panes) = self.control_mut(handle) { panes.notice("Canceling…".into(),false); }
            }
            Action::Details => {
                if let Some(message) = self.control(self.panes).map(|panes| panes.status.clone()) { dialogs::show_status(&message); }
            }
        }
    }
    fn tick(&mut self) -> bool {
        let mut changed = false;
        for _ in 0..64 {
            let Some(event) = self.service.poll() else { break; };
            self.event(event); changed = true;
        }
        if !self.is_visible() || !self.has_focus() { self.suspend(); return changed; }
        let handle = self.panes;
        if let Some(action) = self.control_mut(handle).and_then(Panes::take_action) { self.action(action); changed = true; }
        if !self.closing && self.is_visible() && self.has_focus() && let Some((id,message,kind)) = self.prompt.take() {
            let answer = dialogs::prompt(&message,kind);
            self.service.answer(id,answer);
            changed = true;
        }
        changed
    }
}
impl TimerEvents for TransferWindow {
    fn on_update(&mut self,_: u64) -> EventProcessStatus {
        // A modal runs a nested event loop, including timers for the background window.
        if self.closing || self.updating { return EventProcessStatus::Ignored; }
        self.updating = true;
        let changed = self.tick();
        self.updating = false;
        if changed { EventProcessStatus::Processed } else { EventProcessStatus::Ignored }
    }
}
impl WindowEvents for TransferWindow {
    fn on_cancel(&mut self) -> ActionRequest { self.stop(); ActionRequest::Allow }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDir, UI_LOCK};
    use std::sync::{Arc, atomic::{AtomicUsize,Ordering}};

    #[Desktop(events = DesktopEvents, overwrite = OnResize)]
    struct PromptProbe { path: std::path::PathBuf, window: Handle<TransferWindow>, observed: Arc<AtomicUsize> }
    impl DesktopEvents for PromptProbe {
        fn on_start(&mut self) {
            let service = TransferService::at(self.path.join("sites.json"),self.path.clone()).unwrap();
            let mut window = TransferWindow::with_service(service).unwrap();
            window.set_visible(false);
            let deadline = std::time::Instant::now()+Duration::from_secs(4);
            while window.control(window.panes).unwrap().busy {
                window.tick(); assert!(std::time::Instant::now()<deadline); std::thread::yield_now();
            }
            window.event(Event::Prompt { id:7, message:"Fixture login".into(), kind:PromptKind::Secret });
            self.window = self.add_window(window);
            self.add_window(Window::new("Other app",layout!("a:c,w:30,h:10"),window::Flags::None));
        }
    }
    impl OnResize for PromptProbe {
        fn on_resize(&mut self,_: Size, size: Size) {
            let bit = match size.width { 79 => 1,78 => 2,77 => 4,_ => return };
            if self.observed.fetch_or(bit,Ordering::Relaxed) & bit != 0 { return; }
            let handle = self.window;
            let Some(window) = self.window_mut(handle) else { return; };
            match size.width {
                79 => {
                    window.tick();
                    assert!(window.prompt.is_some());
                    window.set_visible(true);
                }
                78 => {
                    window.tick();
                    assert!(window.prompt.is_some());
                    window.request_focus();
                }
                77 => {
                    assert!(window.has_focus());
                    TimerEvents::on_update(window,0);
                    assert!(window.prompt.is_none());
                    assert!(!window.is_closing());
                }
                _ => {}
            }
        }
    }
    #[test]
    fn hidden_and_unfocused_window_defers_authentication_until_focused() {
        let _lock = UI_LOCK.lock().unwrap();
        let dir = TestDir::new(); let observed = Arc::new(AtomicUsize::new(0));
        let probe = PromptProbe { base: Desktop::new(), path: dir.path().into(), window: Handle::None, observed: observed.clone() };
        App::debug(80,24,"Paint.Enable(false)\nResize(79,24)\nResize(78,24)\nResize(77,24)\nKey.Pressed(Escape)")
            .desktop(probe).timers_count(4).build().unwrap().run();
        assert_eq!(observed.load(Ordering::Relaxed),7);
    }
}
