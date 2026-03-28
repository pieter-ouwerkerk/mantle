use std::path::{Path, PathBuf};
use std::process;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::permissions;
use crate::util::{read_stdin, resolve_repo_root};

#[derive(Deserialize)]
struct CreateHookInput {
    name: Option<String>,
    cwd: Option<String>,
}

#[derive(Deserialize)]
struct RemoveHookInput {
    #[serde(alias = "worktree_path")]
    worktree_path: Option<String>,
}

#[derive(Serialize)]
struct CreateOutput {
    worktree_path: String,
    branch: String,
}

pub fn run_create(name: Option<String>, cwd: Option<String>) {
    let (task_name, working_dir) = if let Some(n) = name {
        (n, cwd.unwrap_or_else(|| ".".to_string()))
    } else if let Some(input) = read_stdin::<CreateHookInput>() {
        let n = input.name.unwrap_or_else(|| {
            eprintln!("error: --name is required");
            process::exit(1);
        });
        let c = input.cwd.unwrap_or_else(|| ".".to_string());
        (n, c)
    } else {
        eprintln!("error: --name is required");
        process::exit(1);
    };

    let Some(repo_root) = resolve_repo_root(&working_dir) else {
        eprintln!("error: not a git repository: {working_dir}");
        process::exit(1);
    };

    let slug = sanitize_slug(&task_name);
    let repo_name = Path::new(&repo_root)
        .file_name()
        .map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().to_string());

    let worktree_base = worktree_base_dir(&repo_name);
    let worktree_path = worktree_base.join(&slug);
    let branch = format!("claude/{slug}");

    if let Err(e) = std::fs::create_dir_all(&worktree_base) {
        eprintln!("error: failed to create worktree directory: {e}");
        process::exit(1);
    }

    let wt_str = worktree_path.to_string_lossy().to_string();

    if let Err(e) = mantle_git::worktree_add_new_branch(&repo_root, &wt_str, &branch, "HEAD") {
        eprintln!("error: failed to create worktree: {e}");
        process::exit(1);
    }

    match mantle_git::hydrate(&repo_root, &wt_str, &[]) {
        Ok(result) => {
            if !result.cloned.is_empty() {
                eprintln!("hydrated {} directories", result.cloned.len());
            }
        }
        Err(e) => {
            eprintln!("warning: hydration failed: {e}");
        }
    }

    let settings_path = worktree_path.join(".claude").join("settings.local.json");
    if let Err(e) = permissions::inject_grants(&wt_str, &settings_path.to_string_lossy()) {
        eprintln!("warning: failed to inject permissions: {e}");
    }

    let mcp_src = Path::new(&repo_root).join(".mcp.json");
    let mcp_dst = worktree_path.join(".mcp.json");
    if mcp_src.exists() && !mcp_dst.exists() {
        let _ = std::fs::copy(&mcp_src, &mcp_dst);
    }

    let output = CreateOutput {
        worktree_path: wt_str,
        branch,
    };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

pub fn run_remove(path: Option<String>, force: bool) {
    let worktree_path = if let Some(p) = path {
        p
    } else if let Some(input) = read_stdin::<RemoveHookInput>() {
        input.worktree_path.unwrap_or_else(|| {
            eprintln!("error: --path is required");
            process::exit(1);
        })
    } else {
        eprintln!("error: --path is required");
        process::exit(1);
    };

    let Some(repo_root) = resolve_repo_root(&worktree_path) else {
        eprintln!("error: could not find parent repo for worktree");
        process::exit(1);
    };

    let result = if force {
        mantle_git::worktree_remove_force(&repo_root, &worktree_path)
    } else {
        mantle_git::worktree_remove_clean(&repo_root, &worktree_path)
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        process::exit(1);
    }

    eprintln!("removed worktree: {worktree_path}");
}

pub fn sha256_prefix(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex_encode(&result[..6])
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

pub fn sanitize_slug(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let mut result = String::new();
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash && !result.is_empty() {
                result.push(c);
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }

    if result.len() > 60 {
        result.truncate(60);
    }

    result.trim_end_matches('-').to_string()
}

fn worktree_base_dir(repo_name: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".cuttlefish")
        .join("worktrees")
        .join(repo_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_slug_basic() {
        assert_eq!(sanitize_slug("My Feature"), "my-feature");
    }

    #[test]
    fn test_sanitize_slug_special_chars() {
        assert_eq!(sanitize_slug("fix/CUT-123: auth bug"), "fix-cut-123-auth-bug");
    }

    #[test]
    fn test_sanitize_slug_truncation() {
        let long_name = "a".repeat(100);
        let result = sanitize_slug(&long_name);
        assert!(result.len() <= 60);
    }

    #[test]
    fn test_sanitize_slug_no_trailing_dash() {
        assert_eq!(sanitize_slug("test-"), "test");
    }

    #[test]
    fn test_sanitize_slug_consecutive_dashes() {
        assert_eq!(sanitize_slug("a---b"), "a-b");
    }
}
