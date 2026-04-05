use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataPoint {
    pub timestamp: i64,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub name: String,
    pub unit: Option<String>,
    pub points: Vec<DataPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeSeriesFrame {
    pub title: String,
    pub series: Vec<Series>,
}

impl TimeSeriesFrame {
    pub fn summary_lines(&self) -> Vec<String> {
        self.series
            .iter()
            .map(|series| {
                let latest = series
                    .points
                    .last()
                    .map(|point| point.value)
                    .unwrap_or_default();
                match &series.unit {
                    Some(unit) => format!("{}: {:.2} {}", series.name, latest, unit),
                    None => format!("{}: {:.2}", series.name, latest),
                }
            })
            .collect()
    }
}
