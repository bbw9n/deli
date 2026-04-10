use std::{
    env, fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::models::{
    config::DocumentProviderConfig,
    error::{DeliError, DeliErrorKind},
};

#[derive(Debug, Clone)]
pub struct FeishuAuthConfig {
    pub endpoint: String,
    pub app_id_env: String,
    pub app_secret_env: String,
    pub token_env: String,
    pub authorize_url: String,
    pub token_url: String,
    pub refresh_url: String,
    pub redirect_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuUserSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub refresh_expires_at: Option<i64>,
    pub name: Option<String>,
    pub user_id: Option<String>,
    pub open_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AppAccessTokenResponse {
    app_access_token: Option<String>,
    tenant_access_token: Option<String>,
    code: Option<i64>,
    msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserTokenEnvelope {
    code: Option<i64>,
    msg: Option<String>,
    data: Option<UserTokenData>,
}

#[derive(Debug, Deserialize)]
struct UserTokenData {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    refresh_expires_in: Option<i64>,
    name: Option<String>,
    user_id: Option<String>,
    open_id: Option<String>,
}

pub fn resolve_feishu_auth_config(configs: &[DocumentProviderConfig]) -> FeishuAuthConfig {
    let base = configs
        .iter()
        .find(|config| {
            matches!(
                config.kind,
                Some(crate::models::config::DocumentProviderKind::Feishu)
            )
        })
        .cloned()
        .unwrap_or(DocumentProviderConfig {
            name: "feishu-auth".into(),
            kind: Some(crate::models::config::DocumentProviderKind::Feishu),
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
        });

    let endpoint = base
        .endpoint
        .unwrap_or_else(|| "https://open.feishu.cn".into());
    FeishuAuthConfig {
        endpoint: endpoint.clone(),
        app_id_env: base.app_id_env.unwrap_or_else(|| "FEISHU_APP_ID".into()),
        app_secret_env: base
            .app_secret_env
            .unwrap_or_else(|| "FEISHU_APP_SECRET".into()),
        token_env: base
            .token_env
            .unwrap_or_else(|| "FEISHU_ACCESS_TOKEN".into()),
        authorize_url: base.authorize_url.unwrap_or_else(|| {
            format!(
                "{}/open-apis/authen/v1/index",
                endpoint.trim_end_matches('/')
            )
        }),
        token_url: base.token_url.unwrap_or_else(|| {
            format!(
                "{}/open-apis/authen/v1/access_token",
                endpoint.trim_end_matches('/')
            )
        }),
        refresh_url: base.refresh_url.unwrap_or_else(|| {
            format!(
                "{}/open-apis/authen/v1/refresh_access_token",
                endpoint.trim_end_matches('/')
            )
        }),
        redirect_port: base.redirect_port,
    }
}

pub fn get_feishu_access_token(configs: &[DocumentProviderConfig]) -> Result<String, DeliError> {
    let config = resolve_feishu_auth_config(configs);
    if let Ok(session) = load_feishu_session() {
        if session.expires_at > now_epoch_seconds() + 60 {
            return Ok(session.access_token);
        }
        if !session.refresh_token.is_empty() {
            let refreshed = refresh_user_session(&config, &session.refresh_token)?;
            save_feishu_session(&refreshed)?;
            return Ok(refreshed.access_token);
        }
    }

    env::var(&config.token_env).map_err(|_| {
        DeliError::new(
            DeliErrorKind::Configuration,
            format!(
                "no Feishu user session or fallback env token found in {}",
                config.token_env
            ),
        )
        .with_retry_hint("Run `connect feishu` in deli or set a fallback access token env var.")
    })
}

pub fn connect_feishu_user_session(
    configs: &[DocumentProviderConfig],
) -> Result<FeishuUserSession, DeliError> {
    let config = resolve_feishu_auth_config(configs);
    let app_id = env::var(&config.app_id_env).map_err(|_| {
        DeliError::new(
            DeliErrorKind::Configuration,
            format!("environment variable '{}' is not set", config.app_id_env),
        )
    })?;
    let state = format!("deli-{}", now_epoch_seconds());
    let redirect_uri = format!("http://127.0.0.1:{}/callback", config.redirect_port);
    let auth_url = build_authorize_url(&config.authorize_url, &app_id, &redirect_uri, &state)?;

    let listener = TcpListener::bind(("127.0.0.1", config.redirect_port)).map_err(|error| {
        DeliError::new(
            DeliErrorKind::Runtime,
            format!("failed to bind local auth callback server: {error}"),
        )
    })?;
    listener
        .set_nonblocking(false)
        .map_err(|error| DeliError::new(DeliErrorKind::Runtime, error.to_string()))?;

    open_browser(&auth_url);
    let code = wait_for_authorization_code(listener, &state)?;
    let session = exchange_authorization_code(&config, &code)?;
    save_feishu_session(&session)?;
    Ok(session)
}

pub fn exchange_authorization_code(
    config: &FeishuAuthConfig,
    code: &str,
) -> Result<FeishuUserSession, DeliError> {
    let client = http_client()?;
    let app_access_token = get_app_access_token(&client, config)?;
    let response = client
        .post(&config.token_url)
        .bearer_auth(app_access_token)
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "code": code,
        }))
        .send()
        .map_err(|error| {
            DeliError::new(
                DeliErrorKind::Provider,
                format!("failed to exchange Feishu authorization code: {error}"),
            )
        })?;
    parse_user_token_response(response, "exchange Feishu authorization code")
}

