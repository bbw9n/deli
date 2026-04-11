use std::{
    env, fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    sync::Mutex,
    sync::mpsc,
    thread,
};

use deli::{
    app::state::{ActiveView, AppState},
    models::{
        config::{AppConfig, MonitorProviderKind},
        dataframe::{CellValue, ExportFormat},
    },
    providers::{documents::feishu_auth::FeishuUserSession, registry::ProviderRegistry},
    runtime::gnuplot::{ChartRenderMode, plan_chart},
    ui::render::render_snapshot,
};
use tempfile::tempdir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn command_backed_providers_load_documents_configs_and_metrics() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_fixture(root.join("docs.sh"), include_str!("fixtures/docs.sh"));
    write_fixture(root.join("configs.sh"), include_str!("fixtures/configs.sh"));
    write_fixture(root.join("metrics.sh"), include_str!("fixtures/metrics.sh"));
    write_fixture(
        root.join("services.sh"),
        r#"#!/usr/bin/env sh
echo '{"columns":[{"name":"kind","kind":"string"},{"name":"name","kind":"string"},{"name":"status","kind":"string"}],"rows":[["deployment","checkout-api","healthy"]]}'
"#,
    );

    let config = AppConfig::from_toml(&format!(
        r#"
        [[workspaces]]
        name = "fixture"
        root = "{}"

        [[document_providers]]
        name = "docs"
        [document_providers.command]
        program = "sh"
        args = ["docs.sh"]

        [[config_providers]]
        name = "config"
        [config_providers.command]
        program = "sh"
        args = ["configs.sh"]

        [[monitor_providers]]
        name = "metrics"
        [monitor_providers.command]
        program = "sh"
        args = ["metrics.sh"]

        [[service_providers]]
        name = "services"
        [service_providers.command]
        program = "sh"
        args = ["services.sh"]
        "#,
        root.display()
    ))
    .unwrap();

    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();
    state.refresh_all();

    assert_eq!(state.documents.len(), 2);
    assert_eq!(state.config_frame.rows.len(), 2);
    assert_eq!(
        state.config_frame.rows[0][0],
        CellValue::String("api".into())
    );
    assert_eq!(state.monitor_frame.as_ref().unwrap().series.len(), 1);
    assert_eq!(state.service_frame.rows.len(), 1);
    assert_eq!(
        state.service_frame.rows[0][1],
        CellValue::String("checkout-api".into())
    );
}

#[test]
fn tui_snapshot_renders_primary_shell() {
    let config = AppConfig::default();
    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();
    let snapshot = render_snapshot(&mut state, 100, 25);

    assert!(snapshot.contains("Views"));
    assert!(snapshot.contains("Documents"));
    assert!(snapshot.contains("Services"));
    assert!(snapshot.contains("Details"));
    assert!(snapshot.contains("*Messages*"));
    assert!(snapshot.contains("Status"));
}

#[test]
fn config_exports_are_nushell_friendly() {
    let config = AppConfig::from_toml(
        r#"
        [[config_providers]]
        name = "config"
        [config_providers.command]
        program = "echo"
        args = ["{\"columns\":[{\"name\":\"service\",\"kind\":\"string\"}],\"rows\":[[\"api\"]]}"]
        "#,
    )
    .unwrap();
    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();
    state.refresh_all();

    let ndjson = state.config_frame.export(ExportFormat::Ndjson).unwrap();
    assert_eq!(ndjson.trim(), "{\"service\":\"api\"}");
}

#[test]
fn monitoring_falls_back_without_kitty() {
    let frame = serde_json::from_str(
        r#"{
            "title":"CPU",
            "series":[{"name":"api","unit":"%","points":[{"timestamp":1712300000,"value":44.5}]}]
        }"#,
    )
    .unwrap();

    let plan = plan_chart(&frame);
    assert!(matches!(
        plan.mode,
        ChartRenderMode::TextFallback | ChartRenderMode::KittyImage
    ));
    assert!(plan.script.contains("transparent"));
}

