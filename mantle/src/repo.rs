use std::path::Path;

use gix::ThreadSafeRepository;

use crate::error::MantleError;

pub(crate) fn open(path: &str) -> Result<ThreadSafeRepository, MantleError> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(MantleError::RepoNotFound {
            path: path.to_owned(),
        });
    }
    ThreadSafeRepository::open(p).map_err(|e| {
        if e.to_string().contains("not a git repository")
            || e.to_string().contains("Missing")
            || e.to_string().contains("missing")
        {
            MantleError::NotARepo {
                path: path.to_owned(),
            }
        } else {
            MantleError::internal(e)
        }
    })
}
