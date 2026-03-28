#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Repository not found at path: {path}")]
    RepoNotFound { path: String },

    #[error("Not a git repository: {path}")]
    NotARepo { path: String },

    #[error("Revision not found: {rev}")]
    RevNotFound { rev: String },

    #[error("Working tree has uncommitted changes")]
    WorkingTreeDirty,

    #[error("Config not found: {path}")]
    ConfigNotFound { path: String },

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Push rejected")]
    PushRejected,

    #[error("Remote not found: {name}")]
    RemoteNotFound { name: String },

    #[error("Merge conflict")]
    MergeConflict,

    #[error("Detached HEAD state")]
    DetachedHead,

    #[error("Operation in progress")]
    OperationInProgress,

    #[error("Merge commit unsupported")]
    MergeCommitUnsupported,

    #[error("Cherry-pick conflict")]
    CherryPickConflict,

    #[error("Cherry-pick empty")]
    CherryPickEmpty,

    #[error("Commit not in chain")]
    CommitNotInChain,

    #[error("Stash pop failed")]
    StashPopFailed,

    #[error("{message}")]
    Internal { message: String },
}

impl Error {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}

impl From<gix::open::Error> for Error {
    fn from(e: gix::open::Error) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}
