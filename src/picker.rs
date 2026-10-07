//! The target picker: a popup of its own (Ctrl+] in the console) listing the
//! tab's panes one per row, to choose which ones receive the broadcast.

use std::ops::Range;

use crate::input::{CANCEL, PICK, QUIT};
use crate::targets::Target;

/// What the console does after a read typed into the picker.
#[derive(Debug, PartialEq, Eq)]
pub enum Pick {
    /// Keep picking.
    Stay,
    /// Back to the console.
    Done,
    /// Close without going back to the console.
    Quit,
}

#[derive(Default)]
pub struct Picker {
    /// The highlighted row.
    pub cursor: usize,
    /// The first row shown, when the list is taller than its space.
    scroll: usize,
    /// The start of an escape sequence cut off at the end of the last feed,
    /// completed by the next one (or settled by `expire`).
    pending: Vec<u8>,
}

impl Picker {
    /// Applies keys typed into the picker:
    /// - ↑ ↓ / Ctrl+P Ctrl+N / k j move the cursor, Space toggles its pane,
    /// - 1-9 toggle that pane, `a` selects all (or none, when all are selected),
    /// - Enter / Esc / Ctrl+] go back to the console, Ctrl+Q / Ctrl+G quit.
    ///
    /// Other escape sequences are skipped whole, so their digits never toggle
    /// panes; one cut off at the end is held until the next feed.
    pub fn feed(&mut self, targets: &mut [Target], bytes: &[u8]) -> Pick {
        let held = std::mem::take(&mut self.pending);
        let bytes = &[held.as_slice(), bytes].concat();
        let mut i = 0;
        while i < bytes.len() {
            let rest = &bytes[i..];
            i += 1;
            match rest[0] {
                b'1'..=b'9' => {
                    let n = usize::from(rest[0] - b'1');
                    if let Some(t) = targets.get_mut(n) {
                        t.selected = !t.selected;
                        self.cursor = n;
                    }
                }
                b' ' => {
                    if let Some(t) = targets.get_mut(self.cursor) {
                        t.selected = !t.selected;
                    }
                }
                b'a' => {
                    let all = targets.iter().all(|t| t.selected);
                    for t in targets.iter_mut() {
                        t.selected = !all;
                    }
                }
                b'k' | 0x10 => self.up(),
                b'j' | 0x0e => self.down(targets.len()),
                b'\r' | b'\n' | PICK => return Pick::Done,
                QUIT | CANCEL => return Pick::Quit,
                0x1b => {
                    let Some(len) = escape_len(rest) else {
                        self.pending = rest.to_vec();
                        return Pick::Stay;
                    };
                    match &rest[..len] {
                        b"\x1b[A" | b"\x1bOA" => self.up(),
                        b"\x1b[B" | b"\x1bOB" => self.down(targets.len()),
                        // A lone Esc goes back to the console.
                        b"\x1b" => return Pick::Done,
                        _ => {}
                    }
                    i += len - 1;
                }
                _ => {}
            }
        }
        Pick::Stay
    }

    /// Whether an escape sequence is waiting for the rest of it.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Settles a held escape sequence when the rest has not come: a lone Esc
    /// goes back to the console, anything else is dropped.
    pub fn expire(&mut self) -> Pick {
        let held = std::mem::take(&mut self.pending);
        if held == b"\x1b" {
            Pick::Done
        } else {
            Pick::Stay
        }
    }

    fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn down(&mut self, len: usize) {
        if self.cursor + 1 < len {
            self.cursor += 1;
        }
    }

    /// The rows of a `len`-row list to show in `rows` lines, scrolled so the
    /// cursor is visible.
    pub fn window(&mut self, len: usize, rows: usize) -> Range<usize> {
        self.cursor = self.cursor.min(len.saturating_sub(1));
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + rows {
            self.scroll = self.cursor + 1 - rows;
        }
        self.scroll = self.scroll.min(len.saturating_sub(rows));
        self.scroll..len.min(self.scroll + rows)
    }
}

