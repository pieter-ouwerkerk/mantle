use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::error::MantleError;
use crate::types::CowCloneResult;

/// `CLONE_NOFOLLOW` flag for `clonefile(2)` — do not follow symlinks.
const CLONE_NOFOLLOW: u32 = 0x0001;

// clonefile(2) is macOS-specific (APFS copy-on-write).
extern "C" {
    fn clonefile(src: *const libc::c_char, dst: *const libc::c_char, flags: u32) -> libc::c_int;
}

/// Attempt to clone a single file via APFS `clonefile(2)`.
/// Returns `true` on success, `false` on failure (caller should fall back).
#[allow(unsafe_code)]
fn try_clonefile(src: &Path, dst: &Path) -> bool {
    let Ok(src_c) = CString::new(src.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    let Ok(dst_c) = CString::new(dst.as_os_str().as_encoded_bytes()) else {
        return false;
    };

    // SAFETY: clonefile is a well-defined macOS syscall.
    // We pass valid null-terminated C strings and a constant flag value.
    let ret = unsafe { clonefile(src_c.as_ptr(), dst_c.as_ptr(), CLONE_NOFOLLOW) };
    ret == 0
}

/// Check whether a symlink target stays within the source tree.
fn symlink_stays_in_tree(link_path: &Path, source_root: &Path) -> bool {
    match fs::read_link(link_path) {
        Ok(target) => {
            let resolved = if target.is_absolute() {
                target
            } else {
                link_path.parent().unwrap_or(link_path).join(&target)
            };
            // Canonicalize both to compare. If canonicalize fails, be conservative.
            match (resolved.canonicalize(), source_root.canonicalize()) {
                (Ok(r), Ok(s)) => r.starts_with(&s),
                _ => false,
            }
        }
        Err(_) => false,
    }
}

/// Recursively walk `src_dir` and clone each entry into `dst_dir`.
fn clone_tree(src_dir: &Path, dst_dir: &Path, source_root: &Path, result: &mut CowCloneResult) {
    let entries = match fs::read_dir(src_dir) {
        Ok(e) => e,
        Err(e) => {
            result.errors.push(format!(
                "Failed to read directory {}: {e}",
                src_dir.display()
            ));
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                result.errors.push(format!("Failed to read dir entry: {e}"));
                continue;
            }
        };

        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst_dir.join(&file_name);

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                result.errors.push(format!(
                    "Failed to get file type for {}: {e}",
                    src_path.display()
                ));
                continue;
            }
        };

        // Skip symlinks pointing outside the source tree
        if file_type.is_symlink() {
            if !symlink_stays_in_tree(&src_path, source_root) {
                continue;
            }
            // For in-tree symlinks, try clonefile; fall back to copy
            if try_clonefile(&src_path, &dst_path) {
                result.cloned_count += 1;
            } else {
                match fs::copy(&src_path, &dst_path) {
                    Ok(_) => result.fallback_count += 1,
                    Err(e) => result.errors.push(format!(
                        "Failed to copy symlink {}: {e}",
                        src_path.display()
                    )),
                }
            }
            continue;
        }

        if file_type.is_dir() {
            if let Err(e) = fs::create_dir_all(&dst_path) {
                result.errors.push(format!(
                    "Failed to create directory {}: {e}",
                    dst_path.display()
                ));
                continue;
            }
            clone_tree(&src_path, &dst_path, source_root, result);
            continue;
        }

        // Regular file: try clonefile, fall back to copy
        if try_clonefile(&src_path, &dst_path) {
            result.cloned_count += 1;
        } else {
            match fs::copy(&src_path, &dst_path) {
                Ok(_) => result.fallback_count += 1,
                Err(e) => result
                    .errors
                    .push(format!("Failed to copy {}: {e}", src_path.display())),
            }
        }
    }
}

