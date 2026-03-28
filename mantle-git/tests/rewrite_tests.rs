use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

use mantle_git::*;

/// Helper to create a temporary git repo for testing.
struct TestRepo {
    _dir: TempDir,
    path: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().to_path_buf();

        run_git(&path, &["init", "-b", "main"]);
        run_git(&path, &["config", "user.name", "Test Author"]);
        run_git(&path, &["config", "user.email", "test@example.com"]);
        run_git(&path, &["config", "commit.gpgsign", "false"]);

        Self { _dir: dir, path }
    }

    fn path_str(&self) -> String {
        self.path.to_str().unwrap().to_owned()
    }

    fn commit(&self, filename: &str, content: &str, message: &str) -> String {
        let file_path = self.path.join(filename);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&file_path, content).expect("write file");
        run_git(&self.path, &["add", filename]);
        run_git(&self.path, &["commit", "-m", message]);
        run_git(&self.path, &["rev-parse", "HEAD"])
    }

    fn commit_with_author(
        &self,
        filename: &str,
        content: &str,
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> String {
        let file_path = self.path.join(filename);
        std::fs::write(&file_path, content).expect("write file");
        run_git(&self.path, &["add", filename]);
        let author = format!("{author_name} <{author_email}>");
        run_git(&self.path, &["commit", "-m", message, "--author", &author]);
        run_git(&self.path, &["rev-parse", "HEAD"])
    }
}

fn run_git(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_COMMITTER_NAME", "Test Author")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("failed to run git");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("git {args:?} failed: {stderr}");
    }
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

// ═══════════════════════════════════════════════════════════════
// Rewrite author
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rewrite_author_on_head() {
    let repo = TestRepo::new();
    let hash = repo.commit("a.txt", "content\n", "Initial commit");

    let result = git_rewrite_commit_author(
        repo.path_str(),
        hash,
        "New Name".into(),
        "new@test.com".into(),
        false,
    )
    .unwrap();

    assert!(!result.new_head.is_empty());
    assert!(result.backup_ref.starts_with("refs/reviser/backups/"));
    assert_eq!(result.rewritten_count, 1); // only the target, no replay needed

    let meta = git_commit_metadata(repo.path_str(), "HEAD".into()).unwrap();
    assert_eq!(meta.author_name, "New Name");
    assert_eq!(meta.author_email, "new@test.com");
}

#[test]
fn test_rewrite_author_on_older_commit_preserves_chain() {
    let repo = TestRepo::new();
    let h1 = repo.commit("a.txt", "a\n", "Commit 1");
    repo.commit("b.txt", "b\n", "Commit 2");
    repo.commit("c.txt", "c\n", "Commit 3");

    let result = git_rewrite_commit_author(
        repo.path_str(),
        h1,
        "Changed".into(),
        "changed@test.com".into(),
        false,
    )
    .unwrap();

    // 1 target + 2 replayed = 3
    assert_eq!(result.rewritten_count, 3);

    // All 3 commits should still exist
    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    assert_eq!(commits.len(), 3);
    assert_eq!(commits[0].message, "Commit 3");
    assert_eq!(commits[1].message, "Commit 2");
    assert_eq!(commits[2].message, "Commit 1");

    // File content preserved
    let content_c =
        git_show_file(repo.path_str(), commits[0].hash.clone(), "c.txt".into()).unwrap();
    assert_eq!(content_c, "c\n");
}

// ═══════════════════════════════════════════════════════════════
// Rewrite date
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rewrite_date_on_head() {
    let repo = TestRepo::new();
    let hash = repo.commit("d.txt", "content\n", "Date test");

    let new_date = "2023-11-14T22:13:20+00:00";
    let result = git_rewrite_commit_date(repo.path_str(), hash, new_date.into(), false).unwrap();

    assert_eq!(result.rewritten_count, 1);

    let meta = git_commit_metadata(repo.path_str(), "HEAD".into()).unwrap();
    assert!(meta.author_date.starts_with("2023-11-14"));
}

