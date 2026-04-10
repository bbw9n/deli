use std::{env, time::Duration};

use reqwest::blocking::Client;
use serde::Deserialize;
use url::Url;

use crate::{
    models::{
        config::{DocumentProviderConfig, DocumentProviderKind},
        document::{DocumentFormat, DocumentResource},
        error::{DeliError, DeliErrorKind},
    },
    providers::{
        DocumentProvider, ProviderContext,
        documents::feishu_auth::{connect_feishu_user_session, get_feishu_access_token},
    },
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
    app_id_env: String,
    app_secret_env: String,
    authorize_url: String,
    token_url: String,
    refresh_url: String,
    redirect_port: u16,
    ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteDocumentRef {
    Notion { page_id: String, url: String },
    Feishu { document_id: String, url: String },
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

pub fn parse_remote_document_url(url: &str) -> Result<RemoteDocumentRef, DeliError> {
    let parsed = Url::parse(url).map_err(|error| {
        DeliError::new(
            DeliErrorKind::Configuration,
            format!("invalid remote document URL: {error}"),
        )
    })?;
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let trimmed_path = parsed.path().trim_matches('/');

    if host.contains("notion.so") || host.contains("notion.site") {
        let candidate = trimmed_path
            .split('/')
            .next_back()
            .and_then(|segment| segment.rsplit('-').next())
            .unwrap_or_default()
            .trim();
        if candidate.len() >= 32 {
            return Ok(RemoteDocumentRef::Notion {
                page_id: candidate.to_string(),
                url: url.to_string(),
            });
        }
    }

    if host.contains("feishu.cn")
        || host.contains("larksuite.com")
        || host.contains("larkoffice.com")
    {
        let segments = trimmed_path.split('/').collect::<Vec<_>>();
        if let Some(index) = segments.iter().position(|segment| *segment == "docx")
            && let Some(document_id) = segments.get(index + 1)
        {
            return Ok(RemoteDocumentRef::Feishu {
                document_id: (*document_id).to_string(),
                url: url.to_string(),
            });
        }
        if let Some(index) = segments.iter().position(|segment| *segment == "wiki")
            && let Some(document_id) = segments.get(index + 1)
        {
            return Ok(RemoteDocumentRef::Feishu {
                document_id: (*document_id).to_string(),
                url: url.to_string(),
            });
        }
    }

    Err(DeliError::new(
        DeliErrorKind::Configuration,
        "unsupported remote document URL",
    )
    .with_retry_hint("Paste a full Notion or Feishu/Lark document URL."))
}

pub fn open_remote_document(
    reference: &RemoteDocumentRef,
    configs: &[DocumentProviderConfig],
) -> Result<DocumentResource, DeliError> {
    match reference {
        RemoteDocumentRef::Notion { page_id, url } => {
            let config = resolve_notion_config(configs);
            let provider = NotionDocumentProvider::new(&config)?;
            let client = http_client()?;
            provider
                .fetch_page(&client, page_id)
                .map(|document| document.with_source_url(url.clone()))
        }
        RemoteDocumentRef::Feishu { document_id, url } => {
            let config = resolve_feishu_config(configs);
            let provider = FeishuDocumentProvider::new(&config)?;
            let client = http_client()?;
            provider
                .fetch_document(&client, document_id)
                .map(|document| document.with_source_url(url.clone()))
        }
    }
}

pub fn save_remote_document(
    document: &DocumentResource,
    configs: &[DocumentProviderConfig],
) -> Result<DocumentResource, DeliError> {
    let Some(source_url) = document.source_url.as_deref() else {
        return Err(DeliError::new(
            DeliErrorKind::Configuration,
            "document has no source URL for remote save",
        ));
    };

    match parse_remote_document_url(source_url)? {
        RemoteDocumentRef::Notion { .. } => {
            let config = resolve_notion_config(configs);
            let provider = NotionDocumentProvider::new(&config)?;
            provider
                .save_document(
                    &ProviderContext {
                        workspace_root: ".".into(),
                    },
                    document,
                )
                .map(|saved| saved.with_source_url(source_url.to_string()))
        }
        RemoteDocumentRef::Feishu { .. } => {
            let config = resolve_feishu_config(configs);
            let provider = FeishuDocumentProvider::new(&config)?;
            provider
                .save_document(
                    &ProviderContext {
                        workspace_root: ".".into(),
                    },
                    document,
                )
                .map(|saved| saved.with_source_url(source_url.to_string()))
        }
    }
}

pub fn connect_feishu_from_configs(
    configs: &[DocumentProviderConfig],
) -> Result<String, DeliError> {
    let session = connect_feishu_user_session(&[resolve_feishu_config(configs)])?;
    Ok(session
        .name
        .or(session.user_id)
        .unwrap_or_else(|| "connected Feishu user".into()))
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
            app_id_env: config
                .app_id_env
                .clone()
                .unwrap_or_else(|| "FEISHU_APP_ID".into()),
            app_secret_env: config
                .app_secret_env
                .clone()
                .unwrap_or_else(|| "FEISHU_APP_SECRET".into()),
            authorize_url: config.authorize_url.clone().unwrap_or_else(|| {
                format!(
                    "{}/open-apis/authen/v1/index",
                    config
                        .endpoint
                        .clone()
                        .unwrap_or_else(|| "https://open.feishu.cn".into())
                        .trim_end_matches('/')
                )
            }),
            token_url: config.token_url.clone().unwrap_or_else(|| {
                format!(
                    "{}/open-apis/authen/v1/access_token",
                    config
                        .endpoint
                        .clone()
                        .unwrap_or_else(|| "https://open.feishu.cn".into())
                        .trim_end_matches('/')
                )
            }),
            refresh_url: config.refresh_url.clone().unwrap_or_else(|| {
                format!(
                    "{}/open-apis/authen/v1/refresh_access_token",
                    config
                        .endpoint
                        .clone()
                        .unwrap_or_else(|| "https://open.feishu.cn".into())
                        .trim_end_matches('/')
                )
            }),
            redirect_port: config.redirect_port,
            ids: config.ids.clone(),
        })
    }

    fn fetch_document(
        &self,
        client: &Client,
        document_id: &str,
    ) -> Result<DocumentResource, DeliError> {
        let token = resolve_feishu_token(self)?;
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

fn resolve_notion_config(configs: &[DocumentProviderConfig]) -> DocumentProviderConfig {
    configs
        .iter()
        .find(|config| document_kind(config) == DocumentProviderKind::Notion)
        .cloned()
        .unwrap_or(DocumentProviderConfig {
            name: "notion-url".into(),
            kind: Some(DocumentProviderKind::Notion),
            command: None,
            endpoint: Some(
                env::var("DELI_NOTION_API_BASE_URL")
                    .unwrap_or_else(|_| "https://api.notion.com".into()),
            ),
            token_env: Some("NOTION_TOKEN".into()),
            app_id_env: None,
            app_secret_env: None,
            authorize_url: None,
            token_url: None,
            refresh_url: None,
            notion_version: "2026-03-11".into(),
            ids: Vec::new(),
            redirect_port: 32323,
        })
}

fn resolve_feishu_config(configs: &[DocumentProviderConfig]) -> DocumentProviderConfig {
    configs
        .iter()
        .find(|config| document_kind(config) == DocumentProviderKind::Feishu)
        .cloned()
        .unwrap_or(DocumentProviderConfig {
            name: "feishu-url".into(),
            kind: Some(DocumentProviderKind::Feishu),
            command: None,
            endpoint: Some(
                env::var("DELI_FEISHU_API_BASE_URL")
                    .unwrap_or_else(|_| "https://open.feishu.cn".into()),
            ),
            token_env: Some("FEISHU_ACCESS_TOKEN".into()),
            app_id_env: Some("FEISHU_APP_ID".into()),
            app_secret_env: Some("FEISHU_APP_SECRET".into()),
            authorize_url: None,
            token_url: None,
            refresh_url: None,
            notion_version: "2026-03-11".into(),
            ids: Vec::new(),
            redirect_port: 32323,
        })
}

fn resolve_feishu_token(provider: &FeishuDocumentProvider) -> Result<String, DeliError> {
    get_feishu_access_token(&[DocumentProviderConfig {
        name: provider.name.clone(),
        kind: Some(DocumentProviderKind::Feishu),
        command: None,
        endpoint: Some(provider.endpoint.clone()),
        token_env: Some(provider.token_env.clone()),
        app_id_env: Some(provider.app_id_env.clone()),
        app_secret_env: Some(provider.app_secret_env.clone()),
        authorize_url: Some(provider.authorize_url.clone()),
        token_url: Some(provider.token_url.clone()),
        refresh_url: Some(provider.refresh_url.clone()),
        notion_version: "2026-03-11".into(),
        ids: provider.ids.clone(),
        redirect_port: provider.redirect_port,
    }])
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
            app_id_env: None,
            app_secret_env: None,
            authorize_url: None,
            token_url: None,
            refresh_url: None,
            notion_version: "2026-03-11".into(),
            ids: vec![],
            redirect_port: 32323,
        };
        assert_eq!(document_kind(&config), DocumentProviderKind::Command);
    }

    #[test]
    fn parses_notion_url_into_page_ref() {
        let parsed = parse_remote_document_url(
            "https://www.notion.so/workspace/Runbook-1234567890abcdef1234567890abcdef",
        )
        .unwrap();
        assert_eq!(
            parsed,
            RemoteDocumentRef::Notion {
                page_id: "1234567890abcdef1234567890abcdef".into(),
                url: "https://www.notion.so/workspace/Runbook-1234567890abcdef1234567890abcdef"
                    .into()
            }
        );
    }

    #[test]
    fn parses_feishu_docx_url_into_document_ref() {
        let parsed =
            parse_remote_document_url("https://team.feishu.cn/docx/AbCdEfGhIjKlMnOpQrStUv")
                .unwrap();
        assert_eq!(
            parsed,
            RemoteDocumentRef::Feishu {
                document_id: "AbCdEfGhIjKlMnOpQrStUv".into(),
                url: "https://team.feishu.cn/docx/AbCdEfGhIjKlMnOpQrStUv".into()
            }
        );
    }
}
