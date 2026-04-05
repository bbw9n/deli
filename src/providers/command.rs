use std::{path::Path, process::Command};

use serde::Deserialize;

use crate::{
    models::{
        config::{CommandSpec, ProviderConfig},
        dataframe::DataFrame,
        document::{DocumentFormat, DocumentResource},
        error::{DeliError, DeliErrorKind},
        timeseries::TimeSeriesFrame,
    },
    providers::{
        ActionProvider, ConfigProvider, DocumentProvider, MonitorProvider, ProviderContext,
    },
};

#[derive(Debug, Clone)]
pub struct CommandDocumentProvider {
    name: String,
    command: CommandSpec,
}

#[derive(Debug, Clone)]
pub struct CommandConfigProvider {
    name: String,
    command: CommandSpec,
}

#[derive(Debug, Clone)]
pub struct CommandMonitorProvider {
    name: String,
    command: CommandSpec,
}

#[derive(Debug, Clone)]
pub struct CommandActionProvider {
    name: String,
    command: CommandSpec,
}

#[derive(Debug, Deserialize)]
struct RawDocument {
    id: String,
    path: String,
    format: String,
    raw: String,
}

impl CommandDocumentProvider {
    pub fn new(config: &ProviderConfig) -> Self {
        Self {
            name: config.name.clone(),
            command: config.command.clone(),
        }
    }
}

impl CommandConfigProvider {
    pub fn new(config: &ProviderConfig) -> Self {
        Self {
            name: config.name.clone(),
            command: config.command.clone(),
        }
    }
}

impl CommandMonitorProvider {
    pub fn new(config: &ProviderConfig) -> Self {
        Self {
            name: config.name.clone(),
            command: config.command.clone(),
        }
    }
}

impl CommandActionProvider {
    pub fn new(config: &ProviderConfig) -> Self {
        Self {
            name: config.name.clone(),
            command: config.command.clone(),
        }
    }
}

impl DocumentProvider for CommandDocumentProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn list_documents(
        &self,
        context: &ProviderContext,
    ) -> Result<Vec<DocumentResource>, DeliError> {
        let output = run_command(&self.command, &context.workspace_root)?;
        let documents: Vec<RawDocument> = serde_json::from_str(&output).map_err(|error| {
            DeliError::new(
                DeliErrorKind::Provider,
                format!("invalid document provider payload: {error}"),
            )
            .with_retry_hint("Return a JSON array of document objects.")
        })?;

        Ok(documents
            .into_iter()
            .map(|document| {
                DocumentResource::from_source(
                    document.id,
                    document.path,
                    parse_format(&document.format),
                    document.raw,
                )
            })
            .collect())
    }
}

impl ConfigProvider for CommandConfigProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetch(&self, context: &ProviderContext) -> Result<DataFrame, DeliError> {
        let output = run_command(&self.command, &context.workspace_root)?;
        serde_json::from_str(&output).map_err(|error| {
            DeliError::new(
                DeliErrorKind::Provider,
                format!("invalid config provider payload: {error}"),
            )
            .with_retry_hint("Return a JSON object that matches the DataFrame schema.")
        })
    }
}

impl MonitorProvider for CommandMonitorProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetch(&self, context: &ProviderContext) -> Result<TimeSeriesFrame, DeliError> {
        let output = run_command(&self.command, &context.workspace_root)?;
        serde_json::from_str(&output).map_err(|error| {
            DeliError::new(
                DeliErrorKind::Provider,
                format!("invalid monitor provider payload: {error}"),
            )
            .with_retry_hint("Return a JSON object that matches the TimeSeriesFrame schema.")
        })
    }
}

impl ActionProvider for CommandActionProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![self.command.clone()]
    }
}

fn run_command(command: &CommandSpec, workspace_root: &Path) -> Result<String, DeliError> {
    let mut process = Command::new(&command.program);
    process.args(&command.args);
    process.current_dir(
        command
            .cwd
            .as_ref()
            .map(|path| workspace_root.join(path))
            .unwrap_or_else(|| workspace_root.to_path_buf()),
    );

    let output = process.output().map_err(|error| {
        DeliError::new(
            DeliErrorKind::Provider,
            format!("failed to launch {}: {error}", command.program),
        )
        .with_retry_hint("Verify the command exists and is executable.")
    })?;

    if !output.status.success() {
        return Err(DeliError::new(
            DeliErrorKind::Provider,
            format!(
                "{} exited with {}: {}",
                command.program,
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        )
        .with_retry_hint("Fix the provider command or inspect stderr output."));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_format(value: &str) -> DocumentFormat {
    match value {
        "mintlify" => DocumentFormat::Mintlify,
        "markdown" => DocumentFormat::Markdown,
        _ => DocumentFormat::PlainText,
    }
}
