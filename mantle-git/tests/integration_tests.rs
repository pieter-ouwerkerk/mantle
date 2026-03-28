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

    /// Create a file, stage it, and commit. Returns the commit hash.
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

    fn create_branch(&self, name: &str) {
        run_git(&self.path, &["branch", name]);
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
// Repository validation
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_is_valid_repo() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    assert!(git_is_valid_repo(repo.path_str()));
}

#[test]
fn test_is_valid_repo_false_for_non_repo() {
    let dir = TempDir::new().unwrap();
    assert!(!git_is_valid_repo(dir.path().to_str().unwrap().to_owned()));
}

#[test]
fn test_is_valid_repo_false_for_missing_path() {
    assert!(!git_is_valid_repo(
        "/tmp/does-not-exist-cuttlefish".to_owned()
    ));
}

// ═══════════════════════════════════════════════════════════════
// Log operations
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_log_single_commit() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].message, "Initial commit");
    assert_eq!(commits[0].author_name, "Test Author");
    assert_eq!(commits[0].author_email, "test@example.com");
}

#[test]
fn test_log_newest_first() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "First");
    repo.commit("b.txt", "b\n", "Second");
    repo.commit("c.txt", "c\n", "Third");

    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    assert_eq!(commits.len(), 3);
    assert_eq!(commits[0].message, "Third");
    assert_eq!(commits[1].message, "Second");
    assert_eq!(commits[2].message, "First");
}

#[test]
fn test_log_max_count() {
    let repo = TestRepo::new();
    for i in 1..=5 {
        repo.commit(
            &format!("f{i}.txt"),
            &format!("{i}\n"),
            &format!("Commit {i}"),
        );
    }

    let commits = git_log(repo.path_str(), 3, 0).unwrap();
    assert_eq!(commits.len(), 3);
}

#[test]
fn test_log_empty_repo() {
    let repo = TestRepo::new();
    // No commits yet
    let commits = git_log(repo.path_str(), 200, 0).unwrap();
    assert!(commits.is_empty());
}

#[test]
fn test_log_with_custom_author() {
    let repo = TestRepo::new();
    repo.commit_with_author(
        "file.txt",
        "content\n",
        "Custom author commit",
        "Jane Smith",
        "jane@example.com",
    );

    let commits = git_log(repo.path_str(), 1, 0).unwrap();
    assert_eq!(commits[0].author_name, "Jane Smith");
    assert_eq!(commits[0].author_email, "jane@example.com");
}

#[test]
fn test_log_hash_is_40_hex_chars() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let commits = git_log(repo.path_str(), 1, 0).unwrap();
    assert_eq!(commits[0].hash.len(), 40);
    assert!(commits[0].hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_log_date_is_iso8601() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let commits = git_log(repo.path_str(), 1, 0).unwrap();
    // ISO 8601 / RFC 3339 format: YYYY-MM-DDTHH:MM:SS+00:00
    assert!(commits[0].author_date.contains('T'));
    assert!(
        commits[0].author_date.contains('+') || commits[0].author_date.contains('-'),
        "date should contain timezone offset: {}",
        commits[0].author_date
    );
}

// ═══════════════════════════════════════════════════════════════
// Full message
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_full_message() {
    let repo = TestRepo::new();
    // git commit -m doesn't support multi-line easily, so use commit-tree approach
    let hash = repo.commit("file.txt", "content\n", "Subject line");

    // The message should contain the subject
    let msg = git_full_message(repo.path_str(), hash).unwrap();
    assert!(msg.contains("Subject line"));
}

#[test]
fn test_full_message_invalid_hash() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let result = git_full_message(repo.path_str(), "deadbeefdeadbeefdeadbeef".to_owned());
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════
// Recent commits for context
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_recent_commits_for_context() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "First commit");
    repo.commit("b.txt", "b\n", "Second commit");

    let context = git_recent_commits_for_context(repo.path_str(), 5).unwrap();
    assert!(context.contains("First commit"));
    assert!(context.contains("Second commit"));
    // Each line should start with a short hash
    for line in context.lines() {
        assert!(line.len() >= 8); // at least "abcdef0 X"
    }
}

// ═══════════════════════════════════════════════════════════════
// Log for path
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_log_for_path() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let commits = git_log_for_path(repo.path_str(), 200, 0).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].message, "Initial commit");
}

// ═══════════════════════════════════════════════════════════════
// Log by file (pattern matching)
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_log_by_file() {
    let repo = TestRepo::new();
    repo.commit("src/main.rs", "fn main() {}\n", "Add main");
    repo.commit("readme.md", "# Readme\n", "Add readme");
    repo.commit("src/lib.rs", "pub fn lib() {}\n", "Add lib");

    let commits = git_log_by_file(repo.path_str(), ".rs".to_owned(), 50, 0).unwrap();
    // Should find commits touching .rs files
    assert!(commits.len() >= 2);
    let messages: Vec<&str> = commits.iter().map(|c| c.message.as_str()).collect();
    assert!(messages.contains(&"Add main"));
    assert!(messages.contains(&"Add lib"));
}

// ═══════════════════════════════════════════════════════════════
// Log for specific paths
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_log_for_paths() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "Add a");
    repo.commit("b.txt", "b\n", "Add b");
    repo.commit("c.txt", "c\n", "Add c");

    let commits = git_log_for_paths(repo.path_str(), vec!["a.txt".to_owned()], 50, 0).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].message, "Add a");
}

// ═══════════════════════════════════════════════════════════════
// Branch operations
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_current_branch() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let branch = git_current_branch(repo.path_str()).unwrap();
    assert_eq!(branch, "main");
}

#[test]
fn test_list_local_branches() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");
    repo.create_branch("feature-a");
    repo.create_branch("feature-b");

    let branches = git_list_local_branches(repo.path_str()).unwrap();
    let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"main"));
    assert!(names.contains(&"feature-a"));
    assert!(names.contains(&"feature-b"));
}

#[test]
fn test_list_local_branches_has_dates() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let branches = git_list_local_branches(repo.path_str()).unwrap();
    assert!(!branches.is_empty());
    // Date should be non-empty ISO 8601
    assert!(!branches[0].date.is_empty());
    assert!(branches[0].date.contains('T'));
}

