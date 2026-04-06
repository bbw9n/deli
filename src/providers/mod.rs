pub mod command;
pub mod metrics_http;
pub mod registry;

use std::path::PathBuf;

use crate::models::{
    config::CommandSpec, dataframe::DataFrame, document::DocumentResource, error::DeliError,
    timeseries::TimeSeriesFrame,
};

#[derive(Debug, Clone)]
pub struct ProviderContext {
    pub workspace_root: PathBuf,
}

pub trait DocumentProvider: Send + Sync {
    fn name(&self) -> &str;
    fn list_documents(&self, context: &ProviderContext)
    -> Result<Vec<DocumentResource>, DeliError>;
}

pub trait ConfigProvider: Send + Sync {
    fn name(&self) -> &str;
    fn fetch(&self, context: &ProviderContext) -> Result<DataFrame, DeliError>;
}

pub trait MonitorProvider: Send + Sync {
    fn name(&self) -> &str;
    fn fetch(&self, context: &ProviderContext, query: &str) -> Result<TimeSeriesFrame, DeliError>;
}

pub trait ActionProvider: Send + Sync {
    fn name(&self) -> &str;
    fn commands(&self) -> Vec<CommandSpec>;
}
