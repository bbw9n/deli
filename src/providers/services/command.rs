use std::{path::Path, process::Command};

use crate::{
    models::{
        config::{CommandSpec, ProviderConfig},
        dataframe::DataFrame,
        error::{DeliError, DeliErrorKind},
    },
    providers::{ProviderContext, ServiceProvider},
};

#[derive(Debug, Clone)]
pub struct CommandServiceProvider {
    name: String,
    command: CommandSpec,
}

impl CommandServiceProvider {
    pub fn new(config: &ProviderConfig) -> Self {
        Self {
            name: config.name.clone(),
            command: config.command.clone(),
        }
    }
}

impl ServiceProvider for CommandServiceProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetch(&self, context: &ProviderContext) -> Result<DataFrame, DeliError> {
        let output = run_command(&self.command, &context.workspace_root)?;
        serde_json::from_str(&output).map_err(|error| {
            DeliError::new(
                DeliErrorKind::Provider,
                format!("invalid service provider payload: {error}"),
            )
            .with_retry_hint("Return a JSON object that matches the DataFrame schema.")
        })
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
        .with_retry_hint("Fix the service provider command or inspect stderr output."));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
