//! Decides which panes receive the broadcast.

use serde::Deserialize;
use serde_json::Value;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PaneInfo {
    pub pane_id: String,
    pub tab_id: String,
    /// The pane's name, set with `herdr pane rename` (prefix+P).
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// A tab's layout, from `pane.layout`.
#[derive(Debug, Clone, Deserialize)]
pub struct Layout {
    pub area: Area,
    pub panes: Vec<PanePlace>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Area {
    pub height: u32,
}

/// Where a pane sits on screen.
#[derive(Debug, Clone, Deserialize)]
pub struct PanePlace {
    pub pane_id: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
}

/// A pane the console can broadcast to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub pane_id: String,
    /// Short name shown in the console.
    pub label: String,
    /// The agent running in it, or empty.
    pub agent: String,
    /// Its working directory, with the home directory shown as `~`.
    pub cwd: String,
    /// Whether keystrokes are sent to it.
    pub selected: bool,
}

/// Longest label, in columns.
pub const LABEL_WIDTH: usize = 16;

/// Every pane in `tab_id`, all selected. The popup console is not a pane, so
/// the pane it was opened from is included. Panes are in reading order on
/// screen (top to bottom, then left to right) when `places` has them, so the
/// numbers shown in the console follow the layout.
pub fn select(panes: &[PaneInfo], tab_id: &str, places: &[PanePlace], home: &str) -> Vec<Target> {
    let mut panes: Vec<&PaneInfo> = panes.iter().filter(|p| p.tab_id == tab_id).collect();
    let place = |p: &PaneInfo| {
        places
            .iter()
            .find(|pl| pl.pane_id == p.pane_id)
            .map(|pl| (pl.rect.y, pl.rect.x))
    };
    // Stable sort: panes missing from the layout keep list order, at the end.
    panes.sort_by_key(|p| place(p).map_or((1, 0, 0), |(y, x)| (0, y, x)));
    panes
        .into_iter()
        .map(|p| Target {
            pane_id: p.pane_id.clone(),
            label: truncate(&label(p), LABEL_WIDTH),
            agent: non_empty(&p.agent).unwrap_or_default().to_string(),
            cwd: tilde(non_empty(&p.cwd).unwrap_or_default(), home),
            selected: true,
        })
        .collect()
}

fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// The pane's name, else the terminal title, else the agent, else the last
/// part of the cwd, else the id.
fn label(p: &PaneInfo) -> String {
    if let Some(name) = non_empty(&p.label) {
        return name.to_string();
    }
    if let Some(title) = non_empty(&p.terminal_title_stripped) {
        return title.to_string();
    }
    if let Some(agent) = non_empty(&p.agent) {
        return agent.to_string();
    }
    if let Some(cwd) = non_empty(&p.cwd) {
        let base = cwd.trim_end_matches('/').rsplit('/').next().unwrap_or(cwd);
        if !base.is_empty() {
            return base.to_string();
        }
    }
    p.pane_id.clone()
}

/// `path` with a leading `home` replaced by `~`.
fn tilde(path: &str, home: &str) -> String {
    match path.strip_prefix(home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            format!("~{rest}")
        }
        _ => path.to_string(),
    }
}

/// `s` cut to `width` columns, ending in "…" when cut.
fn truncate(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            // Make room for the ellipsis.
            while used + 1 > width {
                let Some(last) = out.pop() else { break };
                used -= last.width().unwrap_or(0);
            }
            out.push('…');
            return out;
        }
        used += w;
        out.push(c);
    }
    out
}

