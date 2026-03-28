mod error;
mod hydrate;
mod ops;
mod repo;
mod types;

pub use error::MantleError;
pub use hydrate::hydrate;
pub use ops::artifacts::{
    bootstrap_worktreeinclude, compute_effective_worktreeinclude, generate_default_worktreeinclude,
    scan_clone_candidates, scan_worktreeinclude,
};
pub use ops::cow::cow_clone_directory;
pub use types::*;
pub use ops::worktree::{
    list_worktrees, worktree_add_existing, worktree_add_new_branch, worktree_prune,
    worktree_remove_clean, worktree_remove_force,
};
