use crate::error::Error;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn open_git2(repo_path: &str) -> Result<git2::Repository, Error> {
    git2::Repository::open(repo_path).map_err(Error::internal)
}

// ---------------------------------------------------------------------------
// Repository operations
// ---------------------------------------------------------------------------

/// Initialize a new git repository at the given path (equivalent to `git init`).
pub fn init_repo(path: &str) -> Result<(), Error> {
    git2::Repository::init(path).map_err(Error::internal)?;
    Ok(())
}

/// Resolve the repository root (equivalent to `git rev-parse --show-toplevel`).
pub fn repo_root(path: &str) -> Result<String, Error> {
    let repo = git2::Repository::discover(path).map_err(Error::internal)?;
    let workdir = repo.workdir().ok_or_else(|| Error::Internal {
        message: "bare repository has no working directory".to_owned(),
    })?;
    workdir
        .to_str()
        .map(|s| s.trim_end_matches('/').to_owned())
        .ok_or_else(|| Error::Internal {
            message: "repository path is not valid UTF-8".to_owned(),
        })
}

// ---------------------------------------------------------------------------
// Branch operations
// ---------------------------------------------------------------------------

/// Checkout a local branch (equivalent to `git checkout <branch>`).
pub fn checkout(repo_path: &str, branch: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let branch_ref = repo
        .find_branch(branch, git2::BranchType::Local)
        .map_err(Error::internal)?;
    let refname = branch_ref
        .get()
        .name()
        .ok_or_else(|| Error::Internal {
            message: "branch ref name is not valid UTF-8".to_owned(),
        })?
        .to_owned();
    repo.set_head(&refname).map_err(Error::internal)?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .map_err(Error::internal)?;
    Ok(())
}

/// Delete a local branch (equivalent to `git branch -D <name>`).
pub fn branch_delete(repo_path: &str, name: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let mut branch = repo
        .find_branch(name, git2::BranchType::Local)
        .map_err(Error::internal)?;
    branch.delete().map_err(Error::internal)?;
    Ok(())
}

