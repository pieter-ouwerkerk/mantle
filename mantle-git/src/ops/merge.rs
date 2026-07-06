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

// ---------------------------------------------------------------------------
// Merge execution, merge-tree dry runs, and rebase (CUT-743)
// ---------------------------------------------------------------------------

/// Resolve a revision string (branch name, remote ref, or OID) to a commit.
fn resolve_commit<'r>(
    repo: &'r git2::Repository,
    rev: &str,
) -> Result<git2::Commit<'r>, GitError> {
    repo.revparse_single(rev)
        .and_then(|obj| obj.peel_to_commit())
        .map_err(|_| GitError::RevNotFound {
            rev: rev.to_owned(),
        })
}

/// Find the merge base of two revisions (equivalent to `git merge-base r1 r2`).
pub fn merge_base(repo_path: &str, ref1: &str, ref2: &str) -> Result<String, GitError> {
    let repo = open_git2(repo_path)?;
    let c1 = resolve_commit(&repo, ref1)?;
    let c2 = resolve_commit(&repo, ref2)?;
    let base = repo
        .merge_base(c1.id(), c2.id())
        .map_err(GitError::internal)?;
    Ok(base.to_string())
}

/// Dry-run merge of `theirs` into `ours` without touching the working tree
/// (equivalent to `git merge-tree --write-tree ours theirs`).
///
/// On a clean merge the merged tree is written to the object database and its
/// OID returned. On conflicts the conflicted paths are listed instead.
pub fn merge_tree(
    repo_path: &str,
    ours: &str,
    theirs: &str,
) -> Result<crate::types::MergeTreeInfo, GitError> {
    let repo = open_git2(repo_path)?;
    let our_commit = resolve_commit(&repo, ours)?;
    let their_commit = resolve_commit(&repo, theirs)?;

    let base_oid = repo
        .merge_base(our_commit.id(), their_commit.id())
        .map_err(GitError::internal)?;
    let ancestor_tree = repo
        .find_commit(base_oid)
        .and_then(|c| c.tree())
        .map_err(GitError::internal)?;
    let our_tree = our_commit.tree().map_err(GitError::internal)?;
    let their_tree = their_commit.tree().map_err(GitError::internal)?;

    let mut index = repo
        .merge_trees(&ancestor_tree, &our_tree, &their_tree, None)
        .map_err(GitError::internal)?;

    if index.has_conflicts() {
        let mut conflicts = Vec::new();
        for conflict in index.conflicts().map_err(GitError::internal)? {
            let conflict = conflict.map_err(GitError::internal)?;
            let path = conflict
                .our
                .as_ref()
                .or(conflict.their.as_ref())
                .or(conflict.ancestor.as_ref())
                .map(|e| String::from_utf8_lossy(&e.path).to_string())
                .unwrap_or_default();
            let reason = match (&conflict.ancestor, &conflict.our, &conflict.their) {
                (Some(_), Some(_), Some(_)) => "content",
                (None, Some(_), Some(_)) => "add/add",
                (Some(_), None, Some(_)) | (Some(_), Some(_), None) => "modify/delete",
                _ => "conflict",
            };
            conflicts.push(crate::types::MergeConflictEntry {
                path,
                reason: reason.to_owned(),
            });
        }
        return Ok(crate::types::MergeTreeInfo {
            clean: false,
            tree_oid: String::new(),
            conflicts,
        });
    }

    let tree_oid = index.write_tree_to(&repo).map_err(GitError::internal)?;
    Ok(crate::types::MergeTreeInfo {
        clean: true,
        tree_oid: tree_oid.to_string(),
        conflicts: Vec::new(),
    })
}

/// Run a git command, returning the exit code instead of failing on non-zero.
fn run_git_cmd_status(
    repo_path: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<(i32, String, String), GitError> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(repo_path);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.output().map_err(|e| GitError::Internal {
        message: format!("Failed to run git: {e}"),
    })?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

fn git_failed(args: &[&str], stdout: &str, stderr: &str) -> GitError {
    GitError::Internal {
        message: format!(
            "git {} failed: {}",
            args.join(" "),
            if stderr.trim().is_empty() { stdout } else { stderr }
        ),
    }
}

/// Merge `branch` into the current branch with `--no-ff --no-edit`.
///
/// Returns `true` when the merge stopped on conflicts (merge in progress),
/// `false` when it completed cleanly. These ops intentionally shell out to the
/// git CLI (like `checkout_ours` above): merge/rebase sequencer state on disk
/// must stay byte-compatible with what a user's terminal git expects.
pub fn merge_no_ff(repo_path: &str, branch: &str) -> Result<bool, GitError> {
    let args = ["merge", branch, "--no-ff", "--no-edit"];
    let (code, stdout, stderr) = run_git_cmd_status(repo_path, &args, &[])?;
    match code {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(git_failed(&args, &stdout, &stderr)),
    }
}

/// Merge `branch` into the current branch with default fast-forward behavior
/// (equivalent to plain `git merge <branch>`).
pub fn merge_plain(repo_path: &str, branch: &str) -> Result<(), GitError> {
    run_git_cmd(repo_path, &["merge", branch])?;
    Ok(())
}

/// Abort an in-progress merge (equivalent to `git merge --abort`).
pub fn merge_abort(repo_path: &str) -> Result<(), GitError> {
    run_git_cmd(repo_path, &["merge", "--abort"])?;
    Ok(())
}

/// Continue an in-progress merge after conflict resolution.
/// Note: `git merge --continue` accepts no other flags (`--no-edit` is
/// rejected); `GIT_EDITOR=true` keeps it non-interactive instead.
pub fn merge_continue(repo_path: &str) -> Result<(), GitError> {
    let args = ["merge", "--continue"];
    let (code, stdout, stderr) = run_git_cmd_status(repo_path, &args, &[("GIT_EDITOR", "true")])?;
    if code != 0 {
        return Err(git_failed(&args, &stdout, &stderr));
    }
    Ok(())
}

/// Rebase the current branch onto `onto` (equivalent to `git rebase <onto>`).
pub fn rebase_onto(worktree_path: &str, onto: &str) -> Result<(), GitError> {
    run_git_cmd(worktree_path, &["rebase", onto])?;
    Ok(())
}

/// Abort an in-progress rebase (equivalent to `git rebase --abort`).
pub fn rebase_abort(worktree_path: &str) -> Result<(), GitError> {
    run_git_cmd(worktree_path, &["rebase", "--abort"])?;
    Ok(())
}

/// Continue an in-progress rebase after conflict resolution
/// (equivalent to `git rebase --continue`).
pub fn rebase_continue(worktree_path: &str) -> Result<(), GitError> {
    let args = ["rebase", "--continue"];
    let (code, stdout, stderr) =
        run_git_cmd_status(worktree_path, &args, &[("GIT_EDITOR", "true")])?;
    if code != 0 {
        return Err(git_failed(&args, &stdout, &stderr));
    }
    Ok(())
}