// ═══════════════════════════════════════════════════════════════
// Rewrite message
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rewrite_message_on_head() {
    let repo = TestRepo::new();
    let hash = repo.commit("m.txt", "content\n", "Old message");

    let result =
        git_rewrite_commit_message(repo.path_str(), hash, "Brand new message".into(), false)
            .unwrap();

    assert_eq!(result.rewritten_count, 1);

    let commits = git_log(repo.path_str(), 1, 0).unwrap();
    assert_eq!(commits[0].message, "Brand new message");
}

#[test]
fn test_rewrite_message_preserves_author() {
    let repo = TestRepo::new();
    let hash = repo.commit_with_author(
        "m2.txt",
        "content\n",
        "Preserve metadata",
        "Specific Author",
        "specific@test.com",
    );

    git_rewrite_commit_message(repo.path_str(), hash, "Changed message".into(), false).unwrap();

    let meta = git_commit_metadata(repo.path_str(), "HEAD".into()).unwrap();
    assert_eq!(meta.author_name, "Specific Author");
    assert_eq!(meta.author_email, "specific@test.com");
}

#[test]
fn test_rewrite_message_on_older_commit() {
    let repo = TestRepo::new();
    let h1 = repo.commit("a.txt", "a\n", "First");
    repo.commit("b.txt", "b\n", "Second");
    repo.commit("c.txt", "c\n", "Third");

    git_rewrite_commit_message(repo.path_str(), h1, "Rewritten first".into(), false).unwrap();

    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    assert_eq!(commits.len(), 3);
    assert_eq!(commits[2].message, "Rewritten first");
    assert_eq!(commits[1].message, "Second");
    assert_eq!(commits[0].message, "Third");
}

// ═══════════════════════════════════════════════════════════════
// Rewrite root commit
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rewrite_root_commit_message() {
    let repo = TestRepo::new();
    let root = repo.commit("root.txt", "root\n", "Root commit");
    repo.commit("second.txt", "second\n", "Second commit");

    git_rewrite_commit_message(repo.path_str(), root, "Rewritten root".into(), false).unwrap();

    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[1].message, "Rewritten root");
    assert_eq!(commits[0].message, "Second commit");
}

// ═══════════════════════════════════════════════════════════════
// Error: detached HEAD
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rewrite_fails_on_detached_head() {
    let repo = TestRepo::new();
    let hash = repo.commit("dh.txt", "content\n", "Detached test");
    run_git(&repo.path, &["checkout", "--detach", "HEAD"]);

    let result = git_rewrite_commit_message(repo.path_str(), hash, "Should fail".into(), false);
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("Detached HEAD") || err.contains("detached"),
        "error: {err}"
    );
}

// ═══════════════════════════════════════════════════════════════
// Error: dirty tree without auto_stash
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rewrite_fails_on_dirty_tree_without_autostash() {
    let repo = TestRepo::new();
    let hash = repo.commit("e.txt", "content\n", "Error test");
    std::fs::write(repo.path.join("dirty.txt"), "dirty\n").unwrap();

    let result = git_rewrite_commit_message(repo.path_str(), hash, "Should fail".into(), false);
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("uncommitted") || err.contains("dirty"),
        "error: {err}"
    );
}

// ═══════════════════════════════════════════════════════════════
// Auto-stash: dirty tree with auto_stash succeeds
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rewrite_succeeds_with_autostash_on_dirty_tree() {
    let repo = TestRepo::new();
    let hash = repo.commit("s.txt", "original\n", "Stash test");

    // Modify a tracked file
    std::fs::write(repo.path.join("s.txt"), "modified\n").unwrap();

    git_rewrite_commit_message(repo.path_str(), hash, "Updated with stash".into(), true).unwrap();

    let commits = git_log(repo.path_str(), 1, 0).unwrap();
    assert_eq!(commits[0].message, "Updated with stash");

    // Dirty change should be restored
    let clean = git_is_clean(repo.path_str()).unwrap();
    assert!(!clean);
    let content = std::fs::read_to_string(repo.path.join("s.txt")).unwrap();
    assert_eq!(content, "modified\n");
}

