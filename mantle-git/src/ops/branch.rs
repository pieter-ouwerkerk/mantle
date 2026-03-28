use crate::error::Error;
use crate::ops::log::format_gix_time;
use crate::repo;
use crate::types::BranchInfo;

pub fn current_branch(repo_path: &str) -> Result<String, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();
    let head_ref = repo.head_ref().map_err(Error::internal)?;
    match head_ref {
        Some(r) => {
            let full_name = r.name().as_bstr().to_string();
            Ok(full_name
                .strip_prefix("refs/heads/")
                .unwrap_or(&full_name)
                .to_owned())
        }
        None => Ok("HEAD".to_owned()),
    }
}

pub fn list_local_branches(repo_path: &str) -> Result<Vec<BranchInfo>, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let refs = repo.references().map_err(Error::internal)?;
    let local = refs.local_branches().map_err(Error::internal)?;

    let mut branches: Vec<(BranchInfo, i64)> = Vec::new();

    for reference in local.flatten() {
        let name = reference.name().as_bstr().to_string();
        let short_name = name.strip_prefix("refs/heads/").unwrap_or(&name).to_owned();

        let (date_str, author, hash, sort_time) = match reference
            .into_fully_peeled_id()
        {
            Ok(peeled_id) => {
                let hash = peeled_id.to_string();
                match peeled_id.object().ok().and_then(|obj| obj.try_into_commit().ok()) {
                    Some(commit) => {
                        let sig = commit.committer().map_err(Error::internal)?;
                        let time = sig.time().map_err(Error::internal)?;
                        (format_gix_time(time), sig.name.to_string(), hash, time.seconds)
                    }
                    None => (String::new(), String::new(), hash, 0),
                }
            }
            Err(_) => (String::new(), String::new(), String::new(), 0),
        };

        branches.push((
            BranchInfo {
                name: short_name,
                date: date_str,
                author,
                hash,
            },
            sort_time,
        ));
    }

    // Sort by date descending (newest first)
    branches.sort_by(|a, b| b.1.cmp(&a.1));

    Ok(branches.into_iter().map(|(info, _)| info).collect())
}

pub fn verify_branch_exists(repo_path: &str, branch: &str) -> Result<bool, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();
    let ref_name = format!("refs/heads/{branch}");
    match repo.find_reference(&ref_name) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Check whether `branch` is fully merged into `target_branch`.
/// Returns true if target_branch's tip is a descendant of (or equal to) branch's tip.
pub fn branch_is_merged(repo_path: &str, branch: &str, target_branch: &str) -> Result<bool, Error> {
    let repo = git2::Repository::open(repo_path).map_err(Error::internal)?;

    let branch_ref = format!("refs/heads/{branch}");
    let target_ref = format!("refs/heads/{target_branch}");

    let branch_oid = repo
        .refname_to_id(&branch_ref)
        .map_err(|_| Error::RevNotFound { rev: branch.to_owned() })?;
    let target_oid = repo
        .refname_to_id(&target_ref)
        .map_err(|_| Error::RevNotFound { rev: target_branch.to_owned() })?;

    if branch_oid == target_oid {
        return Ok(true);
    }

    // target is descendant of branch ⇔ branch is merged into target
    repo.graph_descendant_of(target_oid, branch_oid)
        .map_err(Error::internal)
}

/// List remote-tracking branches (refs/remotes/*), sorted by committer date descending.
/// Filters out `*/HEAD` symbolic refs.
pub fn list_remote_branches(repo_path: &str) -> Result<Vec<BranchInfo>, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let refs = repo.references().map_err(Error::internal)?;
    let remote = refs.remote_branches().map_err(Error::internal)?;

    let mut branches: Vec<(BranchInfo, i64)> = Vec::new();

    for reference in remote.flatten() {
        let name = reference.name().as_bstr().to_string();
        let short_name = name.strip_prefix("refs/remotes/").unwrap_or(&name).to_owned();

        // Skip HEAD pointers (e.g. origin/HEAD)
        if short_name.ends_with("/HEAD") {
            continue;
        }

        let (date_str, author, hash, sort_time) = match reference.into_fully_peeled_id() {
            Ok(peeled_id) => {
                let hash = peeled_id.to_string();
                match peeled_id
                    .object()
                    .ok()
                    .and_then(|obj| obj.try_into_commit().ok())
                {
                    Some(commit) => {
                        let sig = commit.committer().map_err(Error::internal)?;
                        let time = sig.time().map_err(Error::internal)?;
                        (
                            format_gix_time(time),
                            sig.name.to_string(),
                            hash,
                            time.seconds,
                        )
                    }
                    None => (String::new(), String::new(), hash, 0),
                }
            }
            Err(_) => (String::new(), String::new(), String::new(), 0),
        };

        branches.push((
            BranchInfo {
                name: short_name,
                date: date_str,
                author,
                hash,
            },
            sort_time,
        ));
    }

    // Sort by date descending (newest first)
    branches.sort_by(|a, b| b.1.cmp(&a.1));

    Ok(branches.into_iter().map(|(info, _)| info).collect())
}

/// Return the ISO-8601 author date of the latest commit on `branch`, or None if the branch
/// has no commits.
pub fn latest_commit_date(repo_path: &str, branch: &str) -> Result<Option<String>, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let ref_name = format!("refs/heads/{branch}");
    let reference = match repo.find_reference(&ref_name) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };

    let peeled = reference.into_fully_peeled_id().map_err(Error::internal)?;
    let object = peeled.object().map_err(Error::internal)?;
    let commit = object.try_into_commit().map_err(Error::internal)?;
    let sig = commit.author().map_err(Error::internal)?;
    let time = sig.time().map_err(Error::internal)?;
    Ok(Some(format_gix_time(time)))
}
