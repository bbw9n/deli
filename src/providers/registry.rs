use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use crate::{
    models::config::{AppConfig, MonitorProviderConfig, MonitorProviderKind},
    providers::{
        ActionProvider, ConfigProvider, DocumentProvider, MonitorProvider, ProviderContext,
        command::{
            CommandActionProvider, CommandConfigProvider, CommandDocumentProvider,
            CommandMonitorProvider, monitor_kind,
        },
        metrics_http::{OpenTsdbMonitorProvider, PrometheusMonitorProvider},
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
            .filter_map(build_monitor_provider)
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

fn build_monitor_provider(
    provider: &MonitorProviderConfig,
) -> Option<(String, Arc<dyn MonitorProvider>)> {
    let built: Result<Arc<dyn MonitorProvider>, _> = match monitor_kind(provider) {
        MonitorProviderKind::Command => CommandMonitorProvider::new(provider)
            .map(|provider| Arc::new(provider) as Arc<dyn MonitorProvider>),
        MonitorProviderKind::Prometheus | MonitorProviderKind::VictoriaMetrics => {
            let product_name = match monitor_kind(provider) {
                MonitorProviderKind::VictoriaMetrics => "VictoriaMetrics",
                _ => "Prometheus",
            };
            PrometheusMonitorProvider::new(provider, product_name)
                .map(|provider| Arc::new(provider) as Arc<dyn MonitorProvider>)
        }
        MonitorProviderKind::OpenTsdb => OpenTsdbMonitorProvider::new(provider)
            .map(|provider| Arc::new(provider) as Arc<dyn MonitorProvider>),
    };

    built
        .ok()
        .map(|provider_impl| (provider.name.clone(), provider_impl))
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

            [[monitor_providers]]
            name = "prom"
            kind = "prometheus"
            endpoint = "http://localhost:9090"
            query = "up"
            "#,
        )
        .unwrap();

        let registry = ProviderRegistry::from_config(&config);
        assert_eq!(registry.documents.len(), 1);
        assert_eq!(registry.configs.len(), 1);
        assert_eq!(registry.monitors.len(), 1);
    }
}
