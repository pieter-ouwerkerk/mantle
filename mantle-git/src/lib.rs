#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

mod error;
mod ops;
mod repo;
mod types;

use std::collections::HashMap;

pub use error::GitError;
pub use types::MergeStateKind;
pub use types::*;

// MARK: - Artifact operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn scan_clone_candidates(
    repo_path: String,
    worktree_path: String,
) -> Result<Vec<CloneCandidate>, GitError> {
    ops::artifacts::scan_clone_candidates(&repo_path, &worktree_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn scan_worktreeinclude(
    repo_path: String,
    worktree_path: String,
    gitignore_fallback: bool,
) -> Result<WorktreeIncludeResult, GitError> {
    ops::artifacts::scan_worktreeinclude(&repo_path, &worktree_path, gitignore_fallback)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn compute_effective_worktreeinclude(
    repo_path: String,
    size_threshold_bytes: u64,
) -> Result<EffectiveWorktreeinclude, GitError> {
    ops::artifacts::compute_effective_worktreeinclude(&repo_path, size_threshold_bytes)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn generate_default_worktreeinclude(
    repo_path: String,
) -> Result<GeneratedWorktreeinclude, GitError> {
    ops::artifacts::generate_default_worktreeinclude(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn bootstrap_worktreeinclude(repo_path: String) -> Result<GeneratedWorktreeinclude, GitError> {
    ops::artifacts::bootstrap_worktreeinclude(&repo_path)
}

// MARK: - CoW operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn cow_clone_directory(
    source: String,
    destination: String,
) -> Result<CowCloneResult, GitError> {
    ops::cow::cow_clone_directory(&source, &destination)
}

// MARK: - Worktree operations (direct)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn list_worktrees(repo_path: String) -> Result<Vec<WorktreeInfo>, GitError> {
    ops::worktree::list_worktrees(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn worktree_add_new_branch(
    repo_path: String,
    path: String,
    branch: String,
    start_point: String,
) -> Result<(), GitError> {
    ops::worktree::worktree_add_new_branch(&repo_path, &path, &branch, &start_point)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn worktree_add_existing(
    repo_path: String,
    path: String,
    branch: String,
) -> Result<(), GitError> {
    ops::worktree::worktree_add_existing(&repo_path, &path, &branch)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn worktree_prune(repo_path: String) -> Result<(), GitError> {
    ops::worktree::worktree_prune(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn worktree_remove_clean(repo_path: String, path: String) -> Result<(), GitError> {
    ops::worktree::worktree_remove_clean(&repo_path, &path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn worktree_remove_force(repo_path: String, path: String) -> Result<(), GitError> {
    ops::worktree::worktree_remove_force(&repo_path, &path)
}

// MARK: - Blame operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_blame_file(
    repo_path: String,
    file_path: String,
) -> Result<Vec<BlameLineInfo>, GitError> {
    ops::blame::blame_file(&repo_path, &file_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_blame_file_at(
    repo_path: String,
    file_path: String,
    commit_hash: String,
) -> Result<Vec<BlameLineInfo>, GitError> {
    ops::blame::blame_file_at(&repo_path, &file_path, &commit_hash)
}

// MARK: - Log operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_log(repo_path: String, max_count: u32, skip: u32) -> Result<Vec<CommitInfo>, GitError> {
    ops::log::log(&repo_path, max_count, skip)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_log_for_ref(
    repo_path: String,
    git_ref: String,
    max_count: u32,
    skip: u32,
) -> Result<Vec<CommitInfo>, GitError> {
    ops::log::log_for_ref(&repo_path, &git_ref, max_count, skip)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_log_for_path(
    worktree_path: String,
    max_count: u32,
    skip: u32,
) -> Result<Vec<CommitInfo>, GitError> {
    ops::log::log_for_path(&worktree_path, max_count, skip)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_log_by_file(
    repo_path: String,
    pattern: String,
    max_count: u32,
    skip: u32,
) -> Result<Vec<CommitInfo>, GitError> {
    ops::log::log_by_file(&repo_path, &pattern, max_count, skip)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_log_for_paths(
    repo_path: String,
    paths: Vec<String>,
    max_count: u32,
    skip: u32,
) -> Result<Vec<CommitInfo>, GitError> {
    ops::log::log_for_paths(&repo_path, &paths, max_count, skip)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_recent_commits_for_context(repo_path: String, count: u32) -> Result<String, GitError> {
    ops::log::recent_commits_for_context(&repo_path, count)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_full_message(repo_path: String, commit_hash: String) -> Result<String, GitError> {
    ops::log::full_message(&repo_path, &commit_hash)
}

// MARK: - Branch operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_current_branch(repo_path: String) -> Result<String, GitError> {
    ops::branch::current_branch(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_list_local_branches(repo_path: String) -> Result<Vec<BranchInfo>, GitError> {
    ops::branch::list_local_branches(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_list_remote_branches(repo_path: String) -> Result<Vec<BranchInfo>, GitError> {
    ops::branch::list_remote_branches(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_verify_branch_exists(repo_path: String, branch: String) -> Result<bool, GitError> {
    ops::branch::verify_branch_exists(&repo_path, &branch)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_branch_is_merged(
    repo_path: String,
    branch: String,
    target_branch: String,
) -> Result<bool, GitError> {
    ops::branch::branch_is_merged(&repo_path, &branch, &target_branch)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_latest_commit_date(
    repo_path: String,
    branch: String,
) -> Result<Option<String>, GitError> {
    ops::branch::latest_commit_date(&repo_path, &branch)
}

// MARK: - Config operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_config_user_name(repo_path: String) -> Result<String, GitError> {
    ops::config::config_user_name(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_config_user_email(repo_path: String) -> Result<String, GitError> {
    ops::config::config_user_email(&repo_path)
}

// MARK: - Ref operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_rev_parse(repo_path: String, rev: String) -> Result<String, GitError> {
    ops::refs::rev_parse(&repo_path, &rev)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_is_valid_repo(path: String) -> bool {
    ops::refs::is_valid_repo(&path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_rev_list_parents(
    repo_path: String,
    commit_hash: String,
) -> Result<Vec<String>, GitError> {
    ops::refs::rev_list_parents(&repo_path, &commit_hash)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_commit_tree_and_refs(
    repo_path: String,
    commit_hash: String,
) -> Result<CommitTreeRefsInfo, GitError> {
    ops::refs::commit_tree_and_refs(&repo_path, &commit_hash)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_ahead_behind(
    repo_path: String,
    ref1: String,
    ref2: String,
) -> Result<AheadBehindResult, GitError> {
    ops::refs::ahead_behind(&repo_path, &ref1, &ref2)
}

// MARK: - Diff operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_show_diff(repo_path: String, commit_hash: String) -> Result<String, GitError> {
    ops::diff::show_diff(&repo_path, &commit_hash)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_working_tree_diff(repo_path: String) -> Result<String, GitError> {
    ops::diff::working_tree_diff(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_working_tree_diff_for_context(repo_path: String) -> Result<String, GitError> {
    ops::diff::working_tree_diff_for_context(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_diff_between_refs(
    repo_path: String,
    base: String,
    head: String,
) -> Result<String, GitError> {
    ops::diff::diff_between_refs(&repo_path, &base, &head)
}

// MARK: - Status operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_is_clean(repo_path: String) -> Result<bool, GitError> {
    ops::status::is_clean(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_status_summary(repo_path: String) -> Result<StatusSummary, GitError> {
    ops::status::status_summary(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_list_tracked_files(repo_path: String) -> Result<Vec<String>, GitError> {
    ops::status::list_tracked_files(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_list_untracked_files(repo_path: String) -> Result<Vec<String>, GitError> {
    ops::status::list_untracked_files(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_changed_paths(repo_path: String) -> Result<Vec<String>, GitError> {
    ops::status::changed_paths(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_worktree_status(path: String) -> Result<WorktreeStatusInfo, GitError> {
    ops::status::worktree_status(&path)
}

// MARK: - Repository operations (git2)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_init_repo(path: String) -> Result<(), GitError> {
    ops::write::init_repo(&path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_repo_root(path: String) -> Result<String, GitError> {
    ops::write::repo_root(&path)
}

// MARK: - Branch write operations (git2)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_checkout(repo_path: String, branch: String) -> Result<(), GitError> {
    ops::write::checkout(&repo_path, &branch)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_branch_delete(repo_path: String, name: String) -> Result<(), GitError> {
    ops::write::branch_delete(&repo_path, &name)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_create_branch_at(
    repo_path: String,
    branch: String,
    start_point: String,
) -> Result<(), GitError> {
    ops::write::create_branch_at(&repo_path, &branch, &start_point)
}

// MARK: - Ref write operations (git2)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_update_ref(repo_path: String, ref_name: String, value: String) -> Result<(), GitError> {
    ops::write::update_ref(&repo_path, &ref_name, &value)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_delete_ref(repo_path: String, ref_name: String) -> Result<(), GitError> {
    ops::write::delete_ref(&repo_path, &ref_name)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_reset_hard(repo_path: String, rev: String) -> Result<(), GitError> {
    ops::write::reset_hard(&repo_path, &rev)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_reset_soft(repo_path: String, rev: String) -> Result<(), GitError> {
    ops::write::reset_soft(&repo_path, &rev)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_reset_mixed(repo_path: String, rev: String) -> Result<(), GitError> {
    ops::write::reset_mixed(&repo_path, &rev)
}

// MARK: - Clean operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_clean_untracked(repo_path: String) -> Result<(), GitError> {
    ops::write::clean_untracked(&repo_path)
}

// MARK: - Stash operations (git2)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_stash_push(repo_path: String, message: String) -> Result<(), GitError> {
    ops::write::stash_push(&repo_path, &message)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_stash_pop(repo_path: String) -> Result<(), GitError> {
    ops::write::stash_pop(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_stash_list(repo_path: String) -> Result<Vec<StashEntry>, GitError> {
    ops::write::stash_list(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_stash_apply(repo_path: String, index: u32) -> Result<(), GitError> {
    ops::write::stash_apply(&repo_path, index)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_stash_drop(repo_path: String, index: u32) -> Result<(), GitError> {
    ops::write::stash_drop(&repo_path, index)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_stash_show(repo_path: String, index: u32) -> Result<String, GitError> {
    ops::write::stash_show(&repo_path, index)
}

// MARK: - Tag operations (git2)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_list_tags(repo_path: String) -> Result<Vec<TagInfo>, GitError> {
    ops::tag::list_tags(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_create_tag(
    repo_path: String,
    name: String,
    target_hash: String,
    message: Option<String>,
) -> Result<(), GitError> {
    ops::tag::create_tag(&repo_path, &name, &target_hash, message)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_delete_tag(repo_path: String, name: String) -> Result<(), GitError> {
    ops::tag::delete_tag(&repo_path, &name)
}

// MARK: - Staging & commit operations (git2)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_add_all(repo_path: String) -> Result<(), GitError> {
    ops::write::add_all(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_add_files(repo_path: String, paths: Vec<String>) -> Result<(), GitError> {
    ops::write::add_files(&repo_path, &paths)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_reset_staging(repo_path: String) -> Result<(), GitError> {
    ops::write::reset_staging(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_commit(repo_path: String, message: String) -> Result<(), GitError> {
    ops::write::commit(&repo_path, &message)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_amend_commit(repo_path: String, message: String) -> Result<(), GitError> {
    ops::write::amend_commit(&repo_path, &message)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_restore_file(repo_path: String, file_path: String) -> Result<(), GitError> {
    ops::write::restore_file(&repo_path, &file_path)
}

// MARK: - Remote operations (git2)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_list_remotes(repo_path: String) -> Result<Vec<RemoteInfo>, GitError> {
    ops::remote::list_remotes(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_fetch(repo_path: String, remote_name: String) -> Result<FetchResult, GitError> {
    ops::remote::fetch(&repo_path, &remote_name)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_push(
    repo_path: String,
    remote_name: String,
    refspec: String,
    force: bool,
) -> Result<PushResult, GitError> {
    ops::remote::push(&repo_path, &remote_name, &refspec, force)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_push_branch(
    repo_path: String,
    branch: String,
    set_upstream: bool,
    force: bool,
) -> Result<PushResult, GitError> {
    ops::remote::push_branch(&repo_path, &branch, set_upstream, force)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_pull(
    repo_path: String,
    remote_name: String,
    branch: String,
) -> Result<PullResult, GitError> {
    ops::remote::pull(&repo_path, &remote_name, &branch)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_remote_tracking_branch(
    repo_path: String,
    branch: String,
) -> Result<Option<String>, GitError> {
    ops::remote::remote_tracking_branch(&repo_path, &branch)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_ahead_behind_remote(
    repo_path: String,
    branch: String,
) -> Result<AheadBehindResult, GitError> {
    ops::remote::ahead_behind_remote(&repo_path, &branch)
}

// MARK: - Blob operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_blob_oids(repo_path: String) -> Result<HashMap<String, String>, GitError> {
    ops::blob::blob_oids(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_show_file(
    repo_path: String,
    commit_hash: String,
    file_path: String,
) -> Result<String, GitError> {
    ops::blob::show_file(&repo_path, &commit_hash, &file_path)
}

// MARK: - Reflog operations (git2)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_reflog(
    repo_path: String,
    refname: String,
    max_count: u32,
) -> Result<Vec<ReflogEntry>, GitError> {
    ops::reflog::reflog(&repo_path, &refname, max_count)
}

// MARK: - Cherry-pick operations (git2)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_cherry_pick(
    repo_path: String,
    commit_hash: String,
    auto_stash: bool,
) -> Result<String, GitError> {
    ops::rewrite::cherry_pick(&repo_path, &commit_hash, auto_stash)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_cherry_pick_to_branch(
    repo_path: String,
    commit_hash: String,
    target_branch: String,
    auto_stash: bool,
) -> Result<String, GitError> {
    ops::rewrite::cherry_pick_to_branch(&repo_path, &commit_hash, &target_branch, auto_stash)
}

// MARK: - Rewrite operations (git2 cherry-pick engine)

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_rewrite_commit_author(
    repo_path: String,
    commit_hash: String,
    new_name: String,
    new_email: String,
    auto_stash: bool,
) -> Result<RewriteResult, GitError> {
    ops::rewrite::rewrite_commit_author(&repo_path, &commit_hash, &new_name, &new_email, auto_stash)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_rewrite_commit_date(
    repo_path: String,
    commit_hash: String,
    new_date_iso: String,
    auto_stash: bool,
) -> Result<RewriteResult, GitError> {
    ops::rewrite::rewrite_commit_date(&repo_path, &commit_hash, &new_date_iso, auto_stash)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_rewrite_commit_message(
    repo_path: String,
    commit_hash: String,
    new_message: String,
    auto_stash: bool,
) -> Result<RewriteResult, GitError> {
    ops::rewrite::rewrite_commit_message(&repo_path, &commit_hash, &new_message, auto_stash)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_fixup_commits(
    repo_path: String,
    commit_hashes: Vec<String>,
    auto_stash: bool,
) -> Result<RewriteResult, GitError> {
    ops::rewrite::fixup_commits(&repo_path, &commit_hashes, auto_stash)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_drop_commits(
    repo_path: String,
    commit_hashes: Vec<String>,
    auto_stash: bool,
) -> Result<RewriteResult, GitError> {
    ops::rewrite::drop_commits(&repo_path, &commit_hashes, auto_stash)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_commit_metadata(
    repo_path: String,
    commit_hash: String,
) -> Result<CommitMetadataInfo, GitError> {
    ops::rewrite::commit_metadata(&repo_path, &commit_hash)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_prune_backup_refs(repo_path: String, retention_days: u32) -> Result<u32, GitError> {
    ops::rewrite::prune_backup_refs(&repo_path, retention_days)
}

// MARK: - Worktree operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_list_worktrees(repo_path: String) -> Result<Vec<WorktreeInfo>, GitError> {
    ops::worktree::list_worktrees(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_worktree_add_new_branch(
    repo_path: String,
    path: String,
    branch: String,
    start_point: String,
) -> Result<(), GitError> {
    ops::worktree::worktree_add_new_branch(&repo_path, &path, &branch, &start_point)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_worktree_add_existing(
    repo_path: String,
    path: String,
    branch: String,
) -> Result<(), GitError> {
    ops::worktree::worktree_add_existing(&repo_path, &path, &branch)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_worktree_remove_clean(repo_path: String, path: String) -> Result<(), GitError> {
    ops::worktree::worktree_remove_clean(&repo_path, &path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_worktree_remove_force(repo_path: String, path: String) -> Result<(), GitError> {
    ops::worktree::worktree_remove_force(&repo_path, &path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_worktree_prune(repo_path: String) -> Result<(), GitError> {
    ops::worktree::worktree_prune(&repo_path)
}

// MARK: - Merge state & conflict operations

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_merge_state(repo_path: String) -> Result<MergeStateInfo, GitError> {
    ops::merge::merge_state(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_list_conflict_paths(repo_path: String) -> Result<Vec<String>, GitError> {
    ops::merge::list_conflict_paths(&repo_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_checkout_ours(repo_path: String, file_path: String) -> Result<(), GitError> {
    ops::merge::checkout_ours(&repo_path, &file_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_checkout_theirs(repo_path: String, file_path: String) -> Result<(), GitError> {
    ops::merge::checkout_theirs(&repo_path, &file_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_mark_resolved(repo_path: String, file_path: String) -> Result<(), GitError> {
    ops::merge::mark_resolved(&repo_path, &file_path)
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn git_conflict_sides(repo_path: String, file_path: String) -> Result<ConflictSides, GitError> {
    ops::merge::conflict_sides(&repo_path, &file_path)
}
