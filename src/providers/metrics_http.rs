use std::time::Duration;

use chrono::Utc;
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::{
    models::{
        config::MonitorProviderConfig,
        error::{DeliError, DeliErrorKind},
        timeseries::{DataPoint, Series, TimeSeriesFrame},
    },
    providers::{MonitorProvider, ProviderContext},
};

#[derive(Debug, Clone)]
pub struct PrometheusMonitorProvider {
    name: String,
    product_name: String,
    endpoint: String,
    step_seconds: u64,
    lookback_minutes: u64,
}

#[derive(Debug, Clone)]
pub struct OpenTsdbMonitorProvider {
    name: String,
    endpoint: String,
    step_seconds: u64,
    lookback_minutes: u64,
}

#[derive(Debug, Deserialize)]
struct PrometheusResponse {
    data: PrometheusData,
}

#[derive(Debug, Deserialize)]
struct PrometheusData {
    result: Vec<PrometheusSeries>,
}

#[derive(Debug, Deserialize)]
struct PrometheusSeries {
    metric: serde_json::Map<String, serde_json::Value>,
    values: Vec<(f64, String)>,
}

#[derive(Debug, Deserialize)]
struct OpenTsdbSeries {
    metric: String,
    dps: std::collections::BTreeMap<String, f64>,
    tags: Option<serde_json::Map<String, serde_json::Value>>,
}

impl PrometheusMonitorProvider {
    pub fn new(config: &MonitorProviderConfig, product_name: &str) -> Result<Self, DeliError> {
        Ok(Self {
            name: config.name.clone(),
            product_name: product_name.into(),
            endpoint: config.endpoint.clone().ok_or_else(|| {
                DeliError::new(
                    DeliErrorKind::Configuration,
                    format!("monitor provider '{}' is missing endpoint", config.name),
                )
            })?,
            step_seconds: config.step_seconds,
            lookback_minutes: config.lookback_minutes,
        })
    }
}

impl OpenTsdbMonitorProvider {
    pub fn new(config: &MonitorProviderConfig) -> Result<Self, DeliError> {
        Ok(Self {
            name: config.name.clone(),
            endpoint: config.endpoint.clone().ok_or_else(|| {
                DeliError::new(
                    DeliErrorKind::Configuration,
                    format!("monitor provider '{}' is missing endpoint", config.name),
                )
            })?,
            step_seconds: config.step_seconds,
            lookback_minutes: config.lookback_minutes,
        })
    }
}

impl MonitorProvider for PrometheusMonitorProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetch(&self, _context: &ProviderContext, query: &str) -> Result<TimeSeriesFrame, DeliError> {
        let client = http_client()?;
        let end = Utc::now().timestamp();
        let start = end - (self.lookback_minutes as i64 * 60);
        let response = client
            .get(format!(
                "{}/api/v1/query_range",
                self.endpoint.trim_end_matches('/')
            ))
            .query(&[
                ("query", query),
                ("start", &start.to_string()),
                ("end", &end.to_string()),
                ("step", &self.step_seconds.to_string()),
            ])
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("prometheus request failed: {error}"),
                )
                .with_retry_hint("Check endpoint reachability and query syntax.")
            })?
            .text()
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("invalid {} response: {error}", self.product_name),
                )
            })?;

        parse_prometheus_frame(&response, &self.product_name, query)
    }
}

impl MonitorProvider for OpenTsdbMonitorProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetch(&self, _context: &ProviderContext, query: &str) -> Result<TimeSeriesFrame, DeliError> {
        let client = http_client()?;
        let response = client
            .post(format!("{}/api/query", self.endpoint.trim_end_matches('/')))
            .json(&serde_json::json!({
                "start": format!("{}m-ago", self.lookback_minutes),
                "queries": [{
                    "aggregator": "avg",
                    "metric": query,
                    "downsample": format!("{}s-avg", self.step_seconds)
                }]
            }))
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("opentsdb request failed: {error}"),
                )
                .with_retry_hint("Check endpoint reachability and metric query syntax.")
            })?
            .text()
            .map_err(|error| {
                DeliError::new(
                    DeliErrorKind::Provider,
                    format!("invalid opentsdb response: {error}"),
                )
            })?;

        parse_opentsdb_frame(&response, query)
    }
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