#[test]
fn test_verify_branch_exists() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");
    repo.create_branch("my-branch");

    assert!(git_verify_branch_exists(repo.path_str(), "my-branch".to_owned()).unwrap());
    assert!(!git_verify_branch_exists(repo.path_str(), "no-such-branch".to_owned()).unwrap());
}

// ═══════════════════════════════════════════════════════════════
// Config operations
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_config_user_name() {
    let repo = TestRepo::new();
    let name = git_config_user_name(repo.path_str()).unwrap();
    assert_eq!(name, "Test Author");
}

#[test]
fn test_config_user_email() {
    let repo = TestRepo::new();
    let email = git_config_user_email(repo.path_str()).unwrap();
    assert_eq!(email, "test@example.com");
}

// ═══════════════════════════════════════════════════════════════
// Rev-parse operations
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rev_parse_head() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let hash = git_rev_parse(repo.path_str(), "HEAD".to_owned()).unwrap();
    assert_eq!(hash.len(), 40);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_rev_parse_invalid() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let result = git_rev_parse(repo.path_str(), "does-not-exist".to_owned());
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════
// Rev-list parents
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_rev_list_parents_non_merge() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "First");
    let child_hash = repo.commit("b.txt", "b\n", "Second");

    let parents = git_rev_list_parents(repo.path_str(), child_hash.clone()).unwrap();
    assert_eq!(parents.len(), 2); // [commit_hash, parent_hash]
    assert_eq!(parents[0], child_hash);
}

#[test]
fn test_rev_list_parents_root_commit() {
    let repo = TestRepo::new();
    let root_hash = repo.commit("file.txt", "content\n", "Initial commit");

    let parents = git_rev_list_parents(repo.path_str(), root_hash.clone()).unwrap();
    assert_eq!(parents.len(), 1); // root commit has no parents
    assert_eq!(parents[0], root_hash);
}

#[test]
fn test_rev_list_parents_invalid_hash() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let result = git_rev_list_parents(repo.path_str(), "deadbeefdeadbeef".to_owned());
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════
// is_clean
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_is_clean_on_clean_repo() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    assert!(git_is_clean(repo.path_str()).unwrap());
}

#[test]
fn test_is_clean_false_on_modified_file() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");
    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();

    assert!(!git_is_clean(repo.path_str()).unwrap());
}

#[test]
fn test_is_clean_false_with_untracked() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");
    std::fs::write(repo.path.join("new.txt"), "untracked\n").unwrap();

    assert!(!git_is_clean(repo.path_str()).unwrap());
}

#[test]
fn test_is_clean_false_with_staged() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");
    std::fs::write(repo.path.join("staged.txt"), "new\n").unwrap();
    run_git(&repo.path, &["add", "staged.txt"]);

    assert!(!git_is_clean(repo.path_str()).unwrap());
}

// ═══════════════════════════════════════════════════════════════
// status_summary
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_status_summary_clean_repo() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let summary = git_status_summary(repo.path_str()).unwrap();
    assert_eq!(summary.file_count, 0);
    assert!(summary.output.is_empty());
}

#[test]
fn test_status_summary_counts_files() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");
    std::fs::write(repo.path.join("a.txt"), "a\n").unwrap();
    std::fs::write(repo.path.join("b.txt"), "b\n").unwrap();

    let summary = git_status_summary(repo.path_str()).unwrap();
    assert_eq!(summary.file_count, 2);
    assert!(!summary.output.is_empty());
}

// ═══════════════════════════════════════════════════════════════
// list_tracked_files
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_list_tracked_files() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "Add a");
    repo.commit("b.txt", "b\n", "Add b");
    std::fs::write(repo.path.join("untracked.txt"), "u\n").unwrap();

    let files = git_list_tracked_files(repo.path_str()).unwrap();
    assert!(files.contains(&"a.txt".to_owned()));
    assert!(files.contains(&"b.txt".to_owned()));
    assert!(!files.contains(&"untracked.txt".to_owned()));
}

#[test]
fn test_list_tracked_files_sorted() {
    let repo = TestRepo::new();
    repo.commit("z.txt", "z\n", "Add z");
    repo.commit("a.txt", "a\n", "Add a");
    repo.commit("m.txt", "m\n", "Add m");

    let files = git_list_tracked_files(repo.path_str()).unwrap();
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted);
}

// ═══════════════════════════════════════════════════════════════
// list_untracked_files
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_list_untracked_files() {
    let repo = TestRepo::new();
    repo.commit("tracked.txt", "t\n", "Initial");
    std::fs::write(repo.path.join("untracked.txt"), "u\n").unwrap();

    let files = git_list_untracked_files(repo.path_str()).unwrap();
    assert!(files.contains(&"untracked.txt".to_owned()));
    assert!(!files.contains(&"tracked.txt".to_owned()));
}

#[test]
fn test_list_untracked_files_respects_gitignore() {
    let repo = TestRepo::new();
    repo.commit(".gitignore", "ignored.txt\n", "Add gitignore");
    std::fs::write(repo.path.join("ignored.txt"), "x\n").unwrap();
    std::fs::write(repo.path.join("visible.txt"), "y\n").unwrap();

    let files = git_list_untracked_files(repo.path_str()).unwrap();
    assert!(files.contains(&"visible.txt".to_owned()));
    assert!(!files.contains(&"ignored.txt".to_owned()));
}

// ═══════════════════════════════════════════════════════════════
// changed_paths
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_changed_paths_unstaged() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial");
    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("new.txt"), "new\n").unwrap();

    let paths = git_changed_paths(repo.path_str()).unwrap();
    assert!(paths.contains(&"file.txt".to_owned()));
    assert!(paths.contains(&"new.txt".to_owned()));
}

#[test]
fn test_changed_paths_includes_staged() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial");
    std::fs::write(repo.path.join("staged.txt"), "s\n").unwrap();
    run_git(&repo.path, &["add", "staged.txt"]);

    let paths = git_changed_paths(repo.path_str()).unwrap();
    assert!(paths.contains(&"staged.txt".to_owned()));
}

#[test]
fn test_changed_paths_clean_repo() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial");

    let paths = git_changed_paths(repo.path_str()).unwrap();
    assert!(paths.is_empty());
}

// ═══════════════════════════════════════════════════════════════
// worktree_status
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_worktree_status_clean() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial");

    let status = git_worktree_status(repo.path_str()).unwrap();
    assert!(!status.is_dirty);
    assert_eq!(status.file_count, 0);
}

