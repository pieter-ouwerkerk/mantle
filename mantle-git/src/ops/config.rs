use crate::error::Error;
use crate::repo;

pub fn config_user_name(repo_path: &str) -> Result<String, Error> {
    read_config_string(repo_path, "user.name")
}

pub fn config_user_email(repo_path: &str) -> Result<String, Error> {
    read_config_string(repo_path, "user.email")
}

fn read_config_string(repo_path: &str, key: &str) -> Result<String, Error> {
    let ts = repo::open(repo_path)?;
    let repo = ts.to_thread_local();
    let config = repo.config_snapshot();
    config
        .string(key)
        .map(|v| v.to_string())
        .ok_or_else(|| Error::ConfigNotFound {
            key: key.to_owned(),
        })
}