#[test]
fn prometheus_http_provider_fetches_range_query() {
    let (endpoint, rx) = spawn_http_server(
        r#"{
          "status":"success",
          "data":{
            "resultType":"matrix",
            "result":[
              {
                "metric":{"__name__":"up","job":"api"},
                "values":[[1712300000,"1"],[1712300060,"1"]]
              }
            ]
          }
        }"#,
    );

    let config = AppConfig::from_toml(&format!(
        r#"
        [[monitor_providers]]
        name = "prom"
        kind = "prometheus"
        endpoint = "{endpoint}"
        query = "up{{job=\"api\"}}"
        step_seconds = 60
        lookback_minutes = 15
        "#
    ))
    .unwrap();

    assert_eq!(
        config.monitor_providers[0].kind,
        Some(MonitorProviderKind::Prometheus)
    );
    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();
    state.refresh_all();

    let request = rx.recv().unwrap();
    assert!(request.starts_with("GET /api/v1/query_range?"));
    assert!(request.contains("query=up%7Bjob%3D%22api%22%7D"));
    assert!(state.monitor_frame.as_ref().unwrap().series.len() == 1);
}

#[test]
fn victoriametrics_http_provider_uses_prometheus_compatible_api() {
    let (endpoint, rx) = spawn_http_server(
        r#"{
          "status":"success",
          "data":{
            "resultType":"matrix",
            "result":[
              {
                "metric":{"__name__":"http_requests_total","service":"edge"},
                "values":[[1712300000,"42.5"],[1712300060,"44.0"]]
              }
            ]
          }
        }"#,
    );

    let config = AppConfig::from_toml(&format!(
        r#"
        [[monitor_providers]]
        name = "vm"
        kind = "victoria_metrics"
        endpoint = "{endpoint}"
        query = "rate(http_requests_total[5m])"
        "#
    ))
    .unwrap();

    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();
    state.refresh_all();

    let request = rx.recv().unwrap();
    assert!(request.starts_with("GET /api/v1/query_range?"));
    assert!(request.contains("query=rate%28http_requests_total%5B5m%5D%29"));
    assert_eq!(
        state.monitor_frame.as_ref().unwrap().series[0].points[0].value,
        42.5
    );
}

#[test]
fn opentsdb_http_provider_posts_query_payload() {
    let (endpoint, rx) = spawn_http_server(
        r#"[
          {
            "metric":"sys.cpu.user",
            "tags":{"host":"api-0","env":"prod"},
            "dps":{"1712300000":12.1,"1712300060":15.4}
          }
        ]"#,
    );

    let config = AppConfig::from_toml(&format!(
        r#"
        [[monitor_providers]]
        name = "tsdb"
        kind = "open_tsdb"
        endpoint = "{endpoint}"
        query = "sys.cpu.user"
        step_seconds = 30
        lookback_minutes = 10
        "#
    ))
    .unwrap();

    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();
    state.refresh_all();

    let request = rx.recv().unwrap();
    assert!(request.starts_with("POST /api/query "));
    assert!(request.contains("\"metric\":\"sys.cpu.user\""));
    assert!(request.contains("\"downsample\":\"30s-avg\""));
    assert_eq!(
        state.monitor_frame.as_ref().unwrap().series[0].points.len(),
        2
    );
}

#[test]
fn multiple_monitor_providers_can_be_switched_in_app_state() {
    let config = AppConfig::from_toml(
        r#"
        [[monitor_providers]]
        name = "prom-a"
        kind = "command"
        query = "up"
        query_presets = ["up", "up{job=\"api\"}"]
        [monitor_providers.command]
        program = "echo"
        args = ["{\"title\":\"A\",\"series\":[]}"]

        [[monitor_providers]]
        name = "prom-b"
        kind = "command"
        query = "rate(http_requests_total[5m])"
        [monitor_providers.command]
        program = "echo"
        args = ["{\"title\":\"B\",\"series\":[]}"]
        "#,
    )
    .unwrap();
    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();
    state.refresh_all();

    state.cycle_monitor_provider(1);

    assert_eq!(state.monitor_query, "rate(http_requests_total[5m])");
}

