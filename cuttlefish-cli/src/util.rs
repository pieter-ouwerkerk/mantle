use std::io::{IsTerminal, Read};
use std::path::Path;

/// Resolve the root of the git repository containing `path`.
///
/// Handles both regular repos (`.git` directory) and linked worktrees
/// (`.git` file with a `gitdir:` pointer).
pub fn resolve_repo_root(path: &str) -> Option<String> {
    let p = Path::new(path);
    let dir = if p.is_dir() {
        p.to_path_buf()
    } else {
        p.parent()?.to_path_buf()
    };
    let mut current = dir;
    loop {
        if current.join(".git").is_dir() {
            return Some(current.to_string_lossy().to_string());
        }
        let git_path = current.join(".git");
        if git_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&git_path) {
                let content = content.trim();
                if let Some(gitdir) = content.strip_prefix("gitdir:") {
                    let gitdir = gitdir.trim();
                    // A linked worktree's .git file points to
                    // <main-repo>/.git/worktrees/<name>. Walk up to find the
                    // main .git directory and return its parent.
                    let gitdir_path = Path::new(gitdir);
                    let abs_gitdir = if gitdir_path.is_absolute() {
                        gitdir_path.to_path_buf()
                    } else {
                        current.join(gitdir_path)
                    };
                    // Walk up from abs_gitdir until we find the directory
                    // whose parent contains a real .git dir.
                    let mut candidate = abs_gitdir.as_path();
                    while let Some(parent) = candidate.parent() {
                        if parent.join(".git").is_dir() {
                            return Some(parent.to_string_lossy().to_string());
                        }
                        candidate = parent;
                    }
                    // Fallback: treat the directory containing the .git file
                    // as the repo root (plain submodule or non-worktree case).
                    return Some(current.to_string_lossy().to_string());
                }
            }
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Read and deserialize a JSON value from stdin.
///
/// Returns `None` if stdin is a terminal (interactive) or if parsing fails.
pub fn read_stdin<T: serde::de::DeserializeOwned>() -> Option<T> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    serde_json::from_str(&buf).ok()
}

/// Resolve the repo root starting from the directory that contains `file_path`.
pub fn resolve_repo_root_from_file(file_path: &str) -> Option<String> {
    let p = Path::new(file_path);
    let dir = if p.is_dir() { p } else { p.parent()? };
    resolve_repo_root(&dir.to_string_lossy())
}
