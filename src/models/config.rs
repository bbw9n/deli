use std::{fs, path::Path};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub workspaces: Vec<WorkspaceConfig>,
    #[serde(default)]
    pub document_providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub config_providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub monitor_providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub action_providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub name: String,
    pub root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub command: CommandSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_refresh_seconds")]
    pub refresh_seconds: u64,
    #[serde(default)]
    pub keybindings: Vec<KeybindingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingConfig {
    pub key: String,
    pub action: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            workspaces: vec![WorkspaceConfig {
                name: "default".into(),
                root: ".".into(),
            }],
            document_providers: Vec::new(),
            config_providers: Vec::new(),
            monitor_providers: Vec::new(),
            action_providers: Vec::new(),
            ui: UiConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        match path {
            Some(path) => Self::from_toml(&fs::read_to_string(path)?),
            None => {
                let default_path = Path::new("deli.toml");
                if default_path.exists() {
                    Self::from_toml(&fs::read_to_string(default_path)?)
                } else {
                    Ok(Self::default())
                }
            }
        }
    }

    pub fn from_toml(content: &str) -> Result<Self> {
        Ok(toml::from_str(content)?)
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            refresh_seconds: default_refresh_seconds(),
            keybindings: vec![
                KeybindingConfig {
                    key: "1".into(),
                    action: "documents".into(),
                },
                KeybindingConfig {
                    key: "2".into(),
                    action: "configs".into(),
                },
                KeybindingConfig {
                    key: "3".into(),
                    action: "worktree".into(),
                },
                KeybindingConfig {
                    key: "4".into(),
                    action: "monitoring".into(),
                },
            ],
        }
    }
}

fn default_refresh_seconds() -> u64 {
    30
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_toml() {
        let config = AppConfig::from_toml(
            r#"
            [[workspaces]]
            name = "prod"
            root = "/srv/prod"

            [[document_providers]]
            name = "docs"
            [document_providers.command]
            program = "cat"
            args = ["README.md"]

            [ui]
            refresh_seconds = 15
            "#,
        )
        .unwrap();

        assert_eq!(config.workspaces[0].name, "prod");
        assert_eq!(config.document_providers[0].name, "docs");
        assert_eq!(config.ui.refresh_seconds, 15);
    }
}