#[test]
fn notion_document_provider_fetches_and_saves_markdown() {
    let (endpoint, rx) = spawn_http_sequence_server(vec![
        r##"{"object":"page_markdown","id":"page-123","markdown":"# Runbook\n\nInitial body"}"##,
        r##"{"object":"page_markdown","id":"page-123","markdown":"# Runbook\n\nUpdated body"}"##,
    ]);

    unsafe {
        env::set_var("DELI_TEST_NOTION_TOKEN", "notion-secret");
    }

    let config = AppConfig::from_toml(&format!(
        r#"
        [[document_providers]]
        name = "notion-docs"
        kind = "notion"
        endpoint = "{endpoint}"
        token_env = "DELI_TEST_NOTION_TOKEN"
        ids = ["page-123"]
        "#
    ))
    .unwrap();
    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();
    state.refresh_all();
    state.activate_view(ActiveView::Documents);
    state.toggle_document_editor_mode();
    state
        .document_editor
        .as_mut()
        .unwrap()
        .set_content("# Runbook\n\nUpdated body");
    state.document_editor_dirty = true;

    state.save_document_editor();

    let get_request = rx.recv().unwrap();
    let patch_request = rx.recv().unwrap();
    assert!(get_request.starts_with("GET /v1/pages/page-123/markdown "));
    assert!(
        get_request
            .to_lowercase()
            .contains("authorization: bearer notion-secret")
    );
    assert!(
        get_request
            .to_lowercase()
            .contains("notion-version: 2026-03-11")
    );
    assert!(patch_request.starts_with("PATCH /v1/pages/page-123/markdown "));
    assert!(patch_request.contains("\"type\":\"replace_content\""));
    assert!(patch_request.contains("\"new_str\":\"# Runbook\\n\\nUpdated body\""));
    assert!(state.documents[0].raw.contains("Updated body"));
}

#[test]
fn feishu_document_provider_fetches_and_saves_markdown() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (endpoint, rx) = spawn_http_sequence_server(vec![
        r##"{"code":0,"msg":"ok","data":{"title":"Ops Notes","raw_content":"# Ops Notes\n\nDraft body"}}"##,
        r##"{"code":0,"msg":"ok","data":{"title":"Ops Notes","raw_content":"# Ops Notes\n\nSaved body"}}"##,
    ]);

    let temp = tempdir().unwrap();

    unsafe {
        env::set_var("HOME", temp.path());
        env::set_var("DELI_TEST_FEISHU_TOKEN", "feishu-secret");
    }

    let config = AppConfig::from_toml(&format!(
        r#"
        [[document_providers]]
        name = "feishu-docs"
        kind = "feishu"
        endpoint = "{endpoint}"
        token_env = "DELI_TEST_FEISHU_TOKEN"
        ids = ["doccn123"]
        "#
    ))
    .unwrap();
    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();
    state.refresh_all();
    state.activate_view(ActiveView::Documents);
    state.toggle_document_editor_mode();
    state
        .document_editor
        .as_mut()
        .unwrap()
        .set_content("# Ops Notes\n\nSaved body");
    state.document_editor_dirty = true;

    state.save_document_editor();

    let get_request = rx.recv().unwrap();
    let patch_request = rx.recv().unwrap();
    assert!(get_request.starts_with("GET /open-apis/docx/v1/documents/doccn123/raw_content "));
    assert!(
        get_request
            .to_lowercase()
            .contains("authorization: bearer feishu-secret")
    );
    assert!(patch_request.starts_with("PATCH /open-apis/docx/v1/documents/doccn123/raw_content "));
    assert!(patch_request.contains("\"content\":\"# Ops Notes\\n\\nSaved body\""));
    assert!(state.documents[0].raw.contains("Saved body"));
}

#[test]
fn notion_url_can_be_opened_without_preconfigured_ids() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (endpoint, rx) = spawn_http_server(
        r##"{"object":"page_markdown","id":"page-1234567890abcdef1234567890abcdef","markdown":"# URL Open\n\nHello from Notion"}"##,
    );

    unsafe {
        env::set_var("DELI_NOTION_API_BASE_URL", endpoint);
        env::set_var("NOTION_TOKEN", "notion-secret");
    }

    let config = AppConfig::default();
    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();

    state.open_remote_url(
        "https://www.notion.so/workspace/Runbook-1234567890abcdef1234567890abcdef",
    );

    let request = rx.recv().unwrap();
    assert!(request.starts_with("GET /v1/pages/1234567890abcdef1234567890abcdef/markdown "));
    assert_eq!(state.active_view, ActiveView::Documents);
    assert_eq!(state.documents.len(), 1);
    assert!(state.documents[0].raw.contains("Hello from Notion"));
    assert_eq!(
        state.documents[0].source_url.as_deref(),
        Some("https://www.notion.so/workspace/Runbook-1234567890abcdef1234567890abcdef")
    );
}

