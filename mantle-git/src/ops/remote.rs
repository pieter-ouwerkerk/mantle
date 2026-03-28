use std::cell::Cell;
use std::sync::{Arc, Mutex, Once};

use crate::error::Error;
use crate::types::{AheadBehindResult, FetchResult, PullResult, PushResult, RemoteInfo};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn open_git2(repo_path: &str) -> Result<git2::Repository, Error> {
    git2::Repository::open(repo_path).map_err(Error::internal)
}

/// Detect the SSH agent socket from ~/.ssh/config (e.g. 1Password IdentityAgent)
/// and set SSH_AUTH_SOCK so libssh2 (used by libgit2) can find it.
/// This runs once per process.
fn ensure_ssh_auth_sock() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Parse ~/.ssh/config for IdentityAgent directive.
        // This takes priority over SSH_AUTH_SOCK because the default macOS launchd
        // agent socket may exist but have no keys (e.g. when 1Password manages keys).
        let home = match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => return,
        };
        let config_path = format!("{home}/.ssh/config");
        let contents = match std::fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("IdentityAgent") {
                // Handle quoted or unquoted paths
                let raw = trimmed.splitn(2, char::is_whitespace).nth(1).unwrap_or("");
                let path = raw.trim().trim_matches('"');
                let expanded = path.replace("~/", &format!("{home}/"));
                if std::path::Path::new(&expanded).exists() {
                    std::env::set_var("SSH_AUTH_SOCK", &expanded);
                    return;
                }
            }
        }
    });
}

/// Build `RemoteCallbacks` with credential support for SSH agent and system defaults.
fn make_callbacks<'a>() -> git2::RemoteCallbacks<'a> {
    ensure_ssh_auth_sock();

    let mut callbacks = git2::RemoteCallbacks::new();

    // Track whether we already tried each credential type to avoid infinite retry loops.
    let tried_ssh = Cell::new(false);
    let tried_default = Cell::new(false);

    callbacks.credentials(move |_url, username_from_url, allowed_types| {
        // SSH key from agent (macOS ssh-agent or 1Password agent)
        if allowed_types.contains(git2::CredentialType::SSH_KEY) && !tried_ssh.get() {
            tried_ssh.set(true);
            let user = username_from_url.unwrap_or("git");
            return git2::Cred::ssh_key_from_agent(user);
        }
        // System default (HTTPS via SecureTransport / Keychain on macOS)
        if allowed_types.contains(git2::CredentialType::DEFAULT) && !tried_default.get() {
            tried_default.set(true);
            return git2::Cred::default();
        }
        // User/pass from credential helper
        if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            return git2::Cred::credential_helper(
                &git2::Config::open_default().unwrap_or_else(|_| {
                    // Fallback: create an empty in-memory config
                    git2::Config::new().expect("create empty git config")
                }),
                _url,
                username_from_url,
            );
        }
        Err(git2::Error::from_str("no credentials available"))
    });

    callbacks
}

