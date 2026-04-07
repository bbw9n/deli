use std::{env, time::Duration};

use reqwest::blocking::Client;
use serde::Deserialize;

use crate::{
    models::{
        config::{DocumentProviderConfig, DocumentProviderKind},
        document::{DocumentFormat, DocumentResource},
        error::{DeliError, DeliErrorKind},
    },
    providers::{DocumentProvider, ProviderContext},
};

#[derive(Debug, Clone)]
pub struct NotionDocumentProvider {
    name: String,
    endpoint: String,
    token_env: String,
    notion_version: String,
    ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FeishuDocumentProvider {
    name: String,
    endpoint: String,
    token_env: String,
    ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct NotionMarkdownResponse {
    id: String,
    markdown: String,
}

#[derive(Debug, Deserialize)]
struct FeishuEnvelope {
    code: i64,
    msg: Option<String>,
    data: Option<FeishuDocumentData>,
}

#[derive(Debug, Deserialize)]
struct FeishuDocumentData {
    title: Option<String>,
    content: Option<String>,
    markdown: Option<String>,
    raw_content: Option<String>,
}

pub fn document_kind(config: &DocumentProviderConfig) -> DocumentProviderKind {
    config.kind.clone().unwrap_or(DocumentProviderKind::Command)
}

impl NotionDocumentProvider {
    pub fn new(config: &DocumentProviderConfig) -> Result<Self, DeliError> {
        Ok(Self {
            name: config.name.clone(),
            endpoint: config.endpoint.clone().ok_or_else(|| {
                DeliError::new(
                    DeliErrorKind::Configuration,
                    format!("document provider '{}' is missing endpoint", config.name),
                )
            })?,
            token_env: config.token_env.clone().ok_or_else(|| {
                DeliError::new(
                    DeliErrorKind::Configuration,
                    format!("document provider '{}' is missing token_env", config.name),
                )
            })?,
            notion_version: config.notion_version.clone(),
            ids: config.ids.clone(),
        })
    }

    fn fetch_page(&self, client: &Client, page_id: &str) -> Result<DocumentResource, DeliError> {
        let token = read_token(&self.token_env)?;
        let response = client
            .get(format!(
                "{}/v1/pages/{}/markdown",
                self.endpoint.trim_end_matches('/'),
                page_id
            ))
            .bearer_auth(token)
            .header("Notion-Version", &self.notion_version)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("notion request failed: {error}"),
                )
                .with_retry_hint("Check the page ID, token, and Notion integration capabilities.")
            })?
            .json::<NotionMarkdownResponse>()
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("invalid notion markdown response: {error}"),
                )
            })?;

        Ok(DocumentResource::from_source(
            response.id.clone(),
            format!("notion:{}", response.id),
            DocumentFormat::Markdown,
            response.markdown,
        )
        .with_origin(self.name.clone(), response.id, true))
    }
}

impl FeishuDocumentProvider {
    pub fn new(config: &DocumentProviderConfig) -> Result<Self, DeliError> {
        Ok(Self {
            name: config.name.clone(),
            endpoint: config.endpoint.clone().ok_or_else(|| {
                DeliError::new(
                    DeliErrorKind::Configuration,
                    format!("document provider '{}' is missing endpoint", config.name),
                )
            })?,
            token_env: config.token_env.clone().ok_or_else(|| {
                DeliError::new(
                    DeliErrorKind::Configuration,
                    format!("document provider '{}' is missing token_env", config.name),
                )
            })?,
            ids: config.ids.clone(),
        })
    }

    fn fetch_document(
        &self,
        client: &Client,
        document_id: &str,
    ) -> Result<DocumentResource, DeliError> {
        let token = read_token(&self.token_env)?;
        let response = client
            .get(format!(
                "{}/open-apis/docx/v1/documents/{}/raw_content",
                self.endpoint.trim_end_matches('/'),
                document_id
            ))
            .bearer_auth(token)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("feishu request failed: {error}"),
                )
                .with_retry_hint("Check the document ID, token, and Feishu app permissions.")
            })?
            .json::<FeishuEnvelope>()
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("invalid feishu document response: {error}"),
                )
            })?;

        if response.code != 0 {
            return Err(DeliError::new(
                DeliErrorKind::Provider,
                format!(
                    "feishu document fetch failed: {}",
                    response.msg.unwrap_or_else(|| "unknown error".into())
                ),
            ));
        }

        let data = response.data.ok_or_else(|| {
            DeliError::new(
                DeliErrorKind::Provider,
                "feishu document response missing data",
            )
        })?;
        let markdown = data
            .raw_content
            .or(data.markdown)
            .or(data.content)
            .ok_or_else(|| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    "feishu document response missing markdown content",
                )
            })?;
        let path = data
            .title
            .unwrap_or_else(|| format!("feishu:{document_id}"));

        Ok(
            DocumentResource::from_source(document_id, path, DocumentFormat::Markdown, markdown)
                .with_origin(self.name.clone(), document_id, true),
        )
    }
}

