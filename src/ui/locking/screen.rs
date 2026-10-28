use super::secret_input::SecretInput;
use crate::runtime::LockCredential;
use appcui::prelude::*;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
enum UnlockResult {
    Unlocked,
    Incorrect,
    Throttled,
}
struct UnlockGate {
    credential: LockCredential,
    retry_at: Instant,
}
impl UnlockGate {
    fn new(credential: LockCredential) -> Self {
        Self {
            credential,
            retry_at: Instant::now(),
        }
    }
    fn attempt(&mut self, password: &str, now: Instant) -> UnlockResult {
        if now < self.retry_at {
            return UnlockResult::Throttled;
        }
        if self.credential.verify(password) {
            UnlockResult::Unlocked
        } else {
            self.retry_at = now + Duration::from_secs(1);
            UnlockResult::Incorrect
        }
    }
}

#[ModalWindow(response: bool,events: ButtonEvents+WindowEvents)]
pub struct TerminalLockScreen {
    gate: UnlockGate,
    password: Handle<SecretInput>,
    status: Handle<Label>,
    unlock: Handle<Button>,
}
impl TerminalLockScreen {
    pub fn new(credential: LockCredential) -> Self {
        let mut screen = Self {
            base: ModalWindow::new(
                "Terminal locked",
                layout!("d:f"),
                window::Flags::FixedPosition,
            ),
            gate: UnlockGate::new(credential),
            password: Handle::None,
            status: Handle::None,
            unlock: Handle::None,
        };
        screen.set_size_bounds(1, 1, u16::MAX, u16::MAX);
        let mut panel = Panel::with_type("", layout!("a:c,w:56,h:14"), panel::Type::Page);
        panel.add(Label::new(
            "Terminal locked\nYour applications are still running.",
            layout!("l:2,t:1,r:2,h:2"),
        ));
        panel.add(Label::new("Password", layout!("l:6,t:4,r:6,h:1")));
        screen.password = panel.add(SecretInput::new(layout!("l:6,t:5,r:6,h:1")));
        screen.status = panel.add(Label::new(
            "Enter your Terminal Lock password.\nCtrl+U clears the input.",
            layout!("l:2,t:7,r:2,h:3"),
        ));
        screen.unlock = panel.add(button!("&Unlock,l:21,b:0,w:14"));
        let mut boundary = LockInputBoundary::new();
        boundary.add(panel);
        screen.add(boundary);
        screen.base.request_focus_for_control(screen.password);
        screen
    }
    fn try_unlock(&mut self) {
        let result = {
            let input = self.base.control(self.password).unwrap();
            self.gate.attempt(&input.value, Instant::now())
        };
        self.base.control_mut(self.password).unwrap().clear();
        match result {
            UnlockResult::Unlocked => self.exit_with(true),
            UnlockResult::Incorrect => self
                .base
                .control_mut(self.status)
                .unwrap()
                .set_caption("Incorrect password. Wait one second before retrying."),
            UnlockResult::Throttled => self
                .base
                .control_mut(self.status)
                .unwrap()
                .set_caption("Please wait one second before trying again."),
        }
        self.base.request_focus_for_control(self.password);
    }
}
impl ButtonEvents for TerminalLockScreen {
    fn on_pressed(&mut self, handle: Handle<Button>) -> EventProcessStatus {
        if handle != self.unlock {
            return EventProcessStatus::Ignored;
        }
        self.try_unlock();
        EventProcessStatus::Processed
    }
}
impl WindowEvents for TerminalLockScreen {
    fn on_accept(&mut self) {
        self.try_unlock();
    }
    fn on_cancel(&mut self) -> ActionRequest {
        ActionRequest::Deny
    }
}