#[test]
fn test_worktree_status_dirty() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial");
    std::fs::write(repo.path.join("file.txt"), "dirty\n").unwrap();

    let status = git_worktree_status(repo.path_str()).unwrap();
    assert!(status.is_dirty);
    assert_eq!(status.file_count, 1);
}

// ═══════════════════════════════════════════════════════════════
// show_diff
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_show_diff_modification() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "hello\n", "Initial");
    let hash = repo.commit("file.txt", "hello world\n", "Modify file");

    let diff = git_show_diff(repo.path_str(), hash).unwrap();
    assert!(diff.contains("diff --git a/file.txt b/file.txt"));
    assert!(diff.contains("-hello"));
    assert!(diff.contains("+hello world"));
}

#[test]
fn test_show_diff_new_file() {
    let repo = TestRepo::new();
    let hash = repo.commit("file.txt", "content\n", "Initial commit");

    let diff = git_show_diff(repo.path_str(), hash).unwrap();
    assert!(diff.contains("diff --git a/file.txt b/file.txt"));
    assert!(diff.contains("new file mode"));
    assert!(diff.contains("--- /dev/null"));
    assert!(diff.contains("+content"));
}

#[test]
fn test_show_diff_deleted_file() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Add file");
    std::fs::remove_file(repo.path.join("file.txt")).unwrap();
    run_git(&repo.path, &["add", "-A"]);
    run_git(&repo.path, &["commit", "-m", "Delete file"]);
    let hash = run_git(&repo.path, &["rev-parse", "HEAD"]);

    let diff = git_show_diff(repo.path_str(), hash).unwrap();
    assert!(diff.contains("diff --git a/file.txt b/file.txt"));
    assert!(diff.contains("deleted file mode"));
    assert!(diff.contains("+++ /dev/null"));
}

#[test]
fn test_show_diff_root_commit() {
    let repo = TestRepo::new();
    let hash = repo.commit("a.txt", "aaa\n", "Root");

    let diff = git_show_diff(repo.path_str(), hash).unwrap();
    assert!(diff.contains("diff --git a/a.txt b/a.txt"));
    assert!(diff.contains("+aaa"));
}

#[test]
fn test_show_diff_binary() {
    let repo = TestRepo::new();
    // Write a file containing a null byte
    let binary_content = b"hello\x00world";
    std::fs::write(repo.path.join("bin.dat"), binary_content).unwrap();
    run_git(&repo.path, &["add", "bin.dat"]);
    run_git(&repo.path, &["commit", "-m", "Add binary"]);
    let hash = run_git(&repo.path, &["rev-parse", "HEAD"]);

    let diff = git_show_diff(repo.path_str(), hash).unwrap();
    assert!(diff.contains("Binary files"));
}

// ═══════════════════════════════════════════════════════════════
// working_tree_diff
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_working_tree_diff_modified() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "original\n", "Initial");
    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();

    let diff = git_working_tree_diff(repo.path_str()).unwrap();
    assert!(diff.contains("diff --git a/file.txt b/file.txt"));
    assert!(diff.contains("-original"));
    assert!(diff.contains("+modified"));
}

#[test]
fn test_working_tree_diff_untracked() {
    let repo = TestRepo::new();
    repo.commit("tracked.txt", "t\n", "Initial");
    std::fs::write(repo.path.join("new.txt"), "untracked content\n").unwrap();

    let diff = git_working_tree_diff(repo.path_str()).unwrap();
    assert!(diff.contains("diff --git a/new.txt b/new.txt"));
    assert!(diff.contains("new file mode"));
    assert!(diff.contains("+untracked content"));
}

#[test]
fn test_working_tree_diff_clean() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial");

    let diff = git_working_tree_diff(repo.path_str()).unwrap();
    assert!(diff.is_empty());
}

// ═══════════════════════════════════════════════════════════════
// working_tree_diff_for_context
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_working_tree_diff_for_context_sections() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "original\n", "Initial");

    // Staged change
    std::fs::write(repo.path.join("staged.txt"), "staged content\n").unwrap();
    run_git(&repo.path, &["add", "staged.txt"]);

    // Unstaged change
    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();

    // Untracked file
    std::fs::write(repo.path.join("untracked.txt"), "u\n").unwrap();

    let diff = git_working_tree_diff_for_context(repo.path_str()).unwrap();
    assert!(
        diff.contains("=== Staged Changes ==="),
        "missing staged section: {diff}"
    );
    assert!(
        diff.contains("=== Unstaged Changes ==="),
        "missing unstaged section: {diff}"
    );
    assert!(
        diff.contains("=== Untracked Files ==="),
        "missing untracked section: {diff}"
    );
    assert!(diff.contains("untracked.txt"));
}

// ═══════════════════════════════════════════════════════════════
// diff_between_refs
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_diff_between_refs() {
    let repo = TestRepo::new();
    repo.commit("base.txt", "base\n", "Base commit");

    // Create a feature branch and add a file
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("feature.txt", "feature content\n", "Feature commit");

    // Go back to main and add a different file
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("main.txt", "main content\n", "Main commit");

    // diff main...feature should show only the feature-side changes
    let diff =
        git_diff_between_refs(repo.path_str(), "main".to_owned(), "feature".to_owned()).unwrap();
    assert!(
        diff.contains("feature.txt"),
        "should contain feature.txt: {diff}"
    );
    assert!(
        !diff.contains("main.txt"),
        "should not contain main.txt: {diff}"
    );
}

// ═══════════════════════════════════════════════════════════════
// Write operations: checkout
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_checkout() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");
    repo.create_branch("feature");

    git_checkout(repo.path_str(), "feature".to_owned()).unwrap();
    let branch = git_current_branch(repo.path_str()).unwrap();
    assert_eq!(branch, "feature");
}

#[test]
fn test_checkout_nonexistent_branch() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let result = git_checkout(repo.path_str(), "nonexistent".to_owned());
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════
// Write operations: branch_delete
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_branch_delete() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");
    repo.create_branch("to-delete");

    assert!(git_verify_branch_exists(repo.path_str(), "to-delete".to_owned()).unwrap());
    git_branch_delete(repo.path_str(), "to-delete".to_owned()).unwrap();
    assert!(!git_verify_branch_exists(repo.path_str(), "to-delete".to_owned()).unwrap());
}

