//! Which keystrokes are broadcast, and what the popup shows of them.
//!
//! Only typing keys are broadcast: text, Enter, Backspace, Tab, Ctrl+C,
//! Alt+Enter, and Emacs-style line editing (Ctrl+A/E/B/F/K/U/W, ← → Home End
//! Del, and Ctrl+P/N ↑ ↓ between rows of a multi-line input). Other shortcuts
//! (Esc, Ctrl+D, Ctrl+L, Ctrl+R, Alt+key, ...) are dropped, and so are the
//! up / down keys when there is no row to move to, so they never recall
//! history in the targets.
//!
//! The popup cannot see the targets' shells (completion, history recall), so
//! it shows a log of the keys sent: text as typed with line editing applied as
//! a shell would, Tab as a label, and Enter / Ctrl+C start a new line.

use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthStr;

/// One entry of a typed line: a character or key label, or a newline typed
/// with Alt+Enter (which does not submit the input). Kept apart from text so
/// that typing the character "↵" is not mistaken for a newline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Token {
    Text(String),
    Newline,
}

impl Token {
    /// What the popup shows for it.
    pub fn as_str(&self) -> &str {
        match self {
            Token::Text(s) => s,
            Token::Newline => "↵",
        }
    }

    fn width(&self) -> usize {
        self.as_str().width()
    }
}

/// The tokens as shown in the popup.
pub fn concat(tokens: &[Token]) -> String {
    tokens.iter().map(Token::as_str).collect()
}

#[derive(Default, Serialize, Deserialize)]
pub struct Echo {
    /// The line finished by the last Enter or Ctrl+C.
    pub last: Vec<Token>,
    /// The line being typed. One entry per character or key label, so
    /// Backspace removes a whole label.
    pub line: Vec<Token>,
    /// Index into `line` where the next key is inserted.
    pub cursor: usize,
    /// The start of an escape sequence cut off at the end of the last feed,
    /// completed by the next one (or dropped by `expire`).
    #[serde(skip)]
    pending: String,
}

impl Echo {
    /// Applies the keys in `text` and returns the part of it to broadcast.
    /// An escape sequence cut off at the end is held until the next feed.
    pub fn feed(&mut self, text: &str) -> String {
        let text = std::mem::take(&mut self.pending) + text;
        let mut out = String::new();
        let mut chars = text.chars();
        loop {
            let key_start = chars.as_str();
            let Some(c) = chars.next() else { break };
            let mut forward = true;
            match c {
                '\r' | '\n' => self.finish("⏎"),
                '\x03' => self.finish("^C"),
                '\x7f' | '\x08' => self.backspace(),
                '\x01' => self.row_start(), // Ctrl+A
                '\x05' => self.row_end(),   // Ctrl+E
                '\x02' => self.left(),      // Ctrl+B
                '\x06' => self.right(),     // Ctrl+F
                '\x0b' => {
                    // Ctrl+K
                    let (_, end) = self.row_bounds(self.cursor);
                    self.line.drain(self.cursor..end);
                }
                '\x15' => {
                    // Ctrl+U
                    let (start, _) = self.row_bounds(self.cursor);
                    self.line.drain(start..self.cursor);
                    self.cursor = start;
                }
                '\x17' => self.kill_word(), // Ctrl+W
                // Up / Down keys move between rows; on the first / last row
                // they would recall history, so they are not broadcast.
                '\x10' => forward = self.up(),
                '\x0e' => forward = self.down(),
                '\t' => self.label("Tab"),
                '\x1b' => match escape_key(&mut chars) {
                    None => {
                        self.pending = key_start.to_string();
                        break;
                    }
                    Some(key) => match key {
                        Key::Left => self.left(),
                        Key::Right => self.right(),
                        Key::Home => self.row_start(),
                        Key::End => self.row_end(),
                        Key::Del => self.delete(),
                        Key::Up => forward = self.up(),
                        Key::Down => forward = self.down(),
                        Key::Newline => self.insert(Token::Newline),
                        Key::Other => forward = false,
                    },
                },
                // Other Ctrl keys (Ctrl+D, history search, clear screen, ...).
                c if c.is_control() => forward = false,
                c => self.insert(Token::Text(c.to_string())),
            }
            if forward {
                out.push_str(&key_start[..key_start.len() - chars.as_str().len()]);
            }
        }
        out
    }

