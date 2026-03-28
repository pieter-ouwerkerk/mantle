use crate::error::GitError;
use crate::repo;
use crate::types::{AheadBehindResult, CommitTreeRefsInfo};

pub fn is_valid_repo(path: &str) -> bool {
    repo::open(path).is_ok()
}

pub fn rev_parse(repo_path: &str, rev: &str) -> Result<String, GitError> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();
    let object = repo
        .rev_parse_single(rev)
        .map_err(|_| GitError::RevNotFound {
            rev: rev.to_owned(),
        })?;
    Ok(object.detach().to_hex().to_string())
}

pub fn rev_list_parents(repo_path: &str, commit_hash: &str) -> Result<Vec<String>, GitError> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();
    let object = repo
        .rev_parse_single(commit_hash)
        .map_err(|_| GitError::RevNotFound {
            rev: commit_hash.to_owned(),
        })?;
    let commit = object
        .object()
        .map_err(GitError::internal)?
        .try_into_commit()
        .map_err(GitError::internal)?;
    let mut result = vec![commit.id().to_hex().to_string()];
    for parent_id in commit.parent_ids() {
        result.push(parent_id.to_hex().to_string());
    }
    Ok(result)
}

/// Get the tree hash and ref decorations for a commit.
/// The `refs` string mimics `git log --format=%D` output: comma-separated ref names
/// (e.g. "HEAD -> main, tag: v1.0, origin/main").
pub fn commit_tree_and_refs(
    repo_path: &str,
    commit_hash: &str,
) -> Result<CommitTreeRefsInfo, GitError> {
    let repo = git2::Repository::open(repo_path).map_err(GitError::internal)?;

    let obj = repo
        .revparse_single(commit_hash)
        .map_err(|_| GitError::RevNotFound {
            rev: commit_hash.to_owned(),
        })?;
    let commit = obj.peel_to_commit().map_err(GitError::internal)?;
    let tree_hash = commit.tree_id().to_string();

    // Collect all refs that point at this commit
    let commit_oid = commit.id();
    let mut decorations: Vec<String> = Vec::new();

    // Check HEAD
    if let Ok(head) = repo.head() {
        if let Some(target) = head.resolve().ok().and_then(|r| r.target()) {
            if target == commit_oid {
                if let Some(branch) = head.shorthand() {
                    if head.is_branch() {
                        decorations.push(format!("HEAD -> {branch}"));
                    } else {
                        decorations.push("HEAD".to_owned());
                    }
                }
            }
        }
    }

    // Iterate all refs
    let refs = repo.references().map_err(GitError::internal)?;
    for reference in refs.flatten() {
        let target = reference
            .resolve()
            .ok()
            .and_then(|r| r.target())
            .or_else(|| reference.target());
        if target != Some(commit_oid) {
            continue;
        }
        let name = match reference.shorthand() {
            Some(n) => n.to_owned(),
            None => continue,
        };
        // Skip HEAD (already handled above) and skip refs we already included
        if name == "HEAD" {
            continue;
        }
        // Format tags with "tag: " prefix
        if reference
            .name()
            .is_some_and(|n| n.starts_with("refs/tags/"))
        {
            decorations.push(format!("tag: {name}"));
        } else {
            // Skip branch names that are part of the HEAD decoration
            let already_in_head = decorations
                .first()
                .is_some_and(|d| d == &format!("HEAD -> {name}"));
            if !already_in_head {
                decorations.push(name);
            }
        }
    }

    Ok(CommitTreeRefsInfo {
        tree_hash,
        refs: decorations.join(", "),
    })
}

/// Get ahead/behind counts between two arbitrary refs.
pub fn ahead_behind(
    repo_path: &str,
    ref1: &str,
    ref2: &str,
) -> Result<AheadBehindResult, GitError> {
    let repo = git2::Repository::open(repo_path).map_err(GitError::internal)?;

    let obj1 = repo
        .revparse_single(ref1)
        .map_err(|_| GitError::RevNotFound {
            rev: ref1.to_owned(),
        })?;
    let obj2 = repo
        .revparse_single(ref2)
        .map_err(|_| GitError::RevNotFound {
            rev: ref2.to_owned(),
        })?;

    let (ahead, behind) = repo
        .graph_ahead_behind(obj1.id(), obj2.id())
        .map_err(GitError::internal)?;

    Ok(AheadBehindResult {
        ahead: u32::try_from(ahead).unwrap_or(u32::MAX),
        behind: u32::try_from(behind).unwrap_or(u32::MAX),
    })
}
