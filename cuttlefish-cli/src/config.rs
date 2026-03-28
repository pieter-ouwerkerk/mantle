use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub app_path: Option<String>,
    pub briefing_nudge_mode: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn nudge_mode(&self) -> &str {
        self.briefing_nudge_mode.as_deref().unwrap_or("guide")
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".config/cuttlefish/config.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.nudge_mode(), "guide");
        assert!(config.app_path.is_none());
    }

    #[test]
    fn test_parse_config() {
        let json =
            r#"{"app_path": "/Applications/Cuttlefish.app", "briefing_nudge_mode": "enforce"}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.app_path.as_deref(),
            Some("/Applications/Cuttlefish.app")
        );
        assert_eq!(config.nudge_mode(), "enforce");
    }
}