fn series_name(metric: &serde_json::Map<String, serde_json::Value>, fallback: &str) -> String {
    metric
        .iter()
        .find_map(|(key, value)| value.as_str().map(|value| format!("{key}={value}")))
        .unwrap_or_else(|| fallback.to_string())
}

fn parse_prometheus_frame(
    body: &str,
    product_name: &str,
    query: &str,
) -> Result<TimeSeriesFrame, DeliError> {
    let response = serde_json::from_str::<PrometheusResponse>(body).map_err(|error| {
        DeliError::new(
            DeliErrorKind::Provider,
            format!("invalid {product_name} response: {error}"),
        )
    })?;

    Ok(TimeSeriesFrame {
        title: format!("{product_name}: {query}"),
        series: response
            .data
            .result
            .into_iter()
            .map(|series| Series {
                name: series_name(&series.metric, product_name),
                unit: None,
                points: series
                    .values
                    .into_iter()
                    .filter_map(|(timestamp, value)| {
                        value.parse::<f64>().ok().map(|value| DataPoint {
                            timestamp: timestamp as i64,
                            value,
                        })
                    })
                    .collect(),
            })
            .collect(),
    })
}

fn parse_opentsdb_frame(body: &str, query: &str) -> Result<TimeSeriesFrame, DeliError> {
    let response = serde_json::from_str::<Vec<OpenTsdbSeries>>(body).map_err(|error| {
        DeliError::new(
            DeliErrorKind::Provider,
            format!("invalid opentsdb response: {error}"),
        )
    })?;

    Ok(TimeSeriesFrame {
        title: format!("OpenTSDB: {query}"),
        series: response
            .into_iter()
            .map(|series| Series {
                name: series
                    .tags
                    .as_ref()
                    .map(|tags| format!("{} {}", series.metric, series_name(tags, "")))
                    .unwrap_or(series.metric),
                unit: None,
                points: series
                    .dps
                    .into_iter()
                    .filter_map(|(timestamp, value)| {
                        timestamp
                            .parse::<i64>()
                            .ok()
                            .map(|timestamp| DataPoint { timestamp, value })
                    })
                    .collect(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    #[test]
    fn parses_prometheus_protocol_payload() {
        let body = r#"{
          "status":"success",
          "data":{
            "resultType":"matrix",
            "result":[
              {
                "metric":{"__name__":"up","job":"api","instance":"api-0"},
                "values":[[1712300000,"1"],[1712300060,"1"]]
              }
            ]
          }
        }"#;

        let frame = parse_prometheus_frame(body, "Prometheus", "up").unwrap();
        assert_eq!(frame.title, "Prometheus: up");
        assert_eq!(frame.series.len(), 1);
        assert_eq!(frame.series[0].points.len(), 2);
        assert_eq!(frame.series[0].points[1].value, 1.0);
    }

    #[test]
    fn parses_victoriametrics_protocol_payload() {
        let body = r#"{
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
        }"#;

        let frame =
            parse_prometheus_frame(body, "VictoriaMetrics", "rate(http_requests_total[5m])")
                .unwrap();
        assert_eq!(
            frame.title,
            "VictoriaMetrics: rate(http_requests_total[5m])"
        );
        assert_eq!(frame.series[0].points[0].value, 42.5);
    }

    #[test]
    fn parses_opentsdb_protocol_payload() {
        let body = r#"[
          {
            "metric":"sys.cpu.user",
            "tags":{"host":"api-0","env":"prod"},
            "dps":{"1712300000":12.1,"1712300060":15.4}
          }
        ]"#;

        let frame = parse_opentsdb_frame(body, "sys.cpu.user").unwrap();
        assert_eq!(frame.title, "OpenTSDB: sys.cpu.user");
        assert_eq!(frame.series.len(), 1);
        assert_eq!(frame.series[0].points.len(), 2);
    }
}
