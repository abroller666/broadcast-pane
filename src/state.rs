//! What the console and the picker hand each other. Herdr shows one popup at
//! a time, so switching between them closes one popup and opens the other;
//! the selection and the typed lines survive in a file in between.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::echo::Echo;

#[derive(Default, Serialize, Deserialize)]
pub struct State {
    /// The tab being broadcast to.
    pub tab: String,
    /// Panes the user turned off. Panes not listed are selected, so panes
    /// opened since are included.
    pub deselected: Vec<String>,
    pub echo: Echo,
}

/// `HERDR_PLUGIN_STATE_DIR/session.json`, or the temp dir outside Herdr.
fn path() -> PathBuf {
    std::env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("broadcast-pane-session.json")
}

/// The state saved by the last popup, if any.
pub fn load() -> Option<State> {
    let text = std::fs::read_to_string(path()).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save(state: &State) -> Result<(), String> {
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string(state).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Forgets the saved state, so the next console starts fresh.
pub fn clear() {
    let _ = std::fs::remove_file(path());
}