// ═══════════════════════════════════════════════════════════════
// Rewrite through merge commits
// ═══════════════════════════════════════════════════════════════

/// Helper to create a repo with a merge commit.
/// Returns (repo, base_hash, merge_hash) where the history is:
///   Base -> Main work -> Merge feature (HEAD)
///                \-> Feature (second parent)
fn create_repo_with_merge() -> (TestRepo, String, String) {
    let repo = TestRepo::new();
    let base = repo.commit("base.txt", "base\n", "Base");

    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("feature.txt", "feature\n", "Feature");
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("main2.txt", "main2\n", "Main work");
    run_git(
        &repo.path,
        &["merge", "feature", "--no-ff", "-m", "Merge feature"],
    );
    let merge_hash = run_git(&repo.path, &["rev-parse", "HEAD"]);

    (repo, base, merge_hash)
}

#[test]
fn test_rewrite_through_merge_commit() {
    let (repo, base, _merge) = create_repo_with_merge();

    // Rewrite the base commit message — chain passes through the merge
    let result = git_rewrite_commit_message(
        repo.path_str(),
        base,
        "Rewritten base".into(),
        false,
    )
    .unwrap();

    // base + Main work + Feature + Merge = 4 rewritten (all descendants of base)
    assert_eq!(result.rewritten_count, 4);

    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    // Should still have all commits (merge shows as one in first-parent log,
    // but git_log uses --all or default traversal)
    assert!(commits.len() >= 3);

    // Find the rewritten base
    let base_commit = commits.iter().find(|c| c.message == "Rewritten base");
    assert!(base_commit.is_some(), "base commit should have new message");

    // Merge commit should still exist
    let merge_commit = commits.iter().find(|c| c.message == "Merge feature");
    assert!(merge_commit.is_some(), "merge commit should be preserved");

    // All files should exist
    let files = git_list_tracked_files(repo.path_str()).unwrap();
    assert!(files.contains(&"base.txt".to_owned()));
    assert!(files.contains(&"main2.txt".to_owned()));
    assert!(files.contains(&"feature.txt".to_owned()));
}

#[test]
fn test_rewrite_merge_commit_message() {
    let (repo, _base, merge) = create_repo_with_merge();

    // Rewrite the merge commit's own message
    let result = git_rewrite_commit_message(
        repo.path_str(),
        merge,
        "Updated merge message".into(),
        false,
    )
    .unwrap();

    assert_eq!(result.rewritten_count, 1);

    let commits = git_log(repo.path_str(), 1, 0).unwrap();
    assert_eq!(commits[0].message, "Updated merge message");

    // All files should still exist
    let files = git_list_tracked_files(repo.path_str()).unwrap();
    assert!(files.contains(&"feature.txt".to_owned()));
    assert!(files.contains(&"main2.txt".to_owned()));
}

#[test]
fn test_rewrite_preserves_merge_parents() {
    let (repo, _base, _merge) = create_repo_with_merge();

    // Rewrite the base commit — all descendants (including Feature) are rewritten
    let base_hash = run_git(&repo.path, &["rev-parse", "HEAD~2"]);
    git_rewrite_commit_message(
        repo.path_str(),
        base_hash,
        "Rewritten base".into(),
        false,
    )
    .unwrap();

    // The new merge commit should still have 2 parents
    let parent_count = run_git(&repo.path, &["rev-list", "--parents", "-1", "HEAD"]);
    let parts: Vec<&str> = parent_count.split_whitespace().collect();
    // Output is: <commit> <parent1> <parent2>
    assert_eq!(parts.len(), 3, "merge should have 2 parents, got: {parent_count}");

    // Second parent (Feature) should also be rewritten — its parent was Base
    // which was modified, so Feature gets a new OID. Verify its content is intact.
    let feature_content = git_show_file(
        repo.path_str(),
        run_git(&repo.path, &["rev-parse", "HEAD^2"]),
        "feature.txt".into(),
    )
    .unwrap();
    assert_eq!(feature_content, "feature\n");

    // Verify no duplicate commits — the rewritten base should appear exactly once
    let base_commits = run_git(
        &repo.path,
        &["log", "--all", "--format=%s", "--grep=Rewritten base"],
    );
    assert_eq!(
        base_commits.lines().count(),
        1,
        "rewritten base should appear exactly once, got: {base_commits}"
    );
}

