use crate::{
    models::config::{CommandSpec, ProviderConfig},
    providers::ActionProvider,
};

#[derive(Debug, Clone)]
pub struct CommandActionProvider {
    name: String,
    command: CommandSpec,
}

impl CommandActionProvider {
    pub fn new(config: &ProviderConfig) -> Self {
        Self {
            name: config.name.clone(),
            command: config.command.clone(),
        }
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
