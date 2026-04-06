use std::path::PathBuf;

use anyhow::{Result, anyhow};

use crate::{
    models::{
        config::{AppConfig, WorkspaceConfig},
        dataframe::{DataFrame, ExportFormat},
        document::{DocumentNode, DocumentResource},
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
    pub document_index: usize,
    pub document_scroll: usize,
    pub config_index: usize,
    pub config_filter_query: String,
    pub config_filter_mode: bool,
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
            document_index: 0,
            document_scroll: 0,
            config_index: 0,
            config_filter_query: String::new(),
            config_filter_mode: false,
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
        self.document_index = clamp_index(self.document_index, self.documents.len());
        self.document_scroll = 0;

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
        self.config_index = clamp_index(self.config_index, self.filtered_config_frame().rows.len());

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

    pub fn activate_view(&mut self, view: ActiveView) {
        self.active_view = view;
        if view != ActiveView::Configs {
            self.config_filter_mode = false;
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        match self.active_view {
            ActiveView::Documents => {
                self.document_index = step_index(self.document_index, self.documents.len(), delta);
                self.document_scroll = 0;
            }
            ActiveView::Configs => {
                let rows = self.filtered_config_frame().rows.len();
                self.config_index = step_index(self.config_index, rows, delta);
            }
            ActiveView::Worktree | ActiveView::Monitoring => {}
        }
    }

    pub fn scroll_current(&mut self, delta: isize) {
        if self.active_view == ActiveView::Documents {
            self.document_scroll = step_index(self.document_scroll, usize::MAX, delta);
        }
    }

    pub fn toggle_config_filter_mode(&mut self) {
        if self.active_view == ActiveView::Configs {
            self.config_filter_mode = !self.config_filter_mode;
        }
    }

    pub fn append_filter_char(&mut self, character: char) {
        if self.config_filter_mode {
            self.config_filter_query.push(character);
            self.config_index = clamp_index(0, self.filtered_config_frame().rows.len());
        }
    }

    pub fn pop_filter_char(&mut self) {
        if self.config_filter_mode {
            self.config_filter_query.pop();
            self.config_index =
                clamp_index(self.config_index, self.filtered_config_frame().rows.len());
        }
    }

    pub fn clear_filter_mode(&mut self) {
        self.config_filter_mode = false;
    }

    pub fn document_list_lines(&self) -> Vec<String> {
        if self.documents.is_empty() {
            return vec!["No documents loaded".into()];
        }

        self.documents
            .iter()
            .enumerate()
            .map(|(index, document)| {
                let marker = if index == self.document_index {
                    ">"
                } else {
                    " "
                };
                format!("{marker} {} ({})", document.title, document.path)
            })
            .collect()
    }

    pub fn document_reader_lines(&self) -> Vec<String> {
        let Some(document) = self.current_document() else {
            return vec!["No document selected".into()];
        };

        let all_lines = render_document(document);
        if self.document_scroll >= all_lines.len() {
            return Vec::new();
        }

        all_lines.into_iter().skip(self.document_scroll).collect()
    }

    pub fn config_lines(&self) -> Vec<String> {
        let frame = self.filtered_config_frame();
        if frame.columns.is_empty() {
            return vec!["No config rows loaded".into()];
        }

        let mut lines = vec![format!(
            "Filter: {}{}",
            if self.config_filter_query.is_empty() {
                "<none>"
            } else {
                &self.config_filter_query
            },
            if self.config_filter_mode {
                " [editing]"
            } else {
                ""
            }
        )];
        lines.push(
            frame
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect::<Vec<_>>()
                .join(" | "),
        );
        lines.extend(frame.rows.iter().enumerate().take(20).map(|(index, row)| {
            let marker = if index == self.config_index { ">" } else { " " };
            format!(
                "{marker} {}",
                row.iter()
                    .map(|cell| cell.as_display())
                    .collect::<Vec<_>>()
                    .join(" | ")
            )
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
            format!("Config rows: {}", self.filtered_config_frame().rows.len()),
            format!("Monitor loaded: {}", self.monitor_frame.is_some()),
        ];
        lines.push(format!(
            "Exports: JSON {} bytes",
            self.config_frame
                .export(ExportFormat::Json)
                .unwrap_or_default()
                .len()
        ));
        match self.active_view {
            ActiveView::Documents => {
                if let Some(document) = self.current_document() {
                    lines.push(format!("Selected doc: {}", document.title));
                    lines.push(format!("Path: {}", document.path));
                    lines.push(format!("Scroll: {}", self.document_scroll));
                }
            }
            ActiveView::Configs => {
                lines.push(format!("Filter query: {}", self.config_filter_query));
                if let Some(record) = self.selected_config_record() {
                    lines.push("Selected row:".into());
                    lines.extend(
                        record
                            .into_iter()
                            .map(|(key, value)| format!("{key}: {value}")),
                    );
                }
            }
            ActiveView::Worktree | ActiveView::Monitoring => {}
        }
        lines.push(monitor_detail_hint(self));
        lines.extend(self.notifications.iter().take(5).cloned());
        lines
    }

    pub fn current_document(&self) -> Option<&DocumentResource> {
        self.documents.get(self.document_index)
    }

    pub fn filtered_config_frame(&self) -> DataFrame {
        self.config_frame.filter_contains(&self.config_filter_query)
    }

    pub fn selected_config_record(&self) -> Option<Vec<(String, String)>> {
        let frame = self.filtered_config_frame();
        let row = frame.rows.get(self.config_index)?;
        Some(
            frame
                .columns
                .iter()
                .zip(row.iter())
                .map(|(column, cell)| (column.name.clone(), cell.as_display()))
                .collect(),
        )
    }

    pub fn selected_config_json(&self) -> String {
        let Some(record) = self.selected_config_record() else {
            return "{\n  \"message\": \"No config row selected\"\n}".into();
        };

        let map = record
            .into_iter()
            .map(|(key, value)| (key, serde_json::Value::String(value)))
            .collect::<serde_json::Map<_, _>>();
        serde_json::to_string_pretty(&serde_json::Value::Object(map))
            .unwrap_or_else(|_| "{\n  \"error\": \"Unable to serialize selection\"\n}".into())
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

fn clamp_index(index: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        index.min(len.saturating_sub(1))
    }
}

fn step_index(index: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }

    if delta.is_negative() {
        index.saturating_sub(delta.unsigned_abs())
    } else {
        clamp_index(index.saturating_add(delta as usize), len)
    }
}

fn render_document(document: &DocumentResource) -> Vec<String> {
    let mut lines = vec![
        format!("# {}", document.title),
        format!("Source: {}", document.path),
        String::new(),
    ];

    for node in &document.nodes {
        match node {
            DocumentNode::Heading { level, text } => {
                lines.push(format!("{} {}", "#".repeat((*level).into()), text));
            }
            DocumentNode::Paragraph(text) => lines.push(text.clone()),
            DocumentNode::CodeBlock(text) => {
                lines.push("```".into());
                lines.extend(text.lines().map(ToString::to_string));
                lines.push("```".into());
            }
            DocumentNode::ListItem(text) => lines.push(format!("- {text}")),
            DocumentNode::Quote(text) => lines.push(format!("> {text}")),
        }
        lines.push(String::new());
    }

    lines
}

#[cfg(test)]
mod tests {
    use crate::{
        models::{
            config::AppConfig,
            dataframe::{CellValue, ColumnSchema, ColumnType},
            document::{DocumentFormat, DocumentResource},
        },
        providers::registry::ProviderRegistry,
    };

    use super::*;

    #[test]
    fn config_filter_reduces_rows_and_resets_selection() {
        let config = AppConfig::default();
        let registry = ProviderRegistry::from_config(&config);
        let mut state = AppState::new(config, registry, None).unwrap();
        state.active_view = ActiveView::Configs;
        state.config_frame = DataFrame {
            columns: vec![ColumnSchema {
                name: "service".into(),
                kind: ColumnType::String,
            }],
            rows: vec![
                vec![CellValue::String("api".into())],
                vec![CellValue::String("worker".into())],
            ],
        };
        state.config_index = 1;
        state.config_filter_mode = true;

        state.append_filter_char('a');

        assert_eq!(state.filtered_config_frame().rows.len(), 1);
        assert_eq!(state.config_index, 0);
    }

    #[test]
    fn document_navigation_selects_reader_target() {
        let config = AppConfig::default();
        let registry = ProviderRegistry::from_config(&config);
        let mut state = AppState::new(config, registry, None).unwrap();
        state.documents = vec![
            DocumentResource::from_source("one", "a.md", DocumentFormat::Markdown, "# One"),
            DocumentResource::from_source("two", "b.md", DocumentFormat::Markdown, "# Two"),
        ];

        state.move_selection(1);

        assert_eq!(state.current_document().unwrap().title, "Two");
    }
}
