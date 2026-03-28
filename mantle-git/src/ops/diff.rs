use gix::bstr::ByteSlice;

use crate::error::Error;
use crate::repo;

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// Simplified change representation for accumulating diffs.
enum FileChange {
    Addition {
        id: gix::ObjectId,
        mode: u32,
    },
    Deletion {
        id: gix::ObjectId,
        mode: u32,
    },
    Modification {
        old_id: gix::ObjectId,
        new_id: gix::ObjectId,
        old_mode: u32,
        new_mode: u32,
    },
    Rename {
        old_path: String,
        old_id: gix::ObjectId,
        new_id: gix::ObjectId,
        old_mode: u32,
        new_mode: u32,
        similarity: u32,
    },
}

/// Convert gix tree EntryMode to u32 for diff headers.
fn mode_to_u32(mode: gix::object::tree::EntryMode) -> u32 {
    match mode.kind() {
        gix::object::tree::EntryKind::Blob => 0o100644,
        gix::object::tree::EntryKind::BlobExecutable => 0o100755,
        gix::object::tree::EntryKind::Link => 0o120000,
        gix::object::tree::EntryKind::Tree => 0o040000,
        gix::object::tree::EntryKind::Commit => 0o160000,
    }
}

// ---------------------------------------------------------------------------
// Internal: Format a unified diff for a single file
// ---------------------------------------------------------------------------

fn is_binary(data: &[u8]) -> bool {
    data.contains(&0)
}

/// Format a rename diff header + optional content diff.
fn format_rename_diff(
    old_path: &str,
    new_path: &str,
    old: &[u8],
    new: &[u8],
    similarity: u32,
    old_mode: u32,
    new_mode: u32,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("diff --git a/{old_path} b/{new_path}\n"));
    out.push_str(&format!("similarity index {similarity}%\n"));
    out.push_str(&format!("rename from {old_path}\n"));
    out.push_str(&format!("rename to {new_path}\n"));

    if old_mode != new_mode {
        out.push_str(&format!("old mode {:o}\n", old_mode));
        out.push_str(&format!("new mode {:o}\n", new_mode));
    }

    if is_binary(old) || is_binary(new) {
        out.push_str(&format!(
            "Binary files a/{old_path} and b/{new_path} differ\n"
        ));
        return out;
    }

    let old_str = String::from_utf8_lossy(old);
    let new_str = String::from_utf8_lossy(new);
    let input = imara_diff::intern::InternedInput::new(old_str.as_ref(), new_str.as_ref());
    let hunks = imara_diff::diff(
        imara_diff::Algorithm::Histogram,
        &input,
        imara_diff::UnifiedDiffBuilder::new(&input),
    );

    if !hunks.is_empty() {
        out.push_str(&format!("--- a/{old_path}\n"));
        out.push_str(&format!("+++ b/{new_path}\n"));
        out.push_str(&hunks);
    }

    out
}

