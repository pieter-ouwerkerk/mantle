use std::collections::{HashMap, HashSet, VecDeque};

use git2::{Oid, Repository, Signature, Time};

use crate::error::Error;
use crate::types::{CommitMetadataInfo, RewriteResult};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn open_git2(repo_path: &str) -> Result<Repository, Error> {
    Repository::open(repo_path).map_err(Error::internal)
}

/// Collect all commits between `target_oid` and `head_oid` that need rewriting.
///
/// When the target is a shared ancestor reachable through multiple parents of a
/// merge, ALL paths through the merge must be rewritten. This function finds
/// every descendant of `target_oid` in the ancestry of `head_oid` and returns
/// them in topological order (target first, HEAD last).
fn collect_commit_chain(
    repo: &Repository,
    target_oid: Oid,
    head_oid: Oid,
) -> Result<Vec<Oid>, Error> {
    if head_oid == target_oid {
        return Ok(vec![target_oid]);
    }

    // Step 1: BFS backwards from HEAD, building forward (parent → child) edges.
    // Stop at target — don't walk past it.
    let mut children: HashMap<Oid, Vec<Oid>> = HashMap::new();
    let mut parent_ids_of: HashMap<Oid, Vec<Oid>> = HashMap::new();
    let mut visited: HashSet<Oid> = HashSet::new();
    let mut queue: VecDeque<Oid> = VecDeque::new();
    queue.push_back(head_oid);
    visited.insert(head_oid);

    while let Some(oid) = queue.pop_front() {
        let commit = repo.find_commit(oid).map_err(Error::internal)?;
        let pids: Vec<Oid> = commit.parent_ids().collect();
        parent_ids_of.insert(oid, pids.clone());

        if oid == target_oid {
            continue; // don't walk past target
        }

        for &pid in &pids {
            children.entry(pid).or_default().push(oid);
            if visited.insert(pid) {
                queue.push_back(pid);
            }
        }
    }

    if !visited.contains(&target_oid) {
        return Err(Error::CommitNotInChain {
            hash: target_oid.to_string(),
        });
    }

    // Step 2: BFS forward from target — find all descendants within HEAD's ancestry.
    let mut rewrite_set: HashSet<Oid> = HashSet::new();
    let mut fwd_queue: VecDeque<Oid> = VecDeque::new();
    fwd_queue.push_back(target_oid);
    rewrite_set.insert(target_oid);

    while let Some(oid) = fwd_queue.pop_front() {
        if let Some(kids) = children.get(&oid) {
            for &kid in kids {
                if rewrite_set.insert(kid) {
                    fwd_queue.push_back(kid);
                }
            }
        }
    }

    // Step 3: Kahn's topological sort over the rewrite set.
    let mut in_degree: HashMap<Oid, usize> = HashMap::new();
    for &oid in &rewrite_set {
        let deg = parent_ids_of
            .get(&oid)
            .map(|ps| ps.iter().filter(|p| rewrite_set.contains(p)).count())
            .unwrap_or(0);
        in_degree.insert(oid, deg);
    }

    let mut topo_queue: VecDeque<Oid> = VecDeque::new();
    for (&oid, &deg) in &in_degree {
        if deg == 0 {
            topo_queue.push_back(oid);
        }
    }

    let mut result = Vec::with_capacity(rewrite_set.len());
    while let Some(oid) = topo_queue.pop_front() {
        result.push(oid);
        if let Some(kids) = children.get(&oid) {
            for &kid in kids {
                if let Some(deg) = in_degree.get_mut(&kid) {
                    *deg -= 1;
                    if *deg == 0 {
                        topo_queue.push_back(kid);
                    }
                }
            }
        }
    }

    Ok(result) // target first, HEAD last
}

