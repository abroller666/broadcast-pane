//! Herdr plugin popup that broadcasts every keystroke typed into it to the
//! panes of the current tab, much like tmux `synchronize-panes`.
//!
//! - `broadcast-pane open` (the plugin action) opens the console popup.
//! - `broadcast-pane` is the console: type here to broadcast.
//! - `broadcast-pane picker` is the target picker popup (Ctrl+] in the
//!   console), listing the tab's panes one per row.
//!
//! Herdr shows one popup at a time and cannot resize it, so the console and
//! the taller picker replace each other: the closing one saves the state and
//! starts `broadcast-pane reopen`, which opens the other once it is gone.

mod echo;
mod herdr;
mod input;
mod picker;
mod state;
mod targets;

use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use nix::sys::termios::{self, SetArg, Termios};
use unicode_width::UnicodeWidthStr;

use echo::Echo;
use herdr::Herdr;
use input::Decoder;
use picker::{Pick, Picker};
use state::State;
use targets::Target;

/// Restores the terminal settings when dropped, including on panic.
struct RawMode(Termios);

impl RawMode {
    fn enable() -> Result<Self, String> {
        let stdin = std::io::stdin();
        let saved = termios::tcgetattr(stdin.as_fd()).map_err(|e| format!("tcgetattr: {e}"))?;
        let mut raw = saved.clone();
        termios::cfmakeraw(&mut raw);
        // TCSANOW keeps keys typed while the popup was opening.
        termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &raw)
            .map_err(|e| format!("tcsetattr: {e}"))?;
        Ok(Self(saved))
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = termios::tcsetattr(std::io::stdin().as_fd(), SetArg::TCSADRAIN, &self.0);
    }
}

/// Terminal size as (columns, rows), or 80x3 if it cannot be read.
fn terminal_size() -> (usize, usize) {
    let mut ws: nix::libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: TIOCGWINSZ only writes into the winsize struct we pass.
    let ok =
        unsafe { nix::libc::ioctl(nix::libc::STDOUT_FILENO, nix::libc::TIOCGWINSZ, &mut ws) } == 0;
    if ok && ws.ws_col > 0 && ws.ws_row > 0 {
        (usize::from(ws.ws_col), usize::from(ws.ws_row))
    } else {
        (80, 3)
    }
}

/// How long to wait for the rest of an escape sequence cut off at the end of
/// a read before taking it as a lone Esc. Terminals send a key's sequence in
/// one write, so a split is rare and the rest follows almost at once.
const ESCAPE_TIMEOUT_MS: i32 = 50;

/// Whether stdin has input within `ms` milliseconds.
fn readable_within(ms: i32) -> bool {
    let mut fds = nix::libc::pollfd {
        fd: nix::libc::STDIN_FILENO,
        events: nix::libc::POLLIN,
        revents: 0,
    };
    // SAFETY: poll only reads and writes the one pollfd we pass.
    unsafe { nix::libc::poll(&mut fds, 1, ms) != 0 }
}

/// Popup width, as in herdr-plugin.toml.
const POPUP_WIDTH: &str = "60%";
/// Console popup height, as in herdr-plugin.toml.
const CONSOLE_HEIGHT: usize = 5;

/// Picker popup height: border, header, and a row per pane, capped at half
/// the screen.
fn picker_height(panes: usize, screen_rows: usize) -> usize {
    (panes + 3)
        .max(CONSOLE_HEIGHT)
        .min((screen_rows / 2).max(CONSOLE_HEIGHT))
}

/// Number key for the pane at `i`; panes past 9 have none.
fn number(i: usize) -> String {
    if i < 9 {
        (i + 1).to_string()
    } else {
        " ".into()
    }
}

/// The console's first line: where keystrokes go.
fn console_header(targets: &[Target]) -> String {
    let chosen: Vec<String> = targets
        .iter()
        .enumerate()
        .filter(|(_, t)| t.selected)
        .map(|(i, t)| format!("{} {}", number(i), t.label))
        .collect();
    let to = if chosen.is_empty() {
        "\x1b[31mno panes\x1b[0m".to_string()
    } else {
        format!("{}/{} ({})", chosen.len(), targets.len(), chosen.join(", "))
    };
    format!("\x1b[1;33mbroadcast\x1b[0m → {to}  \x1b[2mC-]: select  C-g: quit\x1b[0m")
}

/// Header, the previous line (dim), and the row of the input holding the cursor.
fn draw_console(targets: &[Target], echo: &Echo) {
    const PROMPT: &str = "› ";
    // Leave a column for the cursor at the end of the input line.
    let width = terminal_size().0.saturating_sub(PROMPT.width() + 1);
    let (row, cursor) = echo.cursor_row();
    let (line, col) = echo::view(row, cursor, width);
    let mut out = std::io::stdout().lock();
    let _ = write!(
        out,
        // Autowrap off: a long header is clipped instead of pushing lines down.
        "\x1b[?7l\x1b[2J\x1b[H{}\r\n\x1b[2m  {}\x1b[0m\r\n\x1b[1m{PROMPT}\x1b[0m{line}\x1b[3;{}H",
        console_header(targets),
        echo::tail(&echo.last, width),
        PROMPT.width() + col + 1,
    );
    let _ = out.flush();
}