/// Create a new branch at a given start point (equivalent to `git branch <name> <startPoint>`).
pub fn create_branch_at(repo_path: &str, branch: &str, start_point: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let obj = repo
        .revparse_single(start_point)
        .map_err(|_| Error::RevNotFound {
            rev: start_point.to_owned(),
        })?;
    let commit = obj.peel_to_commit().map_err(Error::internal)?;
    repo.branch(branch, &commit, false)
        .map_err(Error::internal)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Ref operations
// ---------------------------------------------------------------------------

/// Update (or create) a reference to point at a given OID (equivalent to `git update-ref`).
pub fn update_ref(repo_path: &str, refname: &str, value: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let oid = git2::Oid::from_str(value).map_err(Error::internal)?;
    repo.reference(refname, oid, true, "update-ref")
        .map_err(Error::internal)?;
    Ok(())
}

/// Delete a reference (equivalent to `git update-ref -d <ref>`).
pub fn delete_ref(repo_path: &str, refname: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let mut reference = repo.find_reference(refname).map_err(Error::internal)?;
    reference.delete().map_err(Error::internal)?;
    Ok(())
}

/// Hard reset to a revision (equivalent to `git reset --hard <rev>`).
pub fn reset_hard(repo_path: &str, rev: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let obj = repo
        .revparse_single(rev)
        .map_err(|_| Error::RevNotFound {
            rev: rev.to_owned(),
        })?;
    repo.reset(&obj, git2::ResetType::Hard, None)
        .map_err(Error::internal)?;
    Ok(())
}

/// Soft reset to a revision (equivalent to `git reset --soft <rev>`).
/// Moves HEAD but leaves both index and working tree unchanged.
pub fn reset_soft(repo_path: &str, rev: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let obj = repo
        .revparse_single(rev)
        .map_err(|_| Error::RevNotFound {
            rev: rev.to_owned(),
        })?;
    repo.reset(&obj, git2::ResetType::Soft, None)
        .map_err(Error::internal)?;
    Ok(())
}

/// Mixed reset to a revision (equivalent to `git reset --mixed <rev>`).
/// Moves HEAD and resets the index, but leaves the working tree unchanged.
pub fn reset_mixed(repo_path: &str, rev: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let obj = repo
        .revparse_single(rev)
        .map_err(|_| Error::RevNotFound {
            rev: rev.to_owned(),
        })?;
    repo.reset(&obj, git2::ResetType::Mixed, None)
        .map_err(Error::internal)?;
    Ok(())
}

/// Remove untracked files and directories (equivalent to `git clean -fd`).
pub fn clean_untracked(repo_path: &str) -> Result<(), Error> {
    let files = super::status::list_untracked_files(repo_path)?;
    let base = std::path::Path::new(repo_path);
    for relative in &files {
        let path = base.join(relative);
        if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| Error::Internal {
                message: format!("Failed to remove directory {}: {}", path.display(), e),
            })?;
        } else {
            std::fs::remove_file(&path).map_err(|e| Error::Internal {
                message: format!("Failed to remove file {}: {}", path.display(), e),
            })?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Stash operations
// ---------------------------------------------------------------------------

/// Stash working directory changes including untracked files
/// (equivalent to `git stash push -u -m <message>`).
pub fn stash_push(repo_path: &str, message: &str) -> Result<(), Error> {
    let mut repo = open_git2(repo_path)?;
    let sig = repo.signature().map_err(Error::internal)?;
    repo.stash_save(&sig, message, Some(git2::StashFlags::INCLUDE_UNTRACKED))
        .map_err(Error::internal)?;
    Ok(())
}

/// Pop the most recent stash entry (equivalent to `git stash pop`).
pub fn stash_pop(repo_path: &str) -> Result<(), Error> {
    let mut repo = open_git2(repo_path)?;
    repo.stash_pop(0, None).map_err(Error::internal)?;
    Ok(())
}

/// List all stash entries (equivalent to `git stash list`).
pub fn stash_list(repo_path: &str) -> Result<Vec<crate::types::StashEntry>, Error> {
    let mut repo = open_git2(repo_path)?;
    let mut entries = Vec::new();
    repo.stash_foreach(|index, message, oid| {
        entries.push(crate::types::StashEntry {
            index: index as u32,
            message: message.to_owned(),
            commit_hash: oid.to_string(),
        });
        true // continue iterating
    })
    .map_err(Error::internal)?;
    Ok(entries)
}

/// Apply a stash entry without removing it (equivalent to `git stash apply stash@{index}`).
pub fn stash_apply(repo_path: &str, index: u32) -> Result<(), Error> {
    let mut repo = open_git2(repo_path)?;
    repo.stash_apply(index as usize, None)
        .map_err(Error::internal)?;
    Ok(())
}

/// Drop a stash entry (equivalent to `git stash drop stash@{index}`).
pub fn stash_drop(repo_path: &str, index: u32) -> Result<(), Error> {
    let mut repo = open_git2(repo_path)?;
    repo.stash_drop(index as usize)
        .map_err(Error::internal)?;
    Ok(())
}

/// Show the diff for a stash entry (equivalent to `git stash show -p stash@{index}`).
pub fn stash_show(repo_path: &str, index: u32) -> Result<String, Error> {
    let mut repo = open_git2(repo_path)?;

    // Find the stash OID at the given index
    let mut stash_oid: Option<git2::Oid> = None;
    repo.stash_foreach(|i, _message, oid| {
        if i as u32 == index {
            stash_oid = Some(*oid);
            false // stop iterating
        } else {
            true
        }
    })
    .map_err(Error::internal)?;

    let oid = stash_oid.ok_or_else(|| Error::Internal {
        message: format!("stash@{{{index}}} not found"),
    })?;

    let stash_commit = repo.find_commit(oid).map_err(Error::internal)?;
    let stash_tree = stash_commit.tree().map_err(Error::internal)?;

    let parent = stash_commit.parent(0).map_err(Error::internal)?;
    let parent_tree = parent.tree().map_err(Error::internal)?;

    let diff = repo
        .diff_tree_to_tree(Some(&parent_tree), Some(&stash_tree), None)
        .map_err(Error::internal)?;

    let mut output = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        match line.origin() {
            '+' | '-' | ' ' => output.push(line.origin()),
            _ => {}
        }
        if let Ok(content) = std::str::from_utf8(line.content()) {
            output.push_str(content);
        }
        true
    })
    .map_err(Error::internal)?;

    Ok(output)
}

// ---------------------------------------------------------------------------
// Staging operations
// ---------------------------------------------------------------------------

/// Stage all changes including untracked files (equivalent to `git add -A`).
pub fn add_all(repo_path: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let mut index = repo.index().map_err(Error::internal)?;
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .map_err(Error::internal)?;
    // Also remove deleted files from the index
    index
        .update_all(["*"].iter(), None)
        .map_err(Error::internal)?;
    index.write().map_err(Error::internal)?;
    Ok(())
}

