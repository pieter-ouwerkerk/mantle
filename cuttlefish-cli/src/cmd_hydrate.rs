use std::io::Read;
use std::process;

use serde::Deserialize;

#[derive(Deserialize)]
struct HookInput {
    #[serde(alias = "worktree_path")]
    worktree_path: Option<String>,
    #[serde(alias = "source_path")]
    #[serde(alias = "cwd")]
    source: Option<String>,
    exclude: Option<Vec<String>>,
}

pub fn run(worktree: Option<String>, source: Option<String>, exclude: Vec<String>) {
    let (worktree_path, source_path, exclude_list) = if let Some(wt) = worktree {
        (wt, source.unwrap_or_else(|| ".".to_string()), exclude)
    } else if let Some(input) = read_hook_input() {
        let wt = input.worktree_path.unwrap_or_else(|| {
            eprintln!("error: --worktree is required (or provide worktree_path in stdin JSON)");
            process::exit(1);
        });
        let src = input.source.unwrap_or_else(|| ".".to_string());
        let exc = input.exclude.unwrap_or_default();
        (wt, src, exc)
    } else {
        eprintln!("error: --worktree is required");
        process::exit(1);
    };

    let repo_path = resolve_repo_root(&source_path).unwrap_or(source_path);

    match mantle::hydrate(&repo_path, &worktree_path, &exclude_list) {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

#[allow(unsafe_code)]
fn read_hook_input() -> Option<HookInput> {
    if unsafe { libc::isatty(libc::STDIN_FILENO) != 0 } {
        return None;
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    serde_json::from_str(&buf).ok()
}

fn resolve_repo_root(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    let dir = if p.is_dir() { p } else { p.parent()? };
    let mut current = dir.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current.to_string_lossy().to_string());
        }
        if !current.pop() {
            return None;
        }
    }
}
