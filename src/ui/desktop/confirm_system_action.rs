use crate::platform::linux::SystemAction;
use appcui::prelude::*;

#[ModalWindow(response: bool, events: ButtonEvents)]
pub struct ConfirmSystemAction {
    cancel: Handle<Button>,
    confirm: Handle<Button>,
}
impl ConfirmSystemAction {
    pub fn new(action: SystemAction) -> Self {
        let mut dialog = Self {
            base: ModalWindow::new(
                action.label(),
                layout!("a:c,w:58,h:10"),
                window::Flags::None,
            ),
            cancel: Handle::None,
            confirm: Handle::None,
        };
        let verb = if action == SystemAction::Shutdown {
            "Shut down"
        } else {
            "Restart"
        };
        dialog.add(Label::new(&format!("{verb} this computer?\nAll applications in this session may be closed.\nSave your work before continuing."),layout!("l:2,t:1,r:2,h:4")));
        // Register Cancel first and explicitly request focus: Enter must never power off by default.
        dialog.cancel = dialog.add(button!("&Cancel,l:10,b:0,w:14"));
        dialog.confirm = dialog.add(Button::new(&format!("&{verb}"), layout!("r:10,b:0,w:14")));
        dialog.base.request_focus_for_control(dialog.cancel);
        dialog
    }
}
impl ButtonEvents for ConfirmSystemAction {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.confirm {
            self.exit_with(true);
        } else {
            self.exit();
        }
        EventProcessStatus::Processed
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    fn debug(script: &str) -> appcui::system::Builder {
        App::debug(80, 24, &format!("{script}\nPaint('end')"))
    }

    #[test]
    fn confirmation_cancels_by_default_and_requires_explicit_confirmation_at_80x24() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        for key in ["Enter", "Escape"] {
            let app = debug(&format!("Key.Pressed({key})")).build().unwrap();
            assert_eq!(
                ConfirmSystemAction::new(SystemAction::Shutdown).show(),
                None
            );
            app.run();
        }
        let app = debug("Key.Pressed(Tab)\nKey.Pressed(Enter)")
            .build()
            .unwrap();
        assert_eq!(
            ConfirmSystemAction::new(SystemAction::Restart).show(),
            Some(true)
        );
        app.run();
    }
}
