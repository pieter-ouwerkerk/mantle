use std::fs;
use std::path::Path;

use serde_json::{json, Value};

fn generate_grants(worktree_path: &str) -> Vec<Value> {
    let wt = worktree_path.trim_end_matches('/');
    vec![
        json!(format!("Read({}/**)", wt)),
        json!(format!("Write({}/**)", wt)),
        json!(format!("Edit({}/**)", wt)),
        json!("Bash(git add:*)"),
        json!("Bash(git commit:*)"),
        json!("Bash(git diff:*)"),
        json!("Bash(git log:*)"),
        json!("Bash(git status:*)"),
        json!("Bash(git push:*)"),
        json!("Bash(git fetch:*)"),
        json!("Bash(git checkout:*)"),
        json!("Bash(xcodegen:*)"),
        json!("Bash(xcodebuild:*)"),
        json!("Bash(cargo:*)"),
        json!("Bash(swift:*)"),
        json!("Bash(npm:*)"),
        json!("Bash(pnpm:*)"),
        json!("Bash(ls:*)"),
        json!("Bash(find:*)"),
        json!("mcp__cuttlefish__*"),
    ]
}

pub fn inject_grants(worktree_path: &str, settings_path: &str) -> Result<(), String> {
    let path = Path::new(settings_path);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("failed to create directory: {e}"))?;
    }

    let mut settings: Value = if path.exists() {
        let content = fs::read_to_string(path).map_err(|e| format!("failed to read settings: {e}"))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let new_grants = generate_grants(worktree_path);

    let allow = settings
        .as_object_mut()
        .ok_or("settings is not an object")?
        .entry("permissions")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("permissions is not an object")?
        .entry("allow")
        .or_insert_with(|| json!([]));

    let allow_arr = allow.as_array_mut().ok_or("allow is not an array")?;

    for grant in new_grants {
        if !allow_arr.contains(&grant) {
            allow_arr.push(grant);
        }
    }

    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("failed to serialize settings: {e}"))?;

    fs::write(path, content).map_err(|e| format!("failed to write settings: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_grants_creates_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join(".claude").join("settings.local.json");

        inject_grants("/path/to/worktree", settings_path.to_str().unwrap()).unwrap();

        let content: Value = serde_json::from_str(
            &fs::read_to_string(&settings_path).unwrap()
        ).unwrap();

        let allow = content["permissions"]["allow"].as_array().unwrap();
        assert!(!allow.is_empty());
        assert!(allow.iter().any(|v| v.as_str().unwrap().contains("Read(")));
    }

    #[test]
    fn test_inject_grants_merges_with_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.local.json");

        let existing = json!({
            "permissions": {
                "allow": ["Bash(echo:*)"]
            }
        });
        fs::write(&settings_path, serde_json::to_string(&existing).unwrap()).unwrap();

        inject_grants("/wt", settings_path.to_str().unwrap()).unwrap();

        let content: Value = serde_json::from_str(
            &fs::read_to_string(&settings_path).unwrap()
        ).unwrap();

        let allow = content["permissions"]["allow"].as_array().unwrap();
        assert!(allow.iter().any(|v| v == "Bash(echo:*)"));
        assert!(allow.iter().any(|v| v.as_str().unwrap().contains("Read(")));
    }

    #[test]
    fn test_inject_grants_no_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.local.json");

        inject_grants("/wt", settings_path.to_str().unwrap()).unwrap();
        let first: Value = serde_json::from_str(
            &fs::read_to_string(&settings_path).unwrap()
        ).unwrap();
        let first_count = first["permissions"]["allow"].as_array().unwrap().len();

        inject_grants("/wt", settings_path.to_str().unwrap()).unwrap();
        let second: Value = serde_json::from_str(
            &fs::read_to_string(&settings_path).unwrap()
        ).unwrap();
        let second_count = second["permissions"]["allow"].as_array().unwrap().len();

        assert_eq!(first_count, second_count);
    }
}
