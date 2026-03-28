use mantle_git::*;
use std::process::Command;
use tempfile::TempDir;

/// Helper: create a fresh git repo with one initial commit.
fn init_repo_with_commit(dir: &std::path::Path) -> String {
    let path = dir.to_str().unwrap();
    Command::new("git")
        .args(["init", "-b", "main", path])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path, "config", "user.name", "Test"])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path, "config", "user.email", "test@test.com"])
        .output()
        .unwrap();
    std::fs::write(dir.join("file.txt"), "hello").unwrap();
    Command::new("git")
        .args(["-C", path, "add", "."])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path, "commit", "-m", "initial"])
        .output()
        .unwrap();
    path.to_owned()
}

/// Helper: create a bare remote repo and add it as "origin" to the given repo.
fn add_bare_remote(repo_path: &str, bare_dir: &std::path::Path) -> String {
    let bare_path = bare_dir.to_str().unwrap();
    Command::new("git")
        .args(["init", "--bare", bare_path])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", repo_path, "remote", "add", "origin", bare_path])
        .output()
        .unwrap();
    bare_path.to_owned()
}

#[test]
fn test_list_remotes_empty() {
    let tmp = TempDir::new().unwrap();
    let path = init_repo_with_commit(tmp.path());
    let remotes = git_list_remotes(path).unwrap();
    assert!(remotes.is_empty());
}

#[test]
fn test_list_remotes_with_origin() {
    let tmp = TempDir::new().unwrap();
    let bare_tmp = TempDir::new().unwrap();
    let path = init_repo_with_commit(tmp.path());
    let bare_path = add_bare_remote(&path, bare_tmp.path());

    let remotes = git_list_remotes(path).unwrap();
    assert_eq!(remotes.len(), 1);
    assert_eq!(remotes[0].name, "origin");
    assert_eq!(remotes[0].fetch_url.as_deref(), Some(bare_path.as_str()));
}

#[test]
fn test_push_fetch_cycle() {
    let tmp = TempDir::new().unwrap();
    let bare_tmp = TempDir::new().unwrap();
    let path = init_repo_with_commit(tmp.path());
    add_bare_remote(&path, bare_tmp.path());

    // Push main branch
    let push_result = git_push_branch(path.clone(), "main".to_owned(), true, false).unwrap();
    assert!(!push_result.up_to_date || push_result.updated_ref.is_some());

    // Now ahead_behind should be 0/0
    let ab = git_ahead_behind_remote(path, "main".to_owned()).unwrap();
    assert_eq!(ab.ahead, 0);
    assert_eq!(ab.behind, 0);
}

#[test]
fn test_push_branch_sets_upstream() {
    let tmp = TempDir::new().unwrap();
    let bare_tmp = TempDir::new().unwrap();
    let path = init_repo_with_commit(tmp.path());
    add_bare_remote(&path, bare_tmp.path());

    git_push_branch(path.clone(), "main".to_owned(), true, false).unwrap();

    let tracking = git_remote_tracking_branch(path, "main".to_owned()).unwrap();
    assert_eq!(tracking.as_deref(), Some("refs/remotes/origin/main"));
}

#[test]
fn test_pull_fast_forward() {
    let tmp_a = TempDir::new().unwrap();
    let bare_tmp = TempDir::new().unwrap();
    let path_a = init_repo_with_commit(tmp_a.path());
    let bare_path = add_bare_remote(&path_a, bare_tmp.path());

    // Push from A
    git_push_branch(path_a.clone(), "main".to_owned(), true, false).unwrap();

    // Clone into B and make a commit there, then push to bare
    let tmp_b = TempDir::new().unwrap();
    let path_b = tmp_b.path().to_str().unwrap();
    Command::new("git")
        .args(["clone", &bare_path, path_b])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path_b, "config", "user.name", "Test"])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path_b, "config", "user.email", "test@test.com"])
        .output()
        .unwrap();
    std::fs::write(tmp_b.path().join("new.txt"), "new content").unwrap();
    Command::new("git")
        .args(["-C", path_b, "add", "."])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path_b, "commit", "-m", "second commit"])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path_b, "push"])
        .output()
        .unwrap();

    // Pull in A — should fast-forward
    let pull_result = git_pull(path_a.clone(), "origin".to_owned(), "main".to_owned()).unwrap();
    assert_eq!(pull_result.merge_type, "fast_forward");
    assert!(pull_result.new_head.is_some());
}

#[test]
fn test_push_rejected_non_fast_forward() {
    let tmp_a = TempDir::new().unwrap();
    let bare_tmp = TempDir::new().unwrap();
    let path_a = init_repo_with_commit(tmp_a.path());
    let bare_path = add_bare_remote(&path_a, bare_tmp.path());

    // Push initial commit
    git_push_branch(path_a.clone(), "main".to_owned(), true, false).unwrap();

    // Clone into B, make a commit, and force-push to bare
    let tmp_b = TempDir::new().unwrap();
    let path_b = tmp_b.path().to_str().unwrap();
    Command::new("git")
        .args(["clone", &bare_path, path_b])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path_b, "config", "user.name", "Test"])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path_b, "config", "user.email", "test@test.com"])
        .output()
        .unwrap();
    std::fs::write(tmp_b.path().join("conflict.txt"), "conflict").unwrap();
    Command::new("git")
        .args(["-C", path_b, "add", "."])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path_b, "commit", "-m", "divergent commit"])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", path_b, "push", "--force"])
        .output()
        .unwrap();

    // Make a different commit in A
    std::fs::write(tmp_a.path().join("local.txt"), "local change").unwrap();
    Command::new("git")
        .args(["-C", &path_a, "add", "."])
        .output()
        .unwrap();
    Command::new("git")
        .args(["-C", &path_a, "commit", "-m", "local commit"])
        .output()
        .unwrap();

    // Push from A should be rejected (non-fast-forward)
    let result = git_push_branch(path_a, "main".to_owned(), false, false);
    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_str = format!("{err}");
    // Should be either PushRejected or an internal error about non-fast-forward
    assert!(
        err_str.contains("rejected")
            || err_str.contains("non-fast-forward")
            || err_str.contains("not present locally")
            || err_str.contains("Git error"),
        "Unexpected error: {err_str}"
    );
}

#[test]
fn test_remote_not_found() {
    let tmp = TempDir::new().unwrap();
    let path = init_repo_with_commit(tmp.path());

    let result = git_fetch(path, "nonexistent".to_owned());
    assert!(result.is_err());
    let err_str = format!("{}", result.unwrap_err());
    assert!(
        err_str.contains("Remote not found") || err_str.contains("not found"),
        "Unexpected error: {err_str}"
    );
}