fn format_blob_diff(
    path: &str,
    old: Option<&[u8]>,
    new: Option<&[u8]>,
    old_mode: Option<u32>,
    new_mode: Option<u32>,
) -> String {
    let mut out = String::new();

    // File-level header
    out.push_str(&format!("diff --git a/{path} b/{path}\n"));

    // Mode lines
    match (old_mode, new_mode) {
        (None, Some(m)) => {
            out.push_str(&format!("new file mode {:o}\n", m));
        }
        (Some(m), None) => {
            out.push_str(&format!("deleted file mode {:o}\n", m));
        }
        (Some(om), Some(nm)) if om != nm => {
            out.push_str(&format!("old mode {:o}\n", om));
            out.push_str(&format!("new mode {:o}\n", nm));
        }
        _ => {}
    }

    let old_bytes = old.unwrap_or(b"");
    let new_bytes = new.unwrap_or(b"");

    // Binary check
    if is_binary(old_bytes) || is_binary(new_bytes) {
        out.push_str(&format!("Binary files a/{path} and b/{path} differ\n"));
        return out;
    }

    // --- / +++ headers
    if old.is_none() {
        out.push_str("--- /dev/null\n");
    } else {
        out.push_str(&format!("--- a/{path}\n"));
    }
    if new.is_none() {
        out.push_str(&format!("+++ /dev/null\n"));
    } else {
        out.push_str(&format!("+++ b/{path}\n"));
    }

    // Use imara_diff to produce unified hunks
    let old_str = String::from_utf8_lossy(old_bytes);
    let new_str = String::from_utf8_lossy(new_bytes);

    let input = imara_diff::intern::InternedInput::new(old_str.as_ref(), new_str.as_ref());
    let hunks = imara_diff::diff(
        imara_diff::Algorithm::Histogram,
        &input,
        imara_diff::UnifiedDiffBuilder::new(&input),
    );

    if hunks.is_empty() {
        // No content difference — mode-only change
        return if old_mode != new_mode {
            let mut mode_only = String::new();
            mode_only.push_str(&format!("diff --git a/{path} b/{path}\n"));
            if let (Some(om), Some(nm)) = (old_mode, new_mode) {
                mode_only.push_str(&format!("old mode {:o}\n", om));
                mode_only.push_str(&format!("new mode {:o}\n", nm));
            }
            mode_only
        } else {
            String::new()
        };
    }

    out.push_str(&hunks);

    out
}

// ---------------------------------------------------------------------------
// Internal: Diff two trees and accumulate unified diff text
// ---------------------------------------------------------------------------

