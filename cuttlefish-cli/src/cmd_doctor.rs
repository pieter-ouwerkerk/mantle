use std::path::PathBuf;

use crate::managed_section;

// ── Section A: Check functions ──────────────────────────────────────────────

pub struct CheckResult {
    pub label: String,
    pub passed: bool,
    pub fix: Option<String>,
}

pub fn check_worktreeinclude(repo_path: &str) -> CheckResult {
    let path = PathBuf::from(repo_path).join(".worktreeinclude");
    CheckResult {
        label: ".worktreeinclude".to_string(),
        passed: path.exists(),
        fix: Some("Run `cuttlefish init` to generate".to_string()),
    }
}

pub fn check_mcp_json(repo_path: &str) -> CheckResult {
    let path = PathBuf::from(repo_path).join(".mcp.json");
    if !path.exists() {
        return CheckResult {
            label: ".mcp.json".to_string(),
            passed: false,
            fix: Some("Run `cuttlefish init` to create".to_string()),
        };
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
    let has_cuttlefish = json.pointer("/mcpServers/cuttlefish").is_some();
    CheckResult {
        label: ".mcp.json".to_string(),
        passed: has_cuttlefish,
        fix: Some("Run `cuttlefish init` to add cuttlefish server".to_string()),
    }
}

pub fn check_agent_config(repo_path: &str) -> CheckResult {
    let root = PathBuf::from(repo_path);
    for candidate in &[
        root.join("CLAUDE.md"),
        root.join(".claude/CLAUDE.md"),
        root.join("AGENTS.md"),
    ] {
        if candidate.exists() {
            let content = std::fs::read_to_string(candidate).unwrap_or_default();
            if managed_section::has_managed_section(&content) {
                let name = candidate.strip_prefix(&root).map_or_else(
                    |_| candidate.display().to_string(),
                    |p| p.display().to_string(),
                );
                return CheckResult {
                    label: name,
                    passed: true,
                    fix: None,
                };
            }
        }
    }
    CheckResult {
        label: "agent config".to_string(),
        passed: false,
        fix: Some("Run `cuttlefish init` to generate worktree instructions".to_string()),
    }
}

pub fn check_agents_md_import(repo_path: &str) -> CheckResult {
    let root = PathBuf::from(repo_path);
    let agents_path = root.join("AGENTS.md");
    if !agents_path.exists() {
        return CheckResult {
            label: "@AGENTS.md import".to_string(),
            passed: true,
            fix: None,
        };
    }
    for candidate in &[root.join("CLAUDE.md"), root.join(".claude/CLAUDE.md")] {
        if candidate.exists() {
            let content = std::fs::read_to_string(candidate).unwrap_or_default();
            if content.contains("@AGENTS.md") {
                return CheckResult {
                    label: "@AGENTS.md import".to_string(),
                    passed: true,
                    fix: None,
                };
            }
        }
    }
    CheckResult {
        label: "@AGENTS.md import".to_string(),
        passed: false,
        fix: Some(
            "AGENTS.md exists but not imported in CLAUDE.md. Run `cuttlefish init` to fix"
                .to_string(),
        ),
    }
}

pub fn check_hooks(repo_path: &str) -> CheckResult {
    let path = PathBuf::from(repo_path).join(".claude/settings.local.json");
    if !path.exists() {
        return CheckResult {
            label: "hooks".to_string(),
            passed: false,
            fix: Some("Run `cuttlefish init` to install hooks".to_string()),
        };
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
    let has_hooks = json.get("hooks").and_then(|h| h.as_object()).is_some();
    CheckResult {
        label: "hooks".to_string(),
        passed: has_hooks,
        fix: Some("Run `cuttlefish init` to install hooks".to_string()),
    }
}

pub fn run_all_checks(repo_path: &str) -> Vec<CheckResult> {
    vec![
        check_worktreeinclude(repo_path),
        check_mcp_json(repo_path),
        check_agent_config(repo_path),
        check_agents_md_import(repo_path),
        check_hooks(repo_path),
    ]
}

// ── Section B: Resolution map ───────────────────────────────────────────────

struct ResolvedFile {
    label: String,
    exists: bool,
    detail: Option<String>,
}

fn resolve_claude_md_files(repo_path: &str) -> Vec<ResolvedFile> {
    let root = PathBuf::from(repo_path);
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    vec![
        ResolvedFile {
            label: "./CLAUDE.md".to_string(),
            exists: root.join("CLAUDE.md").exists(),
            detail: None,
        },
        ResolvedFile {
            label: "./.claude/CLAUDE.md".to_string(),
            exists: root.join(".claude/CLAUDE.md").exists(),
            detail: None,
        },
        ResolvedFile {
            label: "~/.claude/CLAUDE.md".to_string(),
            exists: PathBuf::from(&home).join(".claude/CLAUDE.md").exists(),
            detail: None,
        },
    ]
}

fn resolve_mcp_files(repo_path: &str) -> Vec<ResolvedFile> {
    let root = PathBuf::from(repo_path);
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let claude_json_path = PathBuf::from(&home).join(".claude.json");

    let project_detail = if root.join(".mcp.json").exists() {
        let content = std::fs::read_to_string(root.join(".mcp.json")).unwrap_or_default();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
        json.pointer("/mcpServers/cuttlefish/command")
            .and_then(|v| v.as_str())
            .map(|s| format!("\u{2192} {s}"))
    } else {
        None
    };

    let user_detail = if claude_json_path.exists() {
        let content = std::fs::read_to_string(&claude_json_path).unwrap_or_default();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
        if json.pointer("/mcpServers/cuttlefish").is_some() {
            Some("has cuttlefish entry (overrides project)".to_string())
        } else {
            Some("no cuttlefish entry".to_string())
        }
    } else {
        None
    };

    vec![
        ResolvedFile {
            label: "./.mcp.json (project)".to_string(),
            exists: root.join(".mcp.json").exists(),
            detail: project_detail,
        },
        ResolvedFile {
            label: "~/.claude.json (user)".to_string(),
            exists: claude_json_path.exists(),
            detail: user_detail,
        },
    ]
}

fn resolve_settings_files(repo_path: &str) -> Vec<ResolvedFile> {
    let root = PathBuf::from(repo_path);
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());

    let local_path = root.join(".claude/settings.local.json");
    let local_detail = if local_path.exists() {
        let content = std::fs::read_to_string(&local_path).unwrap_or_default();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
        if json.get("hooks").is_some() {
            Some("hooks installed".to_string())
        } else {
            Some("no hooks".to_string())
        }
    } else {
        None
    };

    vec![
        ResolvedFile {
            label: ".claude/settings.local.json".to_string(),
            exists: local_path.exists(),
            detail: local_detail,
        },
        ResolvedFile {
            label: ".claude/settings.json".to_string(),
            exists: root.join(".claude/settings.json").exists(),
            detail: None,
        },
        ResolvedFile {
            label: "~/.claude/settings.json".to_string(),
            exists: PathBuf::from(&home).join(".claude/settings.json").exists(),
            detail: None,
        },
    ]
}

fn print_resolution_section(title: &str, files: &[ResolvedFile]) {
    eprintln!("  {title}:");
    for f in files {
        let icon = if f.exists { "\u{2713}" } else { "\u{00b7}" };
        let detail = f.detail.as_deref().map_or_else(
            || {
                if f.exists {
                    String::new()
                } else {
                    " (not found)".to_string()
                }
            },
            |d| format!(" ({d})"),
        );
        eprintln!("    {icon} {}{detail}", f.label);
    }
    eprintln!();
}

// ── Section C: Entry point ──────────────────────────────────────────────────

pub fn run(path: &str) {
    let Some(repo_path) = crate::util::resolve_repo_root(path) else {
        eprintln!("error: not a git repository: {path}");
        std::process::exit(1);
    };

    let repo_name = PathBuf::from(&repo_path)
        .file_name()
        .map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().to_string());

    eprintln!("{repo_name} integration health\n");

    let results = run_all_checks(&repo_path);
    for r in &results {
        if r.passed {
            eprintln!("  \u{2713} {}", r.label);
        } else {
            eprintln!("  \u{2717} {}", r.label);
            if let Some(fix) = &r.fix {
                eprintln!("    \u{2192} {fix}");
            }
        }
    }

    let failures = results.iter().filter(|r| !r.passed).count();
    eprintln!();

    eprintln!("Claude Code resolution map:\n");
    print_resolution_section("CLAUDE.md", &resolve_claude_md_files(&repo_path));
    print_resolution_section("MCP servers (cuttlefish)", &resolve_mcp_files(&repo_path));
    print_resolution_section("Settings", &resolve_settings_files(&repo_path));

    match failures {
        0 => eprintln!("All checks passed."),
        1 => eprintln!("1 issue found."),
        n => eprintln!("{n} issues found."),
    }

    if failures > 0 {
        std::process::exit(1);
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

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
    fn test_check_worktreeinclude_pass() {
        let dir = setup_repo();
        fs::write(dir.path().join(".worktreeinclude"), "build/\n").unwrap();
        let result = check_worktreeinclude(dir.path().to_str().unwrap());
        assert!(result.passed);
    }

    #[test]
    fn test_check_worktreeinclude_fail() {
        let dir = setup_repo();
        let result = check_worktreeinclude(dir.path().to_str().unwrap());
        assert!(!result.passed);
        assert!(result.fix.is_some());
    }

    #[test]
    fn test_check_mcp_json_pass() {
        let dir = setup_repo();
        fs::write(
            dir.path().join(".mcp.json"),
            r#"{"mcpServers":{"cuttlefish":{"command":"cuttlefish-mcp-server"}}}"#,
        )
        .unwrap();
        let result = check_mcp_json(dir.path().to_str().unwrap());
        assert!(result.passed);
    }

    #[test]
    fn test_check_mcp_json_missing() {
        let dir = setup_repo();
        let result = check_mcp_json(dir.path().to_str().unwrap());
        assert!(!result.passed);
    }

    #[test]
    fn test_check_mcp_json_no_cuttlefish_entry() {
        let dir = setup_repo();
        fs::write(
            dir.path().join(".mcp.json"),
            r#"{"mcpServers":{"other":{"command":"other"}}}"#,
        )
        .unwrap();
        let result = check_mcp_json(dir.path().to_str().unwrap());
        assert!(!result.passed);
    }

    #[test]
    fn test_check_agent_config_pass_claude_md() {
        let dir = setup_repo();
        fs::write(
            dir.path().join("CLAUDE.md"),
            "# Hi\n<!-- BEGIN CUTTLEFISH-MANAGED -->\nstuff\n<!-- END CUTTLEFISH-MANAGED -->\n",
        )
        .unwrap();
        let result = check_agent_config(dir.path().to_str().unwrap());
        assert!(result.passed);
    }

    #[test]
    fn test_check_agent_config_fail_no_managed_section() {
        let dir = setup_repo();
        fs::write(dir.path().join("CLAUDE.md"), "# Just a readme").unwrap();
        let result = check_agent_config(dir.path().to_str().unwrap());
        assert!(!result.passed);
    }

    #[test]
    fn test_check_agents_md_import_pass() {
        let dir = setup_repo();
        fs::write(dir.path().join("AGENTS.md"), "# agents").unwrap();
        fs::write(dir.path().join("CLAUDE.md"), "@AGENTS.md\n").unwrap();
        let result = check_agents_md_import(dir.path().to_str().unwrap());
        assert!(result.passed);
    }

    #[test]
    fn test_check_agents_md_import_fail() {
        let dir = setup_repo();
        fs::write(dir.path().join("AGENTS.md"), "# agents").unwrap();
        fs::write(dir.path().join("CLAUDE.md"), "# no import\n").unwrap();
        let result = check_agents_md_import(dir.path().to_str().unwrap());
        assert!(!result.passed);
    }

    #[test]
    fn test_check_agents_md_import_skip_when_no_agents_md() {
        let dir = setup_repo();
        let result = check_agents_md_import(dir.path().to_str().unwrap());
        assert!(result.passed);
    }

    #[test]
    fn test_check_hooks_pass() {
        let dir = setup_repo();
        let claude_dir = dir.path().join(".claude");
        fs::create_dir(&claude_dir).unwrap();
        let config = serde_json::json!({ "hooks": { "PreToolUse": [] } });
        fs::write(
            claude_dir.join("settings.local.json"),
            serde_json::to_string_pretty(&config).unwrap(),
        )
        .unwrap();
        let result = check_hooks(dir.path().to_str().unwrap());
        assert!(result.passed);
    }

    #[test]
    fn test_check_hooks_fail() {
        let dir = setup_repo();
        let result = check_hooks(dir.path().to_str().unwrap());
        assert!(!result.passed);
    }

    #[test]
    fn test_run_all_checks_counts_failures() {
        let dir = setup_repo();
        let results = run_all_checks(dir.path().to_str().unwrap());
        let failures = results.iter().filter(|r| !r.passed).count();
        assert!(failures > 0);
    }
}