#[test]
fn test_rewrite_shared_ancestor_no_duplicates() {
    // Key regression test: when the target is a shared ancestor reachable through
    // BOTH parents of a merge, all paths must be rewritten. Otherwise git log
    // shows duplicate commits (old versions still reachable via unrewritten path).
    let (repo, base, _merge) = create_repo_with_merge();

    // Delete feature branch ref so it doesn't keep old commits reachable
    run_git(&repo.path, &["branch", "-D", "feature"]);

    git_rewrite_commit_message(
        repo.path_str(),
        base,
        "Unique base message".into(),
        false,
    )
    .unwrap();

    // Count commits reachable from HEAD (traversing both sides of merge)
    let all_commits = run_git(&repo.path, &["log", "--format=%H %s"]);
    let lines: Vec<&str> = all_commits.lines().collect();

    // Should have exactly 4 commits: Base, Main work, Feature, Merge
    assert_eq!(lines.len(), 4, "expected 4 commits, got:\n{all_commits}");

    // No commit message should appear more than once
    let messages: Vec<&str> = lines.iter().map(|l| l.split_once(' ').unwrap().1).collect();
    let unique: std::collections::HashSet<&&str> = messages.iter().collect();
    assert_eq!(
        messages.len(),
        unique.len(),
        "duplicate commit messages found:\n{all_commits}"
    );
}

#[test]
fn test_rewrite_commit_on_second_parent_side() {
    // This is the key scenario: the target commit is on the feature branch
    // (second parent of merge), NOT on the first-parent chain.
    //
    // History:
    //   Base -- Main work -- Merge (HEAD)
    //     \                  /
    //      Feature ----------
    //
    // Editing "Feature" (only reachable via merge's second parent) must work.
    let (repo, _base, _merge) = create_repo_with_merge();

    let feature_hash = run_git(&repo.path, &["rev-parse", "HEAD^2"]);

    let result = git_rewrite_commit_message(
        repo.path_str(),
        feature_hash,
        "Rewritten feature".into(),
        false,
    )
    .unwrap();

    // Feature + Merge = 2 rewritten (Feature is the target, Merge is reparented)
    assert_eq!(result.rewritten_count, 2);

    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    let feature_commit = commits.iter().find(|c| c.message == "Rewritten feature");
    assert!(feature_commit.is_some(), "feature commit should have new message");

    // All files should exist
    let files = git_list_tracked_files(repo.path_str()).unwrap();
    assert!(files.contains(&"base.txt".to_owned()));
    assert!(files.contains(&"main2.txt".to_owned()));
    assert!(files.contains(&"feature.txt".to_owned()));

    // Merge commit should still have 2 parents
    let parent_count = run_git(&repo.path, &["rev-list", "--parents", "-1", "HEAD"]);
    let parts: Vec<&str> = parent_count.split_whitespace().collect();
    assert_eq!(parts.len(), 3, "merge should still have 2 parents");

    // First parent of merge should be UNCHANGED (Main work wasn't touched)
    let first_parent_after = run_git(&repo.path, &["rev-parse", "HEAD^1"]);
    let main_work_hash = run_git(&repo.path, &["log", "--format=%H", "--all", "--grep=Main work"]);
    assert_eq!(first_parent_after, main_work_hash,
        "first parent of merge should be unchanged Main work commit");
}