/// Stage specific files (equivalent to `git add -- <paths>`).
pub fn add_files(repo_path: &str, paths: &[String]) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let mut index = repo.index().map_err(Error::internal)?;
    for path in paths {
        let full_path = std::path::Path::new(repo_path).join(path);
        if full_path.exists() {
            index
                .add_path(std::path::Path::new(path))
                .map_err(Error::internal)?;
        } else {
            // File was deleted — remove from index
            index
                .remove_path(std::path::Path::new(path))
                .map_err(Error::internal)?;
        }
    }
    index.write().map_err(Error::internal)?;
    Ok(())
}

/// Unstage all staged changes (equivalent to `git reset`).
pub fn reset_staging(repo_path: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    match repo.head() {
        Ok(head) => {
            let obj = head
                .peel(git2::ObjectType::Commit)
                .map_err(Error::internal)?;
            repo.reset(&obj, git2::ResetType::Mixed, None)
                .map_err(Error::internal)?;
        }
        Err(_) => {
            // Unborn branch (no commits yet) — clear the index entirely.
            let mut index = repo.index().map_err(Error::internal)?;
            index.clear().map_err(Error::internal)?;
            index.write().map_err(Error::internal)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Commit
// ---------------------------------------------------------------------------

/// Restore a file to its HEAD version (equivalent to `git checkout HEAD -- <path>`).
pub fn restore_file(repo_path: &str, file_path: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let path = std::path::Path::new(file_path);

    // Check if the file exists in HEAD
    let has_head = repo.head().is_ok();
    if has_head {
        let head = repo.head().map_err(Error::internal)?;
        let tree = head
            .peel_to_tree()
            .map_err(Error::internal)?;

        match tree.get_path(path) {
            Ok(_) => {
                // File exists in HEAD — checkout from HEAD
                let mut cb = git2::build::CheckoutBuilder::new();
                cb.force();
                cb.path(file_path);
                repo.checkout_head(Some(&mut cb))
                    .map_err(Error::internal)?;
            }
            Err(_) => {
                // File is untracked (not in HEAD) — remove from disk
                let full_path = std::path::Path::new(repo_path).join(file_path);
                if full_path.exists() {
                    std::fs::remove_file(&full_path).map_err(|e| Error::Internal {
                        message: format!("failed to remove untracked file: {}", e),
                    })?;
                }
            }
        }
    } else {
        // No HEAD (empty repo) — remove from disk
        let full_path = std::path::Path::new(repo_path).join(file_path);
        if full_path.exists() {
            std::fs::remove_file(&full_path).map_err(|e| Error::Internal {
                message: format!("failed to remove file: {}", e),
            })?;
        }
    }

    // Also unstage the file if it was staged
    let mut index = repo.index().map_err(Error::internal)?;
    if has_head {
        let head = repo.head().map_err(Error::internal)?;
        let obj = head.peel(git2::ObjectType::Commit).map_err(Error::internal)?;
        repo.reset_default(Some(&obj), [file_path])
            .map_err(Error::internal)?;
    } else {
        let _ = index.remove_path(path);
        index.write().map_err(Error::internal)?;
    }

    Ok(())
}

/// Amend the HEAD commit with the current index and a new message.
pub fn amend_commit(repo_path: &str, message: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let head = repo.head().map_err(Error::internal)?;
    let head_commit = head.peel_to_commit().map_err(Error::internal)?;
    let sig = repo.signature().map_err(Error::internal)?;
    let mut index = repo.index().map_err(Error::internal)?;
    let tree_oid = index.write_tree().map_err(Error::internal)?;
    let tree = repo.find_tree(tree_oid).map_err(Error::internal)?;

    head_commit
        .amend(
            Some("HEAD"),
            None,          // keep original author
            Some(&sig),    // update committer
            None,          // keep encoding
            Some(message),
            Some(&tree),
        )
        .map_err(Error::internal)?;
    Ok(())
}

/// Create a commit from the current index (equivalent to `git commit -m <message>`).
pub fn commit(repo_path: &str, message: &str) -> Result<(), Error> {
    let repo = open_git2(repo_path)?;
    let sig = repo.signature().map_err(Error::internal)?;
    let mut index = repo.index().map_err(Error::internal)?;
    let tree_oid = index.write_tree().map_err(Error::internal)?;
    let tree = repo.find_tree(tree_oid).map_err(Error::internal)?;

    let parents = match repo.head() {
        Ok(head) => {
            let parent_commit = head.peel_to_commit().map_err(Error::internal)?;
            vec![parent_commit]
        }
        Err(_) => vec![], // Initial commit — no parents
    };

    let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .map_err(Error::internal)?;

    Ok(())
}