    /// Whether an escape sequence is waiting for the rest of it.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Gives up on a held escape sequence when the rest has not come: it was
    /// a lone Esc (or junk), which is not broadcast either way.
    pub fn expire(&mut self) {
        self.pending.clear();
    }

    fn insert(&mut self, token: Token) {
        self.line.insert(self.cursor, token);
        self.cursor += 1;
    }

    fn label(&mut self, name: &str) {
        self.insert(Token::Text(format!("‹{name}›")));
    }

    fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.line.len());
    }

    /// Moves the cursor to the start of its row.
    fn row_start(&mut self) {
        self.cursor = self.row_bounds(self.cursor).0;
    }

    /// Moves the cursor to the end of its row.
    fn row_end(&mut self) {
        self.cursor = self.row_bounds(self.cursor).1;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.line.remove(self.cursor);
        }
    }

    fn delete(&mut self) {
        if self.cursor < self.line.len() {
            self.line.remove(self.cursor);
        }
    }

    /// Removes the word before the cursor and the spaces after it.
    fn kill_word(&mut self) {
        let is_space =
            |t: &Token| matches!(t, Token::Text(s) if s.chars().all(char::is_whitespace));
        let mut start = self.cursor;
        while start > 0 && is_space(&self.line[start - 1]) {
            start -= 1;
        }
        while start > 0 && !is_space(&self.line[start - 1]) {
            start -= 1;
        }
        self.line.drain(start..self.cursor);
        self.cursor = start;
    }

    /// Moves the cursor to the row above, keeping its column. False if the
    /// cursor is already on the first row.
    fn up(&mut self) -> bool {
        let (start, _) = self.row_bounds(self.cursor);
        if start == 0 {
            return false;
        }
        let col = concat(&self.line[start..self.cursor]).width();
        let (above, _) = self.row_bounds(start - 1);
        self.cursor = above + index_at_column(&self.line[above..start - 1], col);
        true
    }

    /// Moves the cursor to the row below, keeping its column. False if the
    /// cursor is already on the last row.
    fn down(&mut self) -> bool {
        let (start, end) = self.row_bounds(self.cursor);
        if end == self.line.len() {
            return false;
        }
        let col = concat(&self.line[start..self.cursor]).width();
        let (below, below_end) = self.row_bounds(end + 1);
        self.cursor = below + index_at_column(&self.line[below..below_end], col);
        true
    }

    /// Start and end (exclusive, at the NEWLINE or the end of the input) of
    /// the row holding position `at`. Rows are split by Alt+Enter.
    fn row_bounds(&self, at: usize) -> (usize, usize) {
        let start = self.line[..at]
            .iter()
            .rposition(|t| *t == Token::Newline)
            .map_or(0, |i| i + 1);
        let end = self.line[at..]
            .iter()
            .position(|t| *t == Token::Newline)
            .map_or(self.line.len(), |i| at + i);
        (start, end)
    }

    /// The row of a multi-line input that holds the cursor, and the cursor's
    /// index within it.
    pub fn cursor_row(&self) -> (&[Token], usize) {
        let (start, end) = self.row_bounds(self.cursor);
        (&self.line[start..end], self.cursor - start)
    }

    fn finish(&mut self, mark: &str) {
        self.line.push(Token::Text(mark.to_string()));
        self.last = std::mem::take(&mut self.line);
        self.cursor = 0;
    }
}

