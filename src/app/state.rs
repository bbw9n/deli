use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow};
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui_code_editor::{editor::Editor, theme};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};

use crate::{
    models::{
        config::{AppConfig, WorkspaceConfig},
        dataframe::{DataFrame, ExportFormat},
        document::{DocumentNode, DocumentResource},
        timeseries::TimeSeriesFrame,
    },
    providers::registry::ProviderRegistry,
    runtime::gnuplot::{ChartPlan, ChartRenderStatus, render_ascii_chart, render_chart_with_size},
    ui::render::monitor_detail_hint,
};

#[derive(Debug, Clone)]
struct MonitorSourceState {
    name: String,
    query: String,
    query_presets: Vec<String>,
    preset_index: usize,
}

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
    pub document_editor: Option<Editor>,
    pub document_editor_dirty: bool,
    pub document_editor_mode: bool,
    pub config_index: usize,
    pub config_filter_query: String,
    pub config_filter_mode: bool,
    pub config_editor: Option<Editor>,
    pub config_editor_row: Option<usize>,
    pub config_editor_dirty: bool,
    pub config_editor_mode: bool,
    pub monitor_query: String,
    pub monitor_query_editor: Option<Editor>,
    pub monitor_query_mode: bool,
    pub monitor_chart_plan: Option<ChartPlan>,
    pub monitor_chart_status: Option<ChartRenderStatus>,
    pub monitor_chart_ascii: Option<String>,
    pub monitor_image: Option<StatefulProtocol>,
    monitor_sources: Vec<MonitorSourceState>,
    monitor_provider_index: usize,
    monitor_live_poll: bool,
    monitor_refresh_seconds: u64,
    tick_count: u64,
    monitor_graph_area: Rect,
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
            monitor_sources: config
                .monitor_providers
                .iter()
                .map(|provider| {
                    let query = provider.query.clone().unwrap_or_default();
                    let preset_index = provider
                        .query_presets
                        .iter()
                        .position(|preset| preset == &query)
                        .unwrap_or(0);
                    MonitorSourceState {
                        name: provider.name.clone(),
                        query,
                        query_presets: provider.query_presets.clone(),
                        preset_index,
                    }
                })
                .collect(),
            monitor_provider_index: 0,
            monitor_live_poll: false,
            monitor_refresh_seconds: config.ui.refresh_seconds.max(1),
            tick_count: 0,
            monitor_graph_area: Rect::new(0, 0, 96, 18),
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
            document_editor: None,
            document_editor_dirty: false,
            document_editor_mode: false,
            config_index: 0,
            config_filter_query: String::new(),
            config_filter_mode: false,
            config_editor: None,
            config_editor_row: None,
            config_editor_dirty: false,
            config_editor_mode: false,
            monitor_query: config
                .monitor_providers
                .first()
                .and_then(|provider| provider.query.clone())
                .unwrap_or_default(),
            monitor_query_editor: None,
            monitor_query_mode: false,
            monitor_chart_plan: None,
            monitor_chart_status: None,
            monitor_chart_ascii: None,
            monitor_image: None,
            refresh_count: 0,
        })
    }

    pub fn on_tick(&mut self) {
        self.tick_count = self.tick_count.saturating_add(1);
        if self.notifications.len() > 4 {
            self.notifications.truncate(4);
        }
        if self.monitor_live_poll && self.tick_count % self.monitor_refresh_seconds == 0 {
            self.refresh_monitor_only();
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
        self.sync_document_editor(true);

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
        self.sync_config_editor(true);

        self.refresh_monitor_source(&context);
        self.sync_monitor_query_editor();
        self.refresh_monitor_chart();

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
            self.config_editor_mode = false;
        }
        if view != ActiveView::Documents {
            self.document_editor_mode = false;
        }
        if view != ActiveView::Monitoring {
            self.monitor_query_mode = false;
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        match self.active_view {
            ActiveView::Documents => {
                self.document_index = step_index(self.document_index, self.documents.len(), delta);
                self.document_scroll = 0;
                self.sync_document_editor(false);
            }
            ActiveView::Configs => {
                let rows = self.filtered_config_frame().rows.len();
                self.config_index = step_index(self.config_index, rows, delta);
                self.sync_config_editor(false);
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
            if self.config_filter_mode {
                self.config_editor_mode = false;
            }
        }
    }

    pub fn toggle_document_editor_mode(&mut self) {
        if self.active_view != ActiveView::Documents {
            return;
        }
        self.sync_document_editor(false);
        self.document_editor_mode = !self.document_editor_mode;
        self.notifications.insert(
            0,
            if self.document_editor_mode {
                "Document editor enabled".into()
            } else {
                "Document editor disabled".into()
            },
        );
    }

    pub fn handle_document_editor_input(&mut self, key: KeyEvent, area: Rect) {
        self.sync_document_editor(false);
        if let Some(editor) = &mut self.document_editor
            && editor.input(key, &area).is_ok()
        {
            self.document_editor_dirty = true;
        }
    }

    pub fn save_document_editor(&mut self) {
        self.sync_document_editor(false);
        let Some(editor) = &self.document_editor else {
            self.notifications
                .insert(0, "No document editor content to save".into());
            return;
        };
        let Some(current) = self.current_document().cloned() else {
            self.notifications.insert(0, "No document selected".into());
            return;
        };

        let provider_name = current.provider_name.clone();
        let remote_id = current
            .remote_id
            .clone()
            .unwrap_or_else(|| current.id.clone());
        let updated = DocumentResource::from_source(
            current.id.clone(),
            current.path.clone(),
            current.format.clone(),
            editor.get_content(),
        )
        .with_origin(
            provider_name.clone().unwrap_or_else(|| "local".into()),
            remote_id,
            current.editable,
        );

        let context = self.registry.context_for(self.workspace_root.clone());
        let save_result = provider_name
            .as_ref()
            .and_then(|name| self.registry.documents.get(name))
            .map(|provider| provider.save_document(&context, &updated))
            .transpose();

        match save_result {
            Ok(Some(document)) => {
                self.documents[self.document_index] = document;
                self.notifications
                    .insert(0, "Saved document to provider".into());
            }
            Ok(None) => {
                self.documents[self.document_index] = updated;
                self.notifications
                    .insert(0, "Updated document preview in memory".into());
            }
            Err(error) => {
                self.documents[self.document_index] = updated;
                self.notifications.insert(
                    0,
                    format!("Provider save unavailable, updated preview only: {error}"),
                );
            }
        }

        self.document_editor_dirty = false;
        self.sync_document_editor(true);
    }

    pub fn append_filter_char(&mut self, character: char) {
        if self.config_filter_mode {
            self.config_filter_query.push(character);
            self.config_index = clamp_index(0, self.filtered_config_frame().rows.len());
            self.sync_config_editor(false);
        }
    }

    pub fn pop_filter_char(&mut self) {
        if self.config_filter_mode {
            self.config_filter_query.pop();
            self.config_index =
                clamp_index(self.config_index, self.filtered_config_frame().rows.len());
            self.sync_config_editor(false);
        }
    }

    pub fn clear_filter_mode(&mut self) {
        self.config_filter_mode = false;
    }

    pub fn toggle_config_editor_mode(&mut self) {
        if self.active_view != ActiveView::Configs {
            return;
        }
        self.sync_config_editor(false);
        self.config_editor_mode = !self.config_editor_mode;
        self.notifications.insert(
            0,
            if self.config_editor_mode {
                "Config inspector editing enabled".into()
            } else {
                "Config inspector editing disabled".into()
            },
        );
    }

    pub fn handle_config_editor_input(&mut self, key: KeyEvent, area: Rect) {
        self.sync_config_editor(false);
        if let Some(editor) = &mut self.config_editor {
            if editor.input(key, &area).is_ok() {
                self.config_editor_dirty = true;
            }
        }
    }

    pub fn save_config_editor(&mut self) {
        self.sync_config_editor(false);
        let Some(editor) = &self.config_editor else {
            self.notifications
                .insert(0, "No config editor content to save".into());
            return;
        };
        let Some(row_index) = self.selected_config_row_index() else {
            self.notifications
                .insert(0, "No config row selected to save".into());
            return;
        };

        match serde_json::from_str::<serde_json::Value>(&editor.get_content()) {
            Ok(serde_json::Value::Object(record)) => {
                match self.config_frame.update_row_from_record(row_index, &record) {
                    Ok(()) => {
                        self.config_editor_dirty = false;
                        self.sync_config_editor(true);
                        self.notifications.insert(
                            0,
                            format!("Saved selected config row {} in memory", row_index + 1),
                        );
                    }
                    Err(error) => self
                        .notifications
                        .insert(0, format!("Save failed: {error}")),
                };
            }
            Ok(_) => self.notifications.insert(
                0,
                "Save failed: editor content must be a JSON object".into(),
            ),
            Err(error) => self
                .notifications
                .insert(0, format!("Save failed: invalid JSON: {error}")),
        }
    }

    pub fn export_config_editor(&mut self) {
        self.sync_config_editor(false);
        let Some(editor) = &self.config_editor else {
            self.notifications
                .insert(0, "No config editor content to export".into());
            return;
        };

        let export_dir = self.workspace_root.join("exports");
        if let Err(error) = fs::create_dir_all(&export_dir) {
            self.notifications
                .insert(0, format!("Export failed: {error}"));
            return;
        }
        let row = self
            .selected_config_row_index()
            .map(|index| index + 1)
            .unwrap_or(0);
        let export_path = export_dir.join(format!("config-row-{row}.json"));
        match fs::write(&export_path, editor.get_content()) {
            Ok(()) => self.notifications.insert(
                0,
                format!("Exported inspector to {}", export_path.display()),
            ),
            Err(error) => self
                .notifications
                .insert(0, format!("Export failed: {error}")),
        }
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
                let mut lines = self
                    .monitor_chart_ascii
                    .clone()
                    .unwrap_or_else(|| "No chart rendered yet".into())
                    .lines()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                lines.push(String::new());
                lines.push(frame.title.clone());
                lines.push(format!(
                    "Provider: {}",
                    self.current_monitor_provider_name().unwrap_or("none")
                ));
                lines.push(format!("Query: {}", self.monitor_query));
                lines.extend(frame.summary_lines());
                if let Some(status) = &self.monitor_chart_status {
                    lines.push(String::new());
                    lines.push(match status {
                        ChartRenderStatus::Rendered(path) => {
                            format!("gnuplot: rendered {}", path.display())
                        }
                        ChartRenderStatus::Skipped(reason) => format!("gnuplot: {reason}"),
                        ChartRenderStatus::Failed(reason) => format!("gnuplot failed: {reason}"),
                    });
                }
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
                    lines.push(format!(
                        "Editable: {}",
                        if document.editable { "yes" } else { "no" }
                    ));
                    lines.push(format!(
                        "Editor: {}{}",
                        if self.document_editor_mode {
                            "editing"
                        } else {
                            "read-only"
                        },
                        if self.document_editor_dirty {
                            " (dirty)"
                        } else {
                            ""
                        }
                    ));
                }
            }
            ActiveView::Configs => {
                lines.push(format!("Filter query: {}", self.config_filter_query));
                lines.push(format!(
                    "Inspector: {}{}",
                    if self.config_editor_mode {
                        "editing"
                    } else {
                        "read-only"
                    },
                    if self.config_editor_dirty {
                        " (dirty)"
                    } else {
                        ""
                    }
                ));
                if let Some(record) = self.selected_config_record() {
                    lines.push("Selected row:".into());
                    lines.extend(
                        record
                            .into_iter()
                            .map(|(key, value)| format!("{key}: {value}")),
                    );
                }
            }
            ActiveView::Worktree => {}
            ActiveView::Monitoring => {
                lines.push(format!("Metric query: {}", self.monitor_query));
                lines.push(format!(
                    "Metric provider: {}",
                    self.current_monitor_provider_name().unwrap_or("none")
                ));
                lines.push(format!(
                    "Live polling: {} ({}s)",
                    if self.monitor_live_poll { "on" } else { "off" },
                    self.monitor_refresh_seconds
                ));
                lines.push(format!(
                    "Query panel: {}",
                    if self.monitor_query_mode {
                        "editing"
                    } else {
                        "read-only"
                    }
                ));
                if let Some(plan) = &self.monitor_chart_plan {
                    lines.push(format!("Chart output: {}", plan.output_path.display()));
                }
            }
        }
        lines.push(monitor_detail_hint(self));
        lines.extend(self.notifications.iter().take(5).cloned());
        lines
    }

    pub fn current_document(&self) -> Option<&DocumentResource> {
        self.documents.get(self.document_index)
    }

    pub fn document_editor(&self) -> Option<&Editor> {
        self.document_editor.as_ref()
    }

    pub fn document_editor_cursor(&self, area: Rect) -> Option<(u16, u16)> {
        self.document_editor
            .as_ref()
            .and_then(|editor| editor.get_visible_cursor(&area))
    }

    pub fn filtered_config_frame(&self) -> DataFrame {
        self.config_frame.filter_contains(&self.config_filter_query)
    }

    pub fn selected_config_record(&self) -> Option<Vec<(String, String)>> {
        let row_index = self.selected_config_row_index()?;
        let row = self.config_frame.rows.get(row_index)?;
        Some(
            self.config_frame
                .columns
                .iter()
                .zip(row.iter())
                .map(|(column, cell)| (column.name.clone(), cell.as_display()))
                .collect(),
        )
    }

    pub fn selected_config_json(&self) -> String {
        let Some(row_index) = self.selected_config_row_index() else {
            return "{\n  \"message\": \"No config row selected\"\n}".into();
        };
        serde_json::to_string_pretty(&serde_json::Value::Object(
            self.config_frame.record_at(row_index).unwrap_or_default(),
        ))
        .unwrap_or_else(|_| "{\n  \"error\": \"Unable to serialize selection\"\n}".into())
    }

    pub fn selected_config_row_index(&self) -> Option<usize> {
        self.config_frame
            .filter_row_indexes(&self.config_filter_query)
            .get(self.config_index)
            .copied()
    }

    pub fn config_editor_cursor(&self, area: Rect) -> Option<(u16, u16)> {
        self.config_editor
            .as_ref()
            .and_then(|editor| editor.get_visible_cursor(&area))
    }

    pub fn toggle_monitor_query_mode(&mut self) {
        if self.active_view != ActiveView::Monitoring {
            return;
        }
        if !self.monitor_query_mode {
            self.sync_monitor_query_editor();
        }
        self.monitor_query_mode = !self.monitor_query_mode;
        self.notifications.insert(
            0,
            if self.monitor_query_mode {
                "Metric query editing enabled".into()
            } else {
                "Metric query editing disabled".into()
            },
        );
    }

    pub fn handle_monitor_query_input(&mut self, key: KeyEvent, area: Rect) {
        if self.monitor_query_editor.is_none() {
            self.sync_monitor_query_editor();
        }
        if let Some(editor) = &mut self.monitor_query_editor {
            let _ = editor.input(key, &area);
        }
    }

    pub fn apply_monitor_query(&mut self) {
        if let Some(editor) = &self.monitor_query_editor {
            self.monitor_query = editor.get_content().trim().to_string();
        }
        let current_query = self.monitor_query.clone();
        if let Some(source) = self.current_monitor_source_mut() {
            source.query = current_query.clone();
            if let Some(index) = source
                .query_presets
                .iter()
                .position(|preset| preset == &current_query)
            {
                source.preset_index = index;
            }
        }
        self.monitor_query_mode = false;
        self.notifications
            .insert(0, format!("Applied metric query: {}", self.monitor_query));
        self.refresh_monitor_only();
    }

    pub fn cycle_monitor_provider(&mut self, delta: isize) {
        let len = self.monitor_sources.len();
        if len == 0 {
            self.notifications
                .insert(0, "No monitoring providers configured".into());
            return;
        }
        self.monitor_provider_index = wrap_index(self.monitor_provider_index, len, delta);
        if let Some(source) = self.current_monitor_source() {
            self.monitor_query = source.query.clone();
        }
        self.sync_monitor_query_editor();
        self.notifications.insert(
            0,
            format!(
                "Switched monitor provider to {}",
                self.current_monitor_provider_name().unwrap_or("unknown")
            ),
        );
        self.refresh_monitor_only();
    }

    pub fn cycle_monitor_preset(&mut self, delta: isize) {
        let Some(source) = self.current_monitor_source_mut() else {
            self.notifications
                .insert(0, "No monitoring providers configured".into());
            return;
        };
        if source.query_presets.is_empty() {
            self.notifications
                .insert(0, "Current provider has no query presets".into());
            return;
        }

        source.preset_index = wrap_index(source.preset_index, source.query_presets.len(), delta);
        let next_query = source.query_presets[source.preset_index].clone();
        source.query = next_query.clone();
        self.monitor_query = next_query;
        self.sync_monitor_query_editor();
        self.notifications
            .insert(0, format!("Applied query preset: {}", self.monitor_query));
        self.refresh_monitor_only();
    }

    pub fn toggle_monitor_live_poll(&mut self) {
        self.monitor_live_poll = !self.monitor_live_poll;
        self.notifications.insert(
            0,
            if self.monitor_live_poll {
                format!("Live polling enabled ({}s)", self.monitor_refresh_seconds)
            } else {
                "Live polling disabled".into()
            },
        );
    }

    pub fn monitor_live_poll(&self) -> bool {
        self.monitor_live_poll
    }

    pub fn sync_monitor_graph_area(&mut self, area: Rect, picker: &Picker) {
        let normalized = Rect::new(area.x, area.y, area.width.max(20), area.height.max(8));
        if normalized == self.monitor_graph_area {
            return;
        }
        self.monitor_graph_area = normalized;
        self.refresh_monitor_chart();
        self.sync_monitor_image(picker);
    }

    pub fn monitor_query_cursor(&self, area: Rect) -> Option<(u16, u16)> {
        self.monitor_query_editor
            .as_ref()
            .and_then(|editor| editor.get_visible_cursor(&area))
    }

    pub fn monitor_query_editor(&self) -> Option<&Editor> {
        self.monitor_query_editor.as_ref()
    }

    pub fn sync_monitor_image(&mut self, picker: &Picker) {
        let Some(ChartRenderStatus::Rendered(path)) = &self.monitor_chart_status else {
            self.monitor_image = None;
            return;
        };

        match image::ImageReader::open(path)
            .and_then(|reader| reader.decode().map_err(std::io::Error::other))
        {
            Ok(image) => {
                self.monitor_image = Some(picker.new_resize_protocol(image));
            }
            Err(error) => {
                self.monitor_image = None;
                self.notifications
                    .insert(0, format!("PNG chart load failed: {error}"));
            }
        }
    }

    fn sync_config_editor(&mut self, force: bool) {
        let selected_row = self.selected_config_row_index();
        if !force && self.config_editor_dirty && self.config_editor_row == selected_row {
            return;
        }

        self.config_editor_row = selected_row;
        let payload = self.selected_config_json();
        self.config_editor = Editor::new("json", &payload, theme::vesper())
            .or_else(|_| Editor::new("text", &payload, theme::vesper()))
            .ok();
        self.config_editor_dirty = false;
    }

    fn sync_document_editor(&mut self, force: bool) {
        if !force && self.document_editor_dirty {
            return;
        }
        let payload = self
            .current_document()
            .map(|document| document.raw.clone())
            .unwrap_or_else(|| "# No document selected\n".into());
        self.document_editor = Editor::new("md", &payload, theme::vesper())
            .or_else(|_| Editor::new("text", &payload, theme::vesper()))
            .ok();
        self.document_editor_dirty = false;
    }

    fn sync_monitor_query_editor(&mut self) {
        self.monitor_query_editor = Editor::new("text", &self.monitor_query, theme::vesper())
            .or_else(|_| Editor::new("text", &self.monitor_query, theme::vesper()))
            .ok();
    }

    fn refresh_monitor_only(&mut self) {
        let context = self.registry.context_for(self.workspace_root.clone());
        self.refresh_monitor_source(&context);
        self.refresh_monitor_chart();
    }

    fn refresh_monitor_chart(&mut self) {
        if let Some(frame) = &self.monitor_frame {
            let ascii_width = self.monitor_graph_area.width.saturating_sub(2).max(20);
            let ascii_height = self.monitor_graph_area.height.saturating_sub(2).max(8);
            self.monitor_chart_ascii = render_ascii_chart(frame, ascii_width, ascii_height).ok();
            let (pixel_width, pixel_height) = self.monitor_pixel_size();
            let (plan, status) = render_chart_with_size(frame, pixel_width, pixel_height);
            self.monitor_chart_plan = Some(plan);
            self.monitor_chart_status = Some(status);
            self.monitor_image = None;
        } else {
            self.monitor_chart_ascii = None;
            self.monitor_chart_plan = None;
            self.monitor_chart_status = None;
            self.monitor_image = None;
        }
    }

    fn refresh_monitor_source(&mut self, context: &crate::providers::ProviderContext) {
        let Some(provider_name) = self.current_monitor_provider_name().map(str::to_string) else {
            self.monitor_frame = None;
            return;
        };
        let Some(provider) = self.registry.monitors.get(&provider_name) else {
            self.monitor_frame = None;
            self.notifications.insert(
                0,
                format!("Monitor provider '{provider_name}' is not registered"),
            );
            return;
        };

        self.monitor_frame = match provider.fetch(context, &self.monitor_query) {
            Ok(frame) => Some(frame),
            Err(error) => {
                self.notifications
                    .insert(0, format!("{} provider error: {}", provider.name(), error));
                None
            }
        };
    }

    fn current_monitor_provider_name(&self) -> Option<&str> {
        self.monitor_sources
            .get(self.monitor_provider_index)
            .map(|source| source.name.as_str())
    }

    fn current_monitor_source(&self) -> Option<&MonitorSourceState> {
        self.monitor_sources.get(self.monitor_provider_index)
    }

    fn current_monitor_source_mut(&mut self) -> Option<&mut MonitorSourceState> {
        self.monitor_sources.get_mut(self.monitor_provider_index)
    }

    fn monitor_pixel_size(&self) -> (u32, u32) {
        let width = u32::from(self.monitor_graph_area.width.max(20)) * 10;
        let height = u32::from(self.monitor_graph_area.height.max(8)) * 20;
        (width, height)
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

fn wrap_index(index: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let len = len as isize;
    let next = (index as isize + delta).rem_euclid(len);
    next as usize
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
    use tempfile::tempdir;

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

    #[test]
    fn config_editor_save_updates_selected_row_in_memory() {
        let config = AppConfig::default();
        let registry = ProviderRegistry::from_config(&config);
        let mut state = AppState::new(config, registry, None).unwrap();
        state.active_view = ActiveView::Configs;
        state.config_frame = DataFrame {
            columns: vec![
                ColumnSchema {
                    name: "service".into(),
                    kind: ColumnType::String,
                },
                ColumnSchema {
                    name: "replicas".into(),
                    kind: ColumnType::Int,
                },
            ],
            rows: vec![vec![CellValue::String("api".into()), CellValue::Int(4)]],
        };
        state.sync_config_editor(true);
        state
            .config_editor
            .as_mut()
            .unwrap()
            .set_content("{\n  \"service\": \"api\",\n  \"replicas\": 8\n}");
        state.config_editor_dirty = true;

        state.save_config_editor();

        assert_eq!(state.config_frame.rows[0][1], CellValue::Int(8));
        assert!(!state.config_editor_dirty);
    }

    #[test]
    fn config_editor_export_writes_json_file() {
        let temp = tempdir().unwrap();
        let config = AppConfig::default();
        let registry = ProviderRegistry::from_config(&config);
        let mut state = AppState::new(config, registry, None).unwrap();
        state.active_view = ActiveView::Configs;
        state.workspace_root = temp.path().to_path_buf();
        state.config_frame = DataFrame {
            columns: vec![ColumnSchema {
                name: "service".into(),
                kind: ColumnType::String,
            }],
            rows: vec![vec![CellValue::String("api".into())]],
        };
        state.sync_config_editor(true);

        state.export_config_editor();

        let exported = temp.path().join("exports").join("config-row-1.json");
        assert!(exported.exists());
    }

    #[test]
    fn monitor_provider_switch_updates_active_query() {
        let config = AppConfig::from_toml(
            r#"
            [[monitor_providers]]
            name = "alpha"
            kind = "command"
            query = "up"
            query_presets = ["up", "up{job=\"api\"}"]
            [monitor_providers.command]
            program = "echo"
            args = ["{\"title\":\"CPU\",\"series\":[]}"]

            [[monitor_providers]]
            name = "beta"
            kind = "command"
            query = "rate(http_requests_total[5m])"
            query_presets = ["rate(http_requests_total[5m])"]
            [monitor_providers.command]
            program = "echo"
            args = ["{\"title\":\"RPS\",\"series\":[]}"]
            "#,
        )
        .unwrap();
        let registry = ProviderRegistry::from_config(&config);
        let mut state = AppState::new(config, registry, None).unwrap();

        state.cycle_monitor_provider(1);

        assert_eq!(state.monitor_query, "rate(http_requests_total[5m])");
    }

    #[test]
    fn monitor_preset_switch_updates_query() {
        let config = AppConfig::from_toml(
            r#"
            [[monitor_providers]]
            name = "alpha"
            kind = "command"
            query = "up"
            query_presets = ["up", "up{job=\"api\"}"]
            [monitor_providers.command]
            program = "echo"
            args = ["{\"title\":\"CPU\",\"series\":[]}"]
            "#,
        )
        .unwrap();
        let registry = ProviderRegistry::from_config(&config);
        let mut state = AppState::new(config, registry, None).unwrap();

        state.cycle_monitor_preset(1);

        assert_eq!(state.monitor_query, "up{job=\"api\"}");
    }

    #[test]
    fn live_poll_triggers_refresh_on_tick_boundary() {
        let config = AppConfig::from_toml(
            r#"
            [[monitor_providers]]
            name = "alpha"
            kind = "command"
            query = "up"
            [monitor_providers.command]
            program = "echo"
            args = ["{\"title\":\"CPU\",\"series\":[]}"]

            [ui]
            refresh_seconds = 2
            "#,
        )
        .unwrap();
        let registry = ProviderRegistry::from_config(&config);
        let mut state = AppState::new(config, registry, None).unwrap();
        state.toggle_monitor_live_poll();

        state.on_tick();
        assert!(state.monitor_frame.is_none());
        state.on_tick();
        assert!(state.monitor_frame.is_some());
    }
}
