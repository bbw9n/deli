use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    sync::mpsc,
    thread,
};

use deli::{
    app::state::AppState,
    models::{
        config::{AppConfig, MonitorProviderKind},
        dataframe::{CellValue, ExportFormat},
    },
    providers::registry::ProviderRegistry,
    runtime::gnuplot::{ChartRenderMode, plan_chart},
    ui::render::render_snapshot,
};
use tempfile::tempdir;

#[test]
fn command_backed_providers_load_documents_configs_and_metrics() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_fixture(root.join("docs.sh"), include_str!("fixtures/docs.sh"));
    write_fixture(root.join("configs.sh"), include_str!("fixtures/configs.sh"));
    write_fixture(root.join("metrics.sh"), include_str!("fixtures/metrics.sh"));

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
}

#[test]
fn tui_snapshot_renders_primary_shell() {
    let config = AppConfig::default();
    let registry = ProviderRegistry::from_config(&config);
    let mut state = AppState::new(config, registry, None).unwrap();
    let snapshot = render_snapshot(&mut state, 100, 25);

    assert!(snapshot.contains("Views"));
    assert!(snapshot.contains("Documents"));
    assert!(snapshot.contains("Details"));
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
