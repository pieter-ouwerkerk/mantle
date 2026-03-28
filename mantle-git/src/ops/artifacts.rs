use std::collections::HashSet;
use std::path::Path;

use bstr::ByteSlice;
use gix_ignore::search::Ignore;

use crate::error::MantleError;
use crate::types::{
    ArtifactType, CloneCandidate, ConfigSource, EffectiveEntry, EffectiveSource,
    EffectiveWorktreeinclude, FileCandidate, GeneratedWorktreeinclude, HydrationStrategy,
    IncludeSource, WorktreeIncludeResult,
};

/// Build a `gix_ignore::Search` from a file on disk.
/// Returns `None` if the file doesn't exist or can't be read.
fn build_search_from_file(file_path: &Path, repo_root: &Path) -> Option<gix_ignore::Search> {
    let bytes = std::fs::read(file_path).ok()?;
    let mut search = gix_ignore::Search::default();
    search.add_patterns_buffer(
        &bytes,
        file_path,
        Some(repo_root),
        Ignore {
            support_precious: false,
        },
    );
    Some(search)
}

/// Scan a repository using `.worktreeinclude` (or `.gitignore` fallback) with full
/// gitignore pattern syntax via `gix_ignore`. Returns both directory candidates (`CoW` clone)
/// and file candidates (copy).
pub fn scan_worktreeinclude(
    repo_path: &str,
    worktree_path: &str,
    gitignore_fallback: bool,
) -> Result<WorktreeIncludeResult, MantleError> {
    let repo_root = Path::new(repo_path);
    let worktree_root = Path::new(worktree_path);

    if !repo_root.is_dir() {
        return Err(MantleError::RepoNotFound {
            path: repo_path.to_owned(),
        });
    }

    // 1. Read the include file (.worktreeinclude or .gitignore fallback)
    let worktreeinclude_path = repo_root.join(".worktreeinclude");
    let gitignore_path = repo_root.join(".gitignore");

    let (bytes, source) = if worktreeinclude_path.is_file() {
        match std::fs::read(&worktreeinclude_path) {
            Ok(b) => (b, IncludeSource::Worktreeinclude),
            Err(_) => {
                return Ok(WorktreeIncludeResult {
                    clone_candidates: Vec::new(),
                    file_candidates: Vec::new(),
                    source: IncludeSource::None,
                });
            }
        }
    } else if gitignore_fallback && gitignore_path.is_file() {
        match std::fs::read(&gitignore_path) {
            Ok(b) => (b, IncludeSource::GitignoreFallback),
            Err(_) => {
                return Ok(WorktreeIncludeResult {
                    clone_candidates: Vec::new(),
                    file_candidates: Vec::new(),
                    source: IncludeSource::None,
                });
            }
        }
    } else {
        return Ok(WorktreeIncludeResult {
            clone_candidates: Vec::new(),
            file_candidates: Vec::new(),
            source: IncludeSource::None,
        });
    };

    // 2. Parse patterns using gix_ignore
    let mut search = gix_ignore::Search::default();
    let source_path = if source == IncludeSource::Worktreeinclude {
        &worktreeinclude_path
    } else {
        &gitignore_path
    };
    search.add_patterns_buffer(
        &bytes,
        source_path,
        Some(repo_root),
        Ignore {
            support_precious: false,
        },
    );

    // 3. Walk the repo tree recursively
    let mut clone_candidates = Vec::new();
    let mut file_candidates = Vec::new();

    let candidate_config_source = match source {
        IncludeSource::Worktreeinclude => ConfigSource::Worktreeinclude,
        IncludeSource::GitignoreFallback => ConfigSource::Gitignore,
        IncludeSource::None => ConfigSource::BuiltIn, // unreachable — we return early for None
    };
    walk_tree(
        repo_root,
        repo_root,
        worktree_root,
        &search,
        candidate_config_source,
        &mut clone_candidates,
        &mut file_candidates,
    );

    Ok(WorktreeIncludeResult {
        clone_candidates,
        file_candidates,
        source,
    })
}