// ═══════════════════════════════════════════════════════════════
// Write operations: create_branch_at
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_create_branch_at() {
    let repo = TestRepo::new();
    let first_hash = repo.commit("file.txt", "content\n", "First");
    repo.commit("file2.txt", "content2\n", "Second");

    git_create_branch_at(repo.path_str(), "at-first".to_owned(), first_hash.clone()).unwrap();

    assert!(git_verify_branch_exists(repo.path_str(), "at-first".to_owned()).unwrap());

    // Verify the branch points at the first commit
    let branch_hash = git_rev_parse(repo.path_str(), "at-first".to_owned()).unwrap();
    assert_eq!(branch_hash, first_hash);
}

// ═══════════════════════════════════════════════════════════════
// Write operations: update_ref / delete_ref
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_update_ref() {
    let repo = TestRepo::new();
    let hash = repo.commit("file.txt", "content\n", "Initial commit");

    git_update_ref(repo.path_str(), "refs/custom/test".to_owned(), hash.clone()).unwrap();

    let resolved = git_rev_parse(repo.path_str(), "refs/custom/test".to_owned()).unwrap();
    assert_eq!(resolved, hash);
}

#[test]
fn test_delete_ref() {
    let repo = TestRepo::new();
    let hash = repo.commit("file.txt", "content\n", "Initial commit");

    git_update_ref(repo.path_str(), "refs/custom/test".to_owned(), hash).unwrap();

    git_delete_ref(repo.path_str(), "refs/custom/test".to_owned()).unwrap();

    let result = git_rev_parse(repo.path_str(), "refs/custom/test".to_owned());
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════
// Write operations: reset_hard
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_reset_hard() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "original\n", "Initial commit");

    // Modify the file in working tree
    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();
    assert!(!git_is_clean(repo.path_str()).unwrap());

    git_reset_hard(repo.path_str(), "HEAD".to_owned()).unwrap();
    assert!(git_is_clean(repo.path_str()).unwrap());

    // Verify file was restored
    let content = std::fs::read_to_string(repo.path.join("file.txt")).unwrap();
    assert_eq!(content, "original\n");
}

// ═══════════════════════════════════════════════════════════════
// Write operations: stash
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_stash_push_pop() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "original\n", "Initial commit");

    // Dirty the working tree
    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();
    assert!(!git_is_clean(repo.path_str()).unwrap());

    // Stash
    git_stash_push(repo.path_str(), "test stash".to_owned()).unwrap();
    assert!(git_is_clean(repo.path_str()).unwrap());

    // Pop
    git_stash_pop(repo.path_str()).unwrap();
    assert!(!git_is_clean(repo.path_str()).unwrap());

    let content = std::fs::read_to_string(repo.path.join("file.txt")).unwrap();
    assert_eq!(content, "modified\n");
}

#[test]
fn test_stash_push_includes_untracked() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    // Create an untracked file
    std::fs::write(repo.path.join("untracked.txt"), "untracked\n").unwrap();
    assert!(!git_is_clean(repo.path_str()).unwrap());

    // Stash with untracked
    git_stash_push(repo.path_str(), "stash untracked".to_owned()).unwrap();
    assert!(git_is_clean(repo.path_str()).unwrap());
    assert!(!repo.path.join("untracked.txt").exists());

    // Pop brings it back
    git_stash_pop(repo.path_str()).unwrap();
    assert!(repo.path.join("untracked.txt").exists());
}

#[test]
fn test_stash_list_empty() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let stashes = git_stash_list(repo.path_str()).unwrap();
    assert!(stashes.is_empty());
}

#[test]
fn test_stash_list_after_push() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    // Dirty the working tree and stash
    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();
    git_stash_push(repo.path_str(), "my test stash".to_owned()).unwrap();

    let stashes = git_stash_list(repo.path_str()).unwrap();
    assert_eq!(stashes.len(), 1);
    assert_eq!(stashes[0].index, 0);
    assert!(stashes[0].message.contains("my test stash"));
    assert!(!stashes[0].commit_hash.is_empty());
}

#[test]
fn test_stash_apply() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "original\n", "Initial commit");

    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();
    git_stash_push(repo.path_str(), "apply test".to_owned()).unwrap();
    assert!(git_is_clean(repo.path_str()).unwrap());

    // Apply restores changes but keeps stash in list
    git_stash_apply(repo.path_str(), 0).unwrap();
    let content = std::fs::read_to_string(repo.path.join("file.txt")).unwrap();
    assert_eq!(content, "modified\n");

    let stashes = git_stash_list(repo.path_str()).unwrap();
    assert_eq!(stashes.len(), 1);
}

#[test]
fn test_stash_drop() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "original\n", "Initial commit");

    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();
    git_stash_push(repo.path_str(), "drop test".to_owned()).unwrap();

    // Drop removes the stash without applying
    git_stash_drop(repo.path_str(), 0).unwrap();
    assert!(git_is_clean(repo.path_str()).unwrap());

    let stashes = git_stash_list(repo.path_str()).unwrap();
    assert!(stashes.is_empty());
}

#[test]
fn test_stash_apply_then_drop() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "original\n", "Initial commit");

    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();
    git_stash_push(repo.path_str(), "apply-then-drop".to_owned()).unwrap();

    // Apply first
    git_stash_apply(repo.path_str(), 0).unwrap();
    let content = std::fs::read_to_string(repo.path.join("file.txt")).unwrap();
    assert_eq!(content, "modified\n");

    // Then drop
    git_stash_drop(repo.path_str(), 0).unwrap();
    let stashes = git_stash_list(repo.path_str()).unwrap();
    assert!(stashes.is_empty());

    // Working tree still has the applied changes
    assert!(!git_is_clean(repo.path_str()).unwrap());
}

// ═══════════════════════════════════════════════════════════════
// Write operations: staging
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_add_all() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    std::fs::write(repo.path.join("new1.txt"), "a\n").unwrap();
    std::fs::write(repo.path.join("new2.txt"), "b\n").unwrap();

    git_add_all(repo.path_str()).unwrap();

    // Verify files are staged by checking status
    let output = run_git(&repo.path, &["diff", "--cached", "--name-only"]);
    assert!(output.contains("new1.txt"));
    assert!(output.contains("new2.txt"));
}

