use std::path::Path;
use std::time::Instant;

use crate::ops::artifacts::scan_worktreeinclude;
use crate::ops::cow::cow_clone_directory;
use crate::error::Error;
use crate::types::{HydrationResult, HydrationStrategy};

/// Hydrate a worktree by CoW-cloning build artifacts from the source repo.
///
/// Scans the repo for candidates using `.worktreeinclude` (or `.gitignore` fallback),
/// filters out excluded directories, and clones each candidate into the worktree.
pub fn hydrate(
    repo_path: &str,
    worktree_path: &str,
    exclude: &[String],
) -> Result<HydrationResult, Error> {
    let start = Instant::now();

    let scan_result = scan_worktreeinclude(repo_path, worktree_path, true)?;

    let mut cloned = Vec::new();
    let mut skipped = Vec::new();
    let mut errors = Vec::new();

    for candidate in &scan_result.clone_candidates {
        let dir_name = Path::new(&candidate.source_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if exclude.iter().any(|e| e == &dir_name) {
            skipped.push(candidate.dest_path.clone());
            continue;
        }

        if candidate.strategy != HydrationStrategy::CowClone {
            skipped.push(candidate.dest_path.clone());
            continue;
        }

        if Path::new(&candidate.dest_path).exists() {
            skipped.push(candidate.dest_path.clone());
            continue;
        }

        match cow_clone_directory(&candidate.source_path, &candidate.dest_path) {
            Ok(result) => {
                cloned.push(candidate.dest_path.clone());
                errors.extend(result.errors);
            }
            Err(e) => {
                errors.push(format!("Failed to clone {}: {}", candidate.source_path, e));
            }
        }
    }

    // Also copy matched files
    for file_candidate in &scan_result.file_candidates {
        let dest = Path::new(worktree_path).join(&file_candidate.relative_path);
        if dest.exists() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::copy(&file_candidate.absolute_path, &dest) {
            Ok(_) => {}
            Err(e) => {
                errors.push(format!(
                    "Failed to copy {}: {}",
                    file_candidate.relative_path, e
                ));
            }
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    let elapsed_ms = start.elapsed().as_millis() as u64;

    Ok(HydrationResult {
        cloned,
        skipped,
        errors,
        elapsed_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_hydrate_clones_matching_candidates() {
        let repo = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();

        fs::create_dir(repo.path().join("node_modules")).unwrap();
        fs::write(
            repo.path().join("node_modules/index.js"),
            "module.exports = {}",
        )
        .unwrap();
        fs::write(repo.path().join(".worktreeinclude"), "node_modules\n").unwrap();

        let result = hydrate(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            &[],
        )
        .unwrap();

        assert_eq!(result.cloned.len(), 1);
        assert!(result.cloned[0].ends_with("node_modules"));
        assert!(worktree.path().join("node_modules/index.js").exists());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_hydrate_respects_exclude() {
        let repo = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();

        fs::create_dir(repo.path().join("node_modules")).unwrap();
        fs::create_dir(repo.path().join("target")).unwrap();
        fs::write(
            repo.path().join(".worktreeinclude"),
            "node_modules\ntarget\n",
        )
        .unwrap();

        let result = hydrate(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            &["node_modules".to_string()],
        )
        .unwrap();

        assert_eq!(result.cloned.len(), 1);
        assert!(result.cloned[0].ends_with("target"));
        assert!(!worktree.path().join("node_modules").exists());
    }

    #[test]
    fn test_hydrate_no_config_returns_empty() {
        let repo = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();

        let result = hydrate(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            &[],
        )
        .unwrap();

        assert!(result.cloned.is_empty());
        assert!(result.skipped.is_empty());
    }
}