#[test]
fn test_fixup_through_merge_commit() {
    let repo = TestRepo::new();
    let h1 = repo.commit("a.txt", "a\n", "Commit 1");
    let h2 = repo.commit("b.txt", "b\n", "Commit 2");

    // Create a merge
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("feature.txt", "feature\n", "Feature");
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("main3.txt", "main3\n", "Commit 3");
    run_git(
        &repo.path,
        &["merge", "feature", "--no-ff", "-m", "Merge feature"],
    );
    repo.commit("d.txt", "d\n", "Commit 5");

    // Fixup h1 and h2 (before the merge)
    let result = git_fixup_commits(repo.path_str(), vec![h1, h2], false).unwrap();
    assert!(result.rewritten_count > 0);

    // All non-squashed files should exist
    let files = git_list_tracked_files(repo.path_str()).unwrap();
    assert!(files.contains(&"a.txt".to_owned()));
    assert!(files.contains(&"b.txt".to_owned()));
    assert!(files.contains(&"main3.txt".to_owned()));
    assert!(files.contains(&"feature.txt".to_owned()));
    assert!(files.contains(&"d.txt".to_owned()));
}

// ═══════════════════════════════════════════════════════════════
// Fixup: squash selected commits
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_fixup_squashes_commits() {
    let repo = TestRepo::new();
    let h1 = repo.commit("a.txt", "a\n", "Commit 1");
    let h2 = repo.commit("b.txt", "b\n", "Commit 2");
    let h3 = repo.commit("c.txt", "c\n", "Commit 3");

    let result = git_fixup_commits(repo.path_str(), vec![h1, h2, h3], false).unwrap();

    assert!(result.backup_ref.starts_with("refs/reviser/backups/"));

    // Should now have 1 commit (all squashed into one)
    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].message, "Commit 1");

    // All files should exist
    let files = git_list_tracked_files(repo.path_str()).unwrap();
    assert!(files.contains(&"a.txt".to_owned()));
    assert!(files.contains(&"b.txt".to_owned()));
    assert!(files.contains(&"c.txt".to_owned()));
}

#[test]
fn test_fixup_preserves_non_selected_commits() {
    let repo = TestRepo::new();
    let h1 = repo.commit("a.txt", "a\n", "Commit 1");
    repo.commit("b.txt", "b\n", "Commit 2 (keep)");
    let h3 = repo.commit("c.txt", "c\n", "Commit 3");

    // Only squash h1 and h3
    let result = git_fixup_commits(repo.path_str(), vec![h1, h3], false).unwrap();
    assert!(result.rewritten_count > 0);

    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    assert_eq!(commits.len(), 2);

    // The kept commit should still be there
    let messages: Vec<&str> = commits.iter().map(|c| c.message.as_str()).collect();
    assert!(messages.contains(&"Commit 2 (keep)"));
}

#[test]
fn test_fixup_empty_array_is_noop() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "Initial");
    let before_head = git_rev_parse(repo.path_str(), "HEAD".into()).unwrap();

    let result = git_fixup_commits(repo.path_str(), vec![], false).unwrap();

    assert!(result.new_head.is_empty());
    assert!(result.backup_ref.is_empty());
    assert_eq!(result.rewritten_count, 0);

    let after_head = git_rev_parse(repo.path_str(), "HEAD".into()).unwrap();
    assert_eq!(before_head, after_head);
}

// ═══════════════════════════════════════════════════════════════
// Drop: delete selected commits
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_drop_commits_removes_selected() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "Commit 1");
    let h2 = repo.commit("b.txt", "b\n", "Commit 2");
    repo.commit("c.txt", "c\n", "Commit 3");

    let result = git_drop_commits(repo.path_str(), vec![h2], false).unwrap();
    assert!(result.backup_ref.starts_with("refs/reviser/backups/"));

    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    assert_eq!(commits.len(), 2);
    let messages: Vec<&str> = commits.iter().map(|c| c.message.as_str()).collect();
    assert!(messages.contains(&"Commit 1"));
    assert!(messages.contains(&"Commit 3"));
    assert!(!messages.contains(&"Commit 2"));

    // Dropped commit's file should not exist
    let files = git_list_tracked_files(repo.path_str()).unwrap();
    assert!(files.contains(&"a.txt".to_owned()));
    assert!(!files.contains(&"b.txt".to_owned()));
    assert!(files.contains(&"c.txt".to_owned()));
}

