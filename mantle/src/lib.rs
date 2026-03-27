mod artifacts;
mod cow;
mod error;
mod repo;
mod types;
mod worktree;

pub use artifacts::{
    bootstrap_worktreeinclude, compute_effective_worktreeinclude,
    generate_default_worktreeinclude, scan_clone_candidates, scan_worktreeinclude,
};
pub use cow::cow_clone_directory;
pub use error::MantleError;
pub use types::*;
pub use worktree::{
    list_worktrees, worktree_add_existing, worktree_add_new_branch,
    worktree_prune, worktree_remove_clean, worktree_remove_force,
};
