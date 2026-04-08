use std::{path::Path, process::Command};

use crate::{
    models::{
        config::{CommandSpec, ProviderConfig},
        dataframe::DataFrame,
        error::{DeliError, DeliErrorKind},
    },
    providers::{ConfigProvider, ProviderContext},
};

#[derive(Debug, Clone)]
pub struct CommandConfigProvider {
    name: String,
    command: CommandSpec,
}

impl CommandConfigProvider {
    pub fn new(config: &ProviderConfig) -> Self {
        Self {
            name: config.name.clone(),
            command: config.command.clone(),
        }
    }
}

impl ConfigProvider for CommandConfigProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetch(&self, context: &ProviderContext) -> Result<DataFrame, DeliError> {
        let output = run_command(&self.command, &context.workspace_root, None)?;
        serde_json::from_str(&output).map_err(|error| {
            DeliError::new(
                DeliErrorKind::Provider,
                format!("invalid config provider payload: {error}"),
            )
            .with_retry_hint("Return a JSON object that matches the DataFrame schema.")
        })
    }
}

fn run_command(
    command: &CommandSpec,
    workspace_root: &Path,
    query: Option<&str>,
) -> Result<String, DeliError> {
    let mut process = Command::new(expand_template(&command.program, query));
    process.args(command.args.iter().map(|arg| expand_template(arg, query)));
    process.current_dir(
        command
            .cwd
            .as_ref()
            .map(|path| workspace_root.join(expand_template(path, query)))
            .unwrap_or_else(|| workspace_root.to_path_buf()),
    );

    if let Some(query) = query {
        process.env("DELI_QUERY", query);
    }

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

fn expand_template(value: &str, query: Option<&str>) -> String {
    query
        .map(|query| value.replace("{{query}}", query))
        .unwrap_or_else(|| value.to_string())
}