#[test]
fn test_drop_multiple_commits() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "Commit 1");
    let h2 = repo.commit("b.txt", "b\n", "Commit 2");
    let h3 = repo.commit("c.txt", "c\n", "Commit 3");
    repo.commit("d.txt", "d\n", "Commit 4");

    git_drop_commits(repo.path_str(), vec![h2, h3], false).unwrap();

    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    assert_eq!(commits.len(), 2);
    let messages: Vec<&str> = commits.iter().map(|c| c.message.as_str()).collect();
    assert!(messages.contains(&"Commit 1"));
    assert!(messages.contains(&"Commit 4"));
}

#[test]
fn test_drop_empty_array_is_noop() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "Initial");
    let before_head = git_rev_parse(repo.path_str(), "HEAD".into()).unwrap();

    let result = git_drop_commits(repo.path_str(), vec![], false).unwrap();
    assert!(result.new_head.is_empty());
    assert_eq!(result.rewritten_count, 0);

    let after_head = git_rev_parse(repo.path_str(), "HEAD".into()).unwrap();
    assert_eq!(before_head, after_head);
}

// ═══════════════════════════════════════════════════════════════
// Backup ref created and restorable
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_backup_ref_created_and_restorable() {
    let repo = TestRepo::new();
    let hash = repo.commit("bk.txt", "content\n", "Before rewrite");
    let original_head = git_rev_parse(repo.path_str(), "HEAD".into()).unwrap();

    let result =
        git_rewrite_commit_message(repo.path_str(), hash, "After rewrite".into(), false).unwrap();

    // Backup ref should point to original HEAD
    let backup_target = git_rev_parse(repo.path_str(), result.backup_ref.clone()).unwrap();
    assert_eq!(backup_target, original_head);

    // Restore from backup
    git_reset_hard(repo.path_str(), result.backup_ref).unwrap();
    let restored_head = git_rev_parse(repo.path_str(), "HEAD".into()).unwrap();
    assert_eq!(restored_head, original_head);
}

// ═══════════════════════════════════════════════════════════════
// Prune backup refs
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_prune_backup_refs() {
    let repo = TestRepo::new();
    let hash = repo.commit("file.txt", "content\n", "Initial");

    // Create a backup ref
    git_rewrite_commit_message(repo.path_str(), hash, "Rewritten".into(), false).unwrap();

    // With retention_days=0, nothing happens
    let pruned = git_prune_backup_refs(repo.path_str(), 0).unwrap();
    assert_eq!(pruned, 0);

    // With high retention, nothing pruned (ref is too new)
    let pruned = git_prune_backup_refs(repo.path_str(), 365).unwrap();
    assert_eq!(pruned, 0);
}

// ═══════════════════════════════════════════════════════════════
// Commit metadata extraction
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_commit_metadata_extraction() {
    let repo = TestRepo::new();
    repo.commit_with_author(
        "meta.txt",
        "content\n",
        "Metadata test",
        "Meta Author",
        "meta@test.com",
    );

    let meta = git_commit_metadata(repo.path_str(), "HEAD".into()).unwrap();
    assert_eq!(meta.author_name, "Meta Author");
    assert_eq!(meta.author_email, "meta@test.com");
    // Committer is set by env vars in run_git
    assert_eq!(meta.committer_name, "Test Author");
    assert_eq!(meta.committer_email, "test@example.com");
    assert!(!meta.author_date.is_empty());
    assert!(!meta.committer_date.is_empty());
}

