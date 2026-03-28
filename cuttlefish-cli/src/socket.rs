use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SocketClient {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u32,
}

#[derive(Debug, Serialize)]
pub struct SocketMessage {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(rename = "hookEventName", skip_serializing_if = "Option::is_none")]
    pub hook_event_name: Option<String>,
    #[serde(rename = "hookSessionId", skip_serializing_if = "Option::is_none")]
    pub hook_session_id: Option<String>,
    #[serde(rename = "hookModel", skip_serializing_if = "Option::is_none")]
    pub hook_model: Option<String>,
    #[serde(rename = "hookToolName", skip_serializing_if = "Option::is_none")]
    pub hook_tool_name: Option<String>,
    #[serde(rename = "filePath", skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hydrate: Option<bool>,
}

impl SocketMessage {
    pub fn new(type_: &str) -> Self {
        Self {
            type_: type_.to_string(),
            id: None,
            path: None,
            task: None,
            hook_event_name: None,
            hook_session_id: None,
            hook_model: None,
            hook_tool_name: None,
            file_path: None,
            hydrate: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SocketResponse {
    pub ok: Option<bool>,
}

impl SocketClient {
    pub fn connect() -> Option<Self> {
        let path = socket_path();
        let stream = UnixStream::connect(&path).ok()?;
        stream.set_read_timeout(Some(SOCKET_TIMEOUT)).ok()?;
        stream.set_write_timeout(Some(SOCKET_TIMEOUT)).ok()?;
        let writer = stream.try_clone().ok()?;
        Some(Self {
            reader: BufReader::new(stream),
            writer,
            next_id: 1,
        })
    }

    pub fn send(&mut self, msg: &mut SocketMessage) -> Option<SocketResponse> {
        let id = format!("r{}", self.next_id);
        self.next_id += 1;
        msg.id = Some(id);

        let json = serde_json::to_string(msg).ok()?;
        self.writer.write_all(json.as_bytes()).ok()?;
        self.writer.write_all(b"\n").ok()?;
        self.writer.flush().ok()?;

        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    if let Ok(resp) = serde_json::from_str::<SocketResponse>(&line) {
                        return Some(resp);
                    }
                }
            }
        }
    }

    pub fn send_fire_and_forget(&mut self, msg: &mut SocketMessage) {
        let id = format!("r{}", self.next_id);
        self.next_id += 1;
        msg.id = Some(id);

        if let Ok(json) = serde_json::to_string(msg) {
            let _ = self.writer.write_all(json.as_bytes());
            let _ = self.writer.write_all(b"\n");
            let _ = self.writer.flush();
        }
    }
}

fn socket_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".cuttlefish/cuttlefish.sock")
}
