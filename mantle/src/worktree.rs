use std::path::Path;

use crate::error::MantleError;
use crate::repo;
use crate::types::WorktreeInfo;

fn open_git2(repo_path: &str) -> Result<git2::Repository, MantleError> {
    git2::Repository::open(repo_path).map_err(MantleError::internal)
}

/// List all worktrees using gix (pure read-only, no process spawn).
pub fn list_worktrees(repo_path: &str) -> Result<Vec<WorktreeInfo>, MantleError> {
    let ts_repo = repo::open(repo_path)?;
    let repo = ts_repo.to_thread_local();

    let mut result = Vec::new();

    // Main worktree
    let main_head = repo.head_id().map(|id| id.to_string()).unwrap_or_default();
    let main_branch = repo
        .head_ref()
        .ok()
        .flatten()
        .map(|r| r.name().shorten().to_string());
    let main_path = repo.workdir().map_or_else(
        || repo_path.to_owned(),
        |p: &std::path::Path| p.to_string_lossy().to_string(),
    );

    result.push(WorktreeInfo {
        path: main_path,
        head: main_head,
        branch: main_branch,
        is_main: true,
    });

    // Linked worktrees
    let proxies = repo.worktrees().map_err(MantleError::internal)?;
    for proxy in proxies {
        let wt_path = proxy
            .base()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let linked_repo = proxy
            .into_repo_with_possibly_inaccessible_worktree()
            .map_err(MantleError::internal)?;

        let head = linked_repo
            .head_id()
            .map(|id| id.to_string())
            .unwrap_or_default();
        let branch = linked_repo
            .head_ref()
            .ok()
            .flatten()
            .map(|r| r.name().shorten().to_string());

        result.push(WorktreeInfo {
            path: wt_path,
            head,
            branch,
            is_main: false,
        });
    }

    Ok(result)
}

/// Add a worktree with a new branch via git2.
pub fn worktree_add_new_branch(
    repo_path: &str,
    path: &str,
    branch: &str,
    start_point: &str,
) -> Result<(), MantleError> {
    let repo = open_git2(repo_path)?;

    // Resolve start_point to a commit and create the branch
    let start = repo
        .revparse_single(start_point)
        .map_err(|_| MantleError::RevNotFound {
            rev: start_point.to_owned(),
        })?;
    let commit = start.peel_to_commit().map_err(MantleError::internal)?;
    let branch_ref = repo
        .branch(branch, &commit, false)
        .map_err(MantleError::internal)?;

    // Create the worktree using the new branch
    let reference = branch_ref.into_reference();
    let mut opts = git2::WorktreeAddOptions::new();
    opts.reference(Some(&reference));

    let wt_name = Path::new(path)
        .file_name()
        .map_or_else(|| branch.to_owned(), |n| n.to_string_lossy().to_string());

    repo.worktree(&wt_name, Path::new(path), Some(&opts))
        .map_err(MantleError::internal)?;

    Ok(())
}

/// Add a worktree for an existing branch via git2.
pub fn worktree_add_existing(repo_path: &str, path: &str, branch: &str) -> Result<(), MantleError> {
    let repo = open_git2(repo_path)?;

    let branch_ref = repo
        .find_branch(branch, git2::BranchType::Local)
        .map_err(MantleError::internal)?;

    let reference = branch_ref.into_reference();
    let mut opts = git2::WorktreeAddOptions::new();
    opts.reference(Some(&reference));

    let wt_name = Path::new(path)
        .file_name()
        .map_or_else(|| branch.to_owned(), |n| n.to_string_lossy().to_string());

    repo.worktree(&wt_name, Path::new(path), Some(&opts))
        .map_err(MantleError::internal)?;

    Ok(())
}

/// Remove a worktree (clean only) — errors if the worktree has uncommitted changes.
pub fn worktree_remove_clean(repo_path: &str, path: &str) -> Result<(), MantleError> {
    // Check if the worktree is clean before removing
    {
        let wt_repo = open_git2(path)?;
        let statuses = wt_repo
            .statuses(Some(
                git2::StatusOptions::new()
                    .include_untracked(true)
                    .recurse_untracked_dirs(true)
                    .include_ignored(false),
            ))
            .map_err(MantleError::internal)?;
        if !statuses.is_empty() {
            return Err(MantleError::Internal {
                message: format!(
                    "cannot remove worktree '{}': has {} uncommitted change(s); use force to override",
                    path,
                    statuses.len()
                ),
            });
        }
    }

    // Remove the working tree directory
    std::fs::remove_dir_all(path).map_err(|e| MantleError::Internal {
        message: format!("Failed to remove worktree directory: {e}"),
    })?;

    // Prune the now-stale admin entry
    prune_stale_worktrees(repo_path)?;

    Ok(())
}

/// Remove a worktree forcefully — removes even if dirty or locked.
pub fn worktree_remove_force(repo_path: &str, path: &str) -> Result<(), MantleError> {
    // Remove the working tree directory
    std::fs::remove_dir_all(path).map_err(|e| MantleError::Internal {
        message: format!("Failed to remove worktree directory: {e}"),
    })?;

    // Prune the now-stale admin entry
    prune_stale_worktrees(repo_path)?;

    Ok(())
}

/// Prune stale worktree metadata — removes admin entries for worktrees whose directories no longer exist.
pub fn worktree_prune(repo_path: &str) -> Result<(), MantleError> {
    prune_stale_worktrees(repo_path)
}

/// Internal: prune all invalid (stale) worktree entries via git2.
fn prune_stale_worktrees(repo_path: &str) -> Result<(), MantleError> {
    let repo = open_git2(repo_path)?;
    let names = repo.worktrees().map_err(MantleError::internal)?;

    for name in names.iter().flatten() {
        if let Ok(wt) = repo.find_worktree(name) {
            // validate() returns Err if the worktree's working directory is missing
            if wt.validate().is_err() {
                let mut opts = git2::WorktreePruneOptions::new();
                // Prune even though it was once valid (it's now stale)
                opts.valid(true);
                opts.working_tree(true);
                let _ = wt.prune(Some(&mut opts));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_test_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .unwrap();
        tmp
    }

    #[test]
    fn test_list_worktrees_main_only() {
        let tmp = init_test_repo();
        let repo_path = tmp.path().to_str().unwrap();
        let result = list_worktrees(repo_path).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].is_main);
    }

    #[test]
    fn test_create_and_list_worktree() {
        let tmp = init_test_repo();
        let repo_path = tmp.path().to_str().unwrap();
        let wt_path = tmp.path().join("wt1");
        worktree_add_new_branch(repo_path, wt_path.to_str().unwrap(), "test-branch", "HEAD")
            .unwrap();
        let result = list_worktrees(repo_path).unwrap();
        assert_eq!(result.len(), 2);
        let linked = result.iter().find(|w| !w.is_main).unwrap();
        assert_eq!(linked.branch.as_deref(), Some("test-branch"));
    }

    #[test]
    fn test_remove_worktree() {
        let tmp = init_test_repo();
        let repo_path = tmp.path().to_str().unwrap();
        let wt_path = tmp.path().join("wt-remove");
        worktree_add_new_branch(
            repo_path,
            wt_path.to_str().unwrap(),
            "remove-branch",
            "HEAD",
        )
        .unwrap();
        assert!(wt_path.exists());
        worktree_remove_clean(repo_path, wt_path.to_str().unwrap()).unwrap();
        assert!(!wt_path.exists());
        let result = list_worktrees(repo_path).unwrap();
        assert_eq!(result.len(), 1);
    }
}
