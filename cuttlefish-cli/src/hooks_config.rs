use serde_json::{json, Value};

/// Generate the hooks config JSON using command transport.
/// Matches the Swift `AgentConfigFileService.generateHooksConfig()` output
/// for the no-HTTP-port case.
pub fn generate_hooks_config() -> Value {
    let sync_hook = json!({
        "type": "command",
        "command": "cuttlefish hook",
        "timeout": 30
    });

    let async_hook = json!({
        "type": "command",
        "command": "cuttlefish hook"
    });

    json!({
        "hooks": {
            "PreToolUse": [
                { "matcher": "Edit|Update|Write", "hooks": [sync_hook] },
                { "matcher": "Agent", "hooks": [sync_hook] }
            ],
            "WorktreeCreate": [
                { "hooks": [sync_hook] }
            ],
            "WorktreeRemove": [
                { "hooks": [async_hook] }
            ],
            "SessionStart": [
                { "hooks": [async_hook] }
            ],
            "Stop": [
                { "hooks": [async_hook] }
            ],
            "PostToolUse": [
                { "matcher": "Edit|Write|Bash", "hooks": [async_hook] }
            ]
        }
    })
}

/// Merge hooks config into an existing settings.local.json value.
/// Adds hooks, mcp permission grant, and enabled MCP server.
pub fn merge_hooks_into_settings(settings: &mut Value) {
    let hooks = generate_hooks_config();
    settings["hooks"] = hooks["hooks"].clone();

    // Ensure mcp__cuttlefish__* permission
    let allow = settings
        .pointer_mut("/permissions/allow")
        .and_then(|v| v.as_array_mut());
    if let Some(arr) = allow {
        let grant = json!("mcp__cuttlefish__*");
        if !arr.contains(&grant) {
            arr.push(grant);
        }
    }

    // Ensure enabledMcpjsonServers
    if settings.get("enabledMcpjsonServers").is_none() {
        settings["enabledMcpjsonServers"] = json!(["cuttlefish"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_hooks_config_has_expected_events() {
        let config = generate_hooks_config();
        let hooks = config["hooks"].as_object().unwrap();
        assert!(hooks.contains_key("PreToolUse"));
        assert!(hooks.contains_key("WorktreeCreate"));
        assert!(hooks.contains_key("WorktreeRemove"));
        assert!(hooks.contains_key("SessionStart"));
        assert!(hooks.contains_key("Stop"));
        assert!(hooks.contains_key("PostToolUse"));
    }

    #[test]
    fn test_pre_tool_use_has_edit_and_agent_matchers() {
        let config = generate_hooks_config();
        let pre = config["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["matcher"], "Edit|Update|Write");
        assert_eq!(pre[1]["matcher"], "Agent");
    }

    #[test]
    fn test_hooks_use_command_transport() {
        let config = generate_hooks_config();
        let wt = &config["hooks"]["WorktreeCreate"].as_array().unwrap()[0];
        let hook = &wt["hooks"].as_array().unwrap()[0];
        assert_eq!(hook["type"], "command");
        assert_eq!(hook["command"], "cuttlefish hook");
    }

    #[test]
    fn test_merge_into_existing_preserves_other_keys() {
        let mut existing: serde_json::Value = serde_json::json!({
            "permissions": { "allow": ["Bash(ls:*)"] },
            "customKey": true
        });
        merge_hooks_into_settings(&mut existing);
        assert!(existing["customKey"].as_bool().unwrap());
        assert!(existing["hooks"].is_object());
    }

    #[test]
    fn test_merge_adds_mcp_permission() {
        let mut existing: serde_json::Value = serde_json::json!({
            "permissions": { "allow": ["Bash(ls:*)"] }
        });
        merge_hooks_into_settings(&mut existing);
        let allow = existing["permissions"]["allow"].as_array().unwrap();
        assert!(allow.iter().any(|v| v == "mcp__cuttlefish__*"));
    }

    #[test]
    fn test_merge_adds_enabled_mcp_servers() {
        let mut existing: serde_json::Value = serde_json::json!({});
        merge_hooks_into_settings(&mut existing);
        let servers = existing["enabledMcpjsonServers"].as_array().unwrap();
        assert!(servers.iter().any(|v| v == "cuttlefish"));
    }
}