#[test]
fn test_add_files() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    std::fs::write(repo.path.join("a.txt"), "a\n").unwrap();
    std::fs::write(repo.path.join("b.txt"), "b\n").unwrap();

    git_add_files(repo.path_str(), vec!["a.txt".to_owned()]).unwrap();

    let output = run_git(&repo.path, &["diff", "--cached", "--name-only"]);
    assert!(output.contains("a.txt"));
    assert!(!output.contains("b.txt"));
}

#[test]
fn test_reset_staging() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    std::fs::write(repo.path.join("staged.txt"), "s\n").unwrap();
    run_git(&repo.path, &["add", "staged.txt"]);

    // Verify it's staged
    let output = run_git(&repo.path, &["diff", "--cached", "--name-only"]);
    assert!(output.contains("staged.txt"));

    git_reset_staging(repo.path_str()).unwrap();

    // Verify it's unstaged
    let output = run_git(&repo.path, &["diff", "--cached", "--name-only"]);
    assert!(!output.contains("staged.txt"));
}

// ═══════════════════════════════════════════════════════════════
// Write operations: commit
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_commit() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    std::fs::write(repo.path.join("new.txt"), "new content\n").unwrap();
    git_add_all(repo.path_str()).unwrap();
    git_commit(repo.path_str(), "Native commit".to_owned()).unwrap();

    let commits = git_log(repo.path_str(), 1, 0).unwrap();
    assert_eq!(commits[0].message, "Native commit");
}

#[test]
fn test_commit_uses_config_author() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    std::fs::write(repo.path.join("new.txt"), "content\n").unwrap();
    git_add_all(repo.path_str()).unwrap();
    git_commit(repo.path_str(), "Author test".to_owned()).unwrap();

    let commits = git_log(repo.path_str(), 1, 0).unwrap();
    assert_eq!(commits[0].author_name, "Test Author");
    assert_eq!(commits[0].author_email, "test@example.com");
}

#[test]
fn test_amend_commit() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");
    repo.commit("file.txt", "updated\n", "Second commit");

    // Amend the message
    git_amend_commit(repo.path_str(), "Amended message".to_owned()).unwrap();

    let commits = git_log(repo.path_str(), 2, 0).unwrap();
    assert_eq!(commits[0].message, "Amended message");
    assert_eq!(commits.len(), 2);
}

#[test]
fn test_amend_commit_with_staged_changes() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    // Stage new content and amend
    std::fs::write(repo.path.join("file.txt"), "amended content\n").unwrap();
    git_add_all(repo.path_str()).unwrap();
    git_amend_commit(repo.path_str(), "Amended with changes".to_owned()).unwrap();

    let commits = git_log(repo.path_str(), 1, 0).unwrap();
    assert_eq!(commits[0].message, "Amended with changes");

    // Verify the tree includes the new content
    let content = std::fs::read_to_string(repo.path.join("file.txt")).unwrap();
    assert_eq!(content, "amended content\n");
}

// ═══════════════════════════════════════════════════════════════
// Blame operations
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_blame_single_commit() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "line1\nline2\nline3\n", "Initial commit");

    let blame = git_blame_file(repo.path_str(), "file.txt".to_owned()).unwrap();
    assert_eq!(blame.len(), 1);
    assert_eq!(blame[0].line_number, 1);
    assert_eq!(blame[0].num_lines, 3);
    assert_eq!(blame[0].original_line_number, 1);
    assert_eq!(blame[0].author_name, "Test Author");
    assert_eq!(blame[0].author_email, "test@example.com");
}

#[test]
fn test_blame_multiple_commits() {
    let repo = TestRepo::new();
    let hash1 = repo.commit("file.txt", "line1\nline2\n", "First");
    let hash2 = repo.commit("file.txt", "line1\nline2\nline3\n", "Second");

    let blame = git_blame_file(repo.path_str(), "file.txt".to_owned()).unwrap();
    // Should have 2 entries: first 2 lines from first commit, third line from second
    assert_eq!(blame.len(), 2);

    assert_eq!(blame[0].commit_hash, hash1);
    assert_eq!(blame[0].line_number, 1);
    assert_eq!(blame[0].num_lines, 2);

    assert_eq!(blame[1].commit_hash, hash2);
    assert_eq!(blame[1].line_number, 3);
    assert_eq!(blame[1].num_lines, 1);
}

#[test]
fn test_blame_with_different_authors() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "line1\n", "First");
    repo.commit_with_author(
        "file.txt",
        "line1\nline2\n",
        "Add line2",
        "Jane Smith",
        "jane@example.com",
    );

    let blame = git_blame_file(repo.path_str(), "file.txt".to_owned()).unwrap();
    assert_eq!(blame.len(), 2);
    assert_eq!(blame[0].author_name, "Test Author");
    assert_eq!(blame[1].author_name, "Jane Smith");
    assert_eq!(blame[1].author_email, "jane@example.com");
}

#[test]
fn test_blame_nonexistent_file() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial");

    let result = git_blame_file(repo.path_str(), "nonexistent.txt".to_owned());
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════
// Worktree operations
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_list_worktrees_main_only() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let worktrees = git_list_worktrees(repo.path_str()).unwrap();
    assert_eq!(worktrees.len(), 1);
    assert!(worktrees[0].is_main);
    assert!(worktrees[0].branch.as_deref() == Some("main"));
    assert!(!worktrees[0].head.is_empty());
}

#[test]
fn test_list_worktrees_with_linked() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let wt_path = repo.path.join("linked-wt");
    run_git(
        &repo.path,
        &[
            "worktree",
            "add",
            "-b",
            "feature",
            wt_path.to_str().unwrap(),
        ],
    );

    let worktrees = git_list_worktrees(repo.path_str()).unwrap();
    assert_eq!(worktrees.len(), 2);

    let main_wt = worktrees.iter().find(|w| w.is_main).unwrap();
    assert_eq!(main_wt.branch.as_deref(), Some("main"));

    let linked_wt = worktrees.iter().find(|w| !w.is_main).unwrap();
    assert_eq!(linked_wt.branch.as_deref(), Some("feature"));
    assert!(!linked_wt.head.is_empty());

    // Clean up
    run_git(
        &repo.path,
        &["worktree", "remove", wt_path.to_str().unwrap()],
    );
}

