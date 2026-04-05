use std::path::PathBuf;

use anyhow::{Result, anyhow};

use crate::{
    models::{
        config::{AppConfig, WorkspaceConfig},
        dataframe::{DataFrame, ExportFormat},
        document::DocumentResource,
        timeseries::TimeSeriesFrame,
    },
    providers::registry::ProviderRegistry,
    ui::render::monitor_detail_hint,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {
    Documents,
    Configs,
    Worktree,
    Monitoring,
}

impl ActiveView {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Documents => "Documents",
            Self::Configs => "Configs",
            Self::Worktree => "Worktree",
            Self::Monitoring => "Monitoring",
        }
    }

    pub fn all() -> [Self; 4] {
        [
            Self::Documents,
            Self::Configs,
            Self::Worktree,
            Self::Monitoring,
        ]
    }
}

pub struct AppState {
    pub workspace_name: String,
    pub workspace_root: PathBuf,
    pub active_view: ActiveView,
    pub command_palette_open: bool,
    pub command_query: String,
    pub notifications: Vec<String>,
    pub documents: Vec<DocumentResource>,
    pub config_frame: DataFrame,
    pub monitor_frame: Option<TimeSeriesFrame>,
    pub action_commands: Vec<String>,
    pub registry: ProviderRegistry,
    refresh_count: usize,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        registry: ProviderRegistry,
        workspace_override: Option<String>,
    ) -> Result<Self> {
        let workspace = select_workspace(&config, workspace_override)?;
        Ok(Self {
            workspace_name: workspace.name.clone(),
            workspace_root: PathBuf::from(&workspace.root),
            active_view: ActiveView::Documents,
            command_palette_open: false,
            command_query: String::new(),
            notifications: vec!["Welcome to deli".into()],
            documents: Vec::new(),
            config_frame: DataFrame::empty(),
            monitor_frame: None,
            action_commands: Vec::new(),
            registry,
            refresh_count: 0,
        })
    }

    pub fn on_tick(&mut self) {
        if self.notifications.len() > 4 {
            self.notifications.truncate(4);
        }
    }

    pub fn refresh_all(&mut self) {
        let context = self.registry.context_for(self.workspace_root.clone());
        self.refresh_count += 1;
        self.notifications
            .insert(0, format!("Refresh #{}", self.refresh_count));

        self.documents = self
            .registry
            .documents
            .values()
            .flat_map(|provider| match provider.list_documents(&context) {
                Ok(documents) => documents,
                Err(error) => {
                    self.notifications
                        .insert(0, format!("{} provider error: {}", provider.name(), error));
                    Vec::new()
                }
            })
            .collect();

        self.config_frame = self
            .registry
            .configs
            .values()
            .next()
            .and_then(|provider| match provider.fetch(&context) {
                Ok(frame) => Some(frame),
                Err(error) => {
                    self.notifications
                        .insert(0, format!("{} provider error: {}", provider.name(), error));
                    None
                }
            })
            .unwrap_or_else(DataFrame::empty);

        self.monitor_frame = self.registry.monitors.values().next().and_then(|provider| {
            match provider.fetch(&context) {
                Ok(frame) => Some(frame),
                Err(error) => {
                    self.notifications
                        .insert(0, format!("{} provider error: {}", provider.name(), error));
                    None
                }
            }
        });

        self.action_commands = self
            .registry
            .actions
            .values()
            .flat_map(|provider| {
                provider.commands().into_iter().map(move |command| {
                    format!("{} {}", command.program, command.args.join(" "))
                        .trim()
                        .to_string()
                })
            })
            .collect();
    }

    pub fn document_lines(&self) -> Vec<String> {
        if self.documents.is_empty() {
            return vec!["No documents loaded".into()];
        }

        self.documents
            .iter()
            .flat_map(|document| {
                let mut lines = vec![format!("# {}", document.title)];
                lines.extend(
                    document
                        .nodes
                        .iter()
                        .take(4)
                        .map(|node| format!("{node:?}")),
                );
                lines.push(String::new());
                lines
            })
            .collect()
    }

    pub fn config_lines(&self) -> Vec<String> {
        if self.config_frame.columns.is_empty() {
            return vec!["No config rows loaded".into()];
        }

        let mut lines = vec![
            self.config_frame
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect::<Vec<_>>()
                .join(" | "),
        ];
        lines.extend(self.config_frame.rows.iter().take(10).map(|row| {
            row.iter()
                .map(|cell| cell.as_display())
                .collect::<Vec<_>>()
                .join(" | ")
        }));
        lines
    }

    pub fn worktree_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Workspace: {}", self.workspace_root.display()),
            "Planned actions: open path, launch command, jump to worktree".into(),
        ];
        if self.action_commands.is_empty() {
            lines.push("No action providers configured".into());
        } else {
            lines.extend(self.action_commands.iter().cloned());
        }
        lines
    }

    pub fn monitor_lines(&self) -> Vec<String> {
        match &self.monitor_frame {
            Some(frame) => {
                let mut lines = vec![frame.title.clone()];
                lines.extend(frame.summary_lines());
                lines
            }
            None => vec!["No monitoring series loaded".into()],
        }
    }

    pub fn detail_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Workspace root: {}", self.workspace_root.display()),
            format!("Docs: {}", self.documents.len()),
            format!("Config rows: {}", self.config_frame.rows.len()),
            format!("Monitor loaded: {}", self.monitor_frame.is_some()),
        ];
        lines.push(format!(
            "Exports: JSON {} bytes",
            self.config_frame
                .export(ExportFormat::Json)
                .unwrap_or_default()
                .len()
        ));
        lines.push(monitor_detail_hint(self));
        lines.extend(self.notifications.iter().take(5).cloned());
        lines
    }
}

fn select_workspace(
    config: &AppConfig,
    workspace_override: Option<String>,
) -> Result<WorkspaceConfig> {
    let workspaces = if config.workspaces.is_empty() {
        vec![WorkspaceConfig {
            name: "default".into(),
            root: ".".into(),
        }]
    } else {
        config.workspaces.clone()
    };

    if let Some(name) = workspace_override {
        workspaces
            .iter()
            .find(|workspace| workspace.name == name)
            .cloned()
            .ok_or_else(|| anyhow!("workspace '{name}' not found"))
    } else {
        workspaces
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("no workspace configured"))
    }
}
