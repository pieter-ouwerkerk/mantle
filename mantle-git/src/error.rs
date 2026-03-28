#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum GitError {
    #[error("Repository not found at path: {path}")]
    RepoNotFound { path: String },

    #[error("Not a git repository: {path}")]
    NotARepo { path: String },

    #[error("Revision not found: {rev}")]
    RevNotFound { rev: String },

    #[error("Config key not found: {key}")]
    ConfigNotFound { key: String },

    #[error("Authentication failed for remote: {url}")]
    AuthenticationFailed { url: String },

    #[error("Push rejected: {reason}")]
    PushRejected { reason: String },

    #[error("Remote not found: {name}")]
    RemoteNotFound { name: String },

    #[error("Merge conflict: {message}")]
    MergeConflict { message: String },

    #[error("Detached HEAD: cannot rewrite without a branch")]
    DetachedHead,

    #[error("Working tree has uncommitted changes")]
    WorkingTreeDirty,

    #[error("A git operation is already in progress")]
    OperationInProgress,

    #[error("Merge commit cannot be rewritten: {hash}")]
    MergeCommitUnsupported { hash: String },

    #[error("Cherry-pick conflict at commit {hash}: {details}")]
    CherryPickConflict { hash: String, details: String },

    #[error("Cherry-pick produced no changes — commit {hash} is already applied")]
    CherryPickEmpty { hash: String },

    #[error("Commit not found in current branch history: {hash}")]
    CommitNotInChain { hash: String },

    #[error("Stash pop failed after rewrite: {message}")]
    StashPopFailed { message: String },

    #[error("{message}")]
    Internal { message: String },
}

impl GitError {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}

impl From<gix::open::Error> for GitError {
    fn from(e: gix::open::Error) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}

impl From<std::io::Error> for GitError {
    fn from(e: std::io::Error) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}
