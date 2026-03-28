use crate::error::Error;
use crate::ops::log::format_gix_time;
use crate::repo;
use crate::types::BlameLineInfo;

pub fn blame_file(repo_path: &str, file_path: &str) -> Result<Vec<BlameLineInfo>, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let head_id = repo.head_id().map_err(Error::internal)?;

    blame_at_commit(&repo, file_path, head_id.into())
}

pub fn blame_file_at(
    repo_path: &str,
    file_path: &str,
    commit_hash: &str,
) -> Result<Vec<BlameLineInfo>, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();

    let commit_id = repo
        .rev_parse_single(commit_hash)
        .map_err(|_| Error::RevNotFound {
            rev: commit_hash.to_owned(),
        })?
        .detach();

    blame_at_commit(&repo, file_path, commit_id)
}

fn blame_at_commit(
    repo: &gix::Repository,
    file_path: &str,
    commit_id: gix::ObjectId,
) -> Result<Vec<BlameLineInfo>, Error> {
    let outcome = repo
        .blame_file(
            file_path.into(),
            commit_id,
            gix::repository::blame_file::Options::default(),
        )
        .map_err(Error::internal)?;

    let mut results = Vec::with_capacity(outcome.entries.len());

    for entry in &outcome.entries {
        let commit = repo
            .find_object(entry.commit_id)
            .map_err(Error::internal)?
            .try_into_commit()
            .map_err(Error::internal)?;

        let author = commit.author().map_err(Error::internal)?;

        results.push(BlameLineInfo {
            commit_hash: entry.commit_id.to_hex().to_string(),
            author_name: author.name.to_string(),
            author_email: author.email.to_string(),
            author_date: format_gix_time(author.time().map_err(Error::internal)?),
            line_number: entry.start_in_blamed_file + 1, // convert to 1-based
            num_lines: entry.len.get(),
            original_line_number: entry.start_in_source_file + 1, // convert to 1-based
        });
    }

    Ok(results)
}
