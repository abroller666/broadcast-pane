//! Requests to the Herdr socket API.
//!
//! Herdr answers one request per connection, so every call opens a fresh
//! Unix socket connection (about 0.3 ms locally, cheap enough per keystroke).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::{Value, json};

use crate::targets::{Layout, PaneInfo};

const TIMEOUT: Duration = Duration::from_secs(5);

pub struct Herdr {
    socket_path: String,
}

impl Herdr {
    /// Plugin panes always receive `HERDR_SOCKET_PATH`.
    pub fn from_env() -> Result<Self, String> {
        let socket_path = std::env::var("HERDR_SOCKET_PATH").map_err(|_| {
            "HERDR_SOCKET_PATH is not set; run this as a Herdr plugin pane".to_string()
        })?;
        Ok(Self { socket_path })
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| format!("could not reach Herdr socket: {e}"))?;
        stream.set_read_timeout(Some(TIMEOUT)).ok();
        stream.set_write_timeout(Some(TIMEOUT)).ok();

        let mut req =
            json!({"id": "broadcast-pane", "method": method, "params": params}).to_string();
        req.push('\n');
        (&stream)
            .write_all(req.as_bytes())
            .map_err(|e| format!("{method}: {e}"))?;

        let mut line = String::new();
        BufReader::new(&stream)
            .read_line(&mut line)
            .map_err(|e| format!("{method}: {e}"))?;
        let mut resp: Value = serde_json::from_str(&line)
            .map_err(|_| format!("{method}: unexpected reply from Herdr"))?;
        if let Some(err) = resp.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned);
            return Err(format!(
                "{method} failed: {}",
                msg.unwrap_or_else(|| err.to_string())
            ));
        }
        Ok(resp["result"].take())
    }

    pub fn list_panes(&self) -> Result<Vec<PaneInfo>, String> {
        let mut result = self.call("pane.list", json!({}))?;
        serde_json::from_value(result["panes"].take()).map_err(|e| format!("pane.list: {e}"))
    }

    /// The size of `pane_id`'s tab and where its panes sit on screen.
    pub fn layout(&self, pane_id: &str) -> Result<Layout, String> {
        let mut result = self.call("pane.layout", json!({"pane_id": pane_id}))?;
        serde_json::from_value(result["layout"].take()).map_err(|e| format!("pane.layout: {e}"))
    }

    /// Opens a pane entrypoint of `plugin_id` as a popup of the given size
    /// (terminal cells, or a percentage string like "60%").
    pub fn open_popup(
        &self,
        plugin_id: &str,
        entrypoint: &str,
        width: Value,
        height: Value,
    ) -> Result<(), String> {
        self.call(
            "plugin.pane.open",
            json!({
                "plugin_id": plugin_id,
                "entrypoint": entrypoint,
                "placement": "popup",
                "width": width,
                "height": height,
            }),
        )
        .map(drop)
    }

    pub fn send_text(&self, pane_id: &str, text: &str) -> Result<(), String> {
        self.call("pane.send_text", json!({"pane_id": pane_id, "text": text}))
            .map(drop)
    }
}
