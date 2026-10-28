use super::clipboard::Clipboard;

#[test]
fn check_clipboard_api() {
    Clipboard::set_text("Hello");
    assert!(Clipboard::has_text());
    assert_eq!(Clipboard::text(),Some("Hello".to_string()));
    Clipboard::set_text("ăȚțĂÂâă");
    assert_eq!(Clipboard::text(),Some("ăȚțĂÂâă".to_string()));
    Clipboard::set_text("Hello - 🌷🌸💮💮🌻🌻🏵️ - Test");
    assert_eq!(Clipboard::text(),Some("Hello - 🌷🌸💮💮🌻🌻🏵️ - Test".to_string()));
    Clipboard::set_text("");
}