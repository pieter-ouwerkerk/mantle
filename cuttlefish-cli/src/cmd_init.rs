use std::path::Path;
use std::process;

/// Run `cuttlefish init` — bootstrap a .worktreeinclude file for the repo.
pub fn run(path: &str, dry_run: bool, show: bool) {
    let Some(repo_path) = resolve_repo_root(path) else {
        eprintln!("error: not a git repository: {path}");
        process::exit(1);
    };

    if show {
        run_show(&repo_path);
        return;
    }

    match mantle_git::bootstrap_worktreeinclude(repo_path.clone()) {
        Ok(result) => {
            if result.already_exists {
                eprintln!(".worktreeinclude already exists");
                if dry_run {
                    println!("{}", result.content);
                }
                return;
            }

            if dry_run {
                println!("{}", result.content);
                return;
            }

            eprintln!("created .worktreeinclude");
            if !result.gitignore_dirs.is_empty() {
                eprintln!(
                    "  included from .gitignore: {}",
                    result.gitignore_dirs.join(", ")
                );
            }
            if !result.builtin_dirs.is_empty() {
                eprintln!(
                    "  included from built-ins: {}",
                    result.builtin_dirs.join(", ")
                );
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn run_show(repo_path: &str) {
    let threshold = 10 * 1024 * 1024;
    match mantle_git::compute_effective_worktreeinclude(repo_path.to_string(), threshold) {
        Ok(result) => {
            if result.has_worktreeinclude_file {
                eprintln!("source: .worktreeinclude");
            } else {
                eprintln!("source: .gitignore (fallback)");
            }
            eprintln!();

            let included: Vec<_> = result.entries.iter().filter(|e| e.included).collect();
            let suggestions: Vec<_> = result.entries.iter().filter(|e| !e.included).collect();

            if included.is_empty() {
                eprintln!("no directories will be hydrated");
            } else {
                eprintln!("will hydrate:");
                for entry in &included {
                    let size_mb = entry.size_bytes / (1024 * 1024);
                    let exists = if entry.exists_on_disk {
                        ""
                    } else {
                        " (not on disk)"
                    };
                    eprintln!("  {} ({}MB){}", entry.path, size_mb, exists);
                }
            }

            if !suggestions.is_empty() {
                eprintln!();
                eprintln!("suggestions:");
                for entry in &suggestions {
                    let size_mb = entry.size_bytes / (1024 * 1024);
                    eprintln!(
                        "  {} ({}MB) — consider adding to .worktreeinclude",
                        entry.path, size_mb
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn resolve_repo_root(path: &str) -> Option<String> {
    let p = Path::new(path);
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