fn diff_trees(
    repo: &gix::Repository,
    old_tree: Option<gix::Tree<'_>>,
    new_tree: &gix::Tree<'_>,
) -> Result<String, Error> {
    use gix::object::tree::diff::Action;

    let mut diffs: Vec<(String, FileChange)> = Vec::new();

    match old_tree {
        Some(otree) => {
            let mut changes = otree.changes().map_err(Error::internal)?;
            changes
                .for_each_to_obtain_tree(new_tree, |change| {
                    // Skip tree (directory) entries — only diff blobs
                    let is_tree = match &change {
                        gix::object::tree::diff::Change::Addition { entry_mode, .. }
                        | gix::object::tree::diff::Change::Deletion { entry_mode, .. } => {
                            entry_mode.is_tree()
                        }
                        gix::object::tree::diff::Change::Modification { entry_mode, .. }
                        | gix::object::tree::diff::Change::Rewrite { entry_mode, .. } => {
                            entry_mode.is_tree()
                        }
                    };
                    if is_tree {
                        return Ok::<Action, std::convert::Infallible>(Action::Continue(()));
                    }

                    let path = change.location().to_str_lossy().to_string();
                    let fc = match &change {
                        gix::object::tree::diff::Change::Addition { entry_mode, id, .. } => {
                            FileChange::Addition {
                                id: id.detach(),
                                mode: mode_to_u32(*entry_mode),
                            }
                        }
                        gix::object::tree::diff::Change::Deletion { entry_mode, id, .. } => {
                            FileChange::Deletion {
                                id: id.detach(),
                                mode: mode_to_u32(*entry_mode),
                            }
                        }
                        gix::object::tree::diff::Change::Modification {
                            previous_entry_mode,
                            previous_id,
                            entry_mode,
                            id,
                            ..
                        } => FileChange::Modification {
                            old_id: previous_id.detach(),
                            new_id: id.detach(),
                            old_mode: mode_to_u32(*previous_entry_mode),
                            new_mode: mode_to_u32(*entry_mode),
                        },
                        gix::object::tree::diff::Change::Rewrite {
                            source_entry_mode,
                            source_id,
                            entry_mode,
                            id,
                            ..
                        } => FileChange::Modification {
                            old_id: source_id.detach(),
                            new_id: id.detach(),
                            old_mode: mode_to_u32(*source_entry_mode),
                            new_mode: mode_to_u32(*entry_mode),
                        },
                    };
                    diffs.push((path, fc));
                    Ok::<Action, std::convert::Infallible>(Action::Continue(()))
                })
                .map_err(Error::internal)?;
        }
        None => {
            // Empty old tree — treat all entries in new_tree as additions
            collect_tree_entries(repo, new_tree, "", &mut diffs)?;
        }
    }

    // Run rename detection on deletions + additions
    detect_renames(repo, &mut diffs)?;

    let mut result = String::new();

    for (path, change) in &diffs {
        let diff_text = match change {
            FileChange::Addition { id, mode } => {
                let blob = repo.find_object(*id).map_err(Error::internal)?;
                format_blob_diff(path, None, Some(&blob.data), None, Some(*mode))
            }
            FileChange::Deletion { id, mode } => {
                let blob = repo.find_object(*id).map_err(Error::internal)?;
                format_blob_diff(path, Some(&blob.data), None, Some(*mode), None)
            }
            FileChange::Modification {
                old_id,
                new_id,
                old_mode,
                new_mode,
            } => {
                let old_blob = repo.find_object(*old_id).map_err(Error::internal)?;
                let new_blob = repo.find_object(*new_id).map_err(Error::internal)?;
                format_blob_diff(
                    path,
                    Some(&old_blob.data),
                    Some(&new_blob.data),
                    Some(*old_mode),
                    Some(*new_mode),
                )
            }
            FileChange::Rename {
                old_path,
                old_id,
                new_id,
                old_mode,
                new_mode,
                similarity,
            } => {
                let old_blob = repo.find_object(*old_id).map_err(Error::internal)?;
                let new_blob = repo.find_object(*new_id).map_err(Error::internal)?;
                format_rename_diff(
                    old_path,
                    path,
                    &old_blob.data,
                    &new_blob.data,
                    *similarity,
                    *old_mode,
                    *new_mode,
                )
            }
        };

        if !diff_text.is_empty() {
            result.push_str(&diff_text);
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Rename detection
// ---------------------------------------------------------------------------

/// Minimum similarity (0–100) to consider a deletion+addition pair a rename.
const RENAME_THRESHOLD: u32 = 50;

/// Match deletions against additions by content similarity. Matched pairs are
/// replaced in-place with `FileChange::Rename`. Uses exact-match (by OID)
/// first, then falls back to line-level similarity for the remainder.
fn detect_renames(
    repo: &gix::Repository,
    diffs: &mut Vec<(String, FileChange)>,
) -> Result<(), Error> {
    // Collect indices of deletions and additions
    let deletions: Vec<usize> = diffs
        .iter()
        .enumerate()
        .filter_map(|(i, (_, c))| matches!(c, FileChange::Deletion { .. }).then_some(i))
        .collect();
    let additions: Vec<usize> = diffs
        .iter()
        .enumerate()
        .filter_map(|(i, (_, c))| matches!(c, FileChange::Addition { .. }).then_some(i))
        .collect();

    if deletions.is_empty() || additions.is_empty() {
        return Ok(());
    }

    // Phase 1: exact OID matches (100% similarity, O(n) with a hash set)
    let mut matched_del: Vec<bool> = vec![false; deletions.len()];
    let mut matched_add: Vec<bool> = vec![false; additions.len()];
    let mut renames: Vec<(usize, usize, u32)> = Vec::new(); // (del_idx, add_idx, similarity)

    // Build OID→add_index map for exact matching
    let mut oid_to_add: std::collections::HashMap<gix::ObjectId, Vec<usize>> =
        std::collections::HashMap::new();
    for (ai, &add_i) in additions.iter().enumerate() {
        if let FileChange::Addition { id, .. } = &diffs[add_i].1 {
            oid_to_add.entry(*id).or_default().push(ai);
        }
    }
    for (di, &del_i) in deletions.iter().enumerate() {
        if let FileChange::Deletion { id, .. } = &diffs[del_i].1 {
            if let Some(candidates) = oid_to_add.get(id) {
                for &ai in candidates {
                    if !matched_add[ai] {
                        matched_del[di] = true;
                        matched_add[ai] = true;
                        renames.push((del_i, additions[ai], 100));
                        break;
                    }
                }
            }
        }
    }

    // Phase 2: content similarity for remaining unmatched pairs
    // Load blob data for unmatched deletions and additions
    let unmatched_del: Vec<(usize, usize)> = deletions
        .iter()
        .enumerate()
        .filter(|(di, _)| !matched_del[*di])
        .map(|(_, &i)| (i, 0))
        .collect::<Vec<_>>();
    let unmatched_add: Vec<(usize, usize)> = additions
        .iter()
        .enumerate()
        .filter(|(ai, _)| !matched_add[*ai])
        .map(|(_, &i)| (i, 0))
        .collect::<Vec<_>>();

    if !unmatched_del.is_empty() && !unmatched_add.is_empty() {
        // Load blob content for similarity comparison
        let del_data: Vec<(usize, Vec<u8>)> = unmatched_del
            .iter()
            .filter_map(|(i, _)| {
                if let FileChange::Deletion { id, .. } = &diffs[*i].1 {
                    repo.find_object(*id).ok().map(|o| (*i, o.data.to_vec()))
                } else {
                    None
                }
            })
            .collect();
        let add_data: Vec<(usize, Vec<u8>)> = unmatched_add
            .iter()
            .filter_map(|(i, _)| {
                if let FileChange::Addition { id, .. } = &diffs[*i].1 {
                    repo.find_object(*id).ok().map(|o| (*i, o.data.to_vec()))
                } else {
                    None
                }
            })
            .collect();

        // Find best match for each deletion
        let mut used_adds: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for (del_i, del_bytes) in &del_data {
            let mut best: Option<(usize, u32)> = None;
            for (add_i, add_bytes) in &add_data {
                if used_adds.contains(add_i) {
                    continue;
                }
                let sim = content_similarity(del_bytes, add_bytes);
                if sim >= RENAME_THRESHOLD {
                    if best.is_none() || sim > best.unwrap().1 {
                        best = Some((*add_i, sim));
                    }
                }
            }
            if let Some((add_i, sim)) = best {
                used_adds.insert(add_i);
                renames.push((*del_i, add_i, sim));
            }
        }
    }

    // Apply renames: replace the addition entry with Rename, mark deletion for removal
    let mut to_remove: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (del_i, add_i, similarity) in renames {
        let old_path = diffs[del_i].0.clone();
        let (old_id, old_mode) = match &diffs[del_i].1 {
            FileChange::Deletion { id, mode } => (*id, *mode),
            _ => continue,
        };
        let (new_id, new_mode) = match &diffs[add_i].1 {
            FileChange::Addition { id, mode } => (*id, *mode),
            _ => continue,
        };
        diffs[add_i].1 = FileChange::Rename {
            old_path,
            old_id,
            new_id,
            old_mode,
            new_mode,
            similarity,
        };
        to_remove.insert(del_i);
    }

    // Remove consumed deletions (iterate in reverse to preserve indices)
    let mut remove_sorted: Vec<usize> = to_remove.into_iter().collect();
    remove_sorted.sort_unstable_by(|a, b| b.cmp(a));
    for i in remove_sorted {
        diffs.remove(i);
    }

    Ok(())
}

/// Compute line-level similarity between two byte slices (0–100).
fn content_similarity(old: &[u8], new: &[u8]) -> u32 {
    let old_str = String::from_utf8_lossy(old);
    let new_str = String::from_utf8_lossy(new);
    let old_lines: Vec<&str> = old_str.lines().collect();
    let new_lines: Vec<&str> = new_str.lines().collect();
    let total = old_lines.len().max(new_lines.len());
    if total == 0 {
        return 100;
    }

    // Count matching lines using a simple LCS-like approach via set intersection
    let mut old_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for line in &old_lines {
        *old_counts.entry(line).or_default() += 1;
    }
    let mut matched = 0usize;
    for line in &new_lines {
        if let Some(count) = old_counts.get_mut(line) {
            if *count > 0 {
                *count -= 1;
                matched += 1;
            }
        }
    }
    ((matched * 100) / total) as u32
}

/// Recursively collect all blob entries from a tree as additions.
fn collect_tree_entries(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    prefix: &str,
    out: &mut Vec<(String, FileChange)>,
) -> Result<(), Error> {
    for entry in tree.iter() {
        let entry = entry.map_err(Error::internal)?;
        let name = entry.filename().to_str_lossy();
        let full_path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        if entry.mode().is_tree() {
            if let Ok(obj) = repo.find_object(entry.oid().to_owned()) {
                if let Ok(subtree) = obj.try_into_tree() {
                    collect_tree_entries(repo, &subtree, &full_path, out)?;
                }
            }
        } else {
            out.push((
                full_path,
                FileChange::Addition {
                    id: entry.oid().to_owned(),
                    mode: mode_to_u32(entry.mode()),
                },
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Show the diff for a single commit (equivalent to `git show --format= --patch <hash>`).
pub fn show_diff(repo_path: &str, commit_hash: &str) -> Result<String, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let object = repo
        .rev_parse_single(commit_hash)
        .map_err(|_| Error::RevNotFound {
            rev: commit_hash.to_owned(),
        })?;
    let commit = object
        .object()
        .map_err(Error::internal)?
        .try_into_commit()
        .map_err(Error::internal)?;
    let new_tree = commit.tree().map_err(Error::internal)?;

    let parent_tree = commit
        .parent_ids()
        .next()
        .and_then(|pid| pid.object().ok())
        .and_then(|obj| obj.try_into_commit().ok())
        .and_then(|c| c.tree().ok());

    diff_trees(&repo, parent_tree, &new_tree)
}

/// Diff between two refs using their merge-base (equivalent to `git diff base...head`).
pub fn diff_between_refs(repo_path: &str, base: &str, head: &str) -> Result<String, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let base_obj = repo
        .rev_parse_single(base)
        .map_err(|_| Error::RevNotFound {
            rev: base.to_owned(),
        })?;
    let head_obj = repo
        .rev_parse_single(head)
        .map_err(|_| Error::RevNotFound {
            rev: head.to_owned(),
        })?;

    let base_id = base_obj.detach();
    let head_id = head_obj.detach();

    // Find merge-base using git2 (gix doesn't expose merge-base easily)
    let git2_repo = git2::Repository::open(repo_path).map_err(Error::internal)?;
    let merge_base_oid = git2_repo
        .merge_base(
            git2::Oid::from_bytes(base_id.as_slice()).map_err(Error::internal)?,
            git2::Oid::from_bytes(head_id.as_slice()).map_err(Error::internal)?,
        )
        .map_err(Error::internal)?;

    // Convert git2 OID back to gix
    let merge_base_gix = gix::ObjectId::from_bytes_or_panic(merge_base_oid.as_bytes());
    let merge_base_tree = repo
        .find_object(merge_base_gix)
        .map_err(Error::internal)?
        .try_into_commit()
        .map_err(Error::internal)?
        .tree()
        .map_err(Error::internal)?;

    let head_commit = repo
        .find_object(head_id)
        .map_err(Error::internal)?
        .try_into_commit()
        .map_err(Error::internal)?;
    let head_tree = head_commit.tree().map_err(Error::internal)?;

    diff_trees(&repo, Some(merge_base_tree), &head_tree)
}

/// A working-tree file entry with pre-loaded data for rename detection.
struct WtEntry {
    path: String,
    /// Set when this entry is a rename (the original path before renaming).
    old_path: Option<String>,
    old_data: Option<Vec<u8>>,
    new_data: Option<Vec<u8>>,
    old_mode: Option<u32>,
    new_mode: Option<u32>,
    /// Similarity percentage for renames.
    similarity: Option<u32>,
}

impl WtEntry {
    fn is_deletion(&self) -> bool {
        self.old_data.is_some() && self.new_data.is_none()
    }
    fn is_addition(&self) -> bool {
        self.old_data.is_none() && self.new_data.is_some()
    }
}

/// Working tree diff (equivalent to `git diff HEAD` + untracked file diffs).
pub fn working_tree_diff(repo_path: &str) -> Result<String, Error> {
    use gix::dir::entry::Status as DirStatus;
    use gix::status::index_worktree;
    use gix::status::Item;
    use gix::status::UntrackedFiles;

    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();
    let work_dir = repo
        .workdir()
        .ok_or_else(|| Error::Internal {
            message: "bare repository".to_owned(),
        })?
        .to_path_buf();

    let head_tree = repo.head_commit().ok().and_then(|c| c.tree().ok());

    let iter = repo
        .status(gix::progress::Discard)
        .map_err(Error::internal)?
        .untracked_files(UntrackedFiles::Files)
        .into_iter(Vec::new())
        .map_err(Error::internal)?;

    // Phase 1: collect all entries with their data
    let mut entries: Vec<WtEntry> = Vec::new();

    for item in iter {
        let item = item.map_err(Error::internal)?;

        match item {
            Item::TreeIndex(change) => {
                let (path_bstr, _idx, _mode, id) = change.fields();
                let path = path_bstr.to_str_lossy().to_string();
                let new_blob = repo.find_object(id).map_err(Error::internal)?;
                let (old_data, old_mode) = head_blob_for_path(&repo, head_tree.as_ref(), &path);
                entries.push(WtEntry {
                    path,
                    old_path: None,
                    old_data,
                    new_data: Some(new_blob.data.to_vec()),
                    old_mode,
                    new_mode: Some(0o100644),
                    similarity: None,
                });
            }
            Item::IndexWorktree(ref iw_item) => match iw_item {
                index_worktree::Item::Modification {
                    rela_path, status, ..
                } => {
                    use gix::status::plumbing::index_as_worktree::{Change, EntryStatus};
                    let path = rela_path.to_str_lossy().to_string();
                    match status {
                        EntryStatus::Change(Change::Removed) => {
                            let (old_data, old_mode) =
                                head_blob_for_path(&repo, head_tree.as_ref(), &path);
                            entries.push(WtEntry {
                                path,
                                old_path: None,
                                old_data,
                                new_data: None,
                                old_mode,
                                new_mode: None,
                                similarity: None,
                            });
                        }
                        EntryStatus::Change(Change::Modification { .. })
                        | EntryStatus::Change(Change::SubmoduleModification(_))
                        | EntryStatus::Change(Change::Type { .. }) => {
                            let (old_data, old_mode) =
                                head_blob_for_path(&repo, head_tree.as_ref(), &path);
                            let disk_path = work_dir.join(&path);
                            let new_data = std::fs::read(&disk_path).ok();
                            entries.push(WtEntry {
                                path,
                                old_path: None,
                                old_data,
                                new_data,
                                old_mode,
                                new_mode: Some(0o100644),
                                similarity: None,
                            });
                        }
                        _ => {}
                    }
                }
                index_worktree::Item::DirectoryContents { entry, .. } => {
                    if matches!(entry.status, DirStatus::Untracked) {
                        let path = entry.rela_path.to_str_lossy().to_string();
                        let disk_path = work_dir.join(&path);
                        let new_data = std::fs::read(&disk_path).ok();
                        entries.push(WtEntry {
                            path,
                            old_path: None,
                            old_data: None,
                            new_data,
                            old_mode: None,
                            new_mode: Some(0o100644),
                            similarity: None,
                        });
                    }
                }
                _ => {}
            },
        }
    }

    // Phase 2: rename detection on deletions ↔ additions
    detect_wt_renames(&mut entries);

    // Phase 3: format output
    let mut result = String::new();
    for entry in &entries {
        let diff_text = if let (Some(old_path), Some(sim)) = (&entry.old_path, entry.similarity) {
            format_rename_diff(
                old_path,
                &entry.path,
                entry.old_data.as_deref().unwrap_or(b""),
                entry.new_data.as_deref().unwrap_or(b""),
                sim,
                entry.old_mode.unwrap_or(0o100644),
                entry.new_mode.unwrap_or(0o100644),
            )
        } else {
            format_blob_diff(
                &entry.path,
                entry.old_data.as_deref(),
                entry.new_data.as_deref(),
                entry.old_mode,
                entry.new_mode,
            )
        };
        if !diff_text.is_empty() {
            result.push_str(&diff_text);
        }
    }

    Ok(result)
}

/// Rename detection for working-tree entries using raw byte data.
fn detect_wt_renames(entries: &mut Vec<WtEntry>) {
    let deletions: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| e.is_deletion().then_some(i))
        .collect();
    let additions: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| e.is_addition().then_some(i))
        .collect();

    if deletions.is_empty() || additions.is_empty() {
        return;
    }

    let mut used_adds: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut renames: Vec<(usize, usize, u32)> = Vec::new();

    for &del_i in &deletions {
        let del_data = match &entries[del_i].old_data {
            Some(d) => d,
            None => continue,
        };
        let mut best: Option<(usize, u32)> = None;
        for &add_i in &additions {
            if used_adds.contains(&add_i) {
                continue;
            }
            let add_data = match &entries[add_i].new_data {
                Some(d) => d,
                None => continue,
            };
            let sim = content_similarity(del_data, add_data);
            if sim >= RENAME_THRESHOLD {
                if best.is_none() || sim > best.unwrap().1 {
                    best = Some((add_i, sim));
                }
            }
        }
        if let Some((add_i, sim)) = best {
            used_adds.insert(add_i);
            renames.push((del_i, add_i, sim));
        }
    }

    // Convert matched pairs: move old_data into the addition, mark as rename
    let mut to_remove: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (del_i, add_i, similarity) in &renames {
        entries[*add_i].old_path = Some(entries[*del_i].path.clone());
        entries[*add_i].old_data = entries[*del_i].old_data.take();
        entries[*add_i].old_mode = entries[*del_i].old_mode;
        entries[*add_i].similarity = Some(*similarity);
        to_remove.insert(*del_i);
    }

    let mut remove_sorted: Vec<usize> = to_remove.into_iter().collect();
    remove_sorted.sort_unstable_by(|a, b| b.cmp(a));
    for i in remove_sorted {
        entries.remove(i);
    }
}

/// Working tree diff for LLM context — sectioned into staged, unstaged, untracked.
pub fn working_tree_diff_for_context(repo_path: &str) -> Result<String, Error> {
    use gix::diff::index::ChangeRef as TIChange;
    use gix::dir::entry::Status as DirStatus;
    use gix::status::index_worktree;
    use gix::status::Item;
    use gix::status::UntrackedFiles;

    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();
    let work_dir = repo
        .workdir()
        .ok_or_else(|| Error::Internal {
            message: "bare repository".to_owned(),
        })?
        .to_path_buf();

    let head_tree = repo.head_commit().ok().and_then(|c| c.tree().ok());

    let iter = repo
        .status(gix::progress::Discard)
        .map_err(Error::internal)?
        .untracked_files(UntrackedFiles::Files)
        .into_iter(Vec::new())
        .map_err(Error::internal)?;

    let mut staged = String::new();
    let mut unstaged = String::new();
    let mut untracked_files: Vec<String> = Vec::new();

    for item in iter {
        let item = item.map_err(Error::internal)?;

        match item {
            Item::TreeIndex(change) => {
                let (path_bstr, _idx, _mode, id) = change.fields();
                let path = path_bstr.to_str_lossy().to_string();
                let (old_data, old_mode) = head_blob_for_path(&repo, head_tree.as_ref(), &path);

                let is_deletion = matches!(&change, TIChange::Deletion { .. });

                if is_deletion {
                    let diff_text =
                        format_blob_diff(&path, old_data.as_deref(), None, old_mode, None);
                    if !diff_text.is_empty() {
                        staged.push_str(&diff_text);
                    }
                } else {
                    let new_blob = repo.find_object(id).map_err(Error::internal)?;
                    let diff_text = format_blob_diff(
                        &path,
                        old_data.as_deref(),
                        Some(&new_blob.data),
                        old_mode,
                        Some(0o100644),
                    );
                    if !diff_text.is_empty() {
                        staged.push_str(&diff_text);
                    }
                }
            }
            Item::IndexWorktree(ref iw_item) => match iw_item {
                index_worktree::Item::Modification {
                    rela_path, status, ..
                } => {
                    use gix::status::plumbing::index_as_worktree::{Change, EntryStatus};
                    let path = rela_path.to_str_lossy().to_string();
                    match status {
                        EntryStatus::Change(Change::Removed) => {
                            let (old_data, old_mode) =
                                head_blob_for_path(&repo, head_tree.as_ref(), &path);
                            let diff_text =
                                format_blob_diff(&path, old_data.as_deref(), None, old_mode, None);
                            if !diff_text.is_empty() {
                                unstaged.push_str(&diff_text);
                            }
                        }
                        EntryStatus::Change(Change::Modification { .. })
                        | EntryStatus::Change(Change::SubmoduleModification(_))
                        | EntryStatus::Change(Change::Type { .. }) => {
                            let index_data = index_blob_for_path(&repo, &path);
                            let disk_path = work_dir.join(&path);
                            let new_data = std::fs::read(&disk_path).ok();
                            let diff_text = format_blob_diff(
                                &path,
                                index_data.as_deref(),
                                new_data.as_deref(),
                                Some(0o100644),
                                Some(0o100644),
                            );
                            if !diff_text.is_empty() {
                                unstaged.push_str(&diff_text);
                            }
                        }
                        _ => {}
                    }
                }
                index_worktree::Item::DirectoryContents { entry, .. } => {
                    if matches!(entry.status, DirStatus::Untracked) {
                        untracked_files.push(entry.rela_path.to_str_lossy().to_string());
                    }
                }
                _ => {}
            },
        }
    }

    let mut result = String::new();

    if !staged.is_empty() {
        result.push_str("=== Staged Changes ===\n");
        result.push_str(&staged);
    }
    if !unstaged.is_empty() {
        result.push_str("=== Unstaged Changes ===\n");
        result.push_str(&unstaged);
    }
    if !untracked_files.is_empty() {
        result.push_str("=== Untracked Files ===\n");
        for f in &untracked_files {
            result.push_str(f);
            result.push('\n');
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Look up a blob from HEAD tree by relative path.
fn head_blob_for_path(
    repo: &gix::Repository,
    head_tree: Option<&gix::Tree<'_>>,
    path: &str,
) -> (Option<Vec<u8>>, Option<u32>) {
    let tree = match head_tree {
        Some(t) => t,
        None => return (None, None),
    };
    match tree.lookup_entry_by_path(path) {
        Ok(Some(entry)) => {
            let mode = Some(mode_to_u32(entry.mode()));
            match repo.find_object(entry.object_id()) {
                Ok(obj) => (Some(obj.data.to_vec()), mode),
                Err(_) => (None, mode),
            }
        }
        _ => (None, None),
    }
}

/// Look up a blob from the index by relative path.
fn index_blob_for_path(repo: &gix::Repository, path: &str) -> Option<Vec<u8>> {
    let index = repo.index_or_empty().ok()?;
    let entry_idx = index
        .entry_index_by_path(&gix::bstr::BStr::new(path.as_bytes()))
        .ok()?;
    let entry = &index.entries()[entry_idx];
    let obj = repo.find_object(entry.id).ok()?;
    Some(obj.data.to_vec())
}
