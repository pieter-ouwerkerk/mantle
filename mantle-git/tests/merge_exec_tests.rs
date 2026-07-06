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

    fn write(&self, filename: &str, content: &str) {
        std::fs::write(self.path.join(filename), content).expect("write file");
    }

    fn commit(&self, filename: &str, content: &str, message: &str) -> String {
        self.write(filename, content);
        run_git(&self.path, &["add", "."]);
        run_git(&self.path, &["commit", "-m", message]);
        run_git(&self.path, &["rev-parse", "HEAD"])
    }
}

fn run_git(path: &PathBuf, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .env("GIT_EDITOR", "true")
        .output()
        .expect("run git");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Diverged repo: main has base + main-side commit, `feature` has its own commit.
fn diverged_repo() -> TestRepo {
    let repo = TestRepo::new();
    repo.commit("base.txt", "base\n", "base commit");
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("feature.txt", "feature\n", "feature commit");
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("main.txt", "main\n", "main commit");
    repo
}

// ---------------------------------------------------------------------------
// merge_base
// ---------------------------------------------------------------------------

#[test]
fn test_merge_base_matches_cli() {
    let repo = diverged_repo();
    let cli = run_git(&repo.path, &["merge-base", "main", "feature"]);
    let native = git_merge_base(repo.path_str(), "main".into(), "feature".into()).unwrap();
    assert_eq!(cli, native);
}

// ---------------------------------------------------------------------------
// diff_name_status / diff_name_only
// ---------------------------------------------------------------------------

#[test]
fn test_diff_name_status_matches_cli() {
    let repo = TestRepo::new();
    repo.commit("keep.txt", "keep\n", "initial");
    repo.commit("gone.txt", "gone\n", "add gone");
    repo.commit("mod.txt", "v1\n", "add mod");
    let base = run_git(&repo.path, &["rev-parse", "HEAD"]);

    repo.write("mod.txt", "v2\n");
    std::fs::remove_file(repo.path.join("gone.txt")).unwrap();
    run_git(&repo.path, &["mv", "keep.txt", "renamed.txt"]);
    repo.write("new.txt", "new\n");
    run_git(&repo.path, &["add", "-A"]);
    run_git(&repo.path, &["commit", "-m", "changes"]);

    let cli = run_git(
        &repo.path,
        &["diff", "--name-status", &format!("{base}..HEAD")],
    );
    let native = git_diff_name_status(repo.path_str(), base, "HEAD".into()).unwrap();

    let native_lines: Vec<String> = native
        .iter()
        .map(|e| match &e.old_path {
            Some(old) => format!("{}\t{}\t{}", e.status, old, e.path),
            None => format!("{}\t{}", e.status, e.path),
        })
        .collect();
    let mut cli_lines: Vec<&str> = cli.lines().collect();
    let mut sorted_native: Vec<&str> = native_lines.iter().map(String::as_str).collect();
    cli_lines.sort_unstable();
    sorted_native.sort_unstable();
    assert_eq!(cli_lines, sorted_native);
}

#[test]
fn test_diff_name_only_three_dot_matches_cli() {
    let repo = diverged_repo();
    // Three-dot: changes on main after the branch point must NOT appear.
    let cli = run_git(&repo.path, &["diff", "--name-only", "main...feature"]);
    let native = git_diff_name_only(repo.path_str(), "main".into(), "feature".into()).unwrap();

    let mut cli_lines: Vec<&str> = cli.lines().collect();
    let mut native_sorted: Vec<&str> = native.iter().map(String::as_str).collect();
    cli_lines.sort_unstable();
    native_sorted.sort_unstable();
    assert_eq!(cli_lines, native_sorted);
    assert!(!native.iter().any(|p| p == "main.txt"));
}

// ---------------------------------------------------------------------------
// merge_tree
// ---------------------------------------------------------------------------

#[test]
fn test_merge_tree_clean_matches_cli_tree_oid() {
    let repo = diverged_repo();
    let cli_tree = run_git(
        &repo.path,
        &["merge-tree", "--write-tree", "main", "feature"],
    );
    let native = git_merge_tree(repo.path_str(), "main".into(), "feature".into()).unwrap();
    assert!(native.clean);
    assert_eq!(cli_tree.lines().next().unwrap_or(""), native.tree_oid);
}

#[test]
fn test_merge_tree_conflict_lists_paths() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "base\n", "base");
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "feature edit");
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "main edit");

    let native = git_merge_tree(repo.path_str(), "main".into(), "feature".into()).unwrap();
    assert!(!native.clean);
    assert_eq!(native.conflicts.len(), 1);
    assert_eq!(native.conflicts[0].path, "shared.txt");
    assert_eq!(native.conflicts[0].reason, "content");
}

// ---------------------------------------------------------------------------
// merge / abort / continue
// ---------------------------------------------------------------------------

#[test]
fn test_merge_no_ff_clean_creates_merge_commit() {
    let repo = diverged_repo();
    let had_conflicts = git_merge_no_ff(repo.path_str(), "feature".into()).unwrap();
    assert!(!had_conflicts);
    let parents = run_git(&repo.path, &["rev-list", "--parents", "-1", "HEAD"]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "expected a merge commit"
    );
}

