use gix::bstr::ByteSlice;
use gix::object::tree::diff::Action;
use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use crate::error::Error;
use crate::repo;
use crate::types::CommitInfo;

pub fn log(repo_path: &str, max_count: u32, skip: u32) -> Result<Vec<CommitInfo>, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let Ok(head) = repo.head_id() else {
        return Ok(Vec::new()); // Unborn branch — no commits
    };

    let walk = head
        .ancestors()
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .map_err(Error::internal)?;

    let limit = if max_count == 0 {
        usize::MAX
    } else {
        max_count as usize
    };
    let skip_count = skip as usize;

    let mut commits = Vec::new();
    let mut skipped = 0usize;
    for info in walk {
        if commits.len() >= limit {
            break;
        }
        let info = info.map_err(Error::internal)?;
        if skipped < skip_count {
            skipped += 1;
            continue;
        }
        let commit = info
            .id()
            .object()
            .map_err(Error::internal)?
            .try_into_commit()
            .map_err(Error::internal)?;
        commits.push(commit_to_info(&commit)?);
    }

    Ok(commits)
}

pub fn log_for_ref(repo_path: &str, git_ref: &str, max_count: u32, skip: u32) -> Result<Vec<CommitInfo>, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let object = repo
        .rev_parse_single(git_ref)
        .map_err(|_| Error::RevNotFound {
            rev: git_ref.to_owned(),
        })?;

    let walk = object
        .ancestors()
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .map_err(Error::internal)?;

    let limit = if max_count == 0 {
        usize::MAX
    } else {
        max_count as usize
    };
    let skip_count = skip as usize;

    let mut commits = Vec::new();
    let mut skipped = 0usize;
    for info in walk {
        if commits.len() >= limit {
            break;
        }
        let info = info.map_err(Error::internal)?;
        if skipped < skip_count {
            skipped += 1;
            continue;
        }
        let commit = info
            .id()
            .object()
            .map_err(Error::internal)?
            .try_into_commit()
            .map_err(Error::internal)?;
        commits.push(commit_to_info(&commit)?);
    }

    Ok(commits)
}

pub fn log_for_path(worktree_path: &str, max_count: u32, skip: u32) -> Result<Vec<CommitInfo>, Error> {
    // logForPath in the shell version just runs git log in the worktree directory.
    // With gix, opening the worktree path discovers the correct repo.
    log(worktree_path, max_count, skip)
}

pub fn full_message(repo_path: &str, commit_hash: &str) -> Result<String, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();
    let object = repo
        .rev_parse_single(commit_hash)
        .map_err(|_| Error::RevNotFound {
            rev: commit_hash.to_owned(),
        })?;
    let commit = object
        .object()
        .map_err(Error::internal)?
        .try_into_commit()
        .map_err(Error::internal)?;
    let msg = commit.message_raw_sloppy().to_str_lossy().to_string();
    Ok(msg.trim().to_owned())
}

pub fn recent_commits_for_context(repo_path: &str, count: u32) -> Result<String, Error> {
    let commits = log(repo_path, count, 0)?;
    let lines: Vec<String> = commits
        .iter()
        .map(|c| {
            let short = &c.hash[..c.hash.len().min(7)];
            format!("{short} {}", c.message)
        })
        .collect();
    Ok(lines.join("\n"))
}

pub fn log_by_file(
    repo_path: &str,
    pattern: &str,
    max_count: u32,
    skip: u32,
) -> Result<Vec<CommitInfo>, Error> {
    let pat = pattern.to_owned();
    log_with_path_filter(repo_path, max_count, skip, &|path: &str| {
        path.contains(&*pat)
    })
}

pub fn log_for_paths(
    repo_path: &str,
    paths: &[String],
    max_count: u32,
    skip: u32,
) -> Result<Vec<CommitInfo>, Error> {
    let owned: Vec<String> = paths.to_vec();
    log_with_path_filter(repo_path, max_count, skip, &|path: &str| {
        owned.iter().any(|p| path == p.as_str())
    })
}