/// One pane of the picker: selected mark, number key, name, agent, cwd.
fn list_row(i: usize, t: &Target, highlighted: bool) -> String {
    let style = match (highlighted, t.selected) {
        (true, _) => "\x1b[7m",
        (false, true) => "",
        (false, false) => "\x1b[90m",
    };
    let mark = if t.selected {
        "\x1b[32m●\x1b[39m"
    } else {
        "○"
    };
    let pad = " ".repeat(targets::LABEL_WIDTH.saturating_sub(t.label.width()));
    format!(
        "{style}{mark} {} {}{pad}  {:<8} {}\x1b[0m",
        number(i),
        t.label,
        t.agent,
        t.cwd
    )
}

/// The picker's keys, then one row per pane, scrolled to the cursor.
fn draw_picker(targets: &[Target], picker: &mut Picker) {
    let rows = terminal_size().1.saturating_sub(1).max(1);
    let shown = picker.window(targets.len(), rows);
    let scroll = if shown.len() < targets.len() {
        format!(
            "  \x1b[2m({}-{}/{})\x1b[0m",
            shown.start + 1,
            shown.end,
            targets.len()
        )
    } else {
        String::new()
    };
    // Autowrap off: long rows are clipped; cursor hidden: the highlight shows it.
    let mut screen = format!(
        "\x1b[?7l\x1b[?25l\x1b[2J\x1b[H\x1b[1;36mselect\x1b[0m{scroll}  \x1b[2m↑↓: move  ␣: toggle  a: all  ⏎: done\x1b[0m"
    );
    for i in shown {
        screen.push_str("\r\n");
        screen.push_str(&list_row(i, &targets[i], i == picker.cursor));
    }
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(screen.as_bytes());
    let _ = out.flush();
}

/// Shows an error until a key is pressed; otherwise the popup would close before it can be read.
fn fail(msg: &str) -> ! {
    print!(
        "\x1b[2J\x1b[H\x1b[31mbroadcast-pane: {msg}\x1b[0m\r\n\x1b[2mpress any key to close\x1b[0m"
    );
    let _ = std::io::stdout().flush();
    let _ = std::io::stdin().read(&mut [0u8; 1]);
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // Commands that run outside a popup report errors on stderr.
    let detached = match args.as_slice() {
        ["open"] => Some(open()),
        ["reopen", entrypoint, height] => Some(reopen(entrypoint, height)),
        _ => None,
    };
    if let Some(result) = detached {
        if let Err(e) = result {
            eprintln!("broadcast-pane: {e}");
            std::process::exit(1);
        }
        return;
    }
    // Raw mode stays on inside `fail`, so a single key press closes the popup.
    let _raw = RawMode::enable().unwrap_or_else(|e| fail(&e));
    let result = match args.as_slice() {
        ["picker"] => run_picker(),
        _ => run_console(),
    };
    if let Err(e) = result {
        state::clear();
        fail(&e);
    }
}

/// The tab the popup was opened over.
fn current_tab(panes: &[targets::PaneInfo]) -> Result<String, String> {
    if let Ok(id) = std::env::var("HERDR_TAB_ID") {
        return Ok(id);
    }
    let ctx = std::env::var("HERDR_PLUGIN_CONTEXT_JSON").unwrap_or_default();
    let ctx = serde_json::from_str(&ctx).unwrap_or_default();
    let pane_id = targets::context_pane_id(&ctx).ok_or("could not find the current pane")?;
    panes
        .iter()
        .find(|p| p.pane_id == pane_id)
        .map(|p| p.tab_id.clone())
        .ok_or_else(|| format!("could not find the tab of {pane_id}"))
}

/// The layout of `tab`, if it has panes and Herdr can tell.
fn tab_layout(herdr: &Herdr, panes: &[targets::PaneInfo], tab: &str) -> Option<targets::Layout> {
    let pane = panes.iter().find(|p| p.tab_id == tab)?;
    herdr.layout(&pane.pane_id).ok()
}

fn plugin_id() -> Result<String, String> {
    std::env::var("HERDR_PLUGIN_ID").map_err(|_| "HERDR_PLUGIN_ID is not set".into())
}

/// The plugin action: opens a fresh console popup.
fn open() -> Result<(), String> {
    state::clear();
    Herdr::from_env()?.open_popup(
        &plugin_id()?,
        "console",
        POPUP_WIDTH.into(),
        CONSOLE_HEIGHT.into(),
    )
}