/// After a whole-directory clonefile, remove any symlinks in the cloned tree
/// whose targets fall outside BOTH the original source tree and the cloned tree.
/// Relative symlinks typically resolve within the clone, so we must check both roots.
fn remove_external_symlinks(cloned_root: &Path, source_root: &Path) {
    let Ok(entries) = fs::read_dir(cloned_root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };

        if ft.is_dir() {
            remove_external_symlinks(&path, source_root);
        } else if ft.is_symlink()
            && !symlink_stays_in_tree(&path, source_root)
            && !symlink_stays_in_tree(&path, cloned_root)
        {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Clone a directory tree using APFS copy-on-write where possible.
///
/// First attempts to `clonefile` the entire directory in one shot (supported on
/// APFS for directories). If that fails, walks the tree and clones/copies each
/// entry individually.
pub fn cow_clone_directory(source: &str, destination: &str) -> Result<CowCloneResult, MantleError> {
    let src = PathBuf::from(source);
    let dst = PathBuf::from(destination);

    if !src.is_dir() {
        return Err(MantleError::Internal {
            message: format!("Source is not a directory: {source}"),
        });
    }

    if dst.exists() {
        return Err(MantleError::Internal {
            message: format!("Destination already exists: {destination}"),
        });
    }

    let start = Instant::now();

    // Try whole-directory clonefile first (APFS supports this natively)
    if try_clonefile(&src, &dst) {
        // Post-process: remove symlinks in the clone that point outside the
        // *original* source tree (the clone's symlinks still reference the
        // original targets).
        remove_external_symlinks(&dst, &src);

        let count = count_files_recursive(&dst);
        let elapsed = start.elapsed().as_millis();

        return Ok(CowCloneResult {
            cloned_count: count,
            fallback_count: 0,
            #[allow(clippy::cast_possible_truncation)]
            elapsed_ms: elapsed as u64,
            errors: Vec::new(),
        });
    }

    // Whole-directory clone failed; walk and clone per-entry
    if let Err(e) = fs::create_dir_all(&dst) {
        return Err(MantleError::Internal {
            message: format!("Failed to create destination directory: {e}"),
        });
    }

    let mut result = CowCloneResult {
        cloned_count: 0,
        fallback_count: 0,
        elapsed_ms: 0,
        errors: Vec::new(),
    };

    clone_tree(&src, &dst, &src, &mut result);

    let elapsed = start.elapsed().as_millis();
    #[allow(clippy::cast_possible_truncation)]
    {
        result.elapsed_ms = elapsed as u64;
    }

    Ok(result)
}

/// Count regular files recursively (for reporting after whole-directory clone).
fn count_files_recursive(dir: &Path) -> u32 {
    let mut count: u32 = 0;
    let Ok(entries) = fs::read_dir(dir) else {
        return count;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            count += count_files_recursive(&entry.path());
        } else if ft.is_file() || ft.is_symlink() {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_cow_clone_basic_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("source");
        let dst = tmp.path().join("dest");

        fs::create_dir_all(src.join("subdir")).unwrap();
        fs::write(src.join("file1.txt"), "hello").unwrap();
        fs::write(src.join("subdir/file2.txt"), "world").unwrap();

        let result = cow_clone_directory(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap();

        assert!(dst.join("file1.txt").exists());
        assert!(dst.join("subdir/file2.txt").exists());
        assert_eq!(fs::read_to_string(dst.join("file1.txt")).unwrap(), "hello");
        assert_eq!(
            fs::read_to_string(dst.join("subdir/file2.txt")).unwrap(),
            "world"
        );

        // Either whole-dir clone or per-file; total should be 2
        let total = result.cloned_count + result.fallback_count;
        assert_eq!(total, 2);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_cow_clone_source_not_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("not_a_dir");
        fs::write(&file, "data").unwrap();

        let result = cow_clone_directory(
            file.to_str().unwrap(),
            tmp.path().join("dst").to_str().unwrap(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_cow_clone_destination_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();

        let result = cow_clone_directory(src.to_str().unwrap(), dst.to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_cow_clone_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("empty");
        let dst = tmp.path().join("dest");
        fs::create_dir_all(&src).unwrap();

        let result = cow_clone_directory(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap();

        assert!(dst.exists());
        assert_eq!(result.cloned_count + result.fallback_count, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_symlink_outside_tree_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        let outside = tmp.path().join("outside.txt");

        fs::create_dir_all(&src).unwrap();
        fs::write(&outside, "external").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, src.join("link")).unwrap();

        let result = cow_clone_directory(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap();

        // The symlink pointing outside should be skipped
        assert!(!dst.join("link").exists());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_symlink_inside_tree_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("real.txt"), "content").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(src.join("real.txt"), src.join("internal_link")).unwrap();

        let result = cow_clone_directory(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap();

        // The in-tree symlink should be preserved in the clone
        assert!(dst.join("real.txt").exists());
        assert!(dst.join("internal_link").exists());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_symlink_mixed_internal_and_external() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        let outside = tmp.path().join("outside.txt");

        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("real.txt"), "internal").unwrap();
        fs::write(&outside, "external").unwrap();

        #[cfg(unix)]
        {
            // In-tree: relative symlink to sibling
            std::os::unix::fs::symlink("real.txt", src.join("good_link")).unwrap();
            // Out-of-tree: absolute symlink to outside file
            std::os::unix::fs::symlink(&outside, src.join("bad_link")).unwrap();
            // In-tree: symlink in subdirectory pointing up
            std::os::unix::fs::symlink(src.join("real.txt"), src.join("sub/up_link")).unwrap();
        }

        let result = cow_clone_directory(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap();

        assert!(dst.join("real.txt").exists());
        assert!(dst.join("good_link").exists());
        assert!(dst.join("sub/up_link").exists());
        assert!(
            !dst.join("bad_link").exists(),
            "External symlink should be skipped"
        );
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_cow_clone_deeply_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        // Create 5 levels deep with files at each level
        let mut dir = src.clone();
        for i in 0..5 {
            dir = dir.join(format!("level{i}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(format!("file{i}.txt")), format!("depth {i}")).unwrap();
        }

        let result = cow_clone_directory(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap();

        // Verify all 5 files exist at each depth
        let mut check_dir = dst.clone();
        for i in 0..5 {
            check_dir = check_dir.join(format!("level{i}"));
            let content = fs::read_to_string(check_dir.join(format!("file{i}.txt"))).unwrap();
            assert_eq!(content, format!("depth {i}"));
        }

        assert_eq!(result.cloned_count + result.fallback_count, 5);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_cow_clone_reports_clonefile_on_apfs() {
        // On macOS with APFS (the default), clonefile should succeed.
        // tempdir() is on APFS, so whole-dir clone should work.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), "aaa").unwrap();
        fs::write(src.join("b.txt"), "bbb").unwrap();

        let result = cow_clone_directory(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap();

        // On APFS, whole-directory clonefile should succeed → cloned_count > 0, fallback_count == 0
        assert!(
            result.cloned_count > 0,
            "Expected clonefile to succeed on APFS (cloned_count={}, fallback_count={})",
            result.cloned_count,
            result.fallback_count
        );
        assert_eq!(
            result.fallback_count, 0,
            "Expected no fallback copies on APFS"
        );
        assert!(result.errors.is_empty());
    }

    /// Get available disk space in KB for the volume containing `path`,
    /// using `df -k`.
    fn get_available_kb(path: &Path) -> Option<u64> {
        use std::process::Command;
        let output = Command::new("df")
            .args(["-k", path.to_str()?])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        // df -k output: header line, then data line with fields:
        // Filesystem 1024-blocks Used Available Capacity ...
        let text = String::from_utf8_lossy(&output.stdout);
        let data_line = text.lines().nth(1)?;
        let available = data_line.split_whitespace().nth(3)?;
        available.parse::<u64>().ok()
    }

    #[test]
    fn test_cow_clone_no_extra_disk_usage() {
        // Verify CoW: cloning a large file shouldn't double disk usage.
        // We measure volume-level free space (via df) before and after.
        // On APFS with CoW, the clone shares blocks, so free space barely changes.
        // Note: per-file stat().blocks() reports logical blocks on APFS,
        // which doesn't reflect CoW sharing — hence the volume-level check.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        fs::create_dir_all(&src).unwrap();

        // Write a 10MB file so the delta is clearly measurable
        let data = vec![0xABu8; 10 * 1024 * 1024];
        fs::write(src.join("large.bin"), &data).unwrap();

        let free_before_kb = match get_available_kb(tmp.path()) {
            Some(kb) => kb,
            None => {
                eprintln!("Skipping disk usage test: could not read df output");
                return;
            }
        };

        let result = cow_clone_directory(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap();
        assert!(result.errors.is_empty());

        // Verify content is correct
        let cloned = fs::read(dst.join("large.bin")).unwrap();
        assert_eq!(cloned, data);

        let free_after_kb = match get_available_kb(tmp.path()) {
            Some(kb) => kb,
            None => {
                eprintln!("Skipping disk usage test: could not read df output");
                return;
            }
        };

        // With CoW, free space should decrease by much less than 10MB (10240 KB).
        // Allow up to 2MB (2048 KB) of overhead for metadata/bookkeeping.
        // A full copy would consume ~10240 KB.
        let consumed_kb = free_before_kb.saturating_sub(free_after_kb);
        assert!(
            consumed_kb < 2048,
            "Clone consumed {}KB of disk — expected near-zero for CoW (10MB source)",
            consumed_kb
        );
    }

    /// Test that clonefile fallback works when crossing filesystem boundaries.
    /// Creates an HFS+ RAM disk and clones from APFS → HFS+.
    /// Skipped if RAM disk creation fails (e.g., in sandboxed CI).
    #[test]
    fn test_cow_clone_cross_volume_fallback() {
        use std::process::Command;

        // Create a 32MB RAM disk with HFS+
        let output = Command::new("hdiutil")
            .args(["attach", "-nomount", "ram://65536"]) // 32MB
            .output();

        let disk_device = match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => {
                eprintln!("Skipping cross-volume test: could not create RAM disk");
                return;
            }
        };

        // Format as HFS+ (not APFS — clonefile won't work cross-volume regardless,
        // and HFS+ also doesn't support clonefile at all)
        let erase = Command::new("diskutil")
            .args(["eraseDisk", "HFS+", "CowTest", &disk_device])
            .output();

        if erase.is_err() || !erase.as_ref().unwrap().status.success() {
            let _ = Command::new("hdiutil")
                .args(["detach", &disk_device])
                .output();
            eprintln!("Skipping cross-volume test: could not format RAM disk");
            return;
        }

        let mount_point = PathBuf::from("/Volumes/CowTest");
        let cleanup = || {
            let _ = Command::new("hdiutil")
                .args(["detach", &disk_device, "-force"])
                .output();
        };

        // Create source on APFS (tempdir)
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("file1.txt"), "cross-volume").unwrap();
        fs::write(src.join("file2.txt"), "test data").unwrap();

        // Destination on HFS+ RAM disk
        let dst = mount_point.join("cow-test-dst");

        let result = cow_clone_directory(src.to_str().unwrap(), dst.to_str().unwrap());
        cleanup();

        let result = result.unwrap();

        // clonefile should fail cross-volume; all files should use fallback copy
        assert_eq!(
            result.cloned_count, 0,
            "Expected no clonefile success cross-volume"
        );
        assert_eq!(
            result.fallback_count, 2,
            "Expected 2 fallback copies, got {}",
            result.fallback_count
        );
        assert!(result.errors.is_empty());
    }
}
