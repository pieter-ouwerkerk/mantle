use std::path::Path;

use gix::ThreadSafeRepository;

use crate::error::GitError;

pub(crate) fn open(path: &str) -> Result<ThreadSafeRepository, GitError> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(GitError::RepoNotFound {
            path: path.to_owned(),
        });
    }
    ThreadSafeRepository::open(p).map_err(|e| {
        if e.to_string().contains("not a git repository")
            || e.to_string().contains("Missing")
            || e.to_string().contains("missing")
        {
            GitError::NotARepo {
                path: path.to_owned(),
            }
        } else {
            GitError::internal(e)
        }
    })
}