/// The length of the escape sequence at the start of `bytes` (which starts
/// with ESC), or None if it is cut off.
fn escape_len(bytes: &[u8]) -> Option<usize> {
    match bytes.get(1)? {
        // CSI: ESC [ params final
        b'[' => bytes
            .iter()
            .skip(2)
            .position(|b| (0x40..=0x7e).contains(b))
            .map(|j| j + 3),
        // SS3: ESC O final
        b'O' => (bytes.len() >= 3).then_some(3),
        // A lone Esc followed by another escape sequence.
        0x1b => Some(1),
        // Alt+key
        _ => Some(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(n: usize) -> Vec<Target> {
        (1..=n)
            .map(|i| Target {
                pane_id: format!("w1:p{i}"),
                label: format!("pane{i}"),
                agent: String::new(),
                cwd: String::new(),
                selected: true,
            })
            .collect()
    }

    fn selected(targets: &[Target]) -> Vec<bool> {
        targets.iter().map(|t| t.selected).collect()
    }

    fn open() -> Picker {
        Picker::default()
    }

    #[test]
    fn number_keys_toggle_panes_and_move_the_cursor() {
        let mut t = targets(3);
        let mut p = open();
        assert_eq!(p.feed(&mut t, b"2"), Pick::Stay);
        assert_eq!(selected(&t), [true, false, true]);
        assert_eq!(p.cursor, 1);
        p.feed(&mut t, b"229"); // 9 has no pane
        assert_eq!(selected(&t), [true, false, true]);
    }

    #[test]
    fn cursor_moves_and_space_toggles() {
        let mut t = targets(3);
        let mut p = open();
        p.feed(&mut t, b"j\x0e\x1b[B "); // down past the end, toggle pane 3
        assert_eq!(p.cursor, 2);
        assert_eq!(selected(&t), [true, true, false]);
        p.feed(&mut t, b"k\x1bOA\x10 "); // up past the top, toggle pane 1
        assert_eq!(p.cursor, 0);
        assert_eq!(selected(&t), [false, true, false]);
    }

    #[test]
    fn a_selects_all_or_none() {
        let mut t = targets(3);
        let mut p = open();
        p.feed(&mut t, b"a");
        assert_eq!(selected(&t), [false, false, false]);
        p.feed(&mut t, b"2a");
        assert_eq!(selected(&t), [true, true, true]);
    }

    #[test]
    fn enter_esc_and_pick_key_close_the_picker() {
        let mut t = targets(2);
        let mut p = open();
        assert_eq!(p.feed(&mut t, b"1\rls"), Pick::Done);
        assert_eq!(selected(&t), [false, true]);
        assert_eq!(open().feed(&mut t, b"\x1d"), Pick::Done);
        let mut esc = open();
        assert_eq!(esc.feed(&mut t, b"\x1b"), Pick::Stay); // held: could start a sequence
        assert_eq!(esc.expire(), Pick::Done);
        assert_eq!(open().feed(&mut t, b"\x11"), Pick::Quit);
        assert_eq!(open().feed(&mut t, b"\x07"), Pick::Quit);
    }

    #[test]
    fn other_escape_sequences_do_not_toggle_panes() {
        let mut t = targets(2);
        let mut p = open();
        assert_eq!(p.feed(&mut t, b"\x1b[1;5C"), Pick::Stay);
        assert_eq!(selected(&t), [true, true]);
    }

    #[test]
    fn escape_sequence_split_across_feeds_does_not_toggle_panes() {
        let mut t = targets(2);
        let mut p = open();
        assert_eq!(p.feed(&mut t, b"\x1b["), Pick::Stay);
        assert_eq!(p.feed(&mut t, b"1;5C"), Pick::Stay);
        assert_eq!(selected(&t), [true, true]);
        assert!(!p.has_pending());
    }

    #[test]
    fn arrow_split_across_feeds_moves_the_cursor() {
        let mut t = targets(3);
        let mut p = open();
        assert_eq!(p.feed(&mut t, b"\x1b"), Pick::Stay); // could be a lone Esc
        assert_eq!(p.feed(&mut t, b"[B"), Pick::Stay);
        assert_eq!(p.cursor, 1);
        p.feed(&mut t, b"\x1bO");
        p.feed(&mut t, b"A ");
        assert_eq!(p.cursor, 0);
        assert_eq!(selected(&t), [false, true, true]);
    }

    #[test]
    fn expire_settles_a_held_sequence() {
        let mut t = targets(2);
        let mut p = open();
        p.feed(&mut t, b"\x1b");
        assert_eq!(p.expire(), Pick::Done); // a lone Esc
        p.feed(&mut t, b"\x1b[1");
        assert_eq!(p.expire(), Pick::Stay); // junk, dropped
        assert_eq!(p.feed(&mut t, b"1"), Pick::Stay); // a fresh key again
        assert_eq!(selected(&t), [false, true]);
    }

    #[test]
    fn alt_keys_are_skipped_whole() {
        let mut t = targets(2);
        let mut p = open();
        assert_eq!(p.feed(&mut t, b"\x1b1\x1ba2"), Pick::Stay); // Alt+1, Alt+a, then 2
        assert_eq!(selected(&t), [true, false]);
    }

    #[test]
    fn window_scrolls_to_keep_the_cursor_visible() {
        let mut p = open();
        assert_eq!(p.window(3, 5), 0..3);
        assert_eq!(p.window(10, 3), 0..3);
        p.cursor = 4;
        assert_eq!(p.window(10, 3), 2..5);
        p.cursor = 3;
        assert_eq!(p.window(10, 3), 2..5);
        p.cursor = 0;
        assert_eq!(p.window(10, 3), 0..3);
        p.cursor = 9;
        assert_eq!(p.window(5, 3), 2..5); // the list shrank under the cursor
        assert_eq!(p.cursor, 4);
        assert_eq!(p.window(0, 3), 0..0);
    }
}