pub fn refresh_user_session(
    config: &FeishuAuthConfig,
    refresh_token: &str,
) -> Result<FeishuUserSession, DeliError> {
    let client = http_client()?;
    let app_access_token = get_app_access_token(&client, config)?;
    let response = client
        .post(&config.refresh_url)
        .bearer_auth(app_access_token)
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }))
        .send()
        .map_err(|error| {
            DeliError::new(
                DeliErrorKind::Provider,
                format!("failed to refresh Feishu user session: {error}"),
            )
        })?;
    parse_user_token_response(response, "refresh Feishu user session")
}

pub fn load_feishu_session() -> Result<FeishuUserSession, DeliError> {
    let path = feishu_session_path()?;
    let content = fs::read_to_string(path)
        .map_err(|error| DeliError::new(DeliErrorKind::Runtime, error.to_string()))?;
    serde_json::from_str(&content)
        .map_err(|error| DeliError::new(DeliErrorKind::Runtime, error.to_string()))
}

pub fn save_feishu_session(session: &FeishuUserSession) -> Result<(), DeliError> {
    let path = feishu_session_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| DeliError::new(DeliErrorKind::Runtime, error.to_string()))?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(session)
            .map_err(|error| DeliError::new(DeliErrorKind::Runtime, error.to_string()))?,
    )
    .map_err(|error| DeliError::new(DeliErrorKind::Runtime, error.to_string()))
}

fn get_app_access_token(client: &Client, config: &FeishuAuthConfig) -> Result<String, DeliError> {
    let app_id = env::var(&config.app_id_env).map_err(|_| {
        DeliError::new(
            DeliErrorKind::Configuration,
            format!("environment variable '{}' is not set", config.app_id_env),
        )
    })?;
    let app_secret = env::var(&config.app_secret_env).map_err(|_| {
        DeliError::new(
            DeliErrorKind::Configuration,
            format!(
                "environment variable '{}' is not set",
                config.app_secret_env
            ),
        )
    })?;
    let response = client
        .post(format!(
            "{}/open-apis/auth/v3/app_access_token/internal",
            config.endpoint.trim_end_matches('/')
        ))
        .json(&serde_json::json!({
            "app_id": app_id,
            "app_secret": app_secret,
        }))
        .send()
        .map_err(|error| {
            DeliError::new(
                DeliErrorKind::Provider,
                format!("failed to get Feishu app access token: {error}"),
            )
        })?;
    parse_app_access_token_response(response)
}

fn session_from_envelope(envelope: UserTokenEnvelope) -> Result<FeishuUserSession, DeliError> {
    if envelope.code.unwrap_or(0) != 0 {
        return Err(DeliError::new(
            DeliErrorKind::Provider,
            envelope
                .msg
                .unwrap_or_else(|| "Feishu auth request failed".into()),
        ));
    }
    let data = envelope
        .data
        .ok_or_else(|| DeliError::new(DeliErrorKind::Provider, "missing Feishu auth data"))?;
    session_from_user_token_data(data)
}

fn parse_app_access_token_response(
    response: reqwest::blocking::Response,
) -> Result<String, DeliError> {
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| DeliError::new(DeliErrorKind::Provider, error.to_string()))?;
    if !status.is_success() {
        return Err(DeliError::new(
            DeliErrorKind::Provider,
            format!(
                "failed to get Feishu app access token: http {} body: {}",
                status,
                body_snippet(&body)
            ),
        ));
    }

    let parsed = serde_json::from_str::<AppAccessTokenResponse>(&body).map_err(|error| {
        DeliError::new(
            DeliErrorKind::Provider,
            format!(
                "failed to decode Feishu app access token response: {error}; body: {}",
                body_snippet(&body)
            ),
        )
    })?;

    if parsed.code.unwrap_or(0) != 0 {
        return Err(DeliError::new(
            DeliErrorKind::Provider,
            format!(
                "Feishu app access token request failed: {}",
                parsed.msg.unwrap_or_else(|| "unknown error".into())
            ),
        ));
    }

    parsed
        .app_access_token
        .or(parsed.tenant_access_token)
        .ok_or_else(|| {
            DeliError::new(
                DeliErrorKind::Provider,
                format!(
                    "Feishu app access token response missing token field; body: {}",
                    body_snippet(&body)
                ),
            )
        })
}