// This is the only child of the modal, so every focused control bubbles keys
// through this boundary before AppCUI's non-overridable modal key handler.
#[CustomContainer(overwrite = OnKeyPressed)]
struct LockInputBoundary {}
impl LockInputBoundary {
    fn new() -> Self {
        Self {
            base: ContainerBase::new(layout!("d:f"), true),
        }
    }
}
impl OnKeyPressed for LockInputBoundary {
    fn on_key_pressed(&mut self, key: Key, _character: char) -> EventProcessStatus {
        // FixedPosition only prevents mouse moves, not these keyboard modes.
        match key.value() {
            key!("Ctrl+Alt+M") | key!("Ctrl+Alt+R") => EventProcessStatus::Processed,
            _ => EventProcessStatus::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incorrect_unlock_is_throttled_and_later_correct_password_unlocks() {
        let mut gate = UnlockGate::new(LockCredential::new("unlock", "unlock").unwrap());
        let now = Instant::now();
        assert_eq!(gate.attempt("wrong", now), UnlockResult::Incorrect);
        assert_eq!(
            gate.attempt("unlock", now + Duration::from_millis(200)),
            UnlockResult::Throttled
        );
        assert_eq!(
            gate.attempt("unlock", now + Duration::from_secs(1)),
            UnlockResult::Unlocked
        );
    }

    #[test]
    fn lock_input_boundary_blocks_keyboard_move_and_resize_modes() {
        let mut boundary = LockInputBoundary::new();
        assert!(
            OnKeyPressed::on_key_pressed(&mut boundary, key!("Ctrl+Alt+M").into(), '\0')
                == EventProcessStatus::Processed
        );
        assert!(
            OnKeyPressed::on_key_pressed(&mut boundary, key!("Ctrl+Alt+R").into(), '\0')
                == EventProcessStatus::Processed
        );
        assert!(
            OnKeyPressed::on_key_pressed(&mut boundary, key!("Tab").into(), '\0')
                == EventProcessStatus::Ignored
        );
        assert!(
            OnKeyPressed::on_key_pressed(&mut boundary, key!("Enter").into(), '\0')
                == EventProcessStatus::Ignored
        );
    }

    #[test]
    fn lock_remains_fullscreen_through_keyboard_modes_and_terminal_resize() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        let script = "Paint('lock initial')\nCheckHash(0x6BB8842868265047)\nKey.Pressed(Ctrl+Alt+M)\nKey.Pressed(Down)\nKey.Pressed(Escape)\nKey.Pressed(Ctrl+Alt+R)\nKey.Pressed(Ctrl+Left)\nKey.Pressed(Escape)\nMouse.Click(25,10,left)\nPaint('after shortcuts')\nCheckHash(0x6BB8842868265047)\nMouse.Click(0,0,left)\nKey.Pressed(Ctrl+Alt+R)\nKey.Pressed(Ctrl+Left)\nKey.Pressed(Escape)\nMouse.Click(25,10,left)\nPaint('after border focus')\nCheckHash(0x6BB8842868265047)\nResize(90,28)\nPaint('resized lock')\nCheckHash(0x3E003F1D21FFB74F)\nResize(80,24)\nMouse.Click(25,10,left)\nPaint('restored lock')\nCheckHash(0x6BB8842868265047)\nKey.TypeText('unlock')\nKey.Pressed(Enter)\nPaint('end')";
        let app = App::debug(80, 24, script).build().unwrap();
        let credential = LockCredential::new("unlock", "unlock").unwrap();
        assert_eq!(TerminalLockScreen::new(credential).show(), Some(true));
        app.run();
    }

    #[test]
    fn only_the_password_unlocks_the_full_screen() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        let debug = |script: &str| App::debug(80, 24, &format!("{script}\nPaint('end')"));
        let credential = LockCredential::new("unlock", "unlock").unwrap();
        let app=debug("Key.Pressed(Escape)\nKey.Pressed(Alt+C)\nKey.Pressed(Ctrl+C)\nKey.TypeText('unlock')\nKey.Pressed(Enter)").build().unwrap();
        let mut screen = TerminalLockScreen::new(credential);
        assert!(WindowEvents::on_cancel(&mut screen) == ActionRequest::Deny);
        assert_eq!(screen.show(), Some(true));
        app.run();
        let app = debug("Key.TypeText('wrong')\nKey.Pressed(Enter)\nKey.Pressed(Escape)")
            .build()
            .unwrap();
        let credential = LockCredential::new("unlock", "unlock").unwrap();
        assert_eq!(TerminalLockScreen::new(credential).show(), None);
        app.run();
    }
}
