use std::{
    env,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use crate::models::timeseries::TimeSeriesFrame;

const CHART_FG: &str = "#d9d9d9";
const CHART_MUTED: &str = "#8a8a8a";
const SERIES_COLORS: [&str; 6] = [
    "#ffd166", "#7bdff2", "#a8df65", "#f7a072", "#cdb4db", "#f28482",
];

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

#[derive(Debug, Clone)]
pub enum ChartRenderStatus {
    Rendered(PathBuf),
    Skipped(String),
    Failed(String),
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

pub fn render_chart(frame: &TimeSeriesFrame) -> (ChartPlan, ChartRenderStatus) {
    let plan = plan_chart(frame);
    let mut process = match Command::new("gnuplot")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(process) => process,
        Err(error) => {
            return (
                plan.clone(),
                ChartRenderStatus::Skipped(format!("gnuplot unavailable: {error}")),
            );
        }
    };

    if let Some(stdin) = process.stdin.as_mut()
        && stdin.write_all(plan.script.as_bytes()).is_err()
    {
        return (
            plan.clone(),
            ChartRenderStatus::Failed("failed to write gnuplot script".into()),
        );
    }

    match process.wait_with_output() {
        Ok(output) if output.status.success() => (
            plan.clone(),
            ChartRenderStatus::Rendered(plan.output_path.clone()),
        ),
        Ok(output) => (
            plan.clone(),
            ChartRenderStatus::Failed(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        ),
        Err(error) => (plan.clone(), ChartRenderStatus::Failed(error.to_string())),
    }
}

pub fn render_ascii_chart(
    frame: &TimeSeriesFrame,
    width: u16,
    height: u16,
) -> Result<String, String> {
    let script = build_ascii_gnuplot_script(frame, width, height);
    let mut process = Command::new("gnuplot")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("gnuplot unavailable: {error}"))?;

    if let Some(stdin) = process.stdin.as_mut() {
        stdin
            .write_all(script.as_bytes())
            .map_err(|error| format!("failed to write gnuplot script: {error}"))?;
    }

    let output = process
        .wait_with_output()
        .map_err(|error| format!("gnuplot failed: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(
        strip_ansi_sequences(&String::from_utf8_lossy(&output.stdout))
            .trim_end()
            .to_string(),
    )
}

pub fn build_gnuplot_script(frame: &TimeSeriesFrame, output_path: &PathBuf) -> String {
    let mut script = vec![
        "set terminal pngcairo transparent noenhanced size 1200,400".to_string(),
        format!("set output '{}'", output_path.display()),
        format!(
            "set title '{}' textcolor rgb '{}'",
            escape_single_quotes(&frame.title),
            CHART_FG
        ),
        format!("set border lc rgb '{}'", CHART_FG),
        format!("set xtics textcolor rgb '{}'", CHART_MUTED),
        format!("set ytics textcolor rgb '{}'", CHART_MUTED),
        format!("set key outside textcolor rgb '{}'", CHART_FG),
        format!("set grid lc rgb '{}'", CHART_MUTED),
        "set xdata time".to_string(),
        "set timefmt '%s'".to_string(),
        "set format x '%H:%M'".to_string(),
    ];

    let plots = frame
        .series
        .iter()
        .enumerate()
        .map(|(index, series)| {
            let color = SERIES_COLORS[index % SERIES_COLORS.len()];
            format!(
                "'-' using 1:2 with lines lw 2 lc rgb '{}' title '{}'",
                color,
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

pub fn build_ascii_gnuplot_script(frame: &TimeSeriesFrame, width: u16, height: u16) -> String {
    let mut script = vec![
        "set termoption noenhanced".to_string(),
        format!(
            "set terminal dumb mono size {},{}",
            width.max(20),
            height.max(8)
        ),
        format!("set title '{}'", escape_single_quotes(&frame.title)),
        "unset key".to_string(),
        "set border".to_string(),
        "set xdata time".to_string(),
        "set timefmt '%s'".to_string(),
        "set format x '%H:%M'".to_string(),
        "set tics out".to_string(),
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

fn strip_ansi_sequences(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                while let Some(next) = chars.next() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        result.push(ch);
    }

    result
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

        assert!(script.contains("transparent noenhanced"));
        assert!(script.contains("set title 'CPU' textcolor rgb '#d9d9d9'"));
        assert!(script.contains("set key outside textcolor rgb '#d9d9d9'"));
        assert!(script.contains("plot '-' using 1:2 with lines lw 2 lc rgb '#ffd166' title 'api'"));
        assert!(script.contains("1712300000 41.2"));
    }

    #[test]
    fn generates_ascii_gnuplot_script() {
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

        let script = build_ascii_gnuplot_script(&frame, 80, 18);
        assert!(script.contains("set terminal dumb mono size 80,18"));
        assert!(script.contains("set termoption noenhanced"));
        assert!(script.contains("plot '-' using 1:2 with lines title 'api'"));
    }

    #[test]
    fn strips_ansi_sequences_from_ascii_output() {
        let value = "\u{1b}[22;39mhello\u{1b}[0m";
        assert_eq!(strip_ansi_sequences(value), "hello");
    }
}
