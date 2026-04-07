use std::{fs, path::Path};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub workspaces: Vec<WorkspaceConfig>,
    #[serde(default)]
    pub document_providers: Vec<DocumentProviderConfig>,
    #[serde(default)]
    pub config_providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub monitor_providers: Vec<MonitorProviderConfig>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DocumentProviderKind {
    Command,
    Notion,
    Feishu,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentProviderConfig {
    pub name: String,
    #[serde(default)]
    pub kind: Option<DocumentProviderKind>,
    #[serde(default)]
    pub command: Option<CommandSpec>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default = "default_notion_version")]
    pub notion_version: String,
    #[serde(default)]
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MonitorProviderKind {
    Command,
    Prometheus,
    VictoriaMetrics,
    OpenTsdb,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorProviderConfig {
    pub name: String,
    #[serde(default)]
    pub kind: Option<MonitorProviderKind>,
    #[serde(default)]
    pub command: Option<CommandSpec>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub query_presets: Vec<String>,
    #[serde(default = "default_step_seconds")]
    pub step_seconds: u64,
    #[serde(default = "default_lookback_minutes")]
    pub lookback_minutes: u64,
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

fn default_step_seconds() -> u64 {
    60
}

fn default_lookback_minutes() -> u64 {
    60
}

fn default_notion_version() -> String {
    "2026-03-11".into()
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

            [[monitor_providers]]
            name = "prom"
            kind = "prometheus"
            endpoint = "http://localhost:9090"
            query = "up"
            query_presets = ["up", "rate(http_requests_total[5m])"]
            "#,
        )
        .unwrap();

        assert_eq!(config.workspaces[0].name, "prod");
        assert_eq!(config.document_providers[0].name, "docs");
        assert_eq!(config.ui.refresh_seconds, 15);
        assert_eq!(
            config.monitor_providers[0].kind,
            Some(MonitorProviderKind::Prometheus)
        );
        assert_eq!(
            config.monitor_providers[0].query_presets,
            vec!["up", "rate(http_requests_total[5m])"]
        );
        assert_eq!(config.document_providers[0].kind, None);
    }
}
