use git2::Repository;

use crate::error::Error;
use crate::types::ReflogEntry;

pub fn reflog(repo_path: &str, refname: &str, max_count: u32) -> Result<Vec<ReflogEntry>, Error> {
    let repo = Repository::open(repo_path).map_err(Error::internal)?;
    let log = repo.reflog(refname).map_err(Error::internal)?;

    let entries: Vec<ReflogEntry> = log
        .iter()
        .take(max_count as usize)
        .map(|entry| {
            let committer = entry.committer();
            let time = committer.when();
            let secs = time.seconds();
            let offset_mins = time.offset_minutes();

            let date = if let Some(dt) = chrono::DateTime::from_timestamp(secs, 0) {
                let offset = chrono::FixedOffset::east_opt(offset_mins * 60)
                    .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap());
                dt.with_timezone(&offset).to_rfc3339()
            } else {
                format!("{secs}")
            };

            ReflogEntry {
                id: entry.id_new().to_string(),
                previous_id: entry.id_old().to_string(),
                message: entry.message().unwrap_or("").to_owned(),
                committer: committer.name().unwrap_or("").to_owned(),
                date,
            }
        })
        .collect();

    Ok(entries)
}