impl DocumentProvider for NotionDocumentProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn list_documents(
        &self,
        _context: &ProviderContext,
    ) -> Result<Vec<DocumentResource>, DeliError> {
        let client = http_client()?;
        self.ids
            .iter()
            .map(|page_id| self.fetch_page(&client, page_id))
            .collect()
    }

    fn save_document(
        &self,
        _context: &ProviderContext,
        document: &DocumentResource,
    ) -> Result<DocumentResource, DeliError> {
        let remote_id = document.remote_id.as_deref().unwrap_or(&document.id);
        let token = read_token(&self.token_env)?;
        let client = http_client()?;
        let response = client
            .patch(format!(
                "{}/v1/pages/{}/markdown",
                self.endpoint.trim_end_matches('/'),
                remote_id
            ))
            .bearer_auth(token)
            .header("Notion-Version", &self.notion_version)
            .json(&serde_json::json!({
                "type": "replace_content",
                "replace_content": {
                    "new_str": document.raw,
                }
            }))
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("notion save failed: {error}"),
                )
                .with_retry_hint("Check update content capabilities and page sharing.")
            })?
            .json::<NotionMarkdownResponse>()
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("invalid notion markdown update response: {error}"),
                )
            })?;

        Ok(DocumentResource::from_source(
            response.id.clone(),
            format!("notion:{}", response.id),
            DocumentFormat::Markdown,
            response.markdown,
        )
        .with_origin(self.name.clone(), response.id, true))
    }
}

impl DocumentProvider for FeishuDocumentProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn list_documents(
        &self,
        _context: &ProviderContext,
    ) -> Result<Vec<DocumentResource>, DeliError> {
        let client = http_client()?;
        self.ids
            .iter()
            .map(|document_id| self.fetch_document(&client, document_id))
            .collect()
    }

    fn save_document(
        &self,
        _context: &ProviderContext,
        document: &DocumentResource,
    ) -> Result<DocumentResource, DeliError> {
        let remote_id = document.remote_id.as_deref().unwrap_or(&document.id);
        let token = read_token(&self.token_env)?;
        let client = http_client()?;
        let response = client
            .patch(format!(
                "{}/open-apis/docx/v1/documents/{}/raw_content",
                self.endpoint.trim_end_matches('/'),
                remote_id
            ))
            .bearer_auth(token)
            .json(&serde_json::json!({
                "content": document.raw,
            }))
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("feishu save failed: {error}"),
                )
                .with_retry_hint("Check document write permissions for the Feishu app.")
            })?
            .json::<FeishuEnvelope>()
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("invalid feishu document update response: {error}"),
                )
            })?;

        if response.code != 0 {
            return Err(DeliError::new(
                DeliErrorKind::Provider,
                format!(
                    "feishu document save failed: {}",
                    response.msg.unwrap_or_else(|| "unknown error".into())
                ),
            ));
        }

        let data = response.data.ok_or_else(|| {
            DeliError::new(
                DeliErrorKind::Provider,
                "feishu document update response missing data",
            )
        })?;
        let markdown = data
            .raw_content
            .or(data.markdown)
            .or(data.content)
            .unwrap_or_else(|| document.raw.clone());
        let path = data.title.unwrap_or_else(|| format!("feishu:{remote_id}"));

        Ok(
            DocumentResource::from_source(remote_id, path, DocumentFormat::Markdown, markdown)
                .with_origin(self.name.clone(), remote_id, true),
        )
    }
}

fn read_token(token_env: &str) -> Result<String, DeliError> {
    env::var(token_env).map_err(|_| {
        DeliError::new(
            DeliErrorKind::Configuration,
            format!("environment variable '{token_env}' is not set"),
        )
    })
}

fn http_client() -> Result<Client, DeliError> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| {
            DeliError::new(
                DeliErrorKind::Provider,
                format!("http client error: {error}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_kind_defaults_to_command() {
        let config = DocumentProviderConfig {
            name: "docs".into(),
            kind: None,
            command: None,
            endpoint: None,
            token_env: None,
            notion_version: "2026-03-11".into(),
            ids: vec![],
        };
        assert_eq!(document_kind(&config), DocumentProviderKind::Command);
    }
}
