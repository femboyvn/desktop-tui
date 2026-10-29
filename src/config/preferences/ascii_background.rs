use serde::{Deserialize, Serialize};

pub(crate) const MAX_ASCII_BACKGROUND_BYTES: usize = 16 * 1024;

/// Plain ASCII art, normalized to LF and four-column tab stops, at most 16 KiB.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct AsciiBackground(String);

impl AsciiBackground {
    pub fn text(&self) -> &str { &self.0 }
    pub fn is_empty(&self) -> bool { self.0.is_empty() }
}
impl TryFrom<String> for AsciiBackground {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() > MAX_ASCII_BACKGROUND_BYTES {
            return Err("ASCII background exceeds the 16 KiB limit.".into());
        }
        let value = value.replace("\r\n", "\n");
        let mut text = String::with_capacity(value.len());
        let mut column = 0;
        for byte in value.bytes() {
            match byte {
                b'\n' => { text.push('\n'); column = 0; }
                b'\t' => {
                    let spaces = 4 - column % 4;
                    text.extend(std::iter::repeat_n(' ', spaces));
                    column += spaces;
                }
                b' '..=b'~' => { text.push(char::from(byte)); column += 1; }
                _ => return Err("Use plain ASCII text with LF or CRLF line endings; ANSI escape codes and non-ASCII characters are unsupported.".into()),
            }
            if text.len() > MAX_ASCII_BACKGROUND_BYTES {
                return Err("ASCII background exceeds 16 KiB after expanding tabs.".into());
            }
        }
        Ok(Self(text))
    }
}
impl From<AsciiBackground> for String {
    fn from(value: AsciiBackground) -> Self { value.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_background_preserves_spaces_and_normalizes_crlf_and_tab_stops() {
        let art = AsciiBackground::try_from(" /\\_/\\  \r\n\t(o.o)\r\n >\t<\n".to_owned()).unwrap();
        assert_eq!(art.text(), " /\\_/\\  \n    (o.o)\n >  <\n");
        assert!(AsciiBackground::default().is_empty());
    }

    #[test]
    fn ascii_background_rejects_escape_codes_non_ascii_and_oversized_art() {
        for invalid in ["\x1b[31mred".to_owned(), "cat\0".to_owned(), "é".to_owned(), "x\ry".to_owned(), "\x7f".to_owned(), "x".repeat(16385), "\t".repeat(4097)] {
            assert!(AsciiBackground::try_from(invalid).is_err());
        }
        assert!(AsciiBackground::try_from("\\".repeat(16384)).is_ok());
    }
}
