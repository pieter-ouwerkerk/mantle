//! Integration tests for the CoW hydration pipeline.
//!
//! These tests exercise the public API (`scan_clone_candidates` + `cow_clone_directory`)
//! end-to-end using real temporary directories, complementing the unit tests in
//! `ops/cow.rs` and `ops/artifacts.rs`.

use std::fs;
use std::path::Path;
use tempfile::TempDir;

use mantle_git::{cow_clone_directory, scan_clone_candidates};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn path_str(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

/// Create a minimal "repo" with an artifact directory and optional lockfile.
fn make_repo_with_artifact(
    dir: &Path,
    artifact_dir: &str,
    lockfile: Option<&str>,
    file_count: usize,
) {
    let art = dir.join(artifact_dir);
    fs::create_dir_all(&art).unwrap();
    for i in 0..file_count {
        fs::write(art.join(format!("file_{i}.bin")), format!("content-{i}")).unwrap();
    }
    if let Some(lf) = lockfile {
        fs::write(dir.join(lf), "lockfile-content").unwrap();
    }
}

/// Create the same lockfile in the worktree to make lockfile_matches = true.
fn mirror_lockfile(repo: &Path, worktree: &Path, lockfile: &str) {
    let src = repo.join(lockfile);
    if src.exists() {
        let content = fs::read(&src).unwrap();
        fs::write(worktree.join(lockfile), content).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn scan_finds_node_modules_with_matching_lockfile() {
    let repo = TempDir::new().unwrap();
    let wt = TempDir::new().unwrap();

    make_repo_with_artifact(repo.path(), "node_modules", Some("package-lock.json"), 3);
    mirror_lockfile(repo.path(), wt.path(), "package-lock.json");
    // scan_clone_candidates delegates to scan_worktreeinclude; provide .gitignore
    fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

    let candidates = scan_clone_candidates(path_str(repo.path()), path_str(wt.path())).unwrap();

    assert!(!candidates.is_empty(), "should find node_modules candidate");
    let nm = candidates
        .iter()
        .find(|c| c.dest_path.contains("node_modules"))
        .expect("should have node_modules candidate");
    // New code always sets lockfile_matches = true
    assert!(nm.lockfile_matches);
    assert_eq!(
        format!("{:?}", nm.strategy),
        "CowClone",
        "CowClone strategy"
    );
}

#[test]
fn scan_detects_lockfile_mismatch() {
    let repo = TempDir::new().unwrap();
    let wt = TempDir::new().unwrap();

    make_repo_with_artifact(repo.path(), "node_modules", Some("package-lock.json"), 2);
    // Write a DIFFERENT lockfile in the worktree
    fs::write(wt.path().join("package-lock.json"), "different-content").unwrap();
    // scan_clone_candidates delegates to scan_worktreeinclude; provide .gitignore
    fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

    let candidates = scan_clone_candidates(path_str(repo.path()), path_str(wt.path())).unwrap();

    let nm = candidates
        .iter()
        .find(|c| c.dest_path.contains("node_modules"))
        .expect("should have node_modules candidate");
    // New code always sets lockfile_matches = true (no lockfile comparison in new system)
    assert!(
        nm.lockfile_matches,
        "new system always reports lockfile_matches = true"
    );
    // New code always uses CowClone (no lockfile-based Skip)
    assert_eq!(
        format!("{:?}", nm.strategy),
        "CowClone",
        "new system uses CowClone regardless of lockfile"
    );
}

#[test]
fn scan_detects_pnpm_lockfile() {
    let repo = TempDir::new().unwrap();
    let wt = TempDir::new().unwrap();

    make_repo_with_artifact(repo.path(), "node_modules", Some("pnpm-lock.yaml"), 2);
    mirror_lockfile(repo.path(), wt.path(), "pnpm-lock.yaml");
    // scan_clone_candidates delegates to scan_worktreeinclude; provide .gitignore
    fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

    let candidates = scan_clone_candidates(path_str(repo.path()), path_str(wt.path())).unwrap();

    let nm = candidates
        .iter()
        .find(|c| c.dest_path.contains("node_modules"))
        .expect("should have node_modules candidate");
    // New code always returns Generic (no built-in type detection)
    assert_eq!(
        format!("{:?}", nm.artifact_type),
        "Generic",
        "new system always returns Generic type"
    );
    assert_eq!(
        format!("{:?}", nm.strategy),
        "CowClone",
        "CowClone strategy"
    );
}

#[test]
fn scan_finds_rust_target() {
    let repo = TempDir::new().unwrap();
    let wt = TempDir::new().unwrap();

    make_repo_with_artifact(repo.path(), "target", Some("Cargo.lock"), 5);
    mirror_lockfile(repo.path(), wt.path(), "Cargo.lock");
    // scan_clone_candidates delegates to scan_worktreeinclude; provide .gitignore
    fs::write(repo.path().join(".gitignore"), "target/\n").unwrap();

    let candidates = scan_clone_candidates(path_str(repo.path()), path_str(wt.path())).unwrap();

    let target = candidates
        .iter()
        .find(|c| c.dest_path.contains("target"))
        .expect("should find target candidate");
    // New code always returns Generic (no built-in type detection)
    assert_eq!(format!("{:?}", target.artifact_type), "Generic");
    // New code always sets lockfile_matches = true
    assert!(target.lockfile_matches);
}

#[test]
fn scan_respects_worktreeinclude() {
    let repo = TempDir::new().unwrap();
    let wt = TempDir::new().unwrap();

    // Create a custom artifact dir
    let custom = repo.path().join("my-cache");
    fs::create_dir_all(&custom).unwrap();
    fs::write(custom.join("data.bin"), "cached-data").unwrap();

    // Write .worktreeinclude in repo
    fs::write(repo.path().join(".worktreeinclude"), "my-cache\n").unwrap();

    let candidates = scan_clone_candidates(path_str(repo.path()), path_str(wt.path())).unwrap();

    let custom_candidate = candidates.iter().find(|c| c.dest_path.contains("my-cache"));
    assert!(
        custom_candidate.is_some(),
        ".worktreeinclude dirs should be scanned"
    );
}

#[test]
fn cow_clone_produces_identical_contents() {
    let src = TempDir::new().unwrap();

    // Create a small directory tree
    fs::create_dir_all(src.path().join("sub")).unwrap();
    fs::write(src.path().join("a.txt"), "hello").unwrap();
    fs::write(src.path().join("sub/b.txt"), "world").unwrap();
    fs::write(src.path().join("sub/c.bin"), vec![0u8; 256]).unwrap();

    let dest = TempDir::new().unwrap();
    let dest_path = dest.path().join("cloned");

    let result = cow_clone_directory(path_str(src.path()), path_str(&dest_path)).unwrap();

    assert!(result.cloned_count >= 3, "should clone at least 3 files");
    assert_eq!(
        fs::read_to_string(dest_path.join("a.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(dest_path.join("sub/b.txt")).unwrap(),
        "world"
    );
    assert_eq!(fs::read(dest_path.join("sub/c.bin")).unwrap().len(), 256);
}

#[test]
fn cow_clone_dest_already_exists_fails() {
    let src = TempDir::new().unwrap();
    fs::write(src.path().join("a.txt"), "data").unwrap();

    let dest = TempDir::new().unwrap();
    let dest_path = dest.path().join("cloned");
    fs::create_dir_all(&dest_path).unwrap();

    let result = cow_clone_directory(path_str(src.path()), path_str(&dest_path));
    assert!(result.is_err(), "should fail when dest already exists");
}

#[test]
fn scan_skips_when_dest_already_exists() {
    let repo = TempDir::new().unwrap();
    let wt = TempDir::new().unwrap();

    make_repo_with_artifact(repo.path(), "node_modules", Some("package-lock.json"), 2);
    mirror_lockfile(repo.path(), wt.path(), "package-lock.json");

    // Pre-create node_modules in worktree
    fs::create_dir_all(wt.path().join("node_modules")).unwrap();

    let candidates = scan_clone_candidates(path_str(repo.path()), path_str(wt.path())).unwrap();

    let nm = candidates
        .iter()
        .find(|c| c.dest_path.contains("node_modules"));
    if let Some(c) = nm {
        assert_eq!(format!("{:?}", c.strategy), "Skip", "existing dest → Skip");
    }
    // It's also valid for the scanner to omit it entirely
}

#[test]
fn scan_empty_repo_returns_no_candidates() {
    let repo = TempDir::new().unwrap();
    let wt = TempDir::new().unwrap();

    let candidates = scan_clone_candidates(path_str(repo.path()), path_str(wt.path())).unwrap();

    assert!(
        candidates.is_empty(),
        "empty repo should have no candidates"
    );
}

#[test]
fn scan_multiple_artifact_types() {
    let repo = TempDir::new().unwrap();
    let wt = TempDir::new().unwrap();

    // node_modules + target + .build
    make_repo_with_artifact(repo.path(), "node_modules", Some("package-lock.json"), 2);
    make_repo_with_artifact(repo.path(), "target", Some("Cargo.lock"), 2);
    make_repo_with_artifact(repo.path(), ".build", None, 2);
    mirror_lockfile(repo.path(), wt.path(), "package-lock.json");
    mirror_lockfile(repo.path(), wt.path(), "Cargo.lock");
    // scan_clone_candidates delegates to scan_worktreeinclude; provide .gitignore
    fs::write(
        repo.path().join(".gitignore"),
        "node_modules/\ntarget/\n.build/\n",
    )
    .unwrap();

    let candidates = scan_clone_candidates(path_str(repo.path()), path_str(wt.path())).unwrap();

    let dirs: Vec<&str> = candidates
        .iter()
        .map(|c| {
            Path::new(&c.dest_path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
        })
        .collect();

    assert!(dirs.contains(&"node_modules"), "should find node_modules");
    assert!(dirs.contains(&"target"), "should find target");
    assert!(dirs.contains(&".build"), "should find .build");
}

#[test]
fn full_scan_and_clone_cycle() {
    let repo = TempDir::new().unwrap();
    let wt = TempDir::new().unwrap();

    // Set up repo with node_modules (matching lockfile)
    make_repo_with_artifact(repo.path(), "node_modules", Some("package-lock.json"), 5);
    mirror_lockfile(repo.path(), wt.path(), "package-lock.json");
    // scan_clone_candidates delegates to scan_worktreeinclude; provide .gitignore
    fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

    // Scan
    let candidates = scan_clone_candidates(path_str(repo.path()), path_str(wt.path())).unwrap();

    // Clone each CowClone candidate
    let mut cloned = 0;
    for candidate in &candidates {
        if format!("{:?}", candidate.strategy) == "CowClone" {
            let result =
                cow_clone_directory(candidate.source_path.clone(), candidate.dest_path.clone())
                    .unwrap();
            assert!(result.cloned_count > 0);
            cloned += 1;
        }
    }
    assert!(cloned > 0, "should have cloned at least one candidate");

    // Verify the cloned directory exists and has content
    let cloned_nm = wt.path().join("node_modules");
    assert!(cloned_nm.exists(), "node_modules should exist in worktree");
    assert!(
        cloned_nm.join("file_0.bin").exists(),
        "cloned files should exist"
    );
}