/// The first pane id in `HERDR_PLUGIN_CONTEXT_JSON`: the tiled pane under the popup.
pub fn context_pane_id(ctx: &Value) -> Option<String> {
    let Value::Object(map) = ctx else {
        return None;
    };
    for key in ["pane_id", "focused_pane_id"] {
        if let Some(Value::String(id)) = map.get(key) {
            return Some(id.clone());
        }
    }
    for key in ["pane", "focused_pane"] {
        if let Some(id) = map.get(key).and_then(context_pane_id) {
            return Some(id);
        }
    }
    map.values().find_map(context_pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pane(pane_id: &str, tab_id: &str) -> PaneInfo {
        PaneInfo {
            pane_id: pane_id.into(),
            tab_id: tab_id.into(),
            ..Default::default()
        }
    }

    fn place(pane_id: &str, x: u32, y: u32) -> PanePlace {
        PanePlace {
            pane_id: pane_id.into(),
            rect: Rect { x, y },
        }
    }

    fn ids(targets: &[Target]) -> Vec<&str> {
        targets.iter().map(|t| t.pane_id.as_str()).collect()
    }

    #[test]
    fn selects_every_pane_in_the_tab() {
        let panes = [
            pane("w1:p1", "w1:t1"),
            pane("w1:p2", "w1:t1"),
            pane("w1:p3", "w1:t2"),
        ];
        let targets = select(&panes, "w1:t1", &[], "");
        assert_eq!(ids(&targets), ["w1:p1", "w1:p2"]);
        assert!(targets.iter().all(|t| t.selected));
    }

    #[test]
    fn orders_panes_by_screen_position() {
        let panes = [
            pane("w1:pB", "w1:t1"), // bottom right
            pane("w1:pX", "w1:t1"), // not in the layout
            pane("w1:pR", "w1:t1"), // top right
            pane("w1:pL", "w1:t1"), // left, full height
        ];
        let places = [
            place("w1:pB", 87, 26),
            place("w1:pR", 87, 0),
            place("w1:pL", 0, 0),
        ];
        let targets = select(&panes, "w1:t1", &places, "");
        assert_eq!(ids(&targets), ["w1:pL", "w1:pR", "w1:pB", "w1:pX"]);
    }

    #[test]
    fn labels_by_name_then_title_then_agent_then_cwd() {
        let mut p = pane("w1:p1", "w1:t1");
        assert_eq!(label(&p), "w1:p1");
        p.cwd = Some("/Users/me/dev/proj/".into());
        assert_eq!(label(&p), "proj");
        p.agent = Some("claude".into());
        assert_eq!(label(&p), "claude");
        p.terminal_title_stripped = Some("  ".into());
        assert_eq!(label(&p), "claude");
        p.terminal_title_stripped = Some("vim main.rs".into());
        assert_eq!(label(&p), "vim main.rs");
        p.label = Some("api server".into());
        assert_eq!(label(&p), "api server");
    }

    #[test]
    fn shows_home_as_tilde() {
        assert_eq!(tilde("/Users/me/dev", "/Users/me"), "~/dev");
        assert_eq!(tilde("/Users/me", "/Users/me"), "~");
        assert_eq!(tilde("/Users/meg/dev", "/Users/me"), "/Users/meg/dev");
        assert_eq!(tilde("/tmp", ""), "/tmp");
    }

    #[test]
    fn truncates_long_labels() {
        assert_eq!(truncate("abcdef", 6), "abcdef");
        assert_eq!(truncate("abcdefg", 6), "abcde…");
        assert_eq!(truncate("日本語です", 6), "日本…");
    }

    #[test]
    fn context_pane_id_prefers_top_level_key() {
        let ctx = json!({"workspace": {"pane_id": "w1:p9"}, "focused_pane_id": "w1:p1"});
        assert_eq!(context_pane_id(&ctx).as_deref(), Some("w1:p1"));
    }

    #[test]
    fn context_pane_id_finds_nested_pane() {
        let ctx = json!({"tab": {"id": "w1:t1"}, "focused_pane": {"pane_id": "w1:p2"}});
        assert_eq!(context_pane_id(&ctx).as_deref(), Some("w1:p2"));
    }

    #[test]
    fn context_pane_id_none_without_pane() {
        assert_eq!(context_pane_id(&json!({"tab_id": "w1:t1"})), None);
    }
}