#[test]
fn test_merge_no_ff_conflict_then_abort() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "base\n", "base");
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "feature edit");
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "main edit");

    let had_conflicts = git_merge_no_ff(repo.path_str(), "feature".into()).unwrap();
    assert!(had_conflicts);
    assert!(repo.path.join(".git/MERGE_HEAD").exists());

    git_merge_abort(repo.path_str()).unwrap();
    assert!(!repo.path.join(".git/MERGE_HEAD").exists());
    let content = std::fs::read_to_string(repo.path.join("shared.txt")).unwrap();
    assert_eq!(content, "main version\n");
}

#[test]
fn test_merge_continue_after_resolution() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "base\n", "base");
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "feature edit");
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "main edit");

    assert!(git_merge_no_ff(repo.path_str(), "feature".into()).unwrap());
    repo.write("shared.txt", "resolved\n");
    run_git(&repo.path, &["add", "shared.txt"]);
    git_merge_continue(repo.path_str()).unwrap();
    assert!(!repo.path.join(".git/MERGE_HEAD").exists());
    let parents = run_git(&repo.path, &["rev-list", "--parents", "-1", "HEAD"]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "expected a merge commit"
    );
}

// ---------------------------------------------------------------------------
// rebase / abort / continue
// ---------------------------------------------------------------------------

#[test]
fn test_rebase_clean() {
    let repo = diverged_repo();
    run_git(&repo.path, &["checkout", "feature"]);
    git_rebase(repo.path_str(), "main".into()).unwrap();
    // feature is now a descendant of main
    let base = run_git(&repo.path, &["merge-base", "main", "feature"]);
    let main_tip = run_git(&repo.path, &["rev-parse", "main"]);
    assert_eq!(base, main_tip);
}

#[test]
fn test_rebase_conflict_abort_and_continue() {
    let repo = TestRepo::new();
    repo.commit("shared.txt", "base\n", "base");
    run_git(&repo.path, &["checkout", "-b", "feature"]);
    repo.commit("shared.txt", "feature version\n", "feature edit");
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("shared.txt", "main version\n", "main edit");
    run_git(&repo.path, &["checkout", "feature"]);

    // Conflicting rebase surfaces as an error, leaving rebase state on disk.
    assert!(git_rebase(repo.path_str(), "main".into()).is_err());
    let state = git_merge_state(repo.path_str()).unwrap();
    assert!(matches!(state.kind, MergeStateKind::Rebase));

    git_rebase_abort(repo.path_str()).unwrap();
    let state = git_merge_state(repo.path_str()).unwrap();
    assert!(matches!(state.kind, MergeStateKind::None));

    // Again, but resolve and continue this time.
    assert!(git_rebase(repo.path_str(), "main".into()).is_err());
    repo.write("shared.txt", "resolved\n");
    run_git(&repo.path, &["add", "shared.txt"]);
    git_rebase_continue(repo.path_str()).unwrap();
    let state = git_merge_state(repo.path_str()).unwrap();
    assert!(matches!(state.kind, MergeStateKind::None));
    let base = run_git(&repo.path, &["merge-base", "main", "feature"]);
    let main_tip = run_git(&repo.path, &["rev-parse", "main"]);
    assert_eq!(base, main_tip);
}

// ---------------------------------------------------------------------------
// apply_patch_cached
// ---------------------------------------------------------------------------

#[test]
fn test_apply_patch_cached_stages_change() {
    let repo = TestRepo::new();
    repo.commit("file.txt", "line1\nline2\n", "initial");

    // Produce a patch by modifying the file, capturing the diff, then reverting.
    repo.write("file.txt", "line1 changed\nline2\n");
    let patch_output = Command::new("git")
        .args(["diff"])
        .current_dir(&repo.path)
        .output()
        .expect("git diff");
    let patch = String::from_utf8_lossy(&patch_output.stdout).to_string();
    run_git(&repo.path, &["checkout", "--", "file.txt"]);

    git_apply_patch_cached(repo.path_str(), patch).unwrap();

    let staged = run_git(&repo.path, &["diff", "--cached", "--name-only"]);
    assert_eq!(staged, "file.txt");
}

// ---------------------------------------------------------------------------
// merged_branch_names
// ---------------------------------------------------------------------------

#[test]
fn test_merged_branch_names_matches_cli() {
    let repo = TestRepo::new();
    repo.commit("base.txt", "base\n", "base");
    run_git(&repo.path, &["branch", "merged-branch"]);
    run_git(&repo.path, &["checkout", "-b", "unmerged"]);
    repo.commit("unmerged.txt", "x\n", "unmerged work");
    run_git(&repo.path, &["checkout", "main"]);
    repo.commit("more.txt", "y\n", "advance main");

    let cli = run_git(
        &repo.path,
        &["branch", "--merged", "main", "--format=%(refname:short)"],
    );
    let mut cli_names: Vec<&str> = cli.lines().collect();
    let native = git_merged_branch_names(repo.path_str(), "main".into()).unwrap();
    let mut native_sorted: Vec<&str> = native.iter().map(String::as_str).collect();
    cli_names.sort_unstable();
    native_sorted.sort_unstable();
    assert_eq!(cli_names, native_sorted);
    assert!(native.iter().any(|n| n == "merged-branch"));
    assert!(!native.iter().any(|n| n == "unmerged"));
}
