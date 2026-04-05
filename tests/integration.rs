use std::{fs, path::Path};

use deli::{
    app::state::AppState,
    models::{
        config::AppConfig,
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
    let state = AppState::new(config, registry, None).unwrap();
    let snapshot = render_snapshot(&state, 100, 25);

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

fn write_fixture(path: impl AsRef<Path>, body: &str) {
    fs::write(&path, body).unwrap();
}
