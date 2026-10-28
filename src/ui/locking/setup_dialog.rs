use super::secret_input::SecretInput;
use crate::runtime::LockCredential;
use appcui::prelude::*;

#[ModalWindow(response: LockCredential,events: ButtonEvents+WindowEvents)]
pub struct LockSetupDialog {
    password: Handle<SecretInput>,
    confirmation: Handle<SecretInput>,
    status: Handle<Label>,
    lock: Handle<Button>,
    cancel: Handle<Button>,
}
impl LockSetupDialog {
    pub fn new() -> Self {
        let mut dialog = Self {
            base: ModalWindow::new(
                "Terminal Lock",
                layout!("a:c,w:62,h:17"),
                window::Flags::None,
            ),
            password: Handle::None,
            confirmation: Handle::None,
            status: Handle::None,
            lock: Handle::None,
            cancel: Handle::None,
        };
        dialog.add(Label::new(
            "Set a temporary password for this lock session.\nIt is never saved to preferences.",
            layout!("l:2,t:1,r:2,h:2"),
        ));
        dialog.add(Label::new("Password", layout!("l:2,t:4,w:14,h:1")));
        dialog.password = dialog.add(SecretInput::new(layout!("l:18,t:4,r:2,h:1")));
        dialog.add(Label::new("Confirm password", layout!("l:2,t:6,w:16,h:1")));
        dialog.confirmation = dialog.add(SecretInput::new(layout!("l:18,t:6,r:2,h:1")));
        dialog.status=dialog.add(Label::new("Locks only this TUI session; apps keep running.\nOther terminals remain accessible. Stopping Desktop-TUI\nbypasses this lock. Use Screen Lock to lock Linux.",layout!("l:2,t:8,r:2,h:3")));
        dialog.cancel = dialog.add(button!("&Cancel,l:14,b:0,w:14"));
        dialog.lock = dialog.add(button!("&Lock,r:14,b:0,w:14"));
        dialog.base.request_focus_for_control(dialog.password);
        dialog
    }
    fn clear_inputs(&mut self) {
        self.base.control_mut(self.password).unwrap().clear();
        self.base.control_mut(self.confirmation).unwrap().clear();
    }
    fn submit(&mut self) {
        let result = {
            let password = self.base.control(self.password).unwrap();
            let confirmation = self.base.control(self.confirmation).unwrap();
            LockCredential::new(&password.value, &confirmation.value)
        };
        self.clear_inputs();
        match result {
            Ok(credential) => self.exit_with(credential),
            Err(error) => {
                self.base
                    .control_mut(self.status)
                    .unwrap()
                    .set_caption(&error.to_string());
                self.base.request_focus_for_control(self.password);
            }
        }
    }
}
impl ButtonEvents for LockSetupDialog {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle == self.lock {
            self.submit();
        } else if handle == self.cancel {
            self.clear_inputs();
            self.exit();
        } else {
            return EventProcessStatus::Ignored;
        }
        EventProcessStatus::Processed
    }
}
impl WindowEvents for LockSetupDialog {
    fn on_accept(&mut self) {
        if self.control(self.password).unwrap().has_focus() {
            self.base.request_focus_for_control(self.confirmation);
        } else {
            self.submit();
        }
    }
    fn on_cancel(&mut self) -> ActionRequest {
        self.clear_inputs();
        ActionRequest::Allow
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_setup_can_cancel_and_requires_matching_passwords() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        let debug = |script: &str| App::debug(80, 24, &format!("{script}\nPaint('end')"));
        for script in [
            "Key.Pressed(Escape)",
            "Key.TypeText('one')\nKey.Pressed(Tab)\nKey.TypeText('two')\nKey.Pressed(Enter)\nKey.Pressed(Escape)",
        ] {
            let app = debug(script).build().unwrap();
            assert!(LockSetupDialog::new().show().is_none());
            app.run();
        }
        let app = debug(
            "Key.TypeText('unlock')\nKey.Pressed(Tab)\nKey.TypeText('unlock')\nKey.Pressed(Enter)",
        )
        .build()
        .unwrap();
        let credential = LockSetupDialog::new().show().unwrap();
        app.run();
        assert!(credential.verify("unlock"));
    }
}
