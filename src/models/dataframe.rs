use std::cmp::Ordering;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnType {
    String,
    Int,
    Float,
    Bool,
    DateTime,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub name: String,
    pub kind: ColumnType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CellValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    DateTime(DateTime<Utc>),
    Json(serde_json::Value),
    Null,
}

impl CellValue {
    pub fn as_display(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => format!("{value:.3}"),
            Self::Bool(value) => value.to_string(),
            Self::DateTime(value) => value.to_rfc3339(),
            Self::Json(value) => value.to_string(),
            Self::Null => "null".to_string(),
        }
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            Self::String(value) => serde_json::Value::String(value.clone()),
            Self::Int(value) => serde_json::Value::Number((*value).into()),
            Self::Float(value) => serde_json::json!(*value),
            Self::Bool(value) => serde_json::Value::Bool(*value),
            Self::DateTime(value) => serde_json::Value::String(value.to_rfc3339()),
            Self::Json(value) => value.clone(),
            Self::Null => serde_json::Value::Null,
        }
    }

    pub fn from_json_value(kind: &ColumnType, value: &serde_json::Value) -> Option<Self> {
        match (kind, value) {
            (_, serde_json::Value::Null) => Some(Self::Null),
            (ColumnType::String, serde_json::Value::String(value)) => {
                Some(Self::String(value.clone()))
            }
            (ColumnType::Int, serde_json::Value::Number(value)) => value.as_i64().map(Self::Int),
            (ColumnType::Float, serde_json::Value::Number(value)) => {
                value.as_f64().map(Self::Float)
            }
            (ColumnType::Bool, serde_json::Value::Bool(value)) => Some(Self::Bool(*value)),
            (ColumnType::DateTime, serde_json::Value::String(value)) => {
                DateTime::parse_from_rfc3339(value)
                    .ok()
                    .map(|dt| Self::DateTime(dt.with_timezone(&Utc)))
            }
            (ColumnType::Json, value) => Some(Self::Json(value.clone())),
            (ColumnType::String, value) => Some(Self::String(value.to_string())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataFrame {
    pub columns: Vec<ColumnSchema>,
    pub rows: Vec<Vec<CellValue>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Csv,
    Ndjson,
}

impl DataFrame {
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }

    pub fn filter_contains(&self, query: &str) -> Self {
        if query.trim().is_empty() {
            return self.clone();
        }

        let query = query.to_lowercase();
        let rows = self
            .rows
            .iter()
            .filter(|row| {
                row.iter()
                    .any(|cell| cell.as_display().to_lowercase().contains(&query))
            })
            .cloned()
            .collect();

        Self {
            columns: self.columns.clone(),
            rows,
        }
    }

    pub fn sort_by_column(&self, column_name: &str, ascending: bool) -> Self {
        let Some(index) = self
            .columns
            .iter()
            .position(|column| column.name == column_name)
        else {
            return self.clone();
        };

        let mut rows = self.rows.clone();
        rows.sort_by(|left, right| compare_cells(left.get(index), right.get(index), ascending));

        Self {
            columns: self.columns.clone(),
            rows,
        }
    }

    pub fn export(&self, format: ExportFormat) -> Result<String, serde_json::Error> {
        let records = self.as_records();
        match format {
            ExportFormat::Json => serde_json::to_string_pretty(&records),
            ExportFormat::Ndjson => Ok(records
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<Vec<_>, _>>()?
                .join("\n")),
            ExportFormat::Csv => Ok(export_csv(&self.columns, &self.rows)),
        }
    }

    pub fn as_records(&self) -> Vec<serde_json::Map<String, serde_json::Value>> {
        self.rows
            .iter()
            .map(|row| self.record_from_row(row))
            .collect()
    }

    pub fn filter_row_indexes(&self, query: &str) -> Vec<usize> {
        if query.trim().is_empty() {
            return (0..self.rows.len()).collect();
        }

        let query = query.to_lowercase();
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                row.iter()
                    .any(|cell| cell.as_display().to_lowercase().contains(&query))
                    .then_some(index)
            })
            .collect()
    }

    pub fn record_at(
        &self,
        row_index: usize,
    ) -> Option<serde_json::Map<String, serde_json::Value>> {
        let row = self.rows.get(row_index)?;
        Some(self.record_from_row(row))
    }

    pub fn update_row_from_record(
        &mut self,
        row_index: usize,
        record: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), String> {
        let row = self
            .rows
            .get_mut(row_index)
            .ok_or_else(|| "row index out of range".to_string())?;

        let mut updated = Vec::with_capacity(self.columns.len());
        for column in &self.columns {
            let value = record.get(&column.name).unwrap_or(&serde_json::Value::Null);
            let cell = CellValue::from_json_value(&column.kind, value)
                .ok_or_else(|| format!("column '{}' expects {:?}", column.name, column.kind))?;
            updated.push(cell);
        }
        *row = updated;
        Ok(())
    }

    fn record_from_row(&self, row: &[CellValue]) -> serde_json::Map<String, serde_json::Value> {
        self.columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let value = row.get(index).cloned().unwrap_or(CellValue::Null);
                (column.name.clone(), value.to_json_value())
            })
            .collect()
    }
}