fn walk_tree(
    dir: &Path,
    repo_root: &Path,
    worktree_root: &Path,
    search: &gix_ignore::Search,
    candidate_config_source: ConfigSource,
    clone_candidates: &mut Vec<CloneCandidate>,
    file_candidates: &mut Vec<FileCandidate>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };

        // Skip symlinks
        if ft.is_symlink() {
            continue;
        }

        let path = entry.path();
        let Ok(rel) = path.strip_prefix(repo_root) else {
            continue;
        };

        let rel_str = rel.to_string_lossy();

        // Skip .git/
        if rel_str == ".git" || rel_str.starts_with(".git/") {
            continue;
        }

        let is_dir = ft.is_dir();
        let rel_as_bstr = rel_str.as_bytes().as_bstr();

        let matched = search.pattern_matching_relative_path(
            rel_as_bstr,
            Some(is_dir),
            gix_ignore::glob::pattern::Case::Sensitive,
        );

        match matched {
            Some(m) if !m.pattern.is_negative() => {
                if is_dir {
                    // Directory → CloneCandidate (don't recurse into it)
                    let dest_dir = worktree_root.join(rel);
                    if dest_dir.is_dir() {
                        // Already exists in worktree, skip
                        continue;
                    }
                    let size_bytes = dir_size(&path);
                    clone_candidates.push(CloneCandidate {
                        source_path: path.to_string_lossy().to_string(),
                        dest_path: dest_dir.to_string_lossy().to_string(),
                        artifact_type: ArtifactType::Generic,
                        lockfile_matches: true,
                        size_bytes,
                        strategy: HydrationStrategy::CowClone,
                        skip_reason: None,
                        config_source: candidate_config_source,
                    });
                } else {
                    // File → FileCandidate
                    let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    file_candidates.push(FileCandidate {
                        relative_path: rel_str.to_string(),
                        absolute_path: path.to_string_lossy().to_string(),
                        size_bytes,
                    });
                }
            }
            _ => {
                // Not matched (or negated) — recurse into directories
                if is_dir {
                    walk_tree(
                        dir.join(entry.file_name()).as_path(),
                        repo_root,
                        worktree_root,
                        search,
                        candidate_config_source,
                        clone_candidates,
                        file_candidates,
                    );
                }
            }
        }
    }
}

/// Scan a repository for known artifact directories that could be cloned (`CoW`)
/// into a worktree. Compares lockfiles byte-for-byte to determine safety.
///
/// This is the legacy entry point. It delegates to `scan_worktreeinclude` for
/// pattern-matched candidates and combines them with built-in artifact scanning.
pub fn scan_clone_candidates(
    repo_path: &str,
    worktree_path: &str,
) -> Result<Vec<CloneCandidate>, MantleError> {
    let result = scan_worktreeinclude(repo_path, worktree_path, true)?;
    Ok(result.clone_candidates)
}

/// Calculate total size of a directory by walking all files recursively.
fn dir_size(path: &Path) -> u64 {
    walkdir(path)
}

fn walkdir(path: &Path) -> u64 {
    let mut total: u64 = 0;

    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };

    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };

        if ft.is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        } else if ft.is_dir() {
            total += walkdir(&entry.path());
        }
        // Symlinks are intentionally skipped
    }

    total
}

/// Compute the full effective hydration config: what would be hydrated, from which
/// source, plus suggestions for large uncovered directories.
///
/// Uses `gix_ignore::Search` to determine which top-level entries match the
/// `.worktreeinclude` or `.gitignore` patterns.
pub fn compute_effective_worktreeinclude(
    repo_path: &str,
    size_threshold_bytes: u64,
) -> Result<EffectiveWorktreeinclude, MantleError> {
    let repo = Path::new(repo_path);
    if !repo.is_dir() {
        return Err(MantleError::RepoNotFound {
            path: repo_path.to_owned(),
        });
    }

    let has_worktreeinclude_file = repo.join(".worktreeinclude").is_file();

    // Try .worktreeinclude first, then .gitignore
    let (search, config_source, effective_source) = if has_worktreeinclude_file {
        match build_search_from_file(&repo.join(".worktreeinclude"), repo) {
            Some(s) => (
                Some(s),
                ConfigSource::Worktreeinclude,
                EffectiveSource::Worktreeinclude,
            ),
            None => (None, ConfigSource::BuiltIn, EffectiveSource::BuiltIn),
        }
    } else if repo.join(".gitignore").is_file() {
        match build_search_from_file(&repo.join(".gitignore"), repo) {
            Some(s) => (Some(s), ConfigSource::Gitignore, EffectiveSource::Gitignore),
            None => (None, ConfigSource::BuiltIn, EffectiveSource::BuiltIn),
        }
    } else {
        (None, ConfigSource::BuiltIn, EffectiveSource::BuiltIn)
    };

    let mut entries = Vec::new();
    let mut covered_dirs: HashSet<String> = HashSet::new();
    let mut suggestions = Vec::new();

    if let Some(ref search) = search {
        collect_matched_entries(
            repo,
            search,
            effective_source,
            size_threshold_bytes,
            &mut entries,
            &mut covered_dirs,
            &mut suggestions,
        );
    } else {
        collect_large_dir_suggestions(repo, size_threshold_bytes, &mut entries, &mut suggestions);
    }

    Ok(EffectiveWorktreeinclude {
        entries,
        config_source,
        has_worktreeinclude_file,
        suggestions,
    })
}