/// Cherry-pick a commit onto a new parent, preserving original signatures exactly.
/// Returns the OID of the newly created commit.
fn cherry_pick_onto(
    repo: &Repository,
    commit: &git2::Commit<'_>,
    new_parent: &git2::Commit<'_>,
) -> Result<Oid, Error> {
    let index = repo
        .cherrypick_commit(commit, new_parent, 0, None)
        .map_err(|e| {
            if e.code() == git2::ErrorCode::Conflict {
                Error::CherryPickConflict {
                    hash: commit.id().to_string(),
                    details: e.message().to_owned(),
                }
            } else {
                Error::internal(e)
            }
        })?;

    if index.has_conflicts() {
        return Err(Error::CherryPickConflict {
            hash: commit.id().to_string(),
            details: "merge produced conflicts".to_owned(),
        });
    }

    // Write the merged index as a tree
    let mut index = index;
    let tree_oid = index.write_tree_to(repo).map_err(Error::internal)?;

    // Detect empty cherry-pick (changes already applied)
    let parent_tree_oid = new_parent.tree().map_err(Error::internal)?.id();
    if tree_oid == parent_tree_oid {
        return Err(Error::CherryPickEmpty {
            hash: commit.id().to_string(),
        });
    }

    let tree = repo.find_tree(tree_oid).map_err(Error::internal)?;

    // Create commit preserving original author and committer exactly
    let new_oid = repo
        .commit(
            None,
            &commit.author(),
            &commit.committer(),
            commit.message().unwrap_or(""),
            &tree,
            &[new_parent],
        )
        .map_err(Error::internal)?;

    Ok(new_oid)
}

/// Cherry-pick a commit that is a root (no parents), preserving signatures.
fn cherry_pick_root(
    repo: &Repository,
    commit: &git2::Commit<'_>,
    new_parent: &git2::Commit<'_>,
) -> Result<Oid, Error> {
    // For a root commit being replayed onto a new parent, we use cherrypick_commit
    // which handles diffing against an empty tree
    cherry_pick_onto(repo, commit, new_parent)
}

/// Reparent a commit: create a new commit identical to the original but with
/// the parent at `replace_parent_index` replaced by `new_parent`. The tree
/// (which captures the merge resolution for merges) and all other parents are
/// preserved.
fn reparent_commit(
    repo: &Repository,
    commit: &git2::Commit<'_>,
    replace_parent_index: usize,
    new_parent: &git2::Commit<'_>,
) -> Result<Oid, Error> {
    let tree = commit.tree().map_err(Error::internal)?;
    let mut parents: Vec<git2::Commit<'_>> = Vec::with_capacity(commit.parent_count() as usize);
    for i in 0..commit.parent_count() {
        if i as usize == replace_parent_index {
            parents.push(repo.find_commit(new_parent.id()).map_err(Error::internal)?);
        } else {
            parents.push(commit.parent(i).map_err(Error::internal)?);
        }
    }
    let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

    let new_oid = repo
        .commit(
            None,
            &commit.author(),
            &commit.committer(),
            commit.message().unwrap_or(""),
            &tree,
            &parent_refs,
        )
        .map_err(Error::internal)?;

    Ok(new_oid)
}

fn preflight_checks(
    repo: &Repository,
    auto_stash: bool,
) -> Result<bool, Error> {
    // Check for detached HEAD
    if repo.head_detached().unwrap_or(false) {
        return Err(Error::DetachedHead);
    }

    // Check for operation in progress
    // Use repo.path() which returns the correct git dir for worktrees
    // (worktrees have a .git file pointing to .git/worktrees/<name>/)
    let git_dir = repo.path();
    let op_files = [
        "rebase-merge",
        "rebase-apply",
        "CHERRY_PICK_HEAD",
        "MERGE_HEAD",
        "REVERT_HEAD",
    ];
    for f in &op_files {
        if git_dir.join(f).exists() {
            return Err(Error::OperationInProgress);
        }
    }

    // Check dirty working tree
    let statuses = repo
        .statuses(Some(
            git2::StatusOptions::new()
                .include_untracked(true)
                .recurse_untracked_dirs(true),
        ))
        .map_err(Error::internal)?;

    let is_dirty = statuses
        .iter()
        .any(|s| !s.status().is_empty() && s.status() != git2::Status::IGNORED);

    if is_dirty && !auto_stash {
        return Err(Error::WorkingTreeDirty);
    }

    Ok(is_dirty)
}

fn stash_if_needed(repo: &mut Repository, is_dirty: bool) -> Result<bool, Error> {
    if !is_dirty {
        return Ok(false);
    }
    let sig = repo.signature().map_err(Error::internal)?;
    let now = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let message = format!("reviser: auto-stash {now}");
    repo.stash_save(&sig, &message, Some(git2::StashFlags::INCLUDE_UNTRACKED))
        .map_err(Error::internal)?;
    Ok(true)
}

fn pop_stash(repo: &mut Repository) -> Result<(), Error> {
    repo.stash_pop(0, None)
        .map_err(|e| Error::StashPopFailed {
            message: e.message().to_owned(),
        })
}