fn log_with_path_filter(
    repo_path: &str,
    max_count: u32,
    skip: u32,
    matches: &dyn Fn(&str) -> bool,
) -> Result<Vec<CommitInfo>, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let Ok(head) = repo.head_id() else {
        return Ok(Vec::new());
    };

    let walk = head
        .ancestors()
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .map_err(Error::internal)?;

    let limit = if max_count == 0 {
        usize::MAX
    } else {
        max_count as usize
    };
    let skip_count = skip as usize;

    let mut commits = Vec::new();
    let mut matched = 0usize;

    for info in walk {
        if commits.len() >= limit {
            break;
        }
        let info = info.map_err(Error::internal)?;
        let commit_obj = info.id().object().map_err(Error::internal)?;
        let commit = commit_obj.try_into_commit().map_err(Error::internal)?;

        if commit_touches_paths(&repo, &commit, matches)? {
            if matched < skip_count {
                matched += 1;
                continue;
            }
            commits.push(commit_to_info(&commit)?);
        }
    }

    Ok(commits)
}

/// Check if a commit modified any path matching the filter.
/// Compares the commit's tree against its first parent's tree.
/// For root commits (no parent), all paths in the tree match.
fn commit_touches_paths(
    repo: &gix::Repository,
    commit: &gix::Commit<'_>,
    matches: &dyn Fn(&str) -> bool,
) -> Result<bool, Error> {
    let tree = commit.tree().map_err(Error::internal)?;

    let parent_tree = commit
        .parent_ids()
        .next()
        .and_then(|pid| pid.object().ok())
        .and_then(|obj| obj.try_into_commit().ok())
        .and_then(|c| c.tree().ok());

    match parent_tree {
        Some(ptree) => {
            // Use gix tree diff callback API
            let mut found = false;
            let mut changes_platform = ptree.changes().map_err(Error::internal)?;
            let diff_result = changes_platform.for_each_to_obtain_tree(
                &tree,
                |change| -> Result<Action, std::convert::Infallible> {
                    let path = change.location().to_str_lossy();
                    if matches(&path) {
                        found = true;
                        Ok(Action::Break(()))
                    } else {
                        Ok(Action::Continue(()))
                    }
                },
            );
            // When we cancel early after finding a match, gix returns
            // Err("The delegate cancelled the operation"). That's expected
            // — only propagate errors when we didn't find a match.
            if !found {
                diff_result.map_err(Error::internal)?;
            }
            Ok(found)
        }
        None => {
            // Root commit — check all entries in tree recursively
            tree_contains_matching_path(repo, &tree, "", matches)
        }
    }
}

/// Recursively check if any path in the tree matches the filter.
fn tree_contains_matching_path(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    prefix: &str,
    matches: &dyn Fn(&str) -> bool,
) -> Result<bool, Error> {
    for entry in tree.iter() {
        let entry = entry.map_err(Error::internal)?;
        let name = entry.filename().to_str_lossy();
        let full_path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        if entry.mode().is_tree() {
            if let Ok(subtree) = repo
                .find_object(entry.oid())
                .map_err(Error::internal)
                .and_then(|obj| obj.try_into_tree().map_err(Error::internal))
            {
                if tree_contains_matching_path(repo, &subtree, &full_path, matches)? {
                    return Ok(true);
                }
            }
        } else if matches(&full_path) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn commit_to_info(commit: &gix::Commit<'_>) -> Result<CommitInfo, Error> {
    let author = commit.author().map_err(Error::internal)?;
    let committer = commit.committer().map_err(Error::internal)?;
    let message_raw = commit.message_raw_sloppy();
    // Subject is first line only
    let message = message_raw
        .lines()
        .next()
        .map(|l| l.to_str_lossy().to_string())
        .unwrap_or_default();
    let parent_hashes = commit
        .parent_ids()
        .map(|p| p.to_hex().to_string())
        .collect();

    Ok(CommitInfo {
        hash: commit.id().to_hex().to_string(),
        author_name: author.name.to_string(),
        author_email: author.email.to_string(),
        committer_name: committer.name.to_string(),
        committer_email: committer.email.to_string(),
        author_date: format_gix_time(author.time().map_err(Error::internal)?),
        message,
        parent_hashes,
    })
}

pub fn format_gix_time(time: gix::date::Time) -> String {
    use chrono::{FixedOffset, TimeZone};

    let offset_seconds = time.offset;
    let offset = FixedOffset::east_opt(offset_seconds)
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero offset"));
    if let chrono::LocalResult::Single(dt) = offset.timestamp_opt(time.seconds, 0) {
        dt.to_rfc3339()
    } else {
        // Fallback: use UTC
        let utc = FixedOffset::east_opt(0).expect("zero offset");
        utc.timestamp_opt(time.seconds, 0)
            .single()
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default()
    }
}
