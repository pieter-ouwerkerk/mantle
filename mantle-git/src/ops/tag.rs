use crate::error::GitError;
use crate::types::TagInfo;

fn open_git2(repo_path: &str) -> Result<git2::Repository, GitError> {
    git2::Repository::open(repo_path).map_err(GitError::internal)
}

fn format_git2_time(time: &git2::Time) -> String {
    let secs = time.seconds();
    let offset_mins = time.offset_minutes();

    if let Some(dt) = chrono::DateTime::from_timestamp(secs, 0) {
        let offset = chrono::FixedOffset::east_opt(offset_mins * 60)
            .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap());
        let dt_with_tz = dt.with_timezone(&offset);
        dt_with_tz.to_rfc3339()
    } else {
        let sign = if offset_mins >= 0 { '+' } else { '-' };
        let abs_offset = offset_mins.unsigned_abs();
        let hours = abs_offset / 60;
        let mins = abs_offset % 60;
        format!("{secs} {sign}{hours:02}{mins:02}")
    }
}

pub fn list_tags(repo_path: &str) -> Result<Vec<TagInfo>, GitError> {
    let repo = open_git2(repo_path)?;
    let mut tags = Vec::new();

    repo.tag_foreach(|oid, name_bytes| {
        let full_name = String::from_utf8_lossy(name_bytes);
        let name = full_name
            .strip_prefix("refs/tags/")
            .unwrap_or(&full_name)
            .to_string();

        if let Ok(tag_obj) = repo.find_tag(oid) {
            // Annotated tag
            let target_hash = tag_obj
                .target()
                .ok()
                .and_then(|t| t.peel_to_commit().ok())
                .map_or_else(|| tag_obj.target_id().to_string(), |c| c.id().to_string());

            let (tagger_name, tagger_email, tagger_date) = if let Some(tagger) = tag_obj.tagger() {
                (
                    tagger.name().map(str::to_string),
                    tagger.email().map(str::to_string),
                    Some(format_git2_time(&tagger.when())),
                )
            } else {
                (None, None, None)
            };

            let message = tag_obj
                .message()
                .map(|m| m.trim_end().to_string())
                .filter(|m| !m.is_empty());

            tags.push(TagInfo {
                name,
                target_hash,
                is_annotated: true,
                tagger_name,
                tagger_email,
                tagger_date,
                message,
            });
        } else {
            // Lightweight tag — OID points directly to the commit
            tags.push(TagInfo {
                name,
                target_hash: oid.to_string(),
                is_annotated: false,
                tagger_name: None,
                tagger_email: None,
                tagger_date: None,
                message: None,
            });
        }
        true // continue iteration
    })
    .map_err(GitError::internal)?;

    tags.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(tags)
}

/// Create a tag pointing at the given commit.
/// If `message` is provided, creates an annotated tag; otherwise a lightweight tag.
pub fn create_tag(
    repo_path: &str,
    name: &str,
    target_hash: &str,
    message: Option<String>,
) -> Result<(), GitError> {
    let repo = open_git2(repo_path)?;
    let oid = git2::Oid::from_str(target_hash).map_err(GitError::internal)?;
    let obj = repo.find_object(oid, None).map_err(GitError::internal)?;

    if let Some(msg) = message {
        let sig = repo.signature().map_err(GitError::internal)?;
        repo.tag(name, &obj, &sig, &msg, false)
            .map_err(GitError::internal)?;
    } else {
        repo.tag_lightweight(name, &obj, false)
            .map_err(GitError::internal)?;
    }

    Ok(())
}

/// Delete a tag by name.
pub fn delete_tag(repo_path: &str, name: &str) -> Result<(), GitError> {
    let repo = open_git2(repo_path)?;
    let refname = format!("refs/tags/{name}");
    let mut reference = repo
        .find_reference(&refname)
        .map_err(|_| GitError::Internal {
            message: format!("tag '{name}' not found"),
        })?;
    reference.delete().map_err(GitError::internal)?;
    Ok(())
}
