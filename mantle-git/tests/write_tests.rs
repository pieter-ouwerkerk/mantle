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
// reset_soft tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_reset_soft_moves_head() {
    let repo = TestRepo::new();
    let first = repo.commit("a.txt", "first", "first commit");
    let _second = repo.commit("a.txt", "second", "second commit");

    // HEAD should be at second commit
    let head_before = git_rev_parse(repo.path_str(), "HEAD".to_string()).unwrap();
    assert_ne!(head_before, first);

    // Soft reset to first commit
    git_reset_soft(repo.path_str(), first.clone()).unwrap();

    // HEAD should now be at first commit
    let head_after = git_rev_parse(repo.path_str(), "HEAD".to_string()).unwrap();
    assert_eq!(head_after, first);
}

#[test]
fn test_reset_soft_preserves_index_and_worktree() {
    let repo = TestRepo::new();
    let first = repo.commit("a.txt", "first", "first commit");
    let _second = repo.commit("a.txt", "second", "second commit");

    // Soft reset to first commit
    git_reset_soft(repo.path_str(), first.clone()).unwrap();

    // Working tree should still have "second" content
    let content = std::fs::read_to_string(repo.path.join("a.txt")).unwrap();
    assert_eq!(content, "second");

    // The changes should be staged (index has the second version)
    let status = run_git(&repo.path, &["status", "--porcelain"]);
    assert!(
        status.contains("M  a.txt") || status.contains("M "),
        "expected staged changes, got: {status}"
    );
}

// ═══════════════════════════════════════════════════════════════
// reset_mixed tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_reset_mixed_moves_head() {
    let repo = TestRepo::new();
    let first = repo.commit("a.txt", "first", "first commit");
    let _second = repo.commit("a.txt", "second", "second commit");

    git_reset_mixed(repo.path_str(), first.clone()).unwrap();

    let head_after = git_rev_parse(repo.path_str(), "HEAD".to_string()).unwrap();
    assert_eq!(head_after, first);
}

#[test]
fn test_reset_mixed_preserves_worktree_but_unstages() {
    let repo = TestRepo::new();
    let first = repo.commit("a.txt", "first", "first commit");
    let _second = repo.commit("a.txt", "second", "second commit");

    git_reset_mixed(repo.path_str(), first.clone()).unwrap();

    // Working tree should still have "second" content
    let content = std::fs::read_to_string(repo.path.join("a.txt")).unwrap();
    assert_eq!(content, "second");

    // The changes should be unstaged (modified but not staged)
    // git status --porcelain shows " M" for unstaged-only, but some git versions
    // may show "M " when both index and worktree differ. Just verify it's modified and not clean.
    let status = run_git(&repo.path, &["status", "--porcelain"]);
    assert!(
        status.contains("M") && status.contains("a.txt"),
        "expected modified a.txt, got: {status}"
    );

    // Verify it's NOT staged by checking diff --cached (should be empty after mixed reset)
    let cached = run_git(&repo.path, &["diff", "--cached", "--name-only"]);
    assert!(
        cached.is_empty(),
        "expected no staged changes after mixed reset, got: {cached}"
    );
}

// ═══════════════════════════════════════════════════════════════
// reset_hard tests (verify existing)
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_reset_hard_moves_head_and_discards() {
    let repo = TestRepo::new();
    let first = repo.commit("a.txt", "first", "first commit");
    let _second = repo.commit("a.txt", "second", "second commit");

    git_reset_hard(repo.path_str(), first.clone()).unwrap();

    let head_after = git_rev_parse(repo.path_str(), "HEAD".to_string()).unwrap();
    assert_eq!(head_after, first);

    // Working tree should be reverted to "first"
    let content = std::fs::read_to_string(repo.path.join("a.txt")).unwrap();
    assert_eq!(content, "first");

    // No changes should be reported
    let status = run_git(&repo.path, &["status", "--porcelain"]);
    assert!(status.is_empty(), "expected clean state, got: {status}");
}

// ═══════════════════════════════════════════════════════════════
// Unborn branch tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn test_reset_staging_on_unborn_branch() {
    let repo = TestRepo::new();
    // No commits — unborn branch. Stage a file, then reset staging.
    std::fs::write(repo.path.join("a.txt"), "hello").unwrap();
    run_git(&repo.path, &["add", "a.txt"]);

    // Verify file is staged
    let staged = run_git(&repo.path, &["diff", "--cached", "--name-only"]);
    assert!(
        staged.contains("a.txt"),
        "expected a.txt staged, got: {staged}"
    );

    // reset_staging should succeed on unborn branch
    git_reset_staging(repo.path_str()).unwrap();

    // File should now be unstaged
    let staged_after = run_git(&repo.path, &["diff", "--cached", "--name-only"]);
    assert!(
        staged_after.is_empty(),
        "expected no staged files, got: {staged_after}"
    );
}

#[test]
fn test_commit_on_unborn_branch() {
    let repo = TestRepo::new();
    // No commits — unborn branch. Stage a file and commit.
    std::fs::write(repo.path.join("a.txt"), "hello").unwrap();
    git_add_files(repo.path_str(), vec!["a.txt".to_string()]).unwrap();
    git_commit(repo.path_str(), "initial commit".to_string()).unwrap();

    // Verify the commit exists
    let log = run_git(&repo.path, &["log", "--oneline"]);
    assert!(
        log.contains("initial commit"),
        "expected commit in log, got: {log}"
    );
}

#[test]
fn test_reset_invalid_rev() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "first", "first commit");

    let result = git_reset_soft(repo.path_str(), "nonexistent".to_string());
    assert!(result.is_err());

    let result = git_reset_mixed(repo.path_str(), "nonexistent".to_string());
    assert!(result.is_err());
}
