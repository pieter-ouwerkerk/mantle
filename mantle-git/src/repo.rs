use std::path::Path;

use gix::ThreadSafeRepository;

use crate::error::Error;

pub(crate) fn open(path: &str) -> Result<ThreadSafeRepository, Error> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(Error::RepoNotFound {
            path: path.to_owned(),
        });
    }
    ThreadSafeRepository::open(p).map_err(|e| {
        if e.to_string().contains("not a git repository")
            || e.to_string().contains("Missing")
            || e.to_string().contains("missing")
        {
            Error::NotARepo {
                path: path.to_owned(),
            }
        } else {
            Error::internal(e)
        }
    })
}
