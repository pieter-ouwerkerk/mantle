use std::path::Path;
use std::process::Command;

use crate::error::GitError;
use crate::types::{ConflictSides, MergeStateInfo, MergeStateKind};

fn open_git2(repo_path: &str) -> Result<git2::Repository, GitError> {
    git2::Repository::open(repo_path).map_err(GitError::internal)
}

/// Resolve the `.git` directory for a path, handling worktrees where `.git` is a file.
fn resolve_git_dir(repo_path: &str) -> Result<std::path::PathBuf, GitError> {
    let dot_git = Path::new(repo_path).join(".git");
    if dot_git.is_file() {
        // Worktree: .git is a file containing "gitdir: <path>"
        let content = std::fs::read_to_string(&dot_git).map_err(GitError::internal)?;
        let gitdir = content.strip_prefix("gitdir: ").unwrap_or(&content).trim();
        let gitdir_path = Path::new(gitdir);
        if gitdir_path.is_absolute() {
            Ok(gitdir_path.to_path_buf())
        } else {
            Ok(Path::new(repo_path).join(gitdir_path))
        }
    } else {
        Ok(dot_git)
    }
}

/// Check the merge/rebase/cherry-pick state of a repository.
pub fn merge_state(repo_path: &str) -> Result<MergeStateInfo, GitError> {
    let git_dir = resolve_git_dir(repo_path)?;

    // Check for MERGE_HEAD (merge in progress)
    if git_dir.join("MERGE_HEAD").exists() {
        let branch = read_merge_branch(&git_dir);
        let conflict_count = count_conflicts(repo_path)?;
        return Ok(MergeStateInfo {
            kind: MergeStateKind::Merge,
            conflict_count,
            branch,
        });
    }

    // Check for rebase-merge or rebase-apply (rebase in progress)
    if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
        let branch = read_rebase_branch(&git_dir);
        let conflict_count = count_conflicts(repo_path)?;
        return Ok(MergeStateInfo {
            kind: MergeStateKind::Rebase,
            conflict_count,
            branch,
        });
    }

    // Check for CHERRY_PICK_HEAD (cherry-pick in progress)
    if git_dir.join("CHERRY_PICK_HEAD").exists() {
        let conflict_count = count_conflicts(repo_path)?;
        return Ok(MergeStateInfo {
            kind: MergeStateKind::CherryPick,
            conflict_count,
            branch: None,
        });
    }

    Ok(MergeStateInfo {
        kind: MergeStateKind::None,
        conflict_count: 0,
        branch: None,
    })
}

/// Read the branch being merged from `MERGE_MSG` or `MERGE_HEAD`.
fn read_merge_branch(git_dir: &Path) -> Option<String> {
    // Try MERGE_MSG first — usually contains "Merge branch 'name'"
    if let Ok(msg) = std::fs::read_to_string(git_dir.join("MERGE_MSG")) {
        if let Some(rest) = msg.strip_prefix("Merge branch '") {
            if let Some(end) = rest.find('\'') {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

/// Read the branch being rebased onto.
fn read_rebase_branch(git_dir: &Path) -> Option<String> {
    let onto_path = git_dir.join("rebase-merge").join("onto");
    if let Ok(content) = std::fs::read_to_string(&onto_path) {
        return Some(content.trim().to_string());
    }
    let onto_path = git_dir.join("rebase-apply").join("onto");
    if let Ok(content) = std::fs::read_to_string(&onto_path) {
        return Some(content.trim().to_string());
    }
    None
}

/// Count unresolved conflicts using git2's index.
fn count_conflicts(repo_path: &str) -> Result<u32, GitError> {
    let repo = open_git2(repo_path)?;
    let index = repo.index().map_err(GitError::internal)?;
    let conflicts = index.conflicts().map_err(GitError::internal)?;
    Ok(u32::try_from(conflicts.count()).unwrap_or(u32::MAX))
}

/// List all conflict file paths from the index.
pub fn list_conflict_paths(repo_path: &str) -> Result<Vec<String>, GitError> {
    let repo = open_git2(repo_path)?;
    let index = repo.index().map_err(GitError::internal)?;
    let conflicts = index.conflicts().map_err(GitError::internal)?;

    let mut paths = Vec::new();
    for conflict in conflicts {
        let conflict = conflict.map_err(GitError::internal)?;
        // Use whichever side has a path (ours, theirs, or ancestor)
        let path = conflict
            .our
            .as_ref()
            .or(conflict.their.as_ref())
            .or(conflict.ancestor.as_ref())
            .map(|entry| String::from_utf8_lossy(&entry.path).to_string());
        if let Some(p) = path {
            paths.push(p);
        }
    }
    Ok(paths)
}

/// Resolve a conflict by checking out "ours" and staging the file.
pub fn checkout_ours(repo_path: &str, file_path: &str) -> Result<(), GitError> {
    run_git_cmd(repo_path, &["checkout", "--ours", "--", file_path])?;
    run_git_cmd(repo_path, &["add", "--", file_path])?;
    Ok(())
}

/// Resolve a conflict by checking out "theirs" and staging the file.
pub fn checkout_theirs(repo_path: &str, file_path: &str) -> Result<(), GitError> {
    run_git_cmd(repo_path, &["checkout", "--theirs", "--", file_path])?;
    run_git_cmd(repo_path, &["add", "--", file_path])?;
    Ok(())
}

/// Mark a file as resolved by staging it.
pub fn mark_resolved(repo_path: &str, file_path: &str) -> Result<(), GitError> {
    run_git_cmd(repo_path, &["add", "--", file_path])?;
    Ok(())
}

/// Extract the base/ours/theirs content for a conflicted file from the git index.
pub fn conflict_sides(repo_path: &str, file_path: &str) -> Result<ConflictSides, GitError> {
    let repo = open_git2(repo_path)?;
    let index = repo.index().map_err(GitError::internal)?;
    let conflicts = index.conflicts().map_err(GitError::internal)?;

    for conflict in conflicts {
        let conflict = conflict.map_err(GitError::internal)?;
        let entry_path = conflict
            .our
            .as_ref()
            .or(conflict.their.as_ref())
            .or(conflict.ancestor.as_ref())
            .map(|e| String::from_utf8_lossy(&e.path).to_string());

        if entry_path.as_deref() != Some(file_path) {
            continue;
        }

        let base = read_blob_content(&repo, conflict.ancestor.as_ref());
        let ours = read_blob_content(&repo, conflict.our.as_ref());
        let theirs = read_blob_content(&repo, conflict.their.as_ref());

        return Ok(ConflictSides {
            path: file_path.to_string(),
            base,
            ours,
            theirs,
        });
    }

    Err(GitError::Internal {
        message: format!("No conflict found for path: {file_path}"),
    })
}

/// Read blob content for a conflict index entry.
fn read_blob_content(repo: &git2::Repository, entry: Option<&git2::IndexEntry>) -> Option<String> {
    let entry = entry?;
    let blob = repo.find_blob(entry.id).ok()?;
    String::from_utf8(blob.content().to_vec()).ok()
}

/// Run a git command in the given repo path.
fn run_git_cmd(repo_path: &str, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .map_err(|e| GitError::Internal {
            message: format!("Failed to run git: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitError::Internal {
            message: format!("git {} failed: {stderr}", args.join(" ")),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