#[test]
fn feishu_url_can_be_opened_without_preconfigured_ids() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (endpoint, rx) = spawn_http_server(
        r##"{"code":0,"msg":"ok","data":{"title":"Feishu URL Doc","raw_content":"# URL Open\n\nHello from Feishu"}}"##,
    );

    let temp = tempdir().unwrap();

    unsafe {
        env::set_var("HOME", temp.path());
        env::set_var("DELI_FEISHU_API_BASE_URL", endpoint);
        env::set_var("FEISHU_ACCESS_TOKEN", "feishu-secret");
    }

    let config = AppConfig::default();
    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();

    state.open_remote_url("https://team.feishu.cn/docx/AbCdEfGhIjKlMnOpQrStUv");

    let request = rx.recv().unwrap();
    assert!(
        request.starts_with("GET /open-apis/docx/v1/documents/AbCdEfGhIjKlMnOpQrStUv/raw_content ")
    );
    assert_eq!(state.active_view, ActiveView::Documents);
    assert_eq!(state.documents.len(), 1);
    assert!(state.documents[0].raw.contains("Hello from Feishu"));
    assert_eq!(
        state.documents[0].source_url.as_deref(),
        Some("https://team.feishu.cn/docx/AbCdEfGhIjKlMnOpQrStUv")
    );
}

#[test]
fn feishu_expired_user_session_refreshes_before_doc_fetch() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (endpoint, rx) = spawn_http_sequence_server(vec![
        r#"{"app_access_token":"app-token"}"#,
        r#"{"code":0,"msg":"ok","data":{"access_token":"user-token","refresh_token":"refresh-2","expires_in":7200,"refresh_expires_in":86400,"name":"Alice","user_id":"ou_alice"}}"#,
        r##"{"code":0,"msg":"ok","data":{"title":"Session Doc","raw_content":"# Session Doc\n\nBody via refreshed token"}}"##,
    ]);

    let temp = tempdir().unwrap();
    let auth_dir = temp.path().join(".config/deli/auth");
    fs::create_dir_all(&auth_dir).unwrap();
    let session_path = auth_dir.join("feishu_user_session.json");
    fs::write(
        &session_path,
        serde_json::to_vec_pretty(&FeishuUserSession {
            access_token: "stale-token".into(),
            refresh_token: "refresh-1".into(),
            expires_at: 1,
            refresh_expires_at: Some(86_400),
            name: Some("Alice".into()),
            user_id: Some("ou_alice".into()),
            open_id: None,
        })
        .unwrap(),
    )
    .unwrap();

    unsafe {
        env::set_var("HOME", temp.path());
        env::set_var("DELI_FEISHU_API_BASE_URL", endpoint);
        env::set_var("FEISHU_APP_ID", "cli_test");
        env::set_var("FEISHU_APP_SECRET", "secret_test");
    }

    let config = AppConfig::default();
    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();

    state.open_remote_url("https://team.feishu.cn/docx/RefreshDoc123");

    let app_request = rx.recv().unwrap();
    let refresh_request = rx.recv().unwrap();
    let doc_request = rx.recv().unwrap();
    assert!(app_request.starts_with("POST /open-apis/auth/v3/app_access_token/internal "));
    assert!(app_request.contains("\"app_id\":\"cli_test\""));
    assert!(app_request.contains("\"app_secret\":\"secret_test\""));
    assert!(refresh_request.starts_with("POST /open-apis/authen/v1/refresh_access_token "));
    assert!(
        refresh_request
            .to_lowercase()
            .contains("authorization: bearer app-token")
    );
    assert!(refresh_request.contains("\"refresh_token\":\"refresh-1\""));
    assert!(doc_request.starts_with("GET /open-apis/docx/v1/documents/RefreshDoc123/raw_content "));
    assert!(
        doc_request
            .to_lowercase()
            .contains("authorization: bearer user-token")
    );
    assert!(state.documents[0].raw.contains("Body via refreshed token"));
}

fn write_fixture(path: impl AsRef<Path>, body: &str) {
    fs::write(&path, body).unwrap();
}

fn spawn_http_server(response_body: &'static str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0_u8; 16 * 1024];
            let bytes_read = stream.read(&mut buffer).unwrap();
            let request = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
            tx.send(request).unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
    });

    (format!("http://{}", addr), rx)
}

fn spawn_http_sequence_server(
    response_bodies: Vec<&'static str>,
) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for response_body in response_bodies {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 16 * 1024];
                let bytes_read = stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
                tx.send(request).unwrap();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                stream.write_all(response.as_bytes()).unwrap();
                stream.flush().unwrap();
            }
        }
    });

    (format!("http://{}", addr), rx)
}