#[test]
fn test_worktree_add_new_branch() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let wt_path = repo.path.join("new-branch-wt");
    git_worktree_add_new_branch(
        repo.path_str(),
        wt_path.to_str().unwrap().to_owned(),
        "new-feature".to_owned(),
        "HEAD".to_owned(),
    )
    .unwrap();

    assert!(wt_path.exists());

    let worktrees = git_list_worktrees(repo.path_str()).unwrap();
    assert_eq!(worktrees.len(), 2);
    let linked = worktrees.iter().find(|w| !w.is_main).unwrap();
    assert_eq!(linked.branch.as_deref(), Some("new-feature"));

    // Clean up
    run_git(
        &repo.path,
        &["worktree", "remove", wt_path.to_str().unwrap()],
    );
}

#[test]
fn test_worktree_add_existing_branch() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");
    repo.create_branch("existing-branch");

    let wt_path = repo.path.join("existing-wt");
    git_worktree_add_existing(
        repo.path_str(),
        wt_path.to_str().unwrap().to_owned(),
        "existing-branch".to_owned(),
    )
    .unwrap();

    assert!(wt_path.exists());

    let worktrees = git_list_worktrees(repo.path_str()).unwrap();
    assert_eq!(worktrees.len(), 2);
    let linked = worktrees.iter().find(|w| !w.is_main).unwrap();
    assert_eq!(linked.branch.as_deref(), Some("existing-branch"));

    // Clean up
    run_git(
        &repo.path,
        &["worktree", "remove", wt_path.to_str().unwrap()],
    );
}

#[test]
fn test_worktree_remove_clean() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let wt_path = repo.path.join("to-remove-wt");
    git_worktree_add_new_branch(
        repo.path_str(),
        wt_path.to_str().unwrap().to_owned(),
        "to-remove".to_owned(),
        "HEAD".to_owned(),
    )
    .unwrap();
    assert!(wt_path.exists());

    git_worktree_remove_clean(repo.path_str(), wt_path.to_str().unwrap().to_owned()).unwrap();
    assert!(!wt_path.exists());

    let worktrees = git_list_worktrees(repo.path_str()).unwrap();
    assert_eq!(worktrees.len(), 1);
}

#[test]
fn test_worktree_remove_force_dirty() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let wt_path = repo.path.join("dirty-wt");
    git_worktree_add_new_branch(
        repo.path_str(),
        wt_path.to_str().unwrap().to_owned(),
        "dirty-branch".to_owned(),
        "HEAD".to_owned(),
    )
    .unwrap();

    // Make it dirty
    std::fs::write(wt_path.join("dirty.txt"), "uncommitted\n").unwrap();

    // Clean remove should fail
    let result = git_worktree_remove_clean(repo.path_str(), wt_path.to_str().unwrap().to_owned());
    assert!(result.is_err());

    // Force remove should succeed
    git_worktree_remove_force(repo.path_str(), wt_path.to_str().unwrap().to_owned()).unwrap();
    assert!(!wt_path.exists());
}

#[test]
fn test_worktree_prune() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let wt_path = repo.path.join("prune-wt");
    git_worktree_add_new_branch(
        repo.path_str(),
        wt_path.to_str().unwrap().to_owned(),
        "prune-branch".to_owned(),
        "HEAD".to_owned(),
    )
    .unwrap();

    // Manually delete the worktree directory (simulating external removal)
    std::fs::remove_dir_all(&wt_path).unwrap();

    // Worktree is still listed (stale metadata)
    let before = git_list_worktrees(repo.path_str()).unwrap();
    assert_eq!(before.len(), 2);

    // Prune cleans up the stale metadata
    git_worktree_prune(repo.path_str()).unwrap();

    let after = git_list_worktrees(repo.path_str()).unwrap();
    assert_eq!(after.len(), 1);
}

// ═══════════════════════════════════════════════════════════════
// Blob operations
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_blob_oids_basic() {
    let repo = TestRepo::new();
    repo.commit("hello.txt", "hello\n", "Add hello");
    repo.commit("world.txt", "world\n", "Add world");

    let oids = git_blob_oids(repo.path_str()).unwrap();
    assert_eq!(oids.len(), 2);
    assert!(oids.contains_key("hello.txt"));
    assert!(oids.contains_key("world.txt"));
    // SHA-1 hashes are 40 hex chars
    assert_eq!(oids["hello.txt"].len(), 40);
    assert_eq!(oids["world.txt"].len(), 40);
}

#[test]
fn test_blob_oids_empty_repo() {
    let repo = TestRepo::new();
    // Repo with no commits — empty index
    let oids = git_blob_oids(repo.path_str()).unwrap();
    assert!(oids.is_empty());
}

#[test]
fn test_show_file_basic() {
    let repo = TestRepo::new();
    let hash = repo.commit("hello.txt", "hello world\n", "Add hello");

    let content = git_show_file(repo.path_str(), hash, "hello.txt".to_owned()).unwrap();
    assert_eq!(content, "hello world\n");
}

#[test]
fn test_show_file_at_older_commit() {
    let repo = TestRepo::new();
    let first = repo.commit("file.txt", "version 1\n", "First");
    repo.commit("file.txt", "version 2\n", "Second");

    // Reading at the first commit should give the old content
    let content = git_show_file(repo.path_str(), first, "file.txt".to_owned()).unwrap();
    assert_eq!(content, "version 1\n");
}

#[test]
fn test_show_file_nonexistent_path() {
    let repo = TestRepo::new();
    let hash = repo.commit("file.txt", "content\n", "Initial");

    let result = git_show_file(repo.path_str(), hash, "nope.txt".to_owned());
    assert!(result.is_err());
}

#[test]
fn test_show_file_bad_commit() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial");

    let result = git_show_file(
        repo.path_str(),
        "deadbeef".to_owned(),
        "file.txt".to_owned(),
    );
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════
// Log skip (pagination)
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_log_skip() {
    let repo = TestRepo::new();
    for i in 1..=10 {
        repo.commit(
            &format!("f{i}.txt"),
            &format!("{i}\n"),
            &format!("Commit {i}"),
        );
    }
    // Commits are newest-first: 10, 9, 8, 7, 6, 5, 4, 3, 2, 1
    // Skip 3 (skip 10, 9, 8), take 3 → 7, 6, 5
    let commits = git_log(repo.path_str(), 3, 3).unwrap();
    assert_eq!(commits.len(), 3);
    assert_eq!(commits[0].message, "Commit 7");
    assert_eq!(commits[1].message, "Commit 6");
    assert_eq!(commits[2].message, "Commit 5");
}

