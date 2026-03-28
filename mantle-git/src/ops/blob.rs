use std::collections::HashMap;

use gix::bstr::ByteSlice;

use crate::error::GitError;
use crate::repo;

/// Return a map of relative path → blob SHA-1 for all tracked files in the index (stage 0 only).
pub fn blob_oids(repo_path: &str) -> Result<HashMap<String, String>, GitError> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();
    let index = repo.index_or_empty().map_err(GitError::internal)?;

    let mut oids = HashMap::new();
    for entry in index.entries() {
        // Stage 0 = normal entries (not merge conflicts)
        if entry.stage_raw() == 0 {
            let path = entry.path(&index).to_str_lossy().to_string();
            let sha = entry.id.to_string();
            oids.insert(path, sha);
        }
    }
    Ok(oids)
}

/// Read the contents of a file at a specific commit as UTF-8.
/// Resolves `<commit>:<path>` by walking the commit's tree.
pub fn show_file(repo_path: &str, commit_hash: &str, file_path: &str) -> Result<String, GitError> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    // Resolve the commit
    let commit_id = repo
        .rev_parse_single(commit_hash)
        .map_err(|_| GitError::RevNotFound {
            rev: commit_hash.to_owned(),
        })?;

    let commit = commit_id
        .object()
        .map_err(GitError::internal)?
        .try_into_commit()
        .map_err(GitError::internal)?;

    let tree = commit.tree().map_err(GitError::internal)?;

    // Look up the file entry in the tree
    let entry = tree
        .lookup_entry_by_path(file_path)
        .map_err(GitError::internal)?
        .ok_or_else(|| GitError::Internal {
            message: format!("Path '{file_path}' not found in commit {commit_hash}"),
        })?;

    // Read the blob contents
    let object = entry.object().map_err(GitError::internal)?;
    let data = object.data.as_slice();

    String::from_utf8(data.to_vec()).map_err(|_| GitError::Internal {
        message: format!("File '{file_path}' is not valid UTF-8"),
    })
}
