use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

use mantle_git::*;

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
        std::fs::write(&file_path, content).expect("write file");
        run_git(&self.path, &["add", filename]);
        run_git(&self.path, &["commit", "-m", message]);
        run_git(&self.path, &["rev-parse", "HEAD"])
    }
}

fn run_git(path: &PathBuf, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .expect("run git");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn test_branch_is_merged_after_merge() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "initial", "initial commit");

    // Create feature branch and add a commit
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("b.txt", "feature work", "feature commit");
    run_git(&repo.path, &["checkout", "main"]);

    // Before merge, feature is NOT merged into main
    assert!(!git_branch_is_merged(repo.path_str(), "feature".into(), "main".into()).unwrap());

    // Merge feature into main
    run_git(
        &repo.path,
        &["merge", "feature", "--no-ff", "-m", "merge feature"],
    );

    // After merge, feature IS merged into main
    assert!(git_branch_is_merged(repo.path_str(), "feature".into(), "main".into()).unwrap());
}

#[test]
fn test_branch_is_merged_same_commit() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "initial", "initial commit");

    // Create branch at same commit — it's trivially merged
    run_git(&repo.path, &["branch", "same-point"]);
    assert!(git_branch_is_merged(repo.path_str(), "same-point".into(), "main".into()).unwrap());
}

#[test]
fn test_branch_is_merged_with_diverged_branches() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "initial", "initial commit");

    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("b.txt", "feature", "feature commit");
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("c.txt", "main work", "main commit");

    // Branches diverged — neither is merged into the other
    assert!(!git_branch_is_merged(repo.path_str(), "feature".into(), "main".into()).unwrap());
    assert!(!git_branch_is_merged(repo.path_str(), "main".into(), "feature".into()).unwrap());
}

#[test]
fn test_branch_is_merged_fast_forward() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "initial", "initial commit");

    // Create branch, then advance main past it (main is ahead)
    run_git(&repo.path, &["branch", "old-feature"]);
    repo.commit("b.txt", "new", "advance main");

    // old-feature is an ancestor of main => merged
    assert!(git_branch_is_merged(repo.path_str(), "old-feature".into(), "main".into()).unwrap());
    // main is NOT an ancestor of old-feature
    assert!(!git_branch_is_merged(repo.path_str(), "main".into(), "old-feature".into()).unwrap());
}

#[test]
fn test_latest_commit_date_returns_iso8601() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "hello", "first commit");

    let date = git_latest_commit_date(repo.path_str(), "main".into()).unwrap();
    assert!(date.is_some());
    let date_str = date.unwrap();
    // Should be parseable as RFC 3339 / ISO 8601
    assert!(
        date_str.contains("T"),
        "Expected ISO 8601 format, got: {date_str}"
    );
}

#[test]
fn test_latest_commit_date_nonexistent_branch() {
    let repo = TestRepo::new();
    repo.commit("a.txt", "hello", "first commit");

    let date = git_latest_commit_date(repo.path_str(), "nonexistent".into()).unwrap();
    assert!(date.is_none());
}