#[test]
fn test_log_skip_past_end() {
    let repo = TestRepo::new();
    repo.commit("f.txt", "x\n", "Only commit");
    let commits = git_log(repo.path_str(), 10, 100).unwrap();
    assert!(commits.is_empty());
}

#[test]
fn test_log_skip_zero_is_noop() {
    let repo = TestRepo::new();
    for i in 1..=5 {
        repo.commit(
            &format!("f{i}.txt"),
            &format!("{i}\n"),
            &format!("Commit {i}"),
        );
    }
    let without_skip = git_log(repo.path_str(), 5, 0).unwrap();
    assert_eq!(without_skip.len(), 5);
    assert_eq!(without_skip[0].message, "Commit 5");
    assert_eq!(without_skip[4].message, "Commit 1");
}

#[test]
fn test_log_skip_with_max_count() {
    let repo = TestRepo::new();
    for i in 1..=10 {
        repo.commit(
            &format!("f{i}.txt"),
            &format!("{i}\n"),
            &format!("Commit {i}"),
        );
    }
    // Skip 2 (skip 10, 9), take 3 → 8, 7, 6
    let commits = git_log(repo.path_str(), 3, 2).unwrap();
    assert_eq!(commits.len(), 3);
    assert_eq!(commits[0].message, "Commit 8");
    assert_eq!(commits[2].message, "Commit 6");
}

// ═══════════════════════════════════════════════════════════════
// Tag operations
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_list_tags_empty() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let tags = git_list_tags(repo.path_str()).unwrap();
    assert!(tags.is_empty());
}

#[test]
fn test_list_tags_lightweight() {
    let repo = TestRepo::new();
    let head = repo.commit("file.txt", "content\n", "Initial commit");

    run_git(&repo.path, &["tag", "v1.0.0"]);

    let tags = git_list_tags(repo.path_str()).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "v1.0.0");
    assert!(!tags[0].is_annotated);
    assert_eq!(tags[0].target_hash, head);
    assert!(tags[0].tagger_name.is_none());
    assert!(tags[0].tagger_email.is_none());
    assert!(tags[0].tagger_date.is_none());
    assert!(tags[0].message.is_none());
}

#[test]
fn test_list_tags_annotated() {
    let repo = TestRepo::new();
    let head = repo.commit("file.txt", "content\n", "Initial commit");

    run_git(&repo.path, &["tag", "-a", "v2.0.0", "-m", "Release v2"]);

    let tags = git_list_tags(repo.path_str()).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "v2.0.0");
    assert!(tags[0].is_annotated);
    assert_eq!(tags[0].target_hash, head);
    assert_eq!(tags[0].tagger_name.as_deref(), Some("Test Author"));
    assert_eq!(tags[0].tagger_email.as_deref(), Some("test@example.com"));
    assert!(tags[0].tagger_date.is_some());
    assert_eq!(tags[0].message.as_deref(), Some("Release v2"));
}

#[test]
fn test_list_tags_mixed() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    run_git(&repo.path, &["tag", "v1.0.0"]);
    run_git(&repo.path, &["tag", "-a", "v2.0.0", "-m", "Release v2"]);

    let tags = git_list_tags(repo.path_str()).unwrap();
    assert_eq!(tags.len(), 2);

    // Sorted descending by name: v2.0.0, v1.0.0
    let annotated = tags.iter().find(|t| t.name == "v2.0.0").unwrap();
    assert!(annotated.is_annotated);

    let lightweight = tags.iter().find(|t| t.name == "v1.0.0").unwrap();
    assert!(!lightweight.is_annotated);
}

// ═══════════════════════════════════════════════════════════════
// Stash show (diff preview)
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_stash_show() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "original\n", "Initial commit");

    // Modify the file and stash
    std::fs::write(repo.path.join("file.txt"), "modified\n").unwrap();
    run_git(&repo.path, &["stash", "push", "-m", "test stash"]);

    let diff = git_stash_show(repo.path_str(), 0).unwrap();
    assert!(
        diff.contains("file.txt"),
        "diff should mention the changed file"
    );
    assert!(diff.contains("-original"), "diff should show removed line");
    assert!(diff.contains("+modified"), "diff should show added line");
}

#[test]
fn test_stash_show_invalid_index() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let result = git_stash_show(repo.path_str(), 0);
    assert!(result.is_err(), "should fail when no stashes exist");
}

// ═══════════════════════════════════════════════════════════════
// Tag create/delete
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_create_lightweight_tag() {
    let repo = TestRepo::new();
    let hash = repo.commit("file.txt", "content\n", "Initial commit");

    git_create_tag(repo.path_str(), "v1.0".to_owned(), hash, None).unwrap();

    let tags = git_list_tags(repo.path_str()).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "v1.0");
    assert!(!tags[0].is_annotated);
}

#[test]
fn test_create_annotated_tag() {
    let repo = TestRepo::new();
    let hash = repo.commit("file.txt", "content\n", "Initial commit");

    git_create_tag(
        repo.path_str(),
        "v2.0".to_owned(),
        hash,
        Some("Release v2.0".to_owned()),
    )
    .unwrap();

    let tags = git_list_tags(repo.path_str()).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "v2.0");
    assert!(tags[0].is_annotated);
    assert_eq!(tags[0].message.as_deref(), Some("Release v2.0"));
}

#[test]
fn test_delete_tag() {
    let repo = TestRepo::new();
    let hash = repo.commit("file.txt", "content\n", "Initial commit");

    git_create_tag(repo.path_str(), "v1.0".to_owned(), hash, None).unwrap();
    assert_eq!(git_list_tags(repo.path_str()).unwrap().len(), 1);

    git_delete_tag(repo.path_str(), "v1.0".to_owned()).unwrap();
    assert!(git_list_tags(repo.path_str()).unwrap().is_empty());
}

#[test]
fn test_delete_nonexistent_tag() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let result = git_delete_tag(repo.path_str(), "nonexistent".to_owned());
    assert!(result.is_err());
}

// MARK: - Cherry-pick operations

