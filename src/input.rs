//! Turns raw bytes read from the console's terminal into text to broadcast.

/// Ctrl+Q and Ctrl+G close the console instead of being broadcast.
pub const QUIT: u8 = 0x11;
pub const CANCEL: u8 = 0x07;
/// Ctrl+] opens the target picker instead of being broadcast.
pub const PICK: u8 = 0x1d;

/// Splits `bytes` at the first key the console keeps for itself: the bytes
/// before it, the key, and the bytes after it.
pub fn split_reserved(bytes: &[u8]) -> (&[u8], Option<u8>, &[u8]) {
    match bytes
        .iter()
        .position(|&b| matches!(b, QUIT | CANCEL | PICK))
    {
        Some(i) => (&bytes[..i], Some(bytes[i]), &bytes[i + 1..]),
        None => (bytes, None, &[]),
    }
}

/// Splits the byte stream on UTF-8 boundaries. A multi-byte character can
/// arrive across two reads, so an incomplete tail is held until the next feed.
#[derive(Default)]
pub struct Decoder {
    pending: Vec<u8>,
}

impl Decoder {
    pub fn feed(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);

        let mut text = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(s) => {
                    text.push_str(s);
                    self.pending.clear();
                    break;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    // Safe: from_utf8 just confirmed this prefix is valid.
                    text.push_str(std::str::from_utf8(&self.pending[..valid]).unwrap());
                    match e.error_len() {
                        // Invalid bytes: replace them and keep decoding the rest.
                        Some(len) => {
                            text.push(char::REPLACEMENT_CHARACTER);
                            self.pending.drain(..valid + len);
                        }
                        // Incomplete character at the end: wait for more bytes.
                        None => {
                            self.pending.drain(..valid);
                            break;
                        }
                    }
                }
            }
        }
        text
    }

    /// Gives up on a held incomplete character, for when no more bytes of it
    /// will come (a reserved key interrupted it).
    pub fn flush(&mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }
        self.pending.clear();
        char::REPLACEMENT_CHARACTER.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_ascii_and_control_bytes_through() {
        let mut d = Decoder::default();
        assert_eq!(d.feed(b"ls\x1b[A\x03\r"), "ls\x1b[A\x03\r");
    }

    #[test]
    fn splits_at_reserved_keys() {
        assert_eq!(
            split_reserved(b"ab\x11cd"),
            (&b"ab"[..], Some(QUIT), &b"cd"[..])
        );
        assert_eq!(
            split_reserved(b"\x1d1\x11"),
            (&b""[..], Some(PICK), &b"1\x11"[..])
        );
        assert_eq!(
            split_reserved(b"a\x07b"),
            (&b"a"[..], Some(CANCEL), &b"b"[..])
        );
        assert_eq!(split_reserved(b"ab"), (&b"ab"[..], None, &b""[..]));
    }

    #[test]
    fn holds_split_multibyte_character_until_complete() {
        let mut d = Decoder::default();
        let s = "日本".as_bytes(); // 6 bytes, 3 per character
        assert_eq!(d.feed(&s[..4]), "日");
        assert_eq!(d.feed(&s[4..]), "本");
    }

    #[test]
    fn replaces_invalid_bytes() {
        let mut d = Decoder::default();
        assert_eq!(d.feed(b"a\xffb"), "a\u{fffd}b");
    }

    #[test]
    fn flush_replaces_incomplete_tail() {
        let mut d = Decoder::default();
        assert_eq!(d.feed(&"日".as_bytes()[..2]), "");
        assert_eq!(d.flush(), "\u{fffd}");
        assert_eq!(d.flush(), "");
    }
}
