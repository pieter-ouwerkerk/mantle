use std::path::Path;
use std::path::PathBuf;
use std::process;

use crate::hooks_config;
use crate::managed_section;

#[derive(Debug, PartialEq)]
enum AgentConfigTarget {
    ClaudeMd(PathBuf),
    AgentsMd(PathBuf),
}

fn detect_agent_config_file(repo_path: &str) -> AgentConfigTarget {
    let root = PathBuf::from(repo_path);
    let claude_root = root.join("CLAUDE.md");
    if claude_root.exists() {
        return AgentConfigTarget::ClaudeMd(claude_root);
    }
    let claude_dir = root.join(".claude/CLAUDE.md");
    if claude_dir.exists() {
        return AgentConfigTarget::ClaudeMd(claude_dir);
    }
    let agents = root.join("AGENTS.md");
    if agents.exists() {
        return AgentConfigTarget::AgentsMd(agents);
    }
    AgentConfigTarget::ClaudeMd(claude_root)
}

const WORKTREE_SECTION: &str = "\
## Cuttlefish Worktrees

Always use Cuttlefish worktrees for new feature work. **Never use bare `git worktree add`.**

```bash
cuttlefish worktree create --name \"task-name\"
```

This creates a worktree at `~/.cuttlefish/worktrees/<repo>/<slug>/` on a `claude/<slug>` branch, \
hydrates build artifacts, injects Claude Code permissions, and copies `.mcp.json`.

When done, commit and push your branch, then remove the worktree:

```bash
cuttlefish worktree remove --path <worktree-path>
```

Run `cuttlefish doctor` to verify integration health.";

fn resolve_mcp_binary() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    format!("{home}/.config/cuttlefish/bin/cuttlefish-mcp-server")
}

fn upsert_mcp_json(repo_path: &str) {
    let mcp_path = PathBuf::from(repo_path).join(".mcp.json");
    let mut config: serde_json::Value = if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let servers = config
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));

    if servers.get("cuttlefish").is_none() {
        servers["cuttlefish"] = serde_json::json!({
            "command": resolve_mcp_binary(),
            "args": []
        });
    }

    let output = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&mcp_path, format!("{output}\n")).unwrap_or_else(|e| {
        eprintln!("error: failed to write .mcp.json: {e}");
    });
}

fn install_hooks(repo_path: &str) {
    let claude_dir = PathBuf::from(repo_path).join(".claude");
    let settings_path = claude_dir.join("settings.local.json");

    std::fs::create_dir_all(&claude_dir).unwrap_or_else(|e| {
        eprintln!("error: failed to create .claude/: {e}");
    });

    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    hooks_config::merge_hooks_into_settings(&mut settings);

    let output = serde_json::to_string_pretty(&settings).unwrap();
    std::fs::write(&settings_path, format!("{output}\n")).unwrap_or_else(|e| {
        eprintln!("error: failed to write settings.local.json: {e}");
    });
}

fn ensure_agents_md_import(repo_path: &str, claude_path: &PathBuf) {
    let agents_path = PathBuf::from(repo_path).join("AGENTS.md");
    if !agents_path.exists() {
        return;
    }

    let existing = std::fs::read_to_string(claude_path).unwrap_or_default();
    if existing.contains("@AGENTS.md") {
        return;
    }

    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    if !content.is_empty() {
        content.push('\n');
    }
    content.push_str("@AGENTS.md\n");
    std::fs::write(claude_path, content).unwrap_or_else(|e| {
        eprintln!("error: failed to write CLAUDE.md: {e}");
    });
}

