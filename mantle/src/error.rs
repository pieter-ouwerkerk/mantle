#[derive(Debug, thiserror::Error)]
pub enum MantleError {
    #[error("Repository not found at path: {path}")]
    RepoNotFound { path: String },

    #[error("Not a git repository: {path}")]
    NotARepo { path: String },

    #[error("Revision not found: {rev}")]
    RevNotFound { rev: String },

    #[error("Working tree has uncommitted changes")]
    WorkingTreeDirty,

    #[error("{message}")]
    Internal { message: String },
}

impl MantleError {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}

impl From<gix::open::Error> for MantleError {
    fn from(e: gix::open::Error) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}

impl From<std::io::Error> for MantleError {
    fn from(e: std::io::Error) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}