/// Opens the `entrypoint` popup as soon as the current popup has closed.
fn reopen(entrypoint: &str, height: &str) -> Result<(), String> {
    let herdr = Herdr::from_env()?;
    let plugin_id = plugin_id()?;
    let height: usize = height
        .parse()
        .map_err(|_| format!("bad height: {height}"))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match herdr.open_popup(&plugin_id, entrypoint, POPUP_WIDTH.into(), height.into()) {
            Ok(()) => return Ok(()),
            // Most likely the closing popup is still open.
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(30)),
            Err(e) => return Err(e),
        }
    }
}

/// Saves `state` and hands over to the `entrypoint` popup: starts
/// `broadcast-pane reopen` outside this popup, so it survives the popup
/// closing when this process exits.
fn switch_to(state: &State, entrypoint: &str, height: usize) -> Result<(), String> {
    state::save(state)?;
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.args(["reopen", entrypoint, &height.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe. A new session detaches the child
    // from the popup's terminal, so closing the popup does not hang it up.
    unsafe {
        cmd.pre_exec(|| {
            nix::libc::setsid();
            Ok(())
        });
    }
    cmd.spawn().map(drop).map_err(|e| format!("spawn: {e}"))
}

/// The tab's panes with the saved selection applied, the saved state, and the
/// screen height.
fn load(herdr: &Herdr) -> Result<(Vec<Target>, State, usize), String> {
    let mut state = state::load().unwrap_or_default();
    let panes = herdr.list_panes()?;
    if state.tab.is_empty() {
        state.tab = current_tab(&panes)?;
    }
    let layout = tab_layout(herdr, &panes, &state.tab);
    let screen_rows = layout.as_ref().map_or(24, |l| l.area.height as usize);
    // Without the layout, panes keep list order.
    let places = layout.map_or_else(Vec::new, |l| l.panes);
    let home = std::env::var("HOME").unwrap_or_default();
    let mut targets = targets::select(&panes, &state.tab, &places, &home);
    if targets.is_empty() {
        return Err("no panes in this tab".into());
    }
    for t in &mut targets {
        t.selected = !state.deselected.contains(&t.pane_id);
    }
    Ok((targets, state, screen_rows))
}

fn deselected(targets: &[Target]) -> Vec<String> {
    targets
        .iter()
        .filter(|t| !t.selected)
        .map(|t| t.pane_id.clone())
        .collect()
}

fn run_console() -> Result<(), String> {
    let herdr = Herdr::from_env()?;
    let (mut targets, mut state, screen_rows) = load(&herdr)?;
    let mut echo = std::mem::take(&mut state.echo);
    draw_console(&targets, &echo);

    let mut decoder = Decoder::default();
    let mut buf = [0u8; 4096];
    let mut stdin = std::io::stdin().lock();
    loop {
        if echo.has_pending() && !readable_within(ESCAPE_TIMEOUT_MS) {
            echo.expire();
            continue;
        }
        let n = stdin.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            state::clear();
            return Ok(());
        }
        let (keys, reserved, _) = input::split_reserved(&buf[..n]);
        let mut text = decoder.feed(keys);
        if reserved.is_some() {
            text.push_str(&decoder.flush());
        }
        if !text.is_empty() {
            let text = echo.feed(&text);
            if !text.is_empty() {
                // A pane that fails (closed, moved away) leaves the list.
                targets.retain(|t| !t.selected || herdr.send_text(&t.pane_id, &text).is_ok());
                if targets.is_empty() {
                    state::clear();
                    return Ok(());
                }
            }
        }
        match reserved {
            Some(input::PICK) => {
                state.deselected = deselected(&targets);
                state.echo = echo;
                let height = picker_height(targets.len(), screen_rows);
                return switch_to(&state, "picker", height);
            }
            Some(_) => {
                state::clear();
                return Ok(());
            }
            None => {}
        }
        draw_console(&targets, &echo);
    }
}

fn run_picker() -> Result<(), String> {
    let herdr = Herdr::from_env()?;
    let (mut targets, mut state, _) = load(&herdr)?;
    let mut picker = Picker::default();
    draw_picker(&targets, &mut picker);

    let mut buf = [0u8; 4096];
    let mut stdin = std::io::stdin().lock();
    loop {
        let pick = if picker.has_pending() && !readable_within(ESCAPE_TIMEOUT_MS) {
            picker.expire()
        } else {
            let n = stdin.read(&mut buf).map_err(|e| format!("read: {e}"))?;
            if n == 0 {
                state::clear();
                return Ok(());
            }
            picker.feed(&mut targets, &buf[..n])
        };
        match pick {
            Pick::Stay => draw_picker(&targets, &mut picker),
            Pick::Done => {
                state.deselected = deselected(&targets);
                return switch_to(&state, "console", CONSOLE_HEIGHT);
            }
            Pick::Quit => {
                state::clear();
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_fits_the_panes_up_to_half_the_screen() {
        assert_eq!(picker_height(1, 50), 5); // never below the console
        assert_eq!(picker_height(4, 50), 7);
        assert_eq!(picker_height(30, 50), 25);
        assert_eq!(picker_height(4, 6), 5);
    }
}