#[test]
fn test_cherry_pick_basic() {
    let repo = TestRepo::new();
    repo.commit("base.txt", "base\n", "Initial commit");

    // Create a branch with a new file
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("feature.txt", "feature content\n", "Add feature");
    let feature_hash = run_git(&repo.path, &["rev-parse", "HEAD"]);

    // Switch back to main
    run_git(&repo.path, &["checkout", "main"]);
    assert!(!repo.path.join("feature.txt").exists());

    // Cherry-pick the feature commit onto main
    let new_hash = git_cherry_pick(repo.path_str(), feature_hash, false).unwrap();
    assert!(!new_hash.is_empty());
    assert_eq!(new_hash.len(), 40);

    // Verify the file now exists on main
    let content = std::fs::read_to_string(repo.path.join("feature.txt")).unwrap();
    assert_eq!(content, "feature content\n");

    // Verify HEAD moved
    let head = run_git(&repo.path, &["rev-parse", "HEAD"]);
    assert_eq!(head, new_hash);
}

#[test]
fn test_cherry_pick_conflict() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "original\n", "Initial commit");

    // Create conflicting change on a branch
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "Change on feature");
    let feature_hash = run_git(&repo.path, &["rev-parse", "HEAD"]);

    // Create conflicting change on main
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "Change on main");

    // Cherry-pick should fail with conflict
    let result = git_cherry_pick(repo.path_str(), feature_hash, false);
    assert!(result.is_err());
}

// MARK: - Reflog operations

#[test]
fn test_reflog_head() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "a\n", "First commit");
    repo.commit("b.txt", "b\n", "Second commit");
    repo.commit("c.txt", "c\n", "Third commit");

    let entries = git_reflog(repo.path_str(), "HEAD".to_owned(), 100).unwrap();
    // At least 3 commit entries + initial branch creation
    assert!(entries.len() >= 3);

    // Most recent entry should be for "Third commit"
    assert!(entries[0].message.contains("Third commit"));
    assert!(!entries[0].id.is_empty());
    assert_eq!(entries[0].id.len(), 40);
    assert_eq!(entries[0].committer, "Test Author");
    // Date should be ISO 8601
    assert!(entries[0].date.contains("T"));
}

// ═══════════════════════════════════════════════════════════════
// Merge state & conflict operations
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_merge_state_none() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "content\n", "Initial commit");

    let state = git_merge_state(repo.path_str()).unwrap();
    assert!(matches!(state.kind, MergeStateKind::None));
    assert_eq!(state.conflict_count, 0);
    assert!(state.branch.is_none());
}

#[test]
fn test_merge_state_conflict() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "original\n", "Initial commit");

    // Create conflicting branch
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "Change on feature");

    // Create conflicting change on main
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "Change on main");

    // Attempt merge (will conflict) — use --no-ff to prevent ff-only abort
    let output = Command::new("git")
        .args(["merge", "feature", "--no-ff", "--no-edit"])
        .current_dir(&repo.path)
        .output()
        .unwrap();
    assert!(!output.status.success(), "merge should have conflicts");

    let state = git_merge_state(repo.path_str()).unwrap();
    assert!(matches!(state.kind, MergeStateKind::Merge));
    assert!(state.conflict_count > 0);
    assert_eq!(state.branch.as_deref(), Some("feature"));
}

#[test]
fn test_list_conflict_paths() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "original\n", "Initial commit");

    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "Change on feature");

    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "Change on main");

    let _ = Command::new("git")
        .args(["merge", "feature", "--no-ff", "--no-edit"])
        .current_dir(&repo.path)
        .output()
        .unwrap();

    let paths = git_list_conflict_paths(repo.path_str()).unwrap();
    assert_eq!(paths, vec!["shared.txt"]);
}

#[test]
fn test_checkout_ours() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "original\n", "Initial commit");

    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "Change on feature");

    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "Change on main");

    let _ = Command::new("git")
        .args(["merge", "feature", "--no-ff", "--no-edit"])
        .current_dir(&repo.path)
        .output()
        .unwrap();

    git_checkout_ours(repo.path_str(), "shared.txt".to_owned()).unwrap();

    let content = std::fs::read_to_string(repo.path.join("shared.txt")).unwrap();
    assert_eq!(content, "main version\n");

    // Should have no more conflicts for that file
    let paths = git_list_conflict_paths(repo.path_str()).unwrap();
    assert!(!paths.contains(&"shared.txt".to_owned()));
}

#[test]
fn test_checkout_theirs() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "original\n", "Initial commit");

    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "Change on feature");

    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "Change on main");

    let _ = Command::new("git")
        .args(["merge", "feature", "--no-ff", "--no-edit"])
        .current_dir(&repo.path)
        .output()
        .unwrap();

    git_checkout_theirs(repo.path_str(), "shared.txt".to_owned()).unwrap();

    let content = std::fs::read_to_string(repo.path.join("shared.txt")).unwrap();
    assert_eq!(content, "feature version\n");

    let paths = git_list_conflict_paths(repo.path_str()).unwrap();
    assert!(!paths.contains(&"shared.txt".to_owned()));
}

#[test]
fn test_mark_resolved() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "original\n", "Initial commit");

    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "Change on feature");

    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "Change on main");

    let _ = Command::new("git")
        .args(["merge", "feature", "--no-ff", "--no-edit"])
        .current_dir(&repo.path)
        .output()
        .unwrap();

    // Manually edit the file to resolve
    std::fs::write(repo.path.join("shared.txt"), "resolved content\n").unwrap();
    git_mark_resolved(repo.path_str(), "shared.txt".to_owned()).unwrap();

    let paths = git_list_conflict_paths(repo.path_str()).unwrap();
    assert!(paths.is_empty());

    // Verify the file was staged
    let content = std::fs::read_to_string(repo.path.join("shared.txt")).unwrap();
    assert_eq!(content, "resolved content\n");
}

#[test]
fn test_conflict_sides() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "original\n", "Initial commit");

    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "Change on feature");

    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "Change on main");

    let _ = Command::new("git")
        .args(["merge", "feature", "--no-ff", "--no-edit"])
        .current_dir(&repo.path)
        .output()
        .unwrap();

    let sides = git_conflict_sides(repo.path_str(), "shared.txt".to_owned()).unwrap();
    assert_eq!(sides.path, "shared.txt");
    assert_eq!(sides.base.as_deref(), Some("original\n"));
    assert_eq!(sides.ours.as_deref(), Some("main version\n"));
    assert_eq!(sides.theirs.as_deref(), Some("feature version\n"));
}