/// The index in `row` whose column is closest to `col` without passing it.
fn index_at_column(row: &[Token], col: usize) -> usize {
    let mut used = 0;
    for (i, t) in row.iter().enumerate() {
        used += t.width();
        if used > col {
            return i;
        }
    }
    row.len()
}

/// A key sent as an escape sequence.
enum Key {
    Left,
    Right,
    Home,
    End,
    Del,
    Up,
    Down,
    /// Alt+Enter: a newline inside the input that does not submit it.
    Newline,
    /// Esc, Alt+key, PgUp, Shift+Tab, ...: not broadcast.
    Other,
}

/// Reads the rest of an escape sequence after ESC and names the key, or
/// None if the sequence is cut off (nothing consumed then).
fn escape_key(chars: &mut std::str::Chars) -> Option<Key> {
    let mut ahead = chars.clone();
    let key = match ahead.next()? {
        '\r' | '\n' => Key::Newline,
        // CSI: ESC [ params final
        '[' => {
            let mut params = String::new();
            loop {
                match ahead.next()? {
                    c if ('\x40'..='\x7e').contains(&c) => break csi_key(&params, c),
                    c => params.push(c),
                }
            }
        }
        // SS3: ESC O final (arrow keys in application cursor mode)
        'O' => csi_key("", ahead.next()?),
        // A lone Esc followed by another escape sequence: leave that one to
        // be read.
        '\x1b' => return Some(Key::Other),
        // Alt+key, Alt+Backspace, ...
        _ => Key::Other,
    };
    *chars = ahead;
    Some(key)
}

/// The key of a CSI / SS3 sequence. Only unmodified keys are editing keys:
/// with a modifier parameter (Alt+←, Ctrl+→ = `1;3D`, `1;5C`) they are other
/// shortcuts, which the targets would act on in ways the popup cannot follow.
fn csi_key(params: &str, last: char) -> Key {
    match (params, last) {
        ("", 'A') => Key::Up,
        ("", 'B') => Key::Down,
        ("", 'C') => Key::Right,
        ("", 'D') => Key::Left,
        ("", 'H') | ("1" | "7", '~') => Key::Home,
        ("", 'F') | ("4" | "8", '~') => Key::End,
        ("3", '~') => Key::Del,
        _ => Key::Other,
    }
}

/// The tail of `tokens` that fits in `width` columns.
pub fn tail(tokens: &[Token], width: usize) -> String {
    concat(&tokens[tail_start(tokens, width)..])
}

/// Index of the first token of the longest tail that fits in `width` columns.
fn tail_start(tokens: &[Token], width: usize) -> usize {
    let mut used = 0;
    let mut start = tokens.len();
    for (i, t) in tokens.iter().enumerate().rev() {
        let w = t.width();
        if used + w > width {
            break;
        }
        used += w;
        start = i;
    }
    start
}

