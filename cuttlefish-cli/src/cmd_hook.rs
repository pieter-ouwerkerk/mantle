use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process;

use serde::Deserialize;
use serde_json::json;

use crate::cmd_worktree;
use crate::util::{resolve_repo_root, resolve_repo_root_from_file};

#[derive(Deserialize)]
struct HookEvent {
    #[serde(alias = "hook_event_name")]
    event: Option<String>,
    #[serde(alias = "tool_name")]
    tool_name: Option<String>,
    #[serde(alias = "tool_input")]
    tool_input: Option<serde_json::Value>,
    session_id: Option<String>,
}

const WHITELIST: &[&str] = &[".claude/", "CLAUDE.md", "AGENTS.md"];

pub fn run() {
    let Some(event) = read_event() else { return };

    let event_name = event.event.as_deref().unwrap_or("");

    match event_name {
        "PreToolUse" => handle_pre_tool_use(&event),
        "WorktreeCreate" => handle_worktree_create(),
        "WorktreeRemove" => handle_worktree_remove(),
        _ => {}
    }
}

fn handle_pre_tool_use(event: &HookEvent) {
    let tool = event.tool_name.as_deref().unwrap_or("");

    if !matches!(tool, "Edit" | "Write" | "NotebookEdit") {
        return;
    }

    let file_path = event
        .tool_input
        .as_ref()
        .and_then(|v| v.get("file_path").or(v.get("path")))
        .and_then(|v| v.as_str());

    let Some(file_path) = file_path else { return };

    if is_in_worktree(file_path) {
        return;
    }

    if is_whitelisted(file_path) {
        return;
    }

    let session_id = event.session_id.as_deref().unwrap_or("unknown");
    let cache_file = cache_path(session_id);

    if let Some(cached_wt) = read_cached_worktree(&cache_file) {
        deny_with_redirect(file_path, &cached_wt);
        return;
    }

    // Auto-create a worktree
    let cwd = Path::new(file_path)
        .parent()
        .and_then(|p| resolve_repo_root(&p.to_string_lossy()))
        .unwrap_or_else(|| ".".to_string());

    let slug = format!("session-{}", &cmd_worktree::sha256_prefix(session_id));

    let repo_name = Path::new(&cwd)
        .file_name()
        .map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().to_string());

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let wt_path = PathBuf::from(&home)
        .join(".cuttlefish")
        .join("worktrees")
        .join(&repo_name)
        .join(&slug);
    let wt_str = wt_path.to_string_lossy().to_string();
    let branch = format!("claude/{slug}");

    if let Err(e) = std::fs::create_dir_all(wt_path.parent().unwrap()) {
        eprintln!("error: failed to create worktree directory: {e}");
        return;
    }

    if let Err(e) = mantle::worktree_add_new_branch(&cwd, &wt_str, &branch, "HEAD") {
        eprintln!("error: failed to create worktree: {e}");
        return;
    }

    let _ = mantle::hydrate(&cwd, &wt_str, &[]);

    let settings_path = wt_path.join(".claude").join("settings.local.json");
    let _ = crate::permissions::inject_grants(&wt_str, &settings_path.to_string_lossy());

    let _ = std::fs::write(&cache_file, &wt_str);

    deny_with_redirect(file_path, &wt_str);
}

fn handle_worktree_create() {
    cmd_worktree::run_create(None, None);
}

fn handle_worktree_remove() {
    cmd_worktree::run_remove(None, false);
}

fn deny_with_redirect(original_path: &str, worktree_path: &str) {
    let response = json!({
        "permissionDecision": "deny",
        "reason": format!(
            "Edits must go to the worktree at {}. Rewrite your path from {} to the equivalent path under the worktree.",
            worktree_path, original_path
        )
    });
    println!("{response}");
    process::exit(0);
}

fn is_in_worktree(file_path: &str) -> bool {
    let mut current = PathBuf::from(file_path);
    loop {
        let git = current.join(".git");
        if git.is_file() {
            return true;
        }
        if git.is_dir() {
            return false;
        }
        if !current.pop() {
            return false;
        }
    }
}

fn is_whitelisted(file_path: &str) -> bool {
    let Some(repo_root) = resolve_repo_root_from_file(file_path) else {
        return false;
    };

    let relative = match Path::new(file_path).strip_prefix(&repo_root) {
        Ok(r) => r.to_string_lossy().to_string(),
        Err(_) => return false,
    };

    WHITELIST.iter().any(|pattern| {
        if pattern.ends_with('/') {
            relative.starts_with(pattern)
        } else {
            relative == *pattern
        }
    })
}

fn cache_path(session_id: &str) -> PathBuf {
    let hash = cmd_worktree::sha256_prefix(session_id);
    PathBuf::from("/tmp").join(format!("cuttlefish-wt-{hash}"))
}

fn read_cached_worktree(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().filter(|s| !s.is_empty())
}

fn read_event() -> Option<HookEvent> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    if buf.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&buf).ok()
}
