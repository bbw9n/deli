use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use crate::{
    models::config::AppConfig,
    providers::{
        ActionProvider, ConfigProvider, DocumentProvider, MonitorProvider, ProviderContext,
        command::{
            CommandActionProvider, CommandConfigProvider, CommandDocumentProvider,
            CommandMonitorProvider,
        },
    },
};

pub struct ProviderRegistry {
    pub documents: BTreeMap<String, Arc<dyn DocumentProvider>>,
    pub configs: BTreeMap<String, Arc<dyn ConfigProvider>>,
    pub monitors: BTreeMap<String, Arc<dyn MonitorProvider>>,
    pub actions: BTreeMap<String, Arc<dyn ActionProvider>>,
}

impl ProviderRegistry {
    pub fn from_config(config: &AppConfig) -> Self {
        let documents = config
            .document_providers
            .iter()
            .map(|provider| {
                (
                    provider.name.clone(),
                    Arc::new(CommandDocumentProvider::new(provider)) as Arc<dyn DocumentProvider>,
                )
            })
            .collect();
        let configs = config
            .config_providers
            .iter()
            .map(|provider| {
                (
                    provider.name.clone(),
                    Arc::new(CommandConfigProvider::new(provider)) as Arc<dyn ConfigProvider>,
                )
            })
            .collect();
        let monitors = config
            .monitor_providers
            .iter()
            .map(|provider| {
                (
                    provider.name.clone(),
                    Arc::new(CommandMonitorProvider::new(provider)) as Arc<dyn MonitorProvider>,
                )
            })
            .collect();
        let actions = config
            .action_providers
            .iter()
            .map(|provider| {
                (
                    provider.name.clone(),
                    Arc::new(CommandActionProvider::new(provider)) as Arc<dyn ActionProvider>,
                )
            })
            .collect();

        Self {
            documents,
            configs,
            monitors,
            actions,
        }
    }

    pub fn context_for(&self, workspace_root: PathBuf) -> ProviderContext {
        ProviderContext { workspace_root }
    }
}

#[cfg(test)]
mod tests {
    use crate::models::config::AppConfig;

    use super::*;

    #[test]
    fn builds_registry_from_config_sections() {
        let config = AppConfig::from_toml(
            r#"
            [[document_providers]]
            name = "docs"
            [document_providers.command]
            program = "echo"
            args = ["[]"]

            [[config_providers]]
            name = "cfg"
            [config_providers.command]
            program = "echo"
            args = ["{\"columns\":[],\"rows\":[]}"]
            "#,
        )
        .unwrap();

        let registry = ProviderRegistry::from_config(&config);
        assert_eq!(registry.documents.len(), 1);
        assert_eq!(registry.configs.len(), 1);
    }
}