fn create_backup_ref(repo: &Repository, branch: &str) -> Result<String, Error> {
    let head_oid = repo
        .head()
        .map_err(Error::internal)?
        .target()
        .ok_or_else(|| Error::Internal {
            message: "HEAD has no target".to_owned(),
        })?;
    let now = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let refname = format!("refs/reviser/backups/{branch}/{now}");
    repo.reference(&refname, head_oid, true, "reviser backup")
        .map_err(Error::internal)?;
    Ok(refname)
}

/// Update the branch ref + reset working tree to new HEAD.
fn finalize(repo: &Repository, branch_refname: &str, new_tip: Oid) -> Result<(), Error> {
    // Update branch ref
    repo.reference(branch_refname, new_tip, true, "reviser rewrite")
        .map_err(Error::internal)?;
    // Reset working tree to new HEAD
    let obj = repo
        .find_object(new_tip, None)
        .map_err(Error::internal)?;
    repo.reset(&obj, git2::ResetType::Hard, None)
        .map_err(Error::internal)?;
    Ok(())
}

fn get_branch_refname(repo: &Repository) -> Result<String, Error> {
    let head = repo.head().map_err(Error::internal)?;
    if !head.is_branch() {
        return Err(Error::DetachedHead);
    }
    head.name()
        .map(|s| s.to_owned())
        .ok_or_else(|| Error::Internal {
            message: "branch ref name is not UTF-8".to_owned(),
        })
}

fn get_branch_short_name(refname: &str) -> &str {
    refname.strip_prefix("refs/heads/").unwrap_or(refname)
}

fn format_git2_time(time: &Time) -> String {
    let secs = time.seconds();
    let offset_mins = time.offset_minutes();
    let sign = if offset_mins >= 0 { '+' } else { '-' };
    let abs_offset = offset_mins.unsigned_abs();
    let hours = abs_offset / 60;
    let mins = abs_offset % 60;

    // Format as ISO 8601
    if let Some(dt) = chrono::DateTime::from_timestamp(secs, 0) {
        let offset = chrono::FixedOffset::east_opt(offset_mins as i32 * 60)
            .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap());
        let dt_with_tz = dt.with_timezone(&offset);
        dt_with_tz.to_rfc3339()
    } else {
        format!("{secs} {sign}{hours:02}{mins:02}")
    }
}

fn parse_iso_date(iso: &str) -> Result<Time, Error> {
    let dt = chrono::DateTime::parse_from_rfc3339(iso).map_err(|e| Error::Internal {
        message: format!("invalid ISO date '{iso}': {e}"),
    })?;
    Ok(Time::new(
        dt.timestamp(),
        dt.timezone().local_minus_utc() / 60,
    ))
}

// ---------------------------------------------------------------------------
// Replay engine
// ---------------------------------------------------------------------------

/// Replay commits from `rewrite_set[1..]` onto a new base, resolving all
/// rewritten parents via an OID map. This correctly handles DAG topology where
/// a commit is reachable through multiple parents of a merge.
fn replay_chain(
    repo: &Repository,
    rewrite_set: &[Oid],
    new_base_oid: Oid,
) -> Result<(Oid, u32), Error> {
    let mut oid_map: HashMap<Oid, Oid> = HashMap::new();
    oid_map.insert(rewrite_set[0], new_base_oid);
    let mut count = 0u32;

    for &oid in &rewrite_set[1..] {
        let commit = repo.find_commit(oid).map_err(Error::internal)?;

        // Resolve each parent through the oid_map
        let new_parents: Vec<git2::Commit<'_>> = commit
            .parent_ids()
            .map(|pid| {
                let resolved = oid_map.get(&pid).copied().unwrap_or(pid);
                repo.find_commit(resolved).map_err(Error::internal)
            })
            .collect::<Result<_, _>>()?;
        let parent_refs: Vec<&git2::Commit<'_>> = new_parents.iter().collect();

        let new_oid = if commit.parent_count() > 1 {
            // Merge: preserve tree, reparent with all resolved parents
            let tree = commit.tree().map_err(Error::internal)?;
            repo.commit(
                None,
                &commit.author(),
                &commit.committer(),
                commit.message().unwrap_or(""),
                &tree,
                &parent_refs,
            )
            .map_err(Error::internal)?
        } else if commit.parent_count() == 0 {
            cherry_pick_root(repo, &commit, &new_parents[0])?
        } else {
            cherry_pick_onto(repo, &commit, &new_parents[0])?
        };

        oid_map.insert(oid, new_oid);
        count += 1;
    }

    let new_head = oid_map[rewrite_set.last().unwrap()];
    Ok((new_head, count))
}

