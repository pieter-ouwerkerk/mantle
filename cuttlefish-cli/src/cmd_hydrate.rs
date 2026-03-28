use std::process;

use serde::Deserialize;

use crate::util::{read_stdin, resolve_repo_root};

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
    } else if let Some(input) = read_stdin::<HookInput>() {
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

    match mantle_git::hydrate(&repo_path, &worktree_path, &exclude_list) {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
