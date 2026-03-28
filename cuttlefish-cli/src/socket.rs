#![cfg(feature = "cuttlefish-app")]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::Config;

const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(8);
const LAUNCH_POLL_INTERVAL: Duration = Duration::from_millis(200);

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
    pub error: Option<String>,
    #[serde(rename = "worktreePath")]
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
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

    pub fn connect_or_launch() -> Option<Self> {
        if let Some(client) = Self::connect() {
            return Some(client);
        }
        launch_app();
        let start = Instant::now();
        while start.elapsed() < LAUNCH_TIMEOUT {
            std::thread::sleep(LAUNCH_POLL_INTERVAL);
            if let Some(client) = Self::connect() {
                return Some(client);
            }
        }
        None
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
                Ok(0) => return None,
                Ok(_) => {
                    if let Ok(resp) = serde_json::from_str::<SocketResponse>(&line) {
                        return Some(resp);
                    }
                }
                Err(_) => return None,
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

fn launch_app() {
    let config = Config::load();

    if let Some(app_path) = &config.app_path {
        let _ = std::process::Command::new("/usr/bin/open")
            .arg("-a")
            .arg(app_path)
            .output();
        return;
    }

    if let Ok(output) = std::process::Command::new("/usr/bin/mdfind")
        .args(["kMDItemCFBundleIdentifier == 'com.pieterouwerkerk.cuttlefish'"])
        .output()
    {
        let path = String::from_utf8_lossy(&output.stdout);
        if let Some(first) = path.lines().next() {
            let first = first.trim();
            if !first.is_empty() {
                let _ = std::process::Command::new("/usr/bin/open")
                    .arg("-a")
                    .arg(first)
                    .output();
                return;
            }
        }
    }

    let _ = std::process::Command::new("/usr/bin/open")
        .arg("-b")
        .arg("com.pieterouwerkerk.cuttlefish")
        .output();
}
