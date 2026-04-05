use std::{env, path::PathBuf};

use crate::models::timeseries::TimeSeriesFrame;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChartRenderMode {
    KittyImage,
    TextFallback,
}

#[derive(Debug, Clone)]
pub struct ChartPlan {
    pub mode: ChartRenderMode,
    pub script: String,
    pub output_path: PathBuf,
}

pub fn plan_chart(frame: &TimeSeriesFrame) -> ChartPlan {
    let supports_kitty = env::var("KITTY_WINDOW_ID").is_ok();
    let output_path = env::temp_dir().join("deli-monitor.png");
    let mode = if supports_kitty {
        ChartRenderMode::KittyImage
    } else {
        ChartRenderMode::TextFallback
    };

    ChartPlan {
        mode,
        script: build_gnuplot_script(frame, &output_path),
        output_path,
    }
}

pub fn build_gnuplot_script(frame: &TimeSeriesFrame, output_path: &PathBuf) -> String {
    let mut script = vec![
        "set terminal pngcairo transparent size 1200,400".to_string(),
        format!("set output '{}'", output_path.display()),
        format!("set title '{}'", escape_single_quotes(&frame.title)),
        "set key outside".to_string(),
        "set xdata time".to_string(),
        "set timefmt '%s'".to_string(),
        "set format x '%H:%M'".to_string(),
    ];

    let plots = frame
        .series
        .iter()
        .map(|series| {
            format!(
                "'-' using 1:2 with lines title '{}'",
                escape_single_quotes(&series.name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    script.push(format!("plot {plots}"));

    for series in &frame.series {
        for point in &series.points {
            script.push(format!("{} {}", point.timestamp, point.value));
        }
        script.push("e".to_string());
    }

    script.join("\n")
}

fn escape_single_quotes(value: &str) -> String {
    value.replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use crate::models::timeseries::{DataPoint, Series, TimeSeriesFrame};

    use super::*;

    #[test]
    fn generates_transparent_gnuplot_script() {
        let frame = TimeSeriesFrame {
            title: "CPU".into(),
            series: vec![Series {
                name: "api".into(),
                unit: Some("%".into()),
                points: vec![DataPoint {
                    timestamp: 1_712_300_000,
                    value: 41.2,
                }],
            }],
        };

        let output = PathBuf::from("/tmp/chart.png");
        let script = build_gnuplot_script(&frame, &output);

        assert!(script.contains("transparent"));
        assert!(script.contains("plot '-' using 1:2 with lines title 'api'"));
        assert!(script.contains("1712300000 41.2"));
    }
}
