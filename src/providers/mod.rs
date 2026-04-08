pub mod actions;
pub mod configs;
pub mod documents;
pub mod monitoring;
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
    fn save_document(
        &self,
        _context: &ProviderContext,
        _document: &DocumentResource,
    ) -> Result<DocumentResource, DeliError> {
        Err(DeliError::new(
            crate::models::error::DeliErrorKind::Provider,
            "document provider is read-only",
        ))
    }
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