/// Classify a git2 error into the appropriate `GitError` variant.
fn classify_remote_error(e: git2::Error, url: Option<&str>) -> Error {
    let msg = e.message().to_lowercase();
    if msg.contains("authentication")
        || msg.contains("auth")
        || msg.contains("credentials")
        || msg.contains("publickey")
        || msg.contains("permission denied")
    {
        Error::AuthenticationFailed {
            url: url.unwrap_or("unknown").to_owned(),
        }
    } else {
        Error::Internal {
            message: e.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// List all remotes with their fetch and push URLs.
pub fn list_remotes(repo_path: &str) -> Result<Vec<RemoteInfo>, Error> {
    let repo = open_git2(repo_path)?;
    let remote_names = repo.remotes().map_err(Error::internal)?;
    let mut remotes = Vec::new();

    for name in remote_names.iter().flatten() {
        let remote = repo.find_remote(name).map_err(Error::internal)?;
        remotes.push(RemoteInfo {
            name: name.to_owned(),
            fetch_url: remote.url().map(|s| s.to_owned()),
            push_url: remote.pushurl().map(|s| s.to_owned()),
        });
    }

    Ok(remotes)
}

/// Fetch from a remote, returning which refs were updated.
pub fn fetch(repo_path: &str, remote_name: &str) -> Result<FetchResult, Error> {
    let repo = open_git2(repo_path)?;
    let mut remote = repo
        .find_remote(remote_name)
        .map_err(|_| Error::RemoteNotFound {
            name: remote_name.to_owned(),
        })?;

    let url = remote.url().unwrap_or("").to_owned();

    let updated_refs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let refs_clone = Arc::clone(&updated_refs);

    let mut callbacks = make_callbacks();
    callbacks.update_tips(move |refname, _old, _new| {
        refs_clone.lock().unwrap().push(refname.to_owned());
        true
    });

    let mut fetch_opts = git2::FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    // Fetch all branches
    remote
        .fetch(&[] as &[&str], Some(&mut fetch_opts), None)
        .map_err(|e| classify_remote_error(e, Some(&url)))?;

    let refs = updated_refs.lock().unwrap().clone();
    Ok(FetchResult { updated_refs: refs })
}

/// Push a refspec to a remote.
pub fn push(
    repo_path: &str,
    remote_name: &str,
    refspec: &str,
    force: bool,
) -> Result<PushResult, Error> {
    let repo = open_git2(repo_path)?;
    let mut remote = repo
        .find_remote(remote_name)
        .map_err(|_| Error::RemoteNotFound {
            name: remote_name.to_owned(),
        })?;

    let url = remote.url().unwrap_or("").to_owned();

    // Prepend '+' for force push
    let actual_refspec = if force && !refspec.starts_with('+') {
        format!("+{refspec}")
    } else {
        refspec.to_owned()
    };

    let rejection: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let rejection_clone = Arc::clone(&rejection);
    let updated_ref: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let updated_ref_clone = Arc::clone(&updated_ref);

    let mut callbacks = make_callbacks();
    callbacks.push_update_reference(move |refname, status| {
        if let Some(msg) = status {
            *rejection_clone.lock().unwrap() = Some(msg.to_owned());
        } else {
            *updated_ref_clone.lock().unwrap() = Some(refname.to_owned());
        }
        Ok(())
    });

    let mut push_opts = git2::PushOptions::new();
    push_opts.remote_callbacks(callbacks);

    remote
        .push(&[&actual_refspec], Some(&mut push_opts))
        .map_err(|e| classify_remote_error(e, Some(&url)))?;

    // Check for rejection
    if let Some(reason) = rejection.lock().unwrap().take() {
        return Err(Error::PushRejected { reason });
    }

    let ref_updated = updated_ref.lock().unwrap().take();
    let up_to_date = ref_updated.is_none();

    Ok(PushResult {
        updated_ref: ref_updated,
        up_to_date,
    })
}

/// High-level push: resolves remote from tracking config, builds refspec, optionally sets upstream.
pub fn push_branch(
    repo_path: &str,
    branch: &str,
    set_upstream: bool,
    force: bool,
) -> Result<PushResult, Error> {
    let repo = open_git2(repo_path)?;

    // Resolve which remote to push to
    let remote_name = repo
        .branch_upstream_remote(&format!("refs/heads/{branch}"))
        .ok()
        .and_then(|buf| buf.as_str().map(|s| s.to_owned()))
        .unwrap_or_else(|| "origin".to_owned());

    // Build refspec: refs/heads/<branch>:refs/heads/<branch>
    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");

    let result = push(repo_path, &remote_name, &refspec, force)?;

    // Set upstream tracking if requested
    if set_upstream {
        let mut config = repo.config().map_err(Error::internal)?;
        config
            .set_str(&format!("branch.{branch}.remote"), &remote_name)
            .map_err(Error::internal)?;
        config
            .set_str(
                &format!("branch.{branch}.merge"),
                &format!("refs/heads/{branch}"),
            )
            .map_err(Error::internal)?;
    }

    Ok(result)
}

/// Fetch + fast-forward merge. Returns `MergeConflict` if branches have diverged.
pub fn pull(repo_path: &str, remote_name: &str, branch: &str) -> Result<PullResult, Error> {
    // Step 1: Fetch
    let fetch_result = fetch(repo_path, remote_name)?;

    let repo = open_git2(repo_path)?;

    // Step 2: Find local and remote refs
    let local_ref_name = format!("refs/heads/{branch}");
    let remote_ref_name = format!("refs/remotes/{remote_name}/{branch}");

    let local_ref = repo
        .find_reference(&local_ref_name)
        .map_err(Error::internal)?;
    let remote_ref = match repo.find_reference(&remote_ref_name) {
        Ok(r) => r,
        Err(_) => {
            // Remote branch doesn't exist (yet) — nothing to merge
            return Ok(PullResult {
                fetch_updated_refs: fetch_result.updated_refs,
                merge_type: "already_up_to_date".to_owned(),
                new_head: None,
            });
        }
    };

    let local_oid = local_ref.target().ok_or_else(|| Error::Internal {
        message: "local branch has no target".to_owned(),
    })?;
    let remote_oid = remote_ref.target().ok_or_else(|| Error::Internal {
        message: "remote tracking branch has no target".to_owned(),
    })?;

    // Already up to date?
    if local_oid == remote_oid {
        return Ok(PullResult {
            fetch_updated_refs: fetch_result.updated_refs,
            merge_type: "already_up_to_date".to_owned(),
            new_head: None,
        });
    }

    // Check ahead/behind
    let (ahead, behind) = repo
        .graph_ahead_behind(local_oid, remote_oid)
        .map_err(Error::internal)?;

    if ahead > 0 && behind > 0 {
        return Err(Error::MergeConflict {
            message: format!(
                "Branches have diverged: {ahead} ahead, {behind} behind. \
                 Fast-forward not possible."
            ),
        });
    }

    if behind == 0 {
        // Local is ahead or equal — nothing to do
        return Ok(PullResult {
            fetch_updated_refs: fetch_result.updated_refs,
            merge_type: "already_up_to_date".to_owned(),
            new_head: None,
        });
    }

    // Fast-forward: update ref and checkout
    let remote_commit = repo.find_commit(remote_oid).map_err(Error::internal)?;
    let remote_obj = remote_commit.as_object();

    repo.reference(
        &local_ref_name,
        remote_oid,
        true,
        &format!("pull: fast-forward {branch}"),
    )
    .map_err(Error::internal)?;

    repo.checkout_tree(
        remote_obj,
        Some(git2::build::CheckoutBuilder::new().force()),
    )
    .map_err(Error::internal)?;
    repo.set_head(&local_ref_name).map_err(Error::internal)?;

    Ok(PullResult {
        fetch_updated_refs: fetch_result.updated_refs,
        merge_type: "fast_forward".to_owned(),
        new_head: Some(remote_oid.to_string()),
    })
}

/// Get the remote tracking branch for a local branch.
pub fn remote_tracking_branch(repo_path: &str, branch: &str) -> Result<Option<String>, Error> {
    let repo = open_git2(repo_path)?;
    match repo.branch_upstream_name(&format!("refs/heads/{branch}")) {
        Ok(buf) => Ok(buf.as_str().map(|s| s.to_owned())),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(Error::internal(e)),
    }
}

/// Get ahead/behind counts relative to the remote tracking branch.
pub fn ahead_behind_remote(repo_path: &str, branch: &str) -> Result<AheadBehindResult, Error> {
    let repo = open_git2(repo_path)?;

    let local_ref = format!("refs/heads/{branch}");
    let local_oid = repo
        .find_reference(&local_ref)
        .map_err(Error::internal)?
        .target()
        .ok_or_else(|| Error::Internal {
            message: "local branch has no target".to_owned(),
        })?;

    let upstream_name = match repo.branch_upstream_name(&local_ref) {
        Ok(buf) => buf.as_str().unwrap_or("").to_owned(),
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            return Err(Error::Internal {
                message: format!("No upstream configured for branch '{branch}'"),
            });
        }
        Err(e) => return Err(Error::internal(e)),
    };

    let upstream_oid = repo
        .find_reference(&upstream_name)
        .map_err(Error::internal)?
        .target()
        .ok_or_else(|| Error::Internal {
            message: "upstream branch has no target".to_owned(),
        })?;

    let (ahead, behind) = repo
        .graph_ahead_behind(local_oid, upstream_oid)
        .map_err(Error::internal)?;

    Ok(AheadBehindResult {
        ahead: ahead as u32,
        behind: behind as u32,
    })
}