// ═══════════════════════════════════════════════════════════════
// Commit not in chain
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rewrite_commit_not_in_chain() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "Main commit");

    // Create a commit on another branch
    run_git(&repo.path, &["checkout", "-b", "other"]);
    let other_hash = repo.commit("other.txt", "other\n", "Other branch");
    run_git(&repo.path, &["checkout", "main"]);

    let result =
        git_rewrite_commit_message(repo.path_str(), other_hash, "Should fail".into(), false);
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("not found in") || err.contains("not in"),
        "error: {err}"
    );
}

// ═══════════════════════════════════════════════════════════════
// Operation in progress
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rewrite_fails_when_operation_in_progress() {
    let repo = TestRepo::new();
    let hash = repo.commit("op.txt", "content\n", "Test");

    // Simulate an operation in progress
    let rebase_dir = repo.path.join(".git/rebase-merge");
    std::fs::create_dir_all(&rebase_dir).unwrap();

    let result = git_rewrite_commit_message(repo.path_str(), hash, "Should fail".into(), false);
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("already in progress") || err.contains("operation"),
        "error: {err}"
    );

    // Cleanup
    std::fs::remove_dir_all(&rebase_dir).unwrap();
}

// ═══════════════════════════════════════════════════════════════
// Committer signatures preserved on replayed commits
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_replayed_commits_preserve_signatures() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "Commit 1");
    let h2 = repo.commit_with_author("b.txt", "b\n", "Commit 2", "Other Author", "other@test.com");

    // Get committer info before rewrite
    let meta_before = git_commit_metadata(repo.path_str(), h2.clone()).unwrap();

    // Rewrite commit 1 — commit 2 should be replayed preserving its signatures
    let first = git_log(repo.path_str(), 200, 0)
        .unwrap()
        .last()
        .unwrap()
        .hash
        .clone();
    git_rewrite_commit_message(repo.path_str(), first, "Rewritten first".into(), false).unwrap();

    // Verify the replayed commit 2 preserved its author
    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    let replayed_h2 = &commits[0]; // HEAD is "Commit 2"
    assert_eq!(replayed_h2.author_name, "Other Author");
    assert_eq!(replayed_h2.author_email, "other@test.com");

    let meta_after = git_commit_metadata(repo.path_str(), replayed_h2.hash.clone()).unwrap();
    assert_eq!(meta_after.committer_name, meta_before.committer_name);
    assert_eq!(meta_after.committer_email, meta_before.committer_email);
}

// ═══════════════════════════════════════════════════════════════
// Worktree support
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rewrite_commit_via_worktree_path() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "Base commit");

    // Create a branch and add a commit
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    let hash = repo.commit("b.txt", "b\n", "Feature commit");
    run_git(&repo.path, &["checkout", "main"]);

    // Create a worktree for the feature branch
    let wt_dir = TempDir::new().expect("create worktree dir");
    let wt_path = wt_dir.path().to_path_buf();
    run_git(
        &repo.path,
        &["worktree", "add", wt_path.to_str().unwrap(), "feature"],
    );

    // Rewrite the commit using the worktree path (not the main repo path).
    // This is the scenario that was broken: Cuttlefish viewing a worktree's
    // commits and editing one should pass the worktree path so that git2
    // resolves the correct HEAD.
    let result = git_rewrite_commit_message(
        wt_path.to_str().unwrap().to_owned(),
        hash,
        "Rewritten via worktree".into(),
        false,
    );
    assert!(result.is_ok(), "rewrite via worktree failed: {:?}", result.err());

    // Verify the commit was rewritten
    let commits = git_log(wt_path.to_str().unwrap().to_owned(), 200, 0).unwrap();
    assert_eq!(commits[0].message, "Rewritten via worktree");

    // Clean up worktree
    run_git(&repo.path, &["worktree", "remove", wt_path.to_str().unwrap()]);
}

