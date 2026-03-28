use crate::error::Error;
use crate::repo;
use crate::types::{StatusSummary, WorktreeStatusInfo};

// ---------------------------------------------------------------------------
// Internal: StatusEntry mirrors a single line of `git status --porcelain`
// ---------------------------------------------------------------------------

struct StatusEntry {
    index_code: char,    // X: staged status (' ', 'A', 'M', 'D', 'R', '?')
    worktree_code: char, // Y: unstaged status (' ', 'M', 'D', '?')
    path: String,
    orig_path: Option<String>,
}

impl StatusEntry {
    fn to_porcelain(&self) -> String {
        if let Some(ref orig) = self.orig_path {
            format!(
                "{}{} {} -> {}",
                self.index_code, self.worktree_code, orig, self.path
            )
        } else {
            format!("{}{} {}", self.index_code, self.worktree_code, self.path)
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: Collect all status entries via gix
// ---------------------------------------------------------------------------

fn collect_status_entries(repo_path: &str) -> Result<Vec<StatusEntry>, Error> {
    use gix::bstr::ByteSlice;
    use gix::diff::index::ChangeRef as TIChange;
    use gix::dir::entry::Status as DirStatus;
    use gix::status::index_worktree;
    use gix::status::plumbing::index_as_worktree::{Change, EntryStatus};
    use gix::status::Item;
    use gix::status::UntrackedFiles;

    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let iter = repo
        .status(gix::progress::Discard)
        .map_err(Error::internal)?
        .untracked_files(UntrackedFiles::Files)
        .into_iter(Vec::new())
        .map_err(Error::internal)?;

    let mut entries = Vec::new();

    for item in iter {
        let item = item.map_err(Error::internal)?;

        match item {
            // ── HEAD-vs-index (staged) changes ──────────────────────
            Item::TreeIndex(change) => {
                let (path_bstr, _idx, _mode, _id) = change.fields();
                let path = path_bstr.to_str_lossy().to_string();
                let code = match &change {
                    TIChange::Addition { .. } => 'A',
                    TIChange::Deletion { .. } => 'D',
                    TIChange::Modification { .. } => 'M',
                    TIChange::Rewrite {
                        source_location,
                        copy,
                        ..
                    } => {
                        let orig = source_location.to_str_lossy().to_string();
                        let code = if *copy { 'C' } else { 'R' };
                        entries.push(StatusEntry {
                            index_code: code,
                            worktree_code: ' ',
                            path,
                            orig_path: Some(orig),
                        });
                        continue;
                    }
                };
                entries.push(StatusEntry {
                    index_code: code,
                    worktree_code: ' ',
                    path,
                    orig_path: None,
                });
            }

            // ── Index-vs-worktree (unstaged) changes ────────────────
            Item::IndexWorktree(iw_item) => match &iw_item {
                index_worktree::Item::Modification {
                    rela_path, status, ..
                } => {
                    let path = rela_path.to_str_lossy().to_string();
                    let code = match status {
                        EntryStatus::Conflict { .. } => {
                            entries.push(StatusEntry {
                                index_code: 'U',
                                worktree_code: 'U',
                                path,
                                orig_path: None,
                            });
                            continue;
                        }
                        EntryStatus::Change(change) => match change {
                            Change::Removed => 'D',
                            Change::Type { .. } => 'T',
                            Change::Modification { .. } | Change::SubmoduleModification(_) => 'M',
                        },
                        EntryStatus::IntentToAdd => 'A',
                        EntryStatus::NeedsUpdate(_) => continue,
                    };
                    entries.push(StatusEntry {
                        index_code: ' ',
                        worktree_code: code,
                        path,
                        orig_path: None,
                    });
                }
                index_worktree::Item::DirectoryContents { entry, .. } => {
                    if matches!(entry.status, DirStatus::Untracked) {
                        let path = entry.rela_path.to_str_lossy().to_string();
                        entries.push(StatusEntry {
                            index_code: '?',
                            worktree_code: '?',
                            path,
                            orig_path: None,
                        });
                    }
                }
                index_worktree::Item::Rewrite {
                    source,
                    dirwalk_entry,
                    copy,
                    ..
                } => {
                    let path = dirwalk_entry.rela_path.to_str_lossy().to_string();
                    let orig = source.rela_path().to_str_lossy().to_string();
                    let code = if *copy { 'C' } else { 'R' };
                    entries.push(StatusEntry {
                        index_code: ' ',
                        worktree_code: code,
                        path,
                        orig_path: Some(orig),
                    });
                }
            },
        }
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns `true` if the working tree has no staged, unstaged, or untracked changes.
pub fn is_clean(repo_path: &str) -> Result<bool, Error> {
    let entries = collect_status_entries(repo_path)?;
    Ok(entries.is_empty())
}

/// Returns a count of changed files and a porcelain-format output string.
pub fn status_summary(repo_path: &str) -> Result<StatusSummary, Error> {
    let entries = collect_status_entries(repo_path)?;
    let file_count = entries.len() as u32;
    let output = if entries.is_empty() {
        String::new()
    } else {
        entries
            .iter()
            .map(StatusEntry::to_porcelain)
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(StatusSummary { file_count, output })
}

/// Returns sorted list of all tracked files in the index.
pub fn list_tracked_files(repo_path: &str) -> Result<Vec<String>, Error> {
    use gix::bstr::ByteSlice;

    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();
    let index = repo.index_or_empty().map_err(Error::internal)?;

    let mut files: Vec<String> = index
        .entries()
        .iter()
        .map(|e| e.path(&index).to_str_lossy().to_string())
        .collect();
    files.sort();
    Ok(files)
}

/// Returns list of untracked files (respects .gitignore).
pub fn list_untracked_files(repo_path: &str) -> Result<Vec<String>, Error> {
    use gix::bstr::ByteSlice;
    use gix::dir::entry::Status as DirStatus;
    use gix::status::index_worktree;
    use gix::status::Item;
    use gix::status::UntrackedFiles;

    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let iter = repo
        .status(gix::progress::Discard)
        .map_err(Error::internal)?
        .untracked_files(UntrackedFiles::Files)
        .into_iter(Vec::new())
        .map_err(Error::internal)?;

    let mut files = Vec::new();

    for item in iter {
        let item = item.map_err(Error::internal)?;
        if let Item::IndexWorktree(index_worktree::Item::DirectoryContents { entry, .. }) = &item {
            if matches!(entry.status, DirStatus::Untracked) {
                files.push(entry.rela_path.to_str_lossy().to_string());
            }
        }
    }

    files.sort();
    Ok(files)
}

/// Returns deduplicated list of all changed file paths (staged, unstaged, untracked).
/// For renames, returns the destination path.
pub fn changed_paths(repo_path: &str) -> Result<Vec<String>, Error> {
    let entries = collect_status_entries(repo_path)?;
    let mut paths: Vec<String> = entries.into_iter().map(|e| e.path).collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Returns whether the worktree is dirty and how many files changed.
pub fn worktree_status(path: &str) -> Result<WorktreeStatusInfo, Error> {
    let entries = collect_status_entries(path)?;
    let file_count = entries.len() as u32;
    Ok(WorktreeStatusInfo {
        is_dirty: file_count > 0,
        file_count,
    })
}
