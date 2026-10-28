use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::io::Read;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub const MAX_PASSWORD_BYTES: usize = 1024;
#[derive(Clone, PartialEq, Eq)]
pub struct LockCredential {
    hash: Zeroizing<String>,
}
impl LockCredential {
    pub fn new(password: &str, confirmation: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(!password.is_empty(), "Enter a password before locking.");
        anyhow::ensure!(
            password.len() <= MAX_PASSWORD_BYTES,
            "Password is too long."
        );
        anyhow::ensure!(
            password == confirmation,
            "Passwords do not match. Enter both again."
        );
        let mut salt = [0u8; 16];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut salt)?;
        let salt = SaltString::encode_b64(&salt)
            .map_err(|error| anyhow::anyhow!("Could not prepare a lock password: {error}"))?;
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|error| anyhow::anyhow!("Could not prepare a lock password: {error}"))?
            .to_string();
        Ok(Self {
            hash: Zeroizing::new(hash),
        })
    }
    pub fn verify(&self, password: &str) -> bool {
        if password.len() > MAX_PASSWORD_BYTES {
            return false;
        }
        PasswordHash::new(&self.hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    }
}
impl std::fmt::Debug for LockCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LockCredential([redacted])")
    }
}
impl TryFrom<String> for LockCredential {
    type Error = anyhow::Error;
    fn try_from(encoded: String) -> anyhow::Result<Self> {
        let encoded = Zeroizing::new(encoded);
        anyhow::ensure!(encoded.len() <= 256, "Invalid saved lock credential.");
        let hash = PasswordHash::new(&encoded)
            .map_err(|_| anyhow::anyhow!("Invalid saved lock credential."))?;
        // PHC parameters control the verifier's actual work. Accept only our bounded format.
        let mut salt_bytes = [0u8; 64];
        let salt_length = hash
            .salt
            .and_then(|salt| salt.decode_b64(&mut salt_bytes).ok())
            .map(|salt| salt.len());
        anyhow::ensure!(
            hash.algorithm.as_str() == "argon2id"
                && hash.version == Some(19)
                && hash.params.as_str() == "m=19456,t=2,p=1"
                && salt_length == Some(16)
                && hash.hash.is_some_and(|output| output.len() == 32),
            "Unsupported saved lock credential. Expected Argon2id v19 with the application's standard parameters."
        );
        Ok(Self { hash: encoded })
    }
}
impl Serialize for LockCredential {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hash)
    }
}
impl<'de> Deserialize<'de> for LockCredential {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persisted_verifiers_reject_unknown_algorithms_and_unbounded_work() {
        let credential = LockCredential::new("saved", "saved").unwrap();
        let encoded = credential.hash.to_string();
        for bad in [
            encoded.replace("argon2id", "argon2i"),
            encoded.replace("v=19", "v=16"),
            encoded.replace("m=19456", "m=4294967295"),
            encoded.replace("t=2", "t=4294967295"),
            encoded.replace("p=1", "p=8"),
            "plaintext".to_owned(),
        ] {
            assert!(LockCredential::try_from(bad).is_err());
        }
        assert!(LockCredential::try_from(encoded).unwrap().verify("saved"));
    }
    #[test]
    fn credential_requires_confirmation_and_verifies_only_the_same_password() {
        assert!(LockCredential::new("", "").is_err());
        assert!(LockCredential::new("one", "two").is_err());
        assert!(
            LockCredential::new(
                &"x".repeat(MAX_PASSWORD_BYTES + 1),
                &"x".repeat(MAX_PASSWORD_BYTES + 1)
            )
            .is_err()
        );
        let credential = LockCredential::new("local 🔒 pass", "local 🔒 pass").unwrap();
        assert!(credential.verify("local 🔒 pass"));
        assert!(!credential.verify("wrong"));
        assert!(!credential.verify(""));
        let another = LockCredential::new("local 🔒 pass", "local 🔒 pass").unwrap();
        assert_ne!(*credential.hash, *another.hash);
        assert!(credential.hash.starts_with("$argon2id$"));
    }
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
}

use appcui::prelude::*;
use zeroize::Zeroize;

#[CustomControl(overwrite = OnPaint+OnKeyPressed)]
pub(crate) struct SecretInput {
    pub(crate) value: Zeroizing<String>,
    selected_all: bool,
}
impl SecretInput {
    pub(crate) fn new(layout: Layout) -> Self {
        Self {
            base: ControlBase::new(layout, true),
            value: Zeroizing::new(String::with_capacity(MAX_PASSWORD_BYTES)),
            selected_all: false,
        }
    }
    pub(crate) fn clear(&mut self) {
        self.value.zeroize();
        self.selected_all = false;
    }
}
impl OnPaint for SecretInput {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        let attr = if self.selected_all {
            theme.editor.pressed_or_selected
        } else if self.has_focus() {
            theme.editor.focused
        } else {
            theme.editor.normal
        };
        surface.clear(Character::with_attributes(' ', attr));
        let width = surface.size().width;
        let count = (self.value.chars().count() as u32).min(width.saturating_sub(2));
        surface.fill_horizontal_line_with_size(1, 0, count, Character::with_attributes('*', attr));
        if self.has_focus() {
            surface.set_cursor((count + 1).min(width - 1) as i32, 0);
        }
    }
}
impl OnKeyPressed for SecretInput {
    fn on_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
        match key.value() {
            key!("Ctrl+U") => {
                self.clear();
                return EventProcessStatus::Processed;
            }
            key!("Ctrl+A") => {
                self.selected_all = true;
                return EventProcessStatus::Processed;
            }
            key!("Back") => {
                if self.selected_all {
                    self.clear();
                } else {
                    self.value.pop();
                }
                return EventProcessStatus::Processed;
            }
            _ => {}
        }
        if !key
            .modifier
            .contains_one(KeyModifier::Alt | KeyModifier::Ctrl)
            && !character.is_control()
        {
            if self.selected_all {
                self.clear();
            }
            if self.value.len() + character.len_utf8() <= MAX_PASSWORD_BYTES {
                self.value.push(character);
            }
            return EventProcessStatus::Processed;
        }
        EventProcessStatus::Ignored
    }
}
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
mod ui_tests {
    use super::*;
    #[test]
    fn secret_input_masks_unicode_and_supports_clearing() {
        let mut input = SecretInput::new(layout!("x:0,y:0,w:20,h:1"));
        for ch in "local 🔒".chars() {
            OnKeyPressed::on_key_pressed(&mut input, Key::None, ch);
        }
        let mut surface = Surface::new(20, 1);
        input.on_paint(&mut surface, &Theme::default());
        let painted: String = (0..20).map(|x| surface.char(x, 0).unwrap().code).collect();
        assert!(painted.contains("*******"));
        assert!(!painted.contains("local"));
        assert!(!painted.contains('🔒'));
        OnKeyPressed::on_key_pressed(&mut input, key!("Back").into(), '\0');
        assert_eq!(input.value.as_str(), "local ");
        OnKeyPressed::on_key_pressed(&mut input, key!("Ctrl+U").into(), '\0');
        assert!(input.value.is_empty());
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
    fn lock_setup_can_cancel_and_only_the_password_unlocks_the_full_screen() {
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