/// Run `cuttlefish init` — bootstrap a .worktreeinclude file for the repo.
pub fn run(path: &str, dry_run: bool, show: bool) {
    let Some(repo_path) = resolve_repo_root(path) else {
        eprintln!("error: not a git repository: {path}");
        process::exit(1);
    };

    if show {
        run_show(&repo_path);
        return;
    }

    // --- .worktreeinclude (existing behavior) ---
    match mantle_git::bootstrap_worktreeinclude(repo_path.clone()) {
        Ok(result) => {
            if result.already_exists {
                eprintln!(".worktreeinclude already exists");
            } else if dry_run {
                eprintln!("--- .worktreeinclude ---");
                println!("{}", result.content);
            } else {
                eprintln!("  created .worktreeinclude");
                if !result.gitignore_dirs.is_empty() {
                    eprintln!(
                        "    included from .gitignore: {}",
                        result.gitignore_dirs.join(", ")
                    );
                }
                if !result.builtin_dirs.is_empty() {
                    eprintln!(
                        "    included from built-ins: {}",
                        result.builtin_dirs.join(", ")
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("warning: .worktreeinclude failed: {e}");
        }
    }

    // --- Agent config ---
    let target = detect_agent_config_file(&repo_path);
    let (config_path, is_agents_md) = match &target {
        AgentConfigTarget::ClaudeMd(p) => (p.clone(), false),
        AgentConfigTarget::AgentsMd(p) => (p.clone(), true),
    };

    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let updated = managed_section::upsert(&existing, WORKTREE_SECTION);

    if dry_run {
        eprintln!("--- {} ---", config_path.display());
        eprint!("{updated}");
    } else {
        std::fs::write(&config_path, &updated).unwrap_or_else(|e| {
            eprintln!("error: failed to write {}: {e}", config_path.display());
        });
        eprintln!("  updated {}", config_path.display());
    }

    if is_agents_md && !dry_run {
        let claude_path = PathBuf::from(&repo_path).join("CLAUDE.md");
        ensure_agents_md_import(&repo_path, &claude_path);
        if claude_path.exists() {
            eprintln!("  ensured @AGENTS.md import in CLAUDE.md");
        }
    }

    // --- .mcp.json ---
    if dry_run {
        eprintln!("--- .mcp.json ---");
        eprintln!("  would ensure cuttlefish entry");
    } else {
        upsert_mcp_json(&repo_path);
        eprintln!("  updated .mcp.json");
    }

    // --- Hooks ---
    if dry_run {
        eprintln!("--- .claude/settings.local.json ---");
        eprintln!("  would install hooks config");
    } else {
        install_hooks(&repo_path);
        eprintln!("  updated .claude/settings.local.json");
    }
}

fn run_show(repo_path: &str) {
    let threshold = 10 * 1024 * 1024;
    match mantle_git::compute_effective_worktreeinclude(repo_path.to_string(), threshold) {
        Ok(result) => {
            if result.has_worktreeinclude_file {
                eprintln!("source: .worktreeinclude");
            } else {
                eprintln!("source: .gitignore (fallback)");
            }
            eprintln!();

            let included: Vec<_> = result.entries.iter().filter(|e| e.included).collect();
            let suggestions: Vec<_> = result.entries.iter().filter(|e| !e.included).collect();

            if included.is_empty() {
                eprintln!("no directories will be hydrated");
            } else {
                eprintln!("will hydrate:");
                for entry in &included {
                    let size_mb = entry.size_bytes / (1024 * 1024);
                    let exists = if entry.exists_on_disk {
                        ""
                    } else {
                        " (not on disk)"
                    };
                    eprintln!("  {} ({}MB){}", entry.path, size_mb, exists);
                }
            }

            if !suggestions.is_empty() {
                eprintln!();
                eprintln!("suggestions:");
                for entry in &suggestions {
                    let size_mb = entry.size_bytes / (1024 * 1024);
                    eprintln!(
                        "  {} ({}MB) — consider adding to .worktreeinclude",
                        entry.path, size_mb
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn resolve_repo_root(path: &str) -> Option<String> {
    let p = Path::new(path);
    let dir = if p.is_dir() { p } else { p.parent()? };
    let mut current = dir.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current.to_string_lossy().to_string());
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        dir
    }

    #[test]
    fn test_detect_agent_config_prefers_claude_md_at_root() {
        let dir = setup_repo();
        fs::write(dir.path().join("CLAUDE.md"), "# existing").unwrap();
        fs::write(dir.path().join("AGENTS.md"), "# agents").unwrap();
        let result = detect_agent_config_file(dir.path().to_str().unwrap());
        assert_eq!(result, AgentConfigTarget::ClaudeMd(dir.path().join("CLAUDE.md")));
    }

    #[test]
    fn test_detect_agent_config_finds_claude_md_in_dir() {
        let dir = setup_repo();
        fs::create_dir(dir.path().join(".claude")).unwrap();
        fs::write(dir.path().join(".claude/CLAUDE.md"), "# existing").unwrap();
        let result = detect_agent_config_file(dir.path().to_str().unwrap());
        assert_eq!(result, AgentConfigTarget::ClaudeMd(dir.path().join(".claude/CLAUDE.md")));
    }

    #[test]
    fn test_detect_agent_config_finds_agents_md() {
        let dir = setup_repo();
        fs::write(dir.path().join("AGENTS.md"), "# agents").unwrap();
        let result = detect_agent_config_file(dir.path().to_str().unwrap());
        assert_eq!(result, AgentConfigTarget::AgentsMd(dir.path().join("AGENTS.md")));
    }

    #[test]
    fn test_detect_agent_config_defaults_to_claude_md() {
        let dir = setup_repo();
        let result = detect_agent_config_file(dir.path().to_str().unwrap());
        assert_eq!(result, AgentConfigTarget::ClaudeMd(dir.path().join("CLAUDE.md")));
    }

    #[test]
    fn test_ensure_agents_md_import_adds_import() {
        let dir = setup_repo();
        fs::write(dir.path().join("AGENTS.md"), "# agents").unwrap();
        let claude_path = dir.path().join("CLAUDE.md");
        ensure_agents_md_import(dir.path().to_str().unwrap(), &claude_path);
        let content = fs::read_to_string(&claude_path).unwrap();
        assert!(content.contains("@AGENTS.md"));
    }

    #[test]
    fn test_ensure_agents_md_import_skips_if_already_present() {
        let dir = setup_repo();
        fs::write(dir.path().join("AGENTS.md"), "# agents").unwrap();
        let claude_path = dir.path().join("CLAUDE.md");
        fs::write(&claude_path, "# Project\n\n@AGENTS.md\n").unwrap();
        ensure_agents_md_import(dir.path().to_str().unwrap(), &claude_path);
        let content = fs::read_to_string(&claude_path).unwrap();
        assert_eq!(content.matches("@AGENTS.md").count(), 1);
    }

    #[test]
    fn test_upsert_mcp_json_creates_new() {
        let dir = setup_repo();
        let mcp_path = dir.path().join(".mcp.json");
        upsert_mcp_json(dir.path().to_str().unwrap());
        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&mcp_path).unwrap()).unwrap();
        assert!(content["mcpServers"]["cuttlefish"].is_object());
    }

    #[test]
    fn test_upsert_mcp_json_preserves_other_servers() {
        let dir = setup_repo();
        let mcp_path = dir.path().join(".mcp.json");
        fs::write(&mcp_path, r#"{"mcpServers":{"other":{"command":"other-cmd"}}}"#).unwrap();
        upsert_mcp_json(dir.path().to_str().unwrap());
        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&mcp_path).unwrap()).unwrap();
        assert!(content["mcpServers"]["cuttlefish"].is_object());
        assert!(content["mcpServers"]["other"].is_object());
    }

    #[test]
    fn test_install_hooks_creates_settings_file() {
        let dir = setup_repo();
        install_hooks(dir.path().to_str().unwrap());
        let settings_path = dir.path().join(".claude/settings.local.json");
        assert!(settings_path.exists());
        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert!(content["hooks"].is_object());
    }
}
