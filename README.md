# broadcast-pane

[日本語](README.ja.md)

tmux's `synchronize-panes` for [Herdr](https://herdr.dev).

Press one key to open a small popup in the middle of the screen. What you type
there goes to every pane of the current tab (or the panes you choose). The
layout does not change, and the target panes stay visible around the popup.

```
┌─ pane 1 ──────────┬─ pane 2 ──────────┐
│ $ uptime          │ $ uptime          │
│  10:12 up 3d      │  10:12 up 9d      │
│ ┌─ Broadcast ───────────────────────┐ │
│ │ broadcast → 2/2 (1 web, 2 api)  … │ │
│ │   ls⏎                             │ │
│ │ › echo hi                         │ │
│ └───────────────────────────────────┘ │
└───────────────────┴───────────────────┘
```

- **Typing keys only**: text (including text committed by an input method),
  `Enter`, `Backspace`, `Tab`, `Ctrl+c`, `Alt+Enter`, and line editing
  (`Ctrl+a`/`e`/`b`/`f`/`k`/`u`/`w`, `←` `→`, `Home`/`End`/`Del`) are sent.
  Other shortcuts such as `Esc`, `Ctrl+d`, `Ctrl+l`, `Ctrl+r`, `Ctrl+z`, and
  `Alt`+key are not.
- **Targets**: at first, every pane of the tab the popup was opened over
  (including the pane you were in). A pane that closes drops out; when none
  are left, the popup closes.
- **Choose targets**: `Ctrl+]` swaps the popup for a target picker that lists
  the tab's panes one per row, `●` for a target and `○` for a pane left out.
  The picker is as tall as the pane count needs, up to half the screen, and
  scrolls beyond that.
  - `↑` `↓`, `Ctrl+p`/`Ctrl+n`, or `j`/`k` move; `Space` toggles the pane.
  - `1`–`9` toggle that pane directly; `a` selects all (or none, when all are
    selected).
  - `Enter`, `Esc`, or `Ctrl+]` go back to typing, with the line you were
    typing intact.
  - Panes are numbered by position, top to bottom, then left to right. Each
    row shows the pane's name (set with `prefix+P`; otherwise the terminal
    title, the agent, or the last part of the cwd), its agent, and its cwd.
  - The choice lasts until the popup closes; the next popup starts with every
    pane again.
- **See what you typed**: the popup shows the line being typed and the
  previous one. Line editing (`Ctrl+a`, `Ctrl+e`, `Ctrl+k`, `Ctrl+u`,
  `Ctrl+w`, `←` `→`, ...) is applied to it. After `Alt+Enter`, the row holding
  the cursor is shown; `Ctrl+p`/`Ctrl+n` and `↑` `↓` move between rows. `Tab`
  is shown as `‹Tab›`. The popup cannot see the targets' shells (completion,
  history), so this is a record of the keys you sent.
- **No history recall**: `Ctrl+p`/`Ctrl+n` and `↑` `↓` are not sent when
  there is no row to move to.
- **Quit with `Ctrl+g` or `Ctrl+q`**.

## Limitations

- Herdr plugins cannot intercept keys typed into normal panes, so you type
  into a dedicated popup.
- The popup always opens in the middle of the screen and hides the middle of
  the target panes.
- Herdr shows one popup at a time and cannot resize it, so opening the target
  picker closes the typing popup and opens another one. Keys typed during the
  switch are lost.
- While the popup is open, Herdr's prefix key goes to the popup too (prefix
  commands are not available).
- Panes opened after the popup are not targets until you open the target
  picker (`Ctrl+]`), which adds them.
- Keys other than typing keys (`Esc`, `Ctrl+d`, ...) are not sent, so the
  popup cannot drive vim, top, and similar programs.
- `Ctrl+g`, `Ctrl+q`, and `Ctrl+]` cannot be sent.

## Requirements

- Herdr 0.9.2 or newer
- macOS or Linux (tested on macOS)
- Rust 1.85 or newer (`cargo`) to build

## Install

```sh
herdr plugin install abroller666/broadcast-pane
```

The install runs `scripts/build.sh` (`cargo build --release`), which builds
`bin/broadcast-pane`.

Add a keybinding to `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+a"
type = "plugin_action"
command = "abroller666.broadcast-pane.open"
description = "broadcast to all panes"
```

Then reload: `herdr server reload-config`.

## Development

To use a local clone, build it, then link it (`herdr plugin link` does not
build):

```sh
git clone https://github.com/abroller666/broadcast-pane.git
cd broadcast-pane
sh scripts/build.sh
herdr plugin link .
```

```sh
cargo test
cargo clippy --all-targets
sh scripts/build.sh   # rebuild after a change; the next popup picks it up
```

## License

MIT