fn collect_matched_entries(
    repo: &Path,
    search: &gix_ignore::Search,
    effective_source: EffectiveSource,
    size_threshold_bytes: u64,
    entries: &mut Vec<EffectiveEntry>,
    covered_dirs: &mut HashSet<String>,
    suggestions: &mut Vec<String>,
) {
    let Ok(read_dir) = std::fs::read_dir(repo) else {
        return;
    };
    for entry in read_dir.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let is_dir = ft.is_dir();
        let matched = search.pattern_matching_relative_path(
            name.as_bytes().as_bstr(),
            Some(is_dir),
            gix_ignore::glob::pattern::Case::Sensitive,
        );
        let is_matched = matches!(matched, Some(m) if !m.pattern.is_negative());
        if is_matched {
            let size = if is_dir {
                dir_size(&entry.path())
            } else {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            };
            entries.push(EffectiveEntry {
                path: name.clone(),
                source: effective_source,
                exists_on_disk: true,
                size_bytes: size,
                included: true,
            });
            covered_dirs.insert(name);
        } else if is_dir && !name.starts_with('.') {
            let size = dir_size(&entry.path());
            if size >= size_threshold_bytes {
                let size_mb = size / (1024 * 1024);
                suggestions.push(format!(
                    "'{name}' is {size_mb}MB and not included in hydration — consider adding it"
                ));
                entries.push(EffectiveEntry {
                    path: name.clone(),
                    source: EffectiveSource::Suggestion,
                    exists_on_disk: true,
                    size_bytes: size,
                    included: false,
                });
                covered_dirs.insert(name);
            }
        }
    }
}

fn collect_large_dir_suggestions(
    repo: &Path,
    size_threshold_bytes: u64,
    entries: &mut Vec<EffectiveEntry>,
    suggestions: &mut Vec<String>,
) {
    let Ok(read_dir) = std::fs::read_dir(repo) else {
        return;
    };
    for entry in read_dir.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() || ft.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let size = dir_size(&entry.path());
        if size >= size_threshold_bytes {
            let size_mb = size / (1024 * 1024);
            suggestions.push(format!(
                "'{name}' is {size_mb}MB and not included in hydration — consider adding it"
            ));
            entries.push(EffectiveEntry {
                path: name,
                source: EffectiveSource::Suggestion,
                exists_on_disk: true,
                size_bytes: size,
                included: false,
            });
        }
    }
}

/// Generate a `.worktreeinclude` file with sensible defaults for a repository.
///
/// If `.gitignore` exists, the generated `.worktreeinclude` contains the same patterns
/// with a different header comment. If no `.gitignore` exists, generates an empty file
/// with just comments.
///
/// Returns the generated content and metadata. Does not write to disk — the caller
/// decides whether to persist. If `.worktreeinclude` already exists, `already_exists`
/// is set to true and the content reflects what *would* be generated (for preview).
pub fn generate_default_worktreeinclude(
    repo_path: &str,
) -> Result<GeneratedWorktreeinclude, MantleError> {
    let repo = Path::new(repo_path);
    if !repo.is_dir() {
        return Err(MantleError::RepoNotFound {
            path: repo_path.to_owned(),
        });
    }

    let already_exists = repo.join(".worktreeinclude").is_file();

    let header = "# Hydration config — generated by Cuttlefish\n\
                  #\n\
                  # Patterns listed here use gitignore syntax.\n\
                  # Matched directories are CoW-cloned into new worktrees.\n\
                  # Matched files are copied into new worktrees.\n";

    let gitignore_content = std::fs::read_to_string(repo.join(".gitignore")).ok();

    // Collect directory names that exist on disk from gitignore patterns
    let mut gitignore_dirs: Vec<String> = Vec::new();
    if gitignore_content.is_some() {
        // Use gix_ignore to find which top-level dirs match
        if let Some(search) = build_search_from_file(&repo.join(".gitignore"), repo) {
            if let Ok(read_dir) = std::fs::read_dir(repo) {
                for entry in read_dir.flatten() {
                    let Ok(ft) = entry.file_type() else { continue };
                    if !ft.is_dir() || ft.is_symlink() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name == ".git" {
                        continue;
                    }
                    let matched = search.pattern_matching_relative_path(
                        name.as_bytes().as_bstr(),
                        Some(true),
                        gix_ignore::glob::pattern::Case::Sensitive,
                    );
                    if matches!(matched, Some(m) if !m.pattern.is_negative()) {
                        gitignore_dirs.push(name);
                    }
                }
            }
        }
        gitignore_dirs.sort();
    }

    let content = match gitignore_content {
        Some(ref gc) => format!("{header}\n{}\n", gc.trim_end()),
        None => format!("{header}\n"),
    };

    Ok(GeneratedWorktreeinclude {
        content,
        builtin_dirs: Vec::new(), // No more built-in specs
        gitignore_dirs,
        already_exists,
    })
}

