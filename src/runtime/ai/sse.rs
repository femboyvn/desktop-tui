//! Bounded byte-oriented SSE framing; UTF-8 is decoded only after a full event.
const MAX_EVENT: usize = 2 * 1024 * 1024;
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Event { pub name: String, pub data: String }
#[derive(Default)]
pub(super) struct Decoder { bytes: Vec<u8>, lines: Vec<Vec<u8>>, size: usize, after_cr: bool, started: bool }
impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Event>, String> {
        let mut events = Vec::new();
        for &byte in bytes {
            if self.after_cr { self.after_cr = false; if byte == b'\n' { continue; } }
            if byte == b'\n' || byte == b'\r' {
                self.after_cr = byte == b'\r';
                if self.bytes.is_empty() {
                    let mut event = Event { name: String::new(), data: String::new() };
                    let mut has_data = false;
                    for line in self.lines.drain(..) {
                        let line = std::str::from_utf8(&line).map_err(|_| "Invalid UTF-8 in response stream")?;
                        let line = if !self.started { self.started = true; line.strip_prefix('\u{feff}').unwrap_or(line) } else { line };
                        let (field, value) = line.split_once(':').unwrap_or((line, ""));
                        let value = value.strip_prefix(' ').unwrap_or(value);
                        match field {
                            "event" => event.name = value.into(),
                            "data" => { if has_data { event.data.push('\n'); } event.data.push_str(value); has_data = true; }
                            _ => (),
                        }
                    }
                    self.size = 0;
                    if has_data { events.push(event); }
                } else { self.lines.push(std::mem::take(&mut self.bytes)); }
            } else {
                self.size += 1;
                if self.size > MAX_EVENT { return Err("Response event exceeds 2 MiB".into()); }
                self.bytes.push(byte);
            }
        }
        Ok(events)
    }
    pub fn finish(&self) -> Result<(), String> {
        if self.bytes.is_empty() && self.lines.is_empty() { Ok(()) }
        else { Err("Response stream ended during an event".into()) }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_byte_split_preserves_unicode_multiline_and_heartbeats() {
        let bytes = "\u{feff}: ping\r\nevent: delta\r\ndata: {\"text\":\"ไทย😀\",\r\ndata: \"n\":1}\r\n\r\n".as_bytes();
        for split in 0..=bytes.len() {
            let mut decoder = Decoder::default();
            let mut result = decoder.push(&bytes[..split]).unwrap();
            result.extend(decoder.push(&bytes[split..]).unwrap());
            assert_eq!(result, vec![Event { name: "delta".into(), data: "{\"text\":\"ไทย😀\",\n\"n\":1}".into() }]);
            decoder.finish().unwrap();
        }
    }
    #[test]
    fn rejects_unterminated_invalid_utf8_and_oversize() {
        let mut decoder = Decoder::default();
        decoder.push(b"data: partial").unwrap();
        assert!(decoder.finish().is_err());
        assert!(Decoder::default().push(b"data: \xff\n\n").is_err());
        assert!(Decoder::default().push(&vec![b'x'; MAX_EVENT + 1]).is_err());
    }
}
