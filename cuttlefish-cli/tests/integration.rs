use std::fs;
use std::process::Command;

fn cuttlefish_bin() -> String {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    path.pop(); // Remove deps
    path.join("cuttlefish").to_string_lossy().to_string()
}

fn init_test_repo(tmp: &std::path::Path) {
    let repo = git2::Repository::init(tmp).unwrap();
    let sig = git2::Signature::now("Test", "test@test.com").unwrap();
    let tree_id = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
        .unwrap();
}

#[test]
fn test_full_workflow() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path();
    init_test_repo(repo_path);

    // Create build artifacts
    fs::create_dir(repo_path.join("node_modules")).unwrap();
    fs::write(repo_path.join("node_modules/pkg.js"), "exports = {}").unwrap();

    // Step 1: cuttlefish init
    let output = Command::new(cuttlefish_bin())
        .args(["init", repo_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(repo_path.join(".worktreeinclude").exists());

    // Manually add node_modules to .worktreeinclude since auto-detect needs .gitignore
    fs::write(repo_path.join(".worktreeinclude"), "node_modules\n").unwrap();

    // Step 2: cuttlefish worktree create
    let output = Command::new(cuttlefish_bin())
        .args([
            "worktree",
            "create",
            "--name",
            "test-task",
            "--cwd",
            repo_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "worktree create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let created_path = result["worktree_path"].as_str().unwrap();

    // Verify hydration occurred
    let hydrated_pkg = std::path::Path::new(created_path).join("node_modules/pkg.js");
    assert!(
        hydrated_pkg.exists(),
        "hydration should have cloned node_modules"
    );

    // Step 3: cuttlefish worktree remove
    let output = Command::new(cuttlefish_bin())
        .args(["worktree", "remove", "--path", created_path, "--force"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "worktree remove failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!std::path::Path::new(created_path).exists());
}
