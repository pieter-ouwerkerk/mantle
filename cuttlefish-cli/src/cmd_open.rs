#![cfg(feature = "cuttlefish-app")]

use std::process;

use crate::config::Config;
use crate::socket::{SocketClient, SocketMessage};
use crate::util::resolve_repo_root;

pub fn run(path: &str, dry_run: bool) {
    let repo_path = match resolve_repo_root(path) {
        Some(p) => p,
        None => {
            eprintln!("error: not a git repository: {path}");
            process::exit(1);
        }
    };

    if dry_run {
        println!("{{\"repo_path\": \"{repo_path}\", \"valid\": true}}");
        return;
    }

    if let Some(mut client) = SocketClient::connect() {
        let mut msg = SocketMessage::new("open_repo");
        msg.path = Some(repo_path.clone());
        if let Some(resp) = client.send(&mut msg) {
            if resp.ok.unwrap_or(false) {
                return;
            }
        }
    }

    cold_launch(&repo_path);
}

fn cold_launch(repo_path: &str) {
    let config = Config::load();

    let app = if let Some(ref p) = config.app_path {
        p.clone()
    } else if let Some(p) = find_app_mdfind() {
        p
    } else if std::path::Path::new("/Applications/Cuttlefish.app").exists() {
        "/Applications/Cuttlefish.app".to_string()
    } else {
        let _ = std::process::Command::new("/usr/bin/open")
            .arg("-b")
            .arg("com.pieterouwerkerk.cuttlefish")
            .arg("--args")
            .arg("--open")
            .arg(repo_path)
            .output();
        return;
    };

    let _ = std::process::Command::new("/usr/bin/open")
        .arg("-a")
        .arg(&app)
        .arg("--args")
        .arg("--open")
        .arg(repo_path)
        .output();
}

fn find_app_mdfind() -> Option<String> {
    let output = std::process::Command::new("/usr/bin/mdfind")
        .args(["kMDItemCFBundleIdentifier == 'com.pieterouwerkerk.cuttlefish'"])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout);
    let first = path.lines().next()?.trim().to_string();
    if first.is_empty() { None } else { Some(first) }
}
