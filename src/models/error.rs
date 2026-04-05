use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DeliErrorKind {
    #[error("configuration")]
    Configuration,
    #[error("provider")]
    Provider,
    #[error("rendering")]
    Rendering,
    #[error("runtime")]
    Runtime,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{kind}: {message}")]
pub struct DeliError {
    pub kind: DeliErrorKind,
    pub message: String,
    pub retry_hint: Option<String>,
}

impl DeliError {
    pub fn new(kind: DeliErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retry_hint: None,
        }
    }

    pub fn with_retry_hint(mut self, retry_hint: impl Into<String>) -> Self {
        self.retry_hint = Some(retry_hint.into());
        self
    }
}