/// The part of `tokens` to show in `width` columns so the cursor stays
/// visible, and the cursor's column within it. Shows the end of the line
/// when the cursor is there; otherwise scrolls so the cursor is at the right.
pub fn view(tokens: &[Token], cursor: usize, width: usize) -> (String, usize) {
    let mut start = tail_start(tokens, width);
    if start > cursor {
        start = tail_start(&tokens[..cursor], width);
    }
    let mut used = 0;
    let mut text = String::new();
    let mut col = 0;
    for (i, t) in tokens.iter().enumerate().skip(start) {
        if i == cursor {
            col = used;
        }
        let w = t.width();
        if used + w > width {
            break;
        }
        used += w;
        text.push_str(t.as_str());
    }
    if cursor >= tokens.len() {
        col = used;
    }
    (text, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(e: &Echo) -> String {
        concat(&e.line)
    }

    #[test]
    fn shows_typed_text_and_backspace() {
        let mut e = Echo::default();
        e.feed("ecbo\x7f\x7fho 日本");
        assert_eq!(line(&e), "echo 日本");
    }

    #[test]
    fn enter_moves_line_up() {
        let mut e = Echo::default();
        e.feed("ls\rpw");
        assert_eq!(concat(&e.last), "ls⏎");
        assert_eq!(line(&e), "pw");
    }

    #[test]
    fn ctrl_c_moves_line_up() {
        let mut e = Echo::default();
        e.feed("sleep 9\x03");
        assert_eq!(concat(&e.last), "sleep 9^C");
        assert_eq!(line(&e), "");
    }

    #[test]
    fn labels_tab() {
        let mut e = Echo::default();
        e.feed("ls\t");
        assert_eq!(line(&e), "ls‹Tab›");
    }

    #[test]
    fn drops_other_shortcuts() {
        let mut e = Echo::default();
        // Esc, Ctrl+D, Alt+b, Ctrl+L, Ctrl+R, Ctrl+Z, PgUp, Shift+Tab, Insert
        let keys = "a\x1b\x04\x1bb\x0c\x12\x1a\x1b[5~\x1b[Z\x1b[2~b";
        assert_eq!(e.feed(keys), "ab");
        assert_eq!(line(&e), "ab");
        assert_eq!(e.feed("\x04"), ""); // Ctrl+D on a non-empty line too
        assert_eq!(line(&e), "ab");
    }

    #[test]
    fn alt_enter_inserts_newline_without_submitting() {
        let mut e = Echo::default();
        e.feed("ab\x1b\rcd");
        assert_eq!(line(&e), "ab↵cd");
        assert!(e.last.is_empty());
        e.feed("\x7f\x7f\x7fX"); // Backspace back across the newline
        assert_eq!(line(&e), "abX");
    }

    #[test]
    fn cursor_row_is_the_row_holding_the_cursor() {
        let row = |e: &Echo| {
            let (tokens, cursor) = e.cursor_row();
            (concat(tokens), cursor)
        };
        let mut e = Echo::default();
        e.feed("ab\x1b\rcd");
        assert_eq!(row(&e), ("cd".into(), 2));
        e.feed("\x1b\r");
        assert_eq!(row(&e), ("".into(), 0));
        e.feed("\x7f\x01"); // drop the empty row; Ctrl+A goes to the row start
        assert_eq!(row(&e), ("cd".into(), 0));
        e.feed("\x1b[D"); // left across the newline, to the end of "ab"
        assert_eq!(row(&e), ("ab".into(), 2));
    }

    #[test]
    fn ctrl_p_and_ctrl_n_move_between_rows() {
        let row = |e: &Echo| {
            let (tokens, cursor) = e.cursor_row();
            (concat(tokens), cursor)
        };
        let mut e = Echo::default();
        e.feed("abcd\x1b\rx\x1b\r日本語");
        e.feed("\x02"); // Ctrl+B: column 4, between 本 and 語
        e.feed("\x10"); // up to "x", clamped to its end
        assert_eq!(row(&e), ("x".into(), 1));
        e.feed("\x10"); // up to "abcd", column 1
        assert_eq!(row(&e), ("abcd".into(), 1));
        e.feed("\x06\x06\x06\x10"); // column 4; first row: history, not shown
        assert_eq!(row(&e), ("abcd".into(), 4));
        e.feed("\x0e\x0e"); // down to "x" (column 1), then before 日 (width 2)
        assert_eq!(row(&e), ("日本語".into(), 0));
        e.feed("\x06\x06\x1b[D\x0e"); // column 2 on the last row: not shown
        assert_eq!(row(&e), ("日本語".into(), 1));
        assert_eq!(line(&e), "abcd↵x↵日本語");
    }

    #[test]
    fn holds_back_up_and_down_that_would_recall_history() {
        let mut e = Echo::default();
        assert_eq!(e.feed("a\x10\x1b[Ab\x0e\x1bOB"), "ab");
        assert_eq!(line(&e), "ab");
        assert_eq!(e.feed("\x1b\rc\x10"), "\x1b\rc\x10");
        assert_eq!(e.feed("\x10\x1b[A\x0e\x0e"), "\x0e");
        assert_eq!(e.feed("\x1b[A\x1b[B\x1bOB"), "\x1b[A\x1b[B");
    }

    #[test]
    fn broadcasts_everything_else_as_typed() {
        let mut e = Echo::default();
        let keys = "ls\x1b[D\x1bOC\x1b[3~\x01\x05\x02\x06\x0b\x15\x17\t日本\x7f\x1b\r\r\x03";
        assert_eq!(e.feed(keys), keys);
    }

    #[test]
    fn arrows_move_between_rows() {
        let mut e = Echo::default();
        e.feed("ab\x1b\rc\x1b[Az\x1b[B");
        assert_eq!(line(&e), "azb↵c");
        e.feed("\x1b[A\x1b[A");
        assert_eq!(line(&e), "azb↵c");
    }

    #[test]
    fn line_start_end_and_kills_stay_in_the_row() {
        let mut e = Echo::default();
        e.feed("ab\x1b\rcd\x01X\x05Y");
        assert_eq!(line(&e), "ab↵XcdY");
        e.feed("\x10\x1b[HZ\x1b[FW"); // up to row 1; Home / End
        assert_eq!(line(&e), "ZabW↵XcdY");
        e.feed("\x02\x0b"); // Ctrl+K stops at the newline
        assert_eq!(line(&e), "Zab↵XcdY");
        e.feed("\x0e\x15"); // down to row 2, column 3; Ctrl+U stops at the newline
        assert_eq!(line(&e), "Zab↵Y");
    }

    #[test]
    fn escape_sequence_split_across_feeds_is_kept_whole() {
        let mut e = Echo::default();
        e.feed("abc");
        let out = e.feed("\x1b[") + &e.feed("D");
        assert_eq!(out, "\x1b[D");
        assert_eq!(line(&e), "abc");
        assert_eq!(e.cursor, 2);
        // Split right after ESC, and in the middle of the parameters.
        let out = e.feed("\x1b") + &e.feed("[C") + &e.feed("\x1b[3") + &e.feed("~");
        assert_eq!(out, "\x1b[C\x1b[3~");
        assert_eq!(line(&e), "abc");
        assert!(!e.has_pending());
    }

    #[test]
    fn alt_enter_split_across_feeds_does_not_submit() {
        let mut e = Echo::default();
        e.feed("ab");
        let out = e.feed("\x1b") + &e.feed("\r");
        assert_eq!(out, "\x1b\r");
        assert!(e.last.is_empty());
        assert_eq!(line(&e), "ab↵");
    }

    #[test]
    fn expire_drops_a_lone_esc() {
        let mut e = Echo::default();
        assert_eq!(e.feed("a\x1b"), "a");
        assert!(e.has_pending());
        e.expire();
        assert_eq!(e.feed("b"), "b"); // not taken as Alt+b
        assert_eq!(line(&e), "ab");
    }

    #[test]
    fn modified_keys_are_not_broadcast() {
        let mut e = Echo::default();
        e.feed("abc");
        // Alt+←, Ctrl+→, Shift+↑, Alt+Del, Ctrl+Home
        assert_eq!(e.feed("\x1b[1;3D\x1b[1;5C\x1b[1;2A\x1b[3;3~\x1b[1;5H"), "");
        assert_eq!(line(&e), "abc");
        assert_eq!(e.cursor, 3);
    }

    #[test]
    fn alt_backspace_is_not_broadcast() {
        let mut e = Echo::default();
        e.feed("abc");
        assert_eq!(e.feed("\x1b\x7f"), "");
        assert_eq!(line(&e), "abc");
        // Esc then an arrow key: the arrow still works.
        assert_eq!(e.feed("\x1b\x1b[D"), "\x1b[D");
        assert_eq!(e.cursor, 2);
    }

    #[test]
    fn typed_newline_symbol_is_text_not_a_row_break() {
        let mut e = Echo::default();
        e.feed("echo ↵");
        assert_eq!(e.feed("\x10"), ""); // Ctrl+P: no row above, not broadcast
        assert_eq!(e.feed("\x1b[A"), ""); // ↑ likewise
        assert_eq!(e.cursor_row().0.len(), 6);
        e.feed("\x1b\rx");
        assert_eq!(e.feed("\x10"), "\x10"); // a real row above
    }

    #[test]
    fn hides_other_ctrl_keys() {
        let mut e = Echo::default();
        e.feed("a\x10\x0e\x0c\x12b"); // Ctrl+P, N, L, R
        assert_eq!(line(&e), "ab");
    }

    #[test]
    fn backspace_removes_whole_label() {
        let mut e = Echo::default();
        e.feed("a\t\x7f");
        assert_eq!(line(&e), "a");
    }

    #[test]
    fn ctrl_u_clears_line() {
        let mut e = Echo::default();
        e.feed("abc\x15d");
        assert_eq!(line(&e), "d");
    }

    #[test]
    fn ctrl_a_and_ctrl_e_move_cursor() {
        let mut e = Echo::default();
        e.feed("cho\x01e\x05 hi");
        assert_eq!(line(&e), "echo hi");
        assert_eq!(e.cursor, 7);
    }

    #[test]
    fn arrows_home_end_move_cursor() {
        let mut e = Echo::default();
        e.feed("ac\x1b[Db\x1b[Hx\x1b[F!\x1bOD\x1bOC?");
        assert_eq!(line(&e), "xabc!?");
    }

    #[test]
    fn edits_at_cursor() {
        let mut e = Echo::default();
        e.feed("abcd\x02\x02\x7f"); // Ctrl+B twice, Backspace
        assert_eq!(line(&e), "acd");
        e.feed("\x1b[3~"); // Del deletes under the cursor
        assert_eq!(line(&e), "ad");
        e.feed("\x06\x1b[3~"); // Ctrl+F then Del at end: no-op
        assert_eq!(line(&e), "ad");
        e.feed("\x01\x06\x0b"); // Ctrl+K kills to end
        assert_eq!(line(&e), "a");
    }

    #[test]
    fn ctrl_u_kills_before_cursor() {
        let mut e = Echo::default();
        e.feed("abc\x02\x15");
        assert_eq!(line(&e), "c");
        assert_eq!(e.cursor, 0);
    }

    #[test]
    fn ctrl_w_kills_previous_word() {
        let mut e = Echo::default();
        e.feed("git commit  \x17");
        assert_eq!(line(&e), "git ");
    }

    #[test]
    fn view_keeps_cursor_visible() {
        let tokens: Vec<Token> = "abcdefgh"
            .chars()
            .map(|c| Token::Text(c.to_string()))
            .collect();
        assert_eq!(view(&tokens, 8, 4), ("efgh".into(), 4));
        assert_eq!(view(&tokens, 6, 4), ("efgh".into(), 2));
        assert_eq!(view(&tokens, 2, 4), ("abcd".into(), 2));
        assert_eq!(view(&tokens, 0, 4), ("abcd".into(), 0));
        assert_eq!(view(&tokens, 3, 100), ("abcdefgh".into(), 3));
    }

    #[test]
    fn tail_keeps_end_that_fits() {
        let tokens: Vec<Token> = ["a", "日", "b", "‹↓›"]
            .iter()
            .map(|s| Token::Text(s.to_string()))
            .collect();
        assert_eq!(tail(&tokens, 6), "日b‹↓›");
        assert_eq!(tail(&tokens, 5), "b‹↓›");
        assert_eq!(tail(&tokens, 100), "a日b‹↓›");
    }
}