fn parse_user_token_response(
    response: reqwest::blocking::Response,
    action: &str,
) -> Result<FeishuUserSession, DeliError> {
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| DeliError::new(DeliErrorKind::Provider, error.to_string()))?;
    if !status.is_success() {
        return Err(DeliError::new(
            DeliErrorKind::Provider,
            format!(
                "failed to {action}: http {} body: {}",
                status,
                body_snippet(&body)
            ),
        ));
    }

    if let Ok(envelope) = serde_json::from_str::<UserTokenEnvelope>(&body) {
        return session_from_envelope(envelope);
    }

    if let Ok(data) = serde_json::from_str::<UserTokenData>(&body) {
        return session_from_user_token_data(data);
    }

    Err(DeliError::new(
        DeliErrorKind::Provider,
        format!(
            "failed to decode Feishu auth response while trying to {action}; body: {}",
            body_snippet(&body)
        ),
    ))
}

fn body_snippet(body: &str) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(240).collect()
}

fn session_from_user_token_data(data: UserTokenData) -> Result<FeishuUserSession, DeliError> {
    let access_token = data.access_token.ok_or_else(|| {
        DeliError::new(
            DeliErrorKind::Provider,
            "missing Feishu user access token in auth response",
        )
    })?;
    let now = now_epoch_seconds();
    let expires_in = data.expires_in.unwrap_or(7200);

    Ok(FeishuUserSession {
        access_token,
        refresh_token: data.refresh_token.unwrap_or_default(),
        expires_at: now + expires_in,
        refresh_expires_at: data.refresh_expires_in.map(|ttl| now + ttl),
        name: data.name,
        user_id: data.user_id,
        open_id: data.open_id,
    })
}

fn feishu_session_path() -> Result<PathBuf, DeliError> {
    let home =
        env::var("HOME").map_err(|_| DeliError::new(DeliErrorKind::Runtime, "HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/deli/auth/feishu_user_session.json"))
}

fn build_authorize_url(
    authorize_url: &str,
    app_id: &str,
    redirect_uri: &str,
    state: &str,
) -> Result<String, DeliError> {
    let mut url = Url::parse(authorize_url)
        .map_err(|error| DeliError::new(DeliErrorKind::Configuration, error.to_string()))?;
    url.query_pairs_mut()
        .append_pair("app_id", app_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("state", state);
    Ok(url.to_string())
}

fn wait_for_authorization_code(
    listener: TcpListener,
    expected_state: &str,
) -> Result<String, DeliError> {
    listener
        .set_nonblocking(false)
        .map_err(|error| DeliError::new(DeliErrorKind::Runtime, error.to_string()))?;
    if let Ok((mut stream, _)) = listener.accept() {
        let mut buffer = [0_u8; 16 * 1024];
        let bytes = stream
            .read(&mut buffer)
            .map_err(|error| DeliError::new(DeliErrorKind::Runtime, error.to_string()))?;
        let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
        let first_line = request.lines().next().unwrap_or_default();
        let path = first_line.split_whitespace().nth(1).unwrap_or("/callback");
        let url = Url::parse(&format!("http://127.0.0.1{}", path))
            .map_err(|error| DeliError::new(DeliErrorKind::Runtime, error.to_string()))?;
        let code = url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.to_string())
            .ok_or_else(|| DeliError::new(DeliErrorKind::Runtime, "missing auth code"))?;
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.to_string())
            .unwrap_or_default();
        let response = if state == expected_state {
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nFeishu authorization completed. You can return to deli."
        } else {
            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid authorization state."
        };
        let _ = stream.write_all(response.as_bytes());
        if state != expected_state {
            return Err(DeliError::new(
                DeliErrorKind::Runtime,
                "authorization state mismatch",
            ));
        }
        return Ok(code);
    }
    Err(DeliError::new(
        DeliErrorKind::Runtime,
        "failed to receive Feishu authorization callback",
    ))
}

fn open_browser(url: &str) {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("open", &[])]
    } else if cfg!(target_os = "windows") {
        &[("cmd", &["/C", "start"])]
    } else {
        &[("xdg-open", &[])]
    };

    for (program, args) in candidates {
        let mut command = Command::new(program);
        command.args(*args).arg(url);
        if command.spawn().is_ok() {
            break;
        }
    }
}

fn http_client() -> Result<Client, DeliError> {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| DeliError::new(DeliErrorKind::Provider, error.to_string()))
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::blocking::Client;
    use std::thread;

    #[test]
    fn builds_authorize_url_with_redirect_and_state() {
        let url = build_authorize_url(
            "https://open.feishu.cn/open-apis/authen/v1/index",
            "cli_test",
            "http://127.0.0.1:32323/callback",
            "abc123",
        )
        .unwrap();
        assert!(url.contains("app_id=cli_test"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A32323%2Fcallback"));
        assert!(url.contains("state=abc123"));
    }

    #[test]
    fn parses_minimal_feishu_success_payload_without_refresh_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = r#"{"code":0,"data":{"access_token":"u-test-token","avatar_big":"https://example.com/avatar.png","name":"Test User"}}"#;

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let response = Client::new()
            .post(format!("http://{address}/open-apis/authen/v1/access_token"))
            .send()
            .unwrap();
        let session =
            parse_user_token_response(response, "exchange Feishu authorization code").unwrap();

        assert_eq!(session.access_token, "u-test-token");
        assert_eq!(session.refresh_token, "");
        assert_eq!(session.name.as_deref(), Some("Test User"));
        assert!(session.expires_at > now_epoch_seconds());
    }
}