/// Generate and write a `.worktreeinclude` file with sensible defaults.
/// Returns the generation result. If the file already exists, does not overwrite.
pub fn bootstrap_worktreeinclude(repo_path: &str) -> Result<GeneratedWorktreeinclude, MantleError> {
    let result = generate_default_worktreeinclude(repo_path)?;
    if !result.already_exists {
        let file_path = Path::new(repo_path).join(".worktreeinclude");
        std::fs::write(&file_path, &result.content).map_err(|e| MantleError::Internal {
            message: format!("Failed to write .worktreeinclude: {e}"),
        })?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_dirs() -> (TempDir, TempDir) {
        let repo = TempDir::new().unwrap();
        let worktree = TempDir::new().unwrap();
        (repo, worktree)
    }

    #[test]
    fn test_no_artifact_dirs() {
        let (repo, worktree) = setup_dirs();
        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn test_node_modules_with_matching_lockfile() {
        let (repo, worktree) = setup_dirs();

        // Create node_modules dir in repo
        fs::create_dir(repo.path().join("node_modules")).unwrap();
        // Create a file inside to test size calculation
        fs::write(repo.path().join("node_modules/test.js"), "hello").unwrap();

        // Create matching lockfiles
        fs::write(repo.path().join("package-lock.json"), "lockfile-content").unwrap();
        fs::write(
            worktree.path().join("package-lock.json"),
            "lockfile-content",
        )
        .unwrap();

        // scan_clone_candidates delegates to scan_worktreeinclude; provide .gitignore
        fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].artifact_type, ArtifactType::Generic);
        assert!(result[0].lockfile_matches);
        assert_eq!(result[0].size_bytes, 5); // "hello" = 5 bytes
    }

    #[test]
    fn test_node_modules_with_mismatched_lockfile() {
        let (repo, worktree) = setup_dirs();

        fs::create_dir(repo.path().join("node_modules")).unwrap();
        fs::write(repo.path().join("package-lock.json"), "version-1").unwrap();
        fs::write(worktree.path().join("package-lock.json"), "version-2").unwrap();
        fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].lockfile_matches);
    }

    #[test]
    fn test_pnpm_detected_as_generic() {
        let (repo, worktree) = setup_dirs();

        fs::create_dir(repo.path().join("node_modules")).unwrap();
        fs::write(repo.path().join("pnpm-lock.yaml"), "pnpm-lock").unwrap();
        fs::write(worktree.path().join("pnpm-lock.yaml"), "pnpm-lock").unwrap();
        fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].artifact_type, ArtifactType::Generic);
        assert!(result[0].lockfile_matches);
    }

    #[test]
    fn test_rust_target_dir() {
        let (repo, worktree) = setup_dirs();

        fs::create_dir(repo.path().join("target")).unwrap();
        fs::write(repo.path().join("Cargo.lock"), "cargo-lock").unwrap();
        fs::write(worktree.path().join("Cargo.lock"), "cargo-lock").unwrap();
        fs::write(repo.path().join(".gitignore"), "target/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].artifact_type, ArtifactType::Generic);
        assert!(result[0].lockfile_matches);
    }

    #[test]
    fn test_python_venv() {
        let (repo, worktree) = setup_dirs();

        fs::create_dir(repo.path().join("venv")).unwrap();
        fs::write(repo.path().join("requirements.txt"), "flask==2.0").unwrap();
        fs::write(worktree.path().join("requirements.txt"), "flask==2.0").unwrap();
        fs::write(repo.path().join(".gitignore"), "venv/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].artifact_type, ArtifactType::Generic);
        assert!(result[0].lockfile_matches);
    }

    #[test]
    fn test_generic_dir_no_lockfile() {
        let (repo, worktree) = setup_dirs();

        fs::create_dir(repo.path().join("dist")).unwrap();
        fs::write(repo.path().join(".gitignore"), "dist/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].artifact_type, ArtifactType::Generic);
        assert!(result[0].lockfile_matches);
    }

    #[test]
    fn test_swift_build_dir() {
        let (repo, worktree) = setup_dirs();

        fs::create_dir(repo.path().join(".build")).unwrap();
        fs::write(repo.path().join(".gitignore"), ".build/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].artifact_type, ArtifactType::Generic);
    }

    #[test]
    fn test_missing_worktree_lockfile() {
        let (repo, worktree) = setup_dirs();

        fs::create_dir(repo.path().join("node_modules")).unwrap();
        fs::write(repo.path().join("package-lock.json"), "content").unwrap();
        // No lockfile in worktree
        fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].lockfile_matches);
    }

    #[test]
    fn test_dest_path_construction() {
        let (repo, worktree) = setup_dirs();

        fs::create_dir(repo.path().join("node_modules")).unwrap();
        fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        let expected_dest = worktree
            .path()
            .join("node_modules")
            .to_string_lossy()
            .to_string();
        assert_eq!(result[0].dest_path, expected_dest);
    }

    #[test]
    fn test_multiple_artifact_dirs() {
        let (repo, worktree) = setup_dirs();

        fs::create_dir(repo.path().join("node_modules")).unwrap();
        fs::create_dir(repo.path().join("target")).unwrap();
        fs::create_dir(repo.path().join("dist")).unwrap();
        fs::write(
            repo.path().join(".gitignore"),
            "node_modules/\ntarget/\ndist/\n",
        )
        .unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_dir_size_recursive() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("test_dir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("a.txt"), "aaaa").unwrap(); // 4 bytes
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("b.txt"), "bb").unwrap(); // 2 bytes

        assert_eq!(super::dir_size(&dir), 6);
    }

    #[test]
    fn test_invalid_repo_path() {
        let result = scan_clone_candidates("/nonexistent/path", "/tmp");
        assert!(result.is_err());
    }

    // --- .worktreeinclude integration tests ---

    #[test]
    fn test_worktreeinclude_excludes_default() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("dist")).unwrap();
        fs::create_dir(repo.path().join("node_modules")).unwrap();
        fs::write(
            repo.path().join(".worktreeinclude"),
            "node_modules\n!dist\n",
        )
        .unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].source_path.ends_with("node_modules"));
    }

    #[test]
    fn test_worktreeinclude_adds_custom_dir() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("vendor")).unwrap();
        fs::write(repo.path().join("vendor/lib.so"), "binary").unwrap();
        fs::write(repo.path().join(".worktreeinclude"), "vendor\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].source_path.ends_with("vendor"));
        assert_eq!(result[0].artifact_type, ArtifactType::Generic);
        assert!(result[0].lockfile_matches);
        assert_eq!(result[0].size_bytes, 6);
    }

    #[test]
    fn test_worktreeinclude_custom_dir_not_present() {
        let (repo, worktree) = setup_dirs();
        fs::write(repo.path().join(".worktreeinclude"), "vendor\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn test_worktreeinclude_missing_file_uses_gitignore() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("dist")).unwrap();
        // Without .worktreeinclude, scan_clone_candidates falls back to .gitignore
        fs::write(repo.path().join(".gitignore"), "dist/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].source_path.ends_with("dist"));
    }

    #[test]
    fn test_worktreeinclude_redundant_addition_ignored() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("dist")).unwrap();
        fs::write(repo.path().join(".worktreeinclude"), "dist\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_worktreeinclude_exclude_and_add_custom() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("dist")).unwrap();
        fs::create_dir(repo.path().join("vendor")).unwrap();
        fs::write(repo.path().join("vendor/data"), "stuff").unwrap();
        fs::write(repo.path().join(".worktreeinclude"), "!dist\nvendor\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].source_path.ends_with("vendor"));
        assert_eq!(result[0].artifact_type, ArtifactType::Generic);
        assert!(result[0].lockfile_matches);
        assert_eq!(result[0].size_bytes, 5);
    }

    // --- .gitignore fallback integration tests ---

    #[test]
    fn test_gitignore_fallback_when_no_worktreeinclude() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("vendor")).unwrap();
        fs::write(repo.path().join("vendor/lib.so"), "binary").unwrap();
        fs::write(repo.path().join(".gitignore"), "vendor/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].source_path.ends_with("vendor"));
        assert_eq!(result[0].config_source, ConfigSource::Gitignore);
    }

    #[test]
    fn test_worktreeinclude_takes_precedence() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("vendor")).unwrap();
        fs::write(repo.path().join("vendor/lib.so"), "binary").unwrap();
        fs::write(repo.path().join(".gitignore"), "vendor/\ncache/\n").unwrap();
        fs::write(repo.path().join(".worktreeinclude"), "vendor\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].source_path.ends_with("vendor"));
        assert_eq!(result[0].config_source, ConfigSource::Worktreeinclude);
    }

    #[test]
    fn test_gitignore_skips_nonexistent_dirs() {
        let (repo, worktree) = setup_dirs();
        fs::write(repo.path().join(".gitignore"), "vendor/\n").unwrap();
        // vendor dir does NOT exist

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn test_gitignore_dedupes_with_single_entry() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("node_modules")).unwrap();
        fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        // node_modules should appear once (from .gitignore)
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].config_source, ConfigSource::Gitignore);
    }

    #[test]
    fn test_skip_when_dest_already_exists() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("node_modules")).unwrap();
        fs::write(repo.path().join("package-lock.json"), "lock").unwrap();
        fs::write(worktree.path().join("package-lock.json"), "lock").unwrap();
        fs::create_dir(worktree.path().join("node_modules")).unwrap();
        fs::write(repo.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_no_lift_with_worktreeinclude() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir_all(repo.path().join("ThirdParty/A")).unwrap();
        fs::create_dir_all(repo.path().join("ThirdParty/B")).unwrap();
        // .worktreeinclude present → gitignore not used, no lifting
        fs::write(
            repo.path().join(".worktreeinclude"),
            "ThirdParty/A\nThirdParty/B\n",
        )
        .unwrap();

        let result = scan_clone_candidates(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
        )
        .unwrap();

        // Should NOT be lifted — .worktreeinclude is used verbatim
        assert_eq!(result.len(), 2);
        let paths: Vec<&str> = result.iter().map(|c| c.source_path.as_str()).collect();
        assert!(paths.iter().any(|p| p.ends_with("ThirdParty/A")));
        assert!(paths.iter().any(|p| p.ends_with("ThirdParty/B")));
    }

    // --- Effective worktreeinclude tests ---

    #[test]
    fn test_effective_no_config() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("node_modules")).unwrap();

        let result =
            compute_effective_worktreeinclude(tmp.path().to_str().unwrap(), 100 * 1024 * 1024)
                .unwrap();

        assert!(!result.has_worktreeinclude_file);
        assert_eq!(result.config_source, ConfigSource::BuiltIn);
        // No config file means no entries (except suggestions)
        assert!(result.entries.is_empty());
    }

    #[test]
    fn test_effective_with_gitignore() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("node_modules")).unwrap();
        fs::write(tmp.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result =
            compute_effective_worktreeinclude(tmp.path().to_str().unwrap(), 100 * 1024 * 1024)
                .unwrap();

        assert!(!result.has_worktreeinclude_file);
        assert_eq!(result.config_source, ConfigSource::Gitignore);
        let nm = result
            .entries
            .iter()
            .find(|e| e.path == "node_modules")
            .unwrap();
        assert_eq!(nm.source, EffectiveSource::Gitignore);
        assert!(nm.exists_on_disk);
        assert!(nm.included);
    }

    #[test]
    fn test_effective_with_worktreeinclude() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("vendor")).unwrap();
        fs::write(tmp.path().join("vendor/data"), "stuff").unwrap();
        fs::write(tmp.path().join(".worktreeinclude"), "vendor\n").unwrap();

        let result =
            compute_effective_worktreeinclude(tmp.path().to_str().unwrap(), 100 * 1024 * 1024)
                .unwrap();

        assert!(result.has_worktreeinclude_file);
        assert_eq!(result.config_source, ConfigSource::Worktreeinclude);
        let vendor = result.entries.iter().find(|e| e.path == "vendor").unwrap();
        assert_eq!(vendor.source, EffectiveSource::Worktreeinclude);
        assert!(vendor.included);
    }

    #[test]
    fn test_effective_large_untracked_suggestion() {
        let tmp = TempDir::new().unwrap();
        // Create a large directory (>1KB threshold for test)
        fs::create_dir(tmp.path().join("big_stuff")).unwrap();
        let data = vec![0u8; 2048];
        fs::write(tmp.path().join("big_stuff/large.bin"), &data).unwrap();

        let result = compute_effective_worktreeinclude(
            tmp.path().to_str().unwrap(),
            1024, // 1KB threshold
        )
        .unwrap();

        let suggestion = result.entries.iter().find(|e| e.path == "big_stuff");
        assert!(suggestion.is_some());
        let s = suggestion.unwrap();
        assert_eq!(s.source, EffectiveSource::Suggestion);
        assert!(!s.included);
        assert!(!result.suggestions.is_empty());
    }

    #[test]
    fn test_effective_small_untracked_ignored() {
        let tmp = TempDir::new().unwrap();
        // Create a small directory (under threshold)
        fs::create_dir(tmp.path().join("tiny")).unwrap();
        fs::write(tmp.path().join("tiny/small.txt"), "hi").unwrap();

        let result = compute_effective_worktreeinclude(
            tmp.path().to_str().unwrap(),
            100 * 1024 * 1024, // 100MB threshold
        )
        .unwrap();

        let suggestion = result.entries.iter().find(|e| e.path == "tiny");
        assert!(suggestion.is_none());
        assert!(result.suggestions.is_empty());
    }

    // --- generate_default_worktreeinclude tests ---

    #[test]
    fn test_generate_empty_repo() {
        let tmp = TempDir::new().unwrap();
        let result = generate_default_worktreeinclude(tmp.path().to_str().unwrap()).unwrap();

        assert!(result.builtin_dirs.is_empty());
        assert!(result.gitignore_dirs.is_empty());
        assert!(!result.already_exists);
        assert!(result.content.starts_with("# Hydration config"));
    }

    #[test]
    fn test_generate_includes_gitignore_content() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("vendor")).unwrap();
        fs::create_dir(tmp.path().join("generated")).unwrap();
        fs::write(
            tmp.path().join(".gitignore"),
            "*.log\nvendor/\ngenerated/\n.DS_Store\n",
        )
        .unwrap();

        let result = generate_default_worktreeinclude(tmp.path().to_str().unwrap()).unwrap();

        assert!(result.builtin_dirs.is_empty()); // No more built-in specs
                                                 // gitignore_dirs lists directories that match via gix_ignore
        assert!(result.gitignore_dirs.contains(&"vendor".to_string()));
        assert!(result.gitignore_dirs.contains(&"generated".to_string()));
        // Content should contain the gitignore patterns
        assert!(result.content.contains("vendor/"));
        assert!(result.content.contains("generated/"));
    }

    #[test]
    fn test_generate_no_gitignore() {
        let tmp = TempDir::new().unwrap();

        let result = generate_default_worktreeinclude(tmp.path().to_str().unwrap()).unwrap();

        assert!(result.builtin_dirs.is_empty());
        assert!(result.gitignore_dirs.is_empty());
        assert!(result.content.starts_with("# Hydration config"));
        // Content should be just the header
        assert!(!result.content.contains("vendor"));
    }

    #[test]
    fn test_generate_detects_existing_file() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".worktreeinclude"), "vendor\n").unwrap();

        let result = generate_default_worktreeinclude(tmp.path().to_str().unwrap()).unwrap();

        assert!(result.already_exists);
    }

    #[test]
    fn test_generate_content_is_deterministic() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("vendor")).unwrap();
        fs::write(tmp.path().join(".gitignore"), "vendor/\n").unwrap();

        let r1 = generate_default_worktreeinclude(tmp.path().to_str().unwrap()).unwrap();
        let r2 = generate_default_worktreeinclude(tmp.path().to_str().unwrap()).unwrap();

        assert_eq!(r1.content, r2.content);
    }

    // --- bootstrap_worktreeinclude tests ---

    #[test]
    fn test_bootstrap_writes_file() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("vendor")).unwrap();
        fs::write(tmp.path().join(".gitignore"), "vendor/\n").unwrap();

        let result = bootstrap_worktreeinclude(tmp.path().to_str().unwrap()).unwrap();

        assert!(!result.already_exists);
        let written = fs::read_to_string(tmp.path().join(".worktreeinclude")).unwrap();
        assert_eq!(written, result.content);
    }

    #[test]
    fn test_bootstrap_does_not_overwrite() {
        let tmp = TempDir::new().unwrap();
        let existing = "# existing config\nmy-dir\n";
        fs::write(tmp.path().join(".worktreeinclude"), existing).unwrap();

        let result = bootstrap_worktreeinclude(tmp.path().to_str().unwrap()).unwrap();

        assert!(result.already_exists);
        // Original file should be untouched
        let on_disk = fs::read_to_string(tmp.path().join(".worktreeinclude")).unwrap();
        assert_eq!(on_disk, existing);
    }

    #[test]
    fn test_generate_invalid_path() {
        let result = generate_default_worktreeinclude("/nonexistent/path/to/repo");
        assert!(result.is_err());
    }

    // --- scan_worktreeinclude tests ---

    #[test]
    fn test_scan_worktreeinclude_file_pattern() {
        let (repo, worktree) = setup_dirs();
        fs::write(repo.path().join("config.env"), "SECRET=foo").unwrap();
        fs::write(repo.path().join(".worktreeinclude"), "config.env\n").unwrap();

        let result = scan_worktreeinclude(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            false,
        )
        .unwrap();

        assert_eq!(result.source, IncludeSource::Worktreeinclude);
        assert_eq!(result.file_candidates.len(), 1);
        assert_eq!(result.file_candidates[0].relative_path, "config.env");
        assert_eq!(result.file_candidates[0].size_bytes, 10);
        assert_eq!(result.clone_candidates.len(), 0);
    }

    #[test]
    fn test_scan_worktreeinclude_dir_pattern() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("build")).unwrap();
        fs::write(repo.path().join("build/out.o"), "obj").unwrap();
        fs::write(repo.path().join(".worktreeinclude"), "build/\n").unwrap();

        let result = scan_worktreeinclude(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            false,
        )
        .unwrap();

        assert_eq!(result.clone_candidates.len(), 1);
        assert!(result.clone_candidates[0].source_path.ends_with("build"));
        assert_eq!(
            result.clone_candidates[0].artifact_type,
            ArtifactType::Generic
        );
        assert_eq!(
            result.clone_candidates[0].strategy,
            HydrationStrategy::CowClone
        );
        assert_eq!(result.clone_candidates[0].size_bytes, 3);
        assert_eq!(result.file_candidates.len(), 0);
    }

    #[test]
    fn test_scan_worktreeinclude_mixed() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("build")).unwrap();
        fs::write(repo.path().join("build/out.o"), "obj").unwrap();
        fs::write(repo.path().join("config.env"), "SECRET=foo").unwrap();
        fs::write(repo.path().join(".worktreeinclude"), "build/\nconfig.env\n").unwrap();

        let result = scan_worktreeinclude(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            false,
        )
        .unwrap();

        assert_eq!(result.clone_candidates.len(), 1);
        assert_eq!(result.file_candidates.len(), 1);
        assert!(result.clone_candidates[0].source_path.ends_with("build"));
        assert_eq!(result.file_candidates[0].relative_path, "config.env");
    }

    #[test]
    fn test_scan_worktreeinclude_glob_pattern() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("Configs")).unwrap();
        fs::write(repo.path().join("Configs/A.xcconfig"), "a").unwrap();
        fs::write(repo.path().join("Configs/B.xcconfig"), "b").unwrap();
        fs::write(repo.path().join("Configs/C.txt"), "c").unwrap();
        fs::write(repo.path().join(".worktreeinclude"), "*.xcconfig\n").unwrap();

        let result = scan_worktreeinclude(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            false,
        )
        .unwrap();

        assert_eq!(result.file_candidates.len(), 2);
        let paths: Vec<&str> = result
            .file_candidates
            .iter()
            .map(|f| f.relative_path.as_str())
            .collect();
        assert!(paths.contains(&"Configs/A.xcconfig"));
        assert!(paths.contains(&"Configs/B.xcconfig"));
        assert_eq!(result.clone_candidates.len(), 0);
    }

    #[test]
    fn test_scan_worktreeinclude_negation() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("Configs")).unwrap();
        fs::write(repo.path().join("Configs/A.xcconfig"), "a").unwrap();
        fs::write(repo.path().join("Configs/B.xcconfig"), "b").unwrap();
        fs::write(
            repo.path().join(".worktreeinclude"),
            "*.xcconfig\n!Configs/B.xcconfig\n",
        )
        .unwrap();

        let result = scan_worktreeinclude(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            false,
        )
        .unwrap();

        assert_eq!(result.file_candidates.len(), 1);
        assert_eq!(
            result.file_candidates[0].relative_path,
            "Configs/A.xcconfig"
        );
    }

    #[test]
    fn test_scan_worktreeinclude_doublestar() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir_all(repo.path().join("a/b/c")).unwrap();
        fs::write(repo.path().join("a/b/c/secrets.env"), "pw").unwrap();
        fs::write(repo.path().join("a/top.env"), "pw2").unwrap();
        fs::write(repo.path().join(".worktreeinclude"), "**/*.env\n").unwrap();

        let result = scan_worktreeinclude(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            false,
        )
        .unwrap();

        assert_eq!(result.file_candidates.len(), 2);
        let paths: Vec<&str> = result
            .file_candidates
            .iter()
            .map(|f| f.relative_path.as_str())
            .collect();
        assert!(paths.contains(&"a/b/c/secrets.env"));
        assert!(paths.contains(&"a/top.env"));
    }

    #[test]
    fn test_scan_worktreeinclude_absent_with_gitignore_fallback() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("build")).unwrap();
        fs::write(repo.path().join("build/out.o"), "obj").unwrap();
        // No .worktreeinclude, but .gitignore exists
        fs::write(repo.path().join(".gitignore"), "build/\n").unwrap();

        let result = scan_worktreeinclude(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            true,
        )
        .unwrap();

        assert_eq!(result.source, IncludeSource::GitignoreFallback);
        assert_eq!(result.clone_candidates.len(), 1);
        assert!(result.clone_candidates[0].source_path.ends_with("build"));
        assert_eq!(
            result.clone_candidates[0].config_source,
            ConfigSource::Gitignore
        );
    }

    #[test]
    fn test_scan_worktreeinclude_absent_no_fallback() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("build")).unwrap();
        fs::write(repo.path().join("build/out.o"), "obj").unwrap();
        fs::write(repo.path().join(".gitignore"), "build/\n").unwrap();

        let result = scan_worktreeinclude(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            false,
        )
        .unwrap();

        assert_eq!(result.source, IncludeSource::None);
        assert!(result.clone_candidates.is_empty());
        assert!(result.file_candidates.is_empty());
    }

    #[test]
    fn test_scan_worktreeinclude_empty_file() {
        let (repo, worktree) = setup_dirs();
        fs::create_dir(repo.path().join("build")).unwrap();
        fs::write(
            repo.path().join(".worktreeinclude"),
            "# only comments\n# nothing else\n",
        )
        .unwrap();

        let result = scan_worktreeinclude(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            false,
        )
        .unwrap();

        assert_eq!(result.source, IncludeSource::Worktreeinclude);
        assert!(result.clone_candidates.is_empty());
        assert!(result.file_candidates.is_empty());
    }

    #[test]
    fn test_scan_worktreeinclude_dir_only_pattern_ignores_files() {
        let (repo, worktree) = setup_dirs();
        // Create a FILE named "build" (not a directory)
        fs::write(repo.path().join("build"), "i am a file").unwrap();
        // Also create a directory named "output"
        fs::create_dir(repo.path().join("output")).unwrap();
        // Pattern with trailing slash should only match directories
        fs::write(repo.path().join(".worktreeinclude"), "build/\noutput/\n").unwrap();

        let result = scan_worktreeinclude(
            repo.path().to_str().unwrap(),
            worktree.path().to_str().unwrap(),
            false,
        )
        .unwrap();

        // "build" is a file, but pattern "build/" only matches dirs → should not match
        assert_eq!(result.file_candidates.len(), 0);
        // "output" is a directory and matches "output/" → should be a clone candidate
        assert_eq!(result.clone_candidates.len(), 1);
        assert!(result.clone_candidates[0].source_path.ends_with("output"));
    }
}