fn compare_cells(left: Option<&CellValue>, right: Option<&CellValue>, ascending: bool) -> Ordering {
    let ordering = match (left, right) {
        (Some(CellValue::Int(left)), Some(CellValue::Int(right))) => left.cmp(right),
        (Some(CellValue::Float(left)), Some(CellValue::Float(right))) => {
            left.partial_cmp(right).unwrap_or(Ordering::Equal)
        }
        (Some(CellValue::Bool(left)), Some(CellValue::Bool(right))) => left.cmp(right),
        (Some(CellValue::DateTime(left)), Some(CellValue::DateTime(right))) => left.cmp(right),
        (Some(left), Some(right)) => left.as_display().cmp(&right.as_display()),
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
    };

    if ascending {
        ordering
    } else {
        ordering.reverse()
    }
}

fn export_csv(columns: &[ColumnSchema], rows: &[Vec<CellValue>]) -> String {
    let header = columns
        .iter()
        .map(|column| escape_csv(&column.name))
        .collect::<Vec<_>>()
        .join(",");
    let body = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| escape_csv(&cell.as_display()))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n");

    if body.is_empty() {
        format!("{header}\n")
    } else {
        format!("{header}\n{body}\n")
    }
}

fn escape_csv(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn sample_frame() -> DataFrame {
        DataFrame {
            columns: vec![
                ColumnSchema {
                    name: "service".into(),
                    kind: ColumnType::String,
                },
                ColumnSchema {
                    name: "replicas".into(),
                    kind: ColumnType::Int,
                },
                ColumnSchema {
                    name: "healthy".into(),
                    kind: ColumnType::Bool,
                },
            ],
            rows: vec![
                vec![
                    CellValue::String("api".into()),
                    CellValue::Int(4),
                    CellValue::Bool(true),
                ],
                vec![
                    CellValue::String("worker".into()),
                    CellValue::Int(2),
                    CellValue::Bool(false),
                ],
            ],
        }
    }

    #[test]
    fn filters_rows_using_substring_match() {
        let filtered = sample_frame().filter_contains("work");
        assert_eq!(filtered.rows.len(), 1);
        assert_eq!(filtered.rows[0][0], CellValue::String("worker".into()));
    }

    #[test]
    fn sorts_rows_by_typed_column() {
        let sorted = sample_frame().sort_by_column("replicas", false);
        assert_eq!(sorted.rows[0][0], CellValue::String("api".into()));
    }

    #[test]
    fn exports_supported_formats() {
        let frame = DataFrame {
            columns: vec![
                ColumnSchema {
                    name: "at".into(),
                    kind: ColumnType::DateTime,
                },
                ColumnSchema {
                    name: "meta".into(),
                    kind: ColumnType::Json,
                },
            ],
            rows: vec![vec![
                CellValue::DateTime(Utc.with_ymd_and_hms(2026, 4, 6, 1, 2, 3).unwrap()),
                CellValue::Json(serde_json::json!({"env":"prod"})),
            ]],
        };

        let json = frame.export(ExportFormat::Json).unwrap();
        let csv = frame.export(ExportFormat::Csv).unwrap();
        let ndjson = frame.export(ExportFormat::Ndjson).unwrap();

        assert!(json.contains("\"env\": \"prod\""));
        assert!(csv.contains("2026-04-06T01:02:03+00:00"));
        assert!(ndjson.contains("\"meta\":{\"env\":\"prod\"}"));
    }
}