#[test]
fn test_rewrite_older_commit_via_worktree_path() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "Base commit");

    // Create a branch with multiple commits
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    let first = repo.commit("b.txt", "b\n", "First feature commit");
    repo.commit("c.txt", "c\n", "Second feature commit");
    run_git(&repo.path, &["checkout", "main"]);

    // Create a worktree
    let wt_dir = TempDir::new().expect("create worktree dir");
    let wt_path = wt_dir.path().to_path_buf();
    run_git(
        &repo.path,
        &["worktree", "add", wt_path.to_str().unwrap(), "feature"],
    );

    // Rewrite an older commit via the worktree — should rewrite the chain
    let result = git_rewrite_commit_message(
        wt_path.to_str().unwrap().to_owned(),
        first,
        "Rewritten older commit".into(),
        false,
    );
    assert!(result.is_ok(), "rewrite via worktree failed: {:?}", result.err());

    let commits = git_log(wt_path.to_str().unwrap().to_owned(), 200, 0).unwrap();
    assert_eq!(commits[1].message, "Rewritten older commit");
    assert_eq!(commits[0].message, "Second feature commit");

    run_git(&repo.path, &["worktree", "remove", wt_path.to_str().unwrap()]);
}

// ---------------------------------------------------------------------------
// Cherry-pick tests
// ---------------------------------------------------------------------------

#[test]
fn test_cherry_pick_basic() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a", "Initial commit");
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    let feature_hash = repo.commit("b.txt", "b", "Feature commit");
    run_git(&repo.path, &["checkout", "main"]);

    let new_hash = git_cherry_pick(repo.path_str(), feature_hash, false).unwrap();
    assert!(!new_hash.is_empty());

    let commits = git_log(repo.path_str(), 10, 0).unwrap();
    assert_eq!(commits[0].message, "Feature commit");
}

#[test]
fn test_cherry_pick_empty_is_rejected() {
    let repo = TestRepo::new();
    let hash = repo.commit("a.txt", "a", "Initial commit");

    // Cherry-picking the same commit onto itself produces no changes
    let result = git_cherry_pick(repo.path_str(), hash, false);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("already applied"), "unexpected error: {err}");
}

#[test]
fn test_cherry_pick_to_branch() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a", "Initial commit");
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    let feature_hash = repo.commit("b.txt", "b", "Feature work");
    run_git(&repo.path, &["checkout", "main"]);

    // Cherry-pick the feature commit onto main (HEAD branch via target_branch)
    let new_hash =
        git_cherry_pick_to_branch(repo.path_str(), feature_hash, "main".into(), false).unwrap();
    assert!(!new_hash.is_empty());

    let commits = git_log(repo.path_str(), 10, 0).unwrap();
    assert_eq!(commits[0].message, "Feature work");
}

#[test]
fn test_cherry_pick_to_non_head_branch() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a", "Initial on main");
    run_git(&repo.path, &["checkout", "-b", "target"]);
    repo.commit("t.txt", "t", "Target branch commit");
    run_git(&repo.path, &["checkout", "main"]);
    run_git(&repo.path, &["checkout", "-b", "source"]);
    let source_hash = repo.commit("s.txt", "s", "Source commit");
    // Stay on source branch, cherry-pick onto target (non-HEAD)
    let new_hash =
        git_cherry_pick_to_branch(repo.path_str(), source_hash, "target".into(), false).unwrap();
    assert!(!new_hash.is_empty());

    // Verify target branch was updated
    run_git(&repo.path, &["checkout", "target"]);
    let commits = git_log(repo.path_str(), 10, 0).unwrap();
    assert_eq!(commits[0].message, "Source commit");
}

#[test]
fn test_cherry_pick_to_branch_empty_rejected() {
    let repo = TestRepo::new();
    let hash = repo.commit("a.txt", "a", "Initial commit");
    run_git(&repo.path, &["checkout", "-b", "other"]);
    // other branch is at same commit as main
    let result = git_cherry_pick_to_branch(repo.path_str(), hash, "other".into(), false);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("already applied"), "unexpected error: {err}");
}