// ---------------------------------------------------------------------------
// Public API — read-only queries
// ---------------------------------------------------------------------------

pub fn commit_metadata(repo_path: &str, commit_hash: &str) -> Result<CommitMetadataInfo, Error> {
    let repo = open_git2(repo_path)?;
    let oid = repo
        .revparse_single(commit_hash)
        .map_err(|_| Error::RevNotFound {
            rev: commit_hash.to_owned(),
        })?
        .id();
    let commit = repo.find_commit(oid).map_err(Error::internal)?;

    let author = commit.author();
    let committer = commit.committer();

    let info = CommitMetadataInfo {
        author_name: author.name().unwrap_or("").to_owned(),
        author_email: author.email().unwrap_or("").to_owned(),
        committer_name: committer.name().unwrap_or("").to_owned(),
        committer_email: committer.email().unwrap_or("").to_owned(),
        author_date: format_git2_time(&author.when()),
        committer_date: format_git2_time(&committer.when()),
    };

    Ok(info)
}

pub fn prune_backup_refs(repo_path: &str, retention_days: u32) -> Result<u32, Error> {
    if retention_days == 0 {
        return Ok(0);
    }

    let repo = open_git2(repo_path)?;
    let cutoff_secs = chrono::Utc::now().timestamp() - (retention_days as i64 * 86400);
    let mut pruned = 0u32;

    let refs: Vec<String> = repo
        .references_glob("refs/reviser/backups/*")
        .map_err(Error::internal)?
        .filter_map(|r| r.ok())
        .filter_map(|r| r.name().map(|n| n.to_owned()))
        .collect();

    for refname in refs {
        if let Ok(reference) = repo.find_reference(&refname) {
            if let Some(oid) = reference.target() {
                if let Ok(commit) = repo.find_commit(oid) {
                    if commit.committer().when().seconds() < cutoff_secs {
                        if let Ok(mut r) = repo.find_reference(&refname) {
                            if r.delete().is_ok() {
                                pruned += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(pruned)
}

// ---------------------------------------------------------------------------
// Public API — commit rewrite operations
// ---------------------------------------------------------------------------

pub fn rewrite_commit_author(
    repo_path: &str,
    commit_hash: &str,
    new_name: &str,
    new_email: &str,
    auto_stash: bool,
) -> Result<RewriteResult, Error> {
    let mut repo = open_git2(repo_path)?;
    let branch_refname = get_branch_refname(&repo)?;
    let branch_short = get_branch_short_name(&branch_refname).to_owned();

    let target_oid = repo
        .revparse_single(commit_hash)
        .map_err(|_| Error::RevNotFound {
            rev: commit_hash.to_owned(),
        })?
        .id();
    let head_oid = repo.head().map_err(Error::internal)?.target().unwrap();

    let is_dirty = preflight_checks(&repo, auto_stash)?;
    let stashed = stash_if_needed(&mut repo, is_dirty)?;
    let backup_ref = create_backup_ref(&repo, &branch_short)?;

    let result = (|| -> Result<RewriteResult, Error> {
        let chain = collect_commit_chain(&repo, target_oid, head_oid)?;
        let original = repo.find_commit(target_oid).map_err(Error::internal)?;

        // Create new target commit with modified author
        let new_author = Signature::new(new_name, new_email, &original.author().when())
            .map_err(Error::internal)?;
        // Also update committer name/email but keep committer timestamp
        let new_committer = Signature::new(new_name, new_email, &original.committer().when())
            .map_err(Error::internal)?;

        let tree = original.tree().map_err(Error::internal)?;
        let parents: Vec<git2::Commit<'_>> = original
            .parent_ids()
            .map(|id| repo.find_commit(id).map_err(Error::internal))
            .collect::<Result<_, _>>()?;
        let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

        let new_target_oid = repo
            .commit(
                None,
                &new_author,
                &new_committer,
                original.message().unwrap_or(""),
                &tree,
                &parent_refs,
            )
            .map_err(Error::internal)?;

        let (new_tip, replayed) = replay_chain(&repo, &chain, new_target_oid)?;
        finalize(&repo, &branch_refname, new_tip)?;

        Ok(RewriteResult {
            new_head: new_tip.to_string(),
            backup_ref: backup_ref.clone(),
            rewritten_count: replayed + 1,
        })
    })();

    if result.is_err() && stashed {
        let _ = pop_stash(&mut repo);
    }
    if let Ok(_) = &result {
        if stashed {
            pop_stash(&mut repo)?;
        }
    }

    result
}

pub fn rewrite_commit_date(
    repo_path: &str,
    commit_hash: &str,
    new_date_iso: &str,
    auto_stash: bool,
) -> Result<RewriteResult, Error> {
    let mut repo = open_git2(repo_path)?;
    let branch_refname = get_branch_refname(&repo)?;
    let branch_short = get_branch_short_name(&branch_refname).to_owned();

    let target_oid = repo
        .revparse_single(commit_hash)
        .map_err(|_| Error::RevNotFound {
            rev: commit_hash.to_owned(),
        })?
        .id();
    let head_oid = repo.head().map_err(Error::internal)?.target().unwrap();

    let is_dirty = preflight_checks(&repo, auto_stash)?;
    let stashed = stash_if_needed(&mut repo, is_dirty)?;
    let backup_ref = create_backup_ref(&repo, &branch_short)?;

    let result = (|| -> Result<RewriteResult, Error> {
        let chain = collect_commit_chain(&repo, target_oid, head_oid)?;
        let original = repo.find_commit(target_oid).map_err(Error::internal)?;
        let new_time = parse_iso_date(new_date_iso)?;

        let new_author = Signature::new(
            original.author().name().unwrap_or(""),
            original.author().email().unwrap_or(""),
            &new_time,
        )
        .map_err(Error::internal)?;
        let new_committer = Signature::new(
            original.committer().name().unwrap_or(""),
            original.committer().email().unwrap_or(""),
            &new_time,
        )
        .map_err(Error::internal)?;

        let tree = original.tree().map_err(Error::internal)?;
        let parents: Vec<git2::Commit<'_>> = original
            .parent_ids()
            .map(|id| repo.find_commit(id).map_err(Error::internal))
            .collect::<Result<_, _>>()?;
        let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

        let new_target_oid = repo
            .commit(
                None,
                &new_author,
                &new_committer,
                original.message().unwrap_or(""),
                &tree,
                &parent_refs,
            )
            .map_err(Error::internal)?;

        let (new_tip, replayed) = replay_chain(&repo, &chain, new_target_oid)?;
        finalize(&repo, &branch_refname, new_tip)?;

        Ok(RewriteResult {
            new_head: new_tip.to_string(),
            backup_ref: backup_ref.clone(),
            rewritten_count: replayed + 1,
        })
    })();

    if result.is_err() && stashed {
        let _ = pop_stash(&mut repo);
    }
    if let Ok(_) = &result {
        if stashed {
            pop_stash(&mut repo)?;
        }
    }

    result
}

pub fn rewrite_commit_message(
    repo_path: &str,
    commit_hash: &str,
    new_message: &str,
    auto_stash: bool,
) -> Result<RewriteResult, Error> {
    let mut repo = open_git2(repo_path)?;
    let branch_refname = get_branch_refname(&repo)?;
    let branch_short = get_branch_short_name(&branch_refname).to_owned();

    let target_oid = repo
        .revparse_single(commit_hash)
        .map_err(|_| Error::RevNotFound {
            rev: commit_hash.to_owned(),
        })?
        .id();
    let head_oid = repo.head().map_err(Error::internal)?.target().unwrap();

    let is_dirty = preflight_checks(&repo, auto_stash)?;
    let stashed = stash_if_needed(&mut repo, is_dirty)?;
    let backup_ref = create_backup_ref(&repo, &branch_short)?;

    let result = (|| -> Result<RewriteResult, Error> {
        let chain = collect_commit_chain(&repo, target_oid, head_oid)?;
        let original = repo.find_commit(target_oid).map_err(Error::internal)?;

        let tree = original.tree().map_err(Error::internal)?;
        let parents: Vec<git2::Commit<'_>> = original
            .parent_ids()
            .map(|id| repo.find_commit(id).map_err(Error::internal))
            .collect::<Result<_, _>>()?;
        let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

        let new_target_oid = repo
            .commit(
                None,
                &original.author(),
                &original.committer(),
                new_message,
                &tree,
                &parent_refs,
            )
            .map_err(Error::internal)?;

        let (new_tip, replayed) = replay_chain(&repo, &chain, new_target_oid)?;
        finalize(&repo, &branch_refname, new_tip)?;

        Ok(RewriteResult {
            new_head: new_tip.to_string(),
            backup_ref: backup_ref.clone(),
            rewritten_count: replayed + 1,
        })
    })();

    if result.is_err() && stashed {
        let _ = pop_stash(&mut repo);
    }
    if let Ok(_) = &result {
        if stashed {
            pop_stash(&mut repo)?;
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Public API — cherry-pick
// ---------------------------------------------------------------------------

pub fn cherry_pick(
    repo_path: &str,
    commit_hash: &str,
    auto_stash: bool,
) -> Result<String, Error> {
    let mut repo = open_git2(repo_path)?;
    let branch_refname = get_branch_refname(&repo)?;

    let source_oid = repo
        .revparse_single(commit_hash)
        .map_err(|_| Error::RevNotFound {
            rev: commit_hash.to_owned(),
        })?
        .id();
    let head_oid = repo.head().map_err(Error::internal)?.target().unwrap();

    let is_dirty = preflight_checks(&repo, auto_stash)?;
    let stashed = stash_if_needed(&mut repo, is_dirty)?;

    let result = (|| -> Result<String, Error> {
        let source_commit = repo.find_commit(source_oid).map_err(Error::internal)?;
        let head_commit = repo.find_commit(head_oid).map_err(Error::internal)?;
        let new_oid = cherry_pick_onto(&repo, &source_commit, &head_commit)?;

        finalize(&repo, &branch_refname, new_oid)?;
        Ok(new_oid.to_string())
    })();

    if result.is_err() && stashed {
        let _ = pop_stash(&mut repo);
    }
    if result.is_ok() && stashed {
        pop_stash(&mut repo)?;
    }

    result
}

/// Cherry-pick a commit onto a specific target branch (by name).
///
/// When the target branch IS the current HEAD branch, this behaves identically
/// to `cherry_pick` (including working tree reset). When the target is a
/// different branch, only the branch ref is updated — the working tree is
/// untouched.
pub fn cherry_pick_to_branch(
    repo_path: &str,
    commit_hash: &str,
    target_branch: &str,
    auto_stash: bool,
) -> Result<String, Error> {
    let target_refname = format!("refs/heads/{}", target_branch);

    let mut repo = open_git2(repo_path)?;

    // Resolve the source commit
    let source_oid = repo
        .revparse_single(commit_hash)
        .map_err(|_| Error::RevNotFound {
            rev: commit_hash.to_owned(),
        })?
        .id();

    // Resolve the target branch tip
    let target_ref = repo
        .find_reference(&target_refname)
        .map_err(|_| Error::RevNotFound {
            rev: target_branch.to_owned(),
        })?;
    let target_oid = target_ref.target().ok_or_else(|| Error::Internal {
        message: format!("branch {} has no target", target_branch),
    })?;

    // Check if target is the HEAD branch — if so, use the full cherry_pick flow
    // (with stash/unstash and working tree reset).
    let is_head_branch = repo
        .head()
        .ok()
        .and_then(|h| h.name().map(|n| n == target_refname))
        .unwrap_or(false);

    if is_head_branch {
        return cherry_pick(repo_path, commit_hash, auto_stash);
    }

    // For non-HEAD branches: no stash needed (working tree unaffected).
    let source_commit = repo.find_commit(source_oid).map_err(Error::internal)?;
    let target_commit = repo.find_commit(target_oid).map_err(Error::internal)?;
    let new_oid = cherry_pick_onto(&repo, &source_commit, &target_commit)?;

    // Update only the branch ref — do NOT reset the working tree.
    repo.reference(&target_refname, new_oid, true, "cuttlefish cherry-pick")
        .map_err(Error::internal)?;

    Ok(new_oid.to_string())
}

// ---------------------------------------------------------------------------
// Public API — fixup/squash
// ---------------------------------------------------------------------------

pub fn fixup_commits(
    repo_path: &str,
    commit_hashes: &[String],
    auto_stash: bool,
) -> Result<RewriteResult, Error> {
    if commit_hashes.is_empty() {
        return Ok(RewriteResult {
            new_head: String::new(),
            backup_ref: String::new(),
            rewritten_count: 0,
        });
    }

    let mut repo = open_git2(repo_path)?;
    let branch_refname = get_branch_refname(&repo)?;
    let branch_short = get_branch_short_name(&branch_refname).to_owned();
    let head_oid = repo.head().map_err(Error::internal)?.target().unwrap();

    // Resolve all hashes to OIDs
    let mut target_oids: Vec<Oid> = Vec::new();
    for hash in commit_hashes {
        let oid = repo
            .revparse_single(hash)
            .map_err(|_| Error::RevNotFound { rev: hash.clone() })?
            .id();
        target_oids.push(oid);
    }

    let is_dirty = preflight_checks(&repo, auto_stash)?;
    let stashed = stash_if_needed(&mut repo, is_dirty)?;
    let backup_ref = create_backup_ref(&repo, &branch_short)?;

    let result = (|| -> Result<RewriteResult, Error> {
        // Walk from HEAD to find the earliest selected commit
        let mut all_chain = Vec::new();
        let mut current = head_oid;
        loop {
            all_chain.push(current);
            let commit = repo.find_commit(current).map_err(Error::internal)?;
            let parents: Vec<_> = commit.parent_ids().collect();
            if parents.is_empty() {
                break;
            }
            // Follow first parent (works for both regular and merge commits)
            current = parents[0];
        }
        all_chain.reverse(); // oldest first

        let target_set: std::collections::HashSet<Oid> = target_oids.iter().copied().collect();

        // Find the earliest selected commit in the chain
        let earliest_idx = all_chain
            .iter()
            .position(|oid| target_set.contains(oid))
            .ok_or_else(|| Error::CommitNotInChain {
                hash: commit_hashes[0].clone(),
            })?;

        let earliest_oid = all_chain[earliest_idx];
        let earliest_commit = repo.find_commit(earliest_oid).map_err(Error::internal)?;

        // Merge trees: start from earliest's tree, then for each fixup commit,
        // apply its diff (parent_tree → commit_tree) onto the accumulated tree.
        let mut merged_tree_oid = earliest_commit.tree_id();

        for &oid in &target_oids {
            if oid == earliest_oid {
                continue;
            }
            let fixup_commit = repo.find_commit(oid).map_err(Error::internal)?;
            let fixup_tree = fixup_commit.tree().map_err(Error::internal)?;
            let ancestor_tree = if fixup_commit.parent_count() > 0 {
                fixup_commit
                    .parent(0)
                    .map_err(Error::internal)?
                    .tree()
                    .map_err(Error::internal)?
            } else {
                // Root commit — use empty tree as ancestor
                repo.find_tree(
                    repo.treebuilder(None)
                        .map_err(Error::internal)?
                        .write()
                        .map_err(Error::internal)?,
                )
                .map_err(Error::internal)?
            };

            let our_tree = repo
                .find_tree(merged_tree_oid)
                .map_err(Error::internal)?;
            let mut index = repo
                .merge_trees(&ancestor_tree, &our_tree, &fixup_tree, None)
                .map_err(|e| {
                    if e.code() == git2::ErrorCode::Conflict {
                        Error::CherryPickConflict {
                            hash: oid.to_string(),
                            details: e.message().to_owned(),
                        }
                    } else {
                        Error::internal(e)
                    }
                })?;
            if index.has_conflicts() {
                return Err(Error::CherryPickConflict {
                    hash: oid.to_string(),
                    details: "fixup produced conflicts".to_owned(),
                });
            }
            merged_tree_oid = index.write_tree_to(&repo).map_err(Error::internal)?;
        }

        // Create the squashed commit with the earliest commit's metadata
        let merged_tree = repo
            .find_tree(merged_tree_oid)
            .map_err(Error::internal)?;
        let parents: Vec<git2::Commit<'_>> = earliest_commit
            .parent_ids()
            .map(|id| repo.find_commit(id).map_err(Error::internal))
            .collect::<Result<_, _>>()?;
        let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

        let squashed_oid = repo
            .commit(
                None,
                &earliest_commit.author(),
                &earliest_commit.committer(),
                earliest_commit.message().unwrap_or(""),
                &merged_tree,
                &parent_refs,
            )
            .map_err(Error::internal)?;

        // Replay non-selected commits after earliest
        let mut current_parent_oid = squashed_oid;
        let mut rewritten = 1u32;

        for &oid in &all_chain[(earliest_idx + 1)..] {
            if target_set.contains(&oid) {
                // Skip fixup commits — their changes are already merged
                continue;
            }
            let commit = repo.find_commit(oid).map_err(Error::internal)?;
            let new_parent = repo
                .find_commit(current_parent_oid)
                .map_err(Error::internal)?;
            current_parent_oid = if commit.parent_count() > 1 {
                reparent_commit(&repo, &commit, 0, &new_parent)?
            } else {
                cherry_pick_onto(&repo, &commit, &new_parent)?
            };
            rewritten += 1;
        }

        finalize(&repo, &branch_refname, current_parent_oid)?;

        Ok(RewriteResult {
            new_head: current_parent_oid.to_string(),
            backup_ref: backup_ref.clone(),
            rewritten_count: rewritten,
        })
    })();

    if result.is_err() && stashed {
        let _ = pop_stash(&mut repo);
    }
    if let Ok(_) = &result {
        if stashed {
            pop_stash(&mut repo)?;
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Public API — drop commits
// ---------------------------------------------------------------------------

pub fn drop_commits(
    repo_path: &str,
    commit_hashes: &[String],
    auto_stash: bool,
) -> Result<RewriteResult, Error> {
    if commit_hashes.is_empty() {
        return Ok(RewriteResult {
            new_head: String::new(),
            backup_ref: String::new(),
            rewritten_count: 0,
        });
    }

    let mut repo = open_git2(repo_path)?;
    let branch_refname = get_branch_refname(&repo)?;
    let branch_short = get_branch_short_name(&branch_refname).to_owned();
    let head_oid = repo.head().map_err(Error::internal)?.target().unwrap();

    let mut target_oids: Vec<Oid> = Vec::new();
    for hash in commit_hashes {
        let oid = repo
            .revparse_single(hash)
            .map_err(|_| Error::RevNotFound { rev: hash.clone() })?
            .id();
        target_oids.push(oid);
    }

    let is_dirty = preflight_checks(&repo, auto_stash)?;
    let stashed = stash_if_needed(&mut repo, is_dirty)?;
    let backup_ref = create_backup_ref(&repo, &branch_short)?;

    let result = (|| -> Result<RewriteResult, Error> {
        let mut all_chain = Vec::new();
        let mut current = head_oid;
        loop {
            all_chain.push(current);
            let commit = repo.find_commit(current).map_err(Error::internal)?;
            let parents: Vec<_> = commit.parent_ids().collect();
            if parents.is_empty() {
                break;
            }
            current = parents[0];
        }
        all_chain.reverse();

        let target_set: std::collections::HashSet<Oid> = target_oids.iter().copied().collect();

        let earliest_idx = all_chain
            .iter()
            .position(|oid| target_set.contains(oid))
            .ok_or_else(|| Error::CommitNotInChain {
                hash: commit_hashes[0].clone(),
            })?;

        let earliest_commit = repo
            .find_commit(all_chain[earliest_idx])
            .map_err(Error::internal)?;

        let mut current_parent_oid = if earliest_commit.parent_count() > 0 {
            earliest_commit.parent_id(0).map_err(Error::internal)?
        } else {
            return Err(Error::Internal {
                message: "Cannot drop the root commit".to_owned(),
            });
        };

        let mut rewritten = 0u32;

        for &oid in &all_chain[(earliest_idx + 1)..] {
            if target_set.contains(&oid) {
                continue;
            }
            let commit = repo.find_commit(oid).map_err(Error::internal)?;
            let new_parent = repo
                .find_commit(current_parent_oid)
                .map_err(Error::internal)?;
            current_parent_oid = if commit.parent_count() > 1 {
                reparent_commit(&repo, &commit, 0, &new_parent)?
            } else {
                cherry_pick_onto(&repo, &commit, &new_parent)?
            };
            rewritten += 1;
        }

        finalize(&repo, &branch_refname, current_parent_oid)?;

        Ok(RewriteResult {
            new_head: current_parent_oid.to_string(),
            backup_ref: backup_ref.clone(),
            rewritten_count: rewritten,
        })
    })();

    if result.is_err() && stashed {
        let _ = pop_stash(&mut repo);
    }
    if result.is_ok() && stashed {
        pop_stash(&mut repo)?;
    }

    result
}
