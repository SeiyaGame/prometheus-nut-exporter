//! Renders a scrape in the OpenMetrics text format: https://prometheus.io/docs/specs/om/open_metrics_spec/

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::metrics::{Kind, METRICS, Metric, UPS_INFO_LABELS, UPS_STATUSES, Value};
use crate::nut::{Scrape, Ups};

pub fn render(scrape: &Scrape, exporter_version: &str) -> String {
    let mut out = String::new();
    for metric in METRICS {
        let samples = samples(metric, scrape, exporter_version);
        if samples.is_empty() {
            continue;
        }
        let _ = writeln!(out, "# TYPE {} {}", metric.name, metric.type_name());
        let _ = writeln!(out, "# UNIT {} {}", metric.name, metric.unit);
        if metric.nut_var.is_empty() {
            let _ = writeln!(out, "# HELP {} {}", metric.name, metric.help);
        } else {
            let _ = writeln!(out, "# HELP {} {} (\"{}\")", metric.name, metric.help, metric.nut_var);
        }
        out.push_str(&samples);
    }
    out.push_str("# EOF\n");
    out
}

/// All the sample lines of one metric.
fn samples(metric: &Metric, scrape: &Scrape, exporter_version: &str) -> String {
    let mut out = String::new();
    let mut sample = |labels: &[(&str, &str)], value: &str| {
        let labels: Vec<String> = labels.iter().map(|(name, value)| format!("{name}=\"{}\"", escape(value))).collect();
        let _ = writeln!(out, "{}{{{}}} {value}", metric.name, labels.join(","));
    };

    match metric.kind {
        Kind::ExporterInfo => sample(&[("version", exporter_version)], "1"),
        Kind::ServerInfo => sample(&[("version", &scrape.nut_version)], "1"),
        Kind::UpsInfo => {
            for (name, ups) in &scrape.upses {
                sample(&ups_info_labels(name, ups), "1");
            }
        }
        Kind::UpsStatus => {
            for (name, ups) in &scrape.upses {
                let Some(status) = ups.vars.get(metric.nut_var) else { continue };
                let statuses: HashSet<&str> = status.split(' ').collect();
                for (state, _) in UPS_STATUSES {
                    sample(&[("ups", name), ("status", state)], if statuses.contains(state) { "1" } else { "0" });
                }
            }
        }
        Kind::Gauge(value) => {
            for (name, ups) in &scrape.upses {
                if let Some(number) = ups.vars.get(metric.nut_var).and_then(|raw| convert(value, raw)) {
                    sample(&[("ups", name)], &number);
                }
            }
        }
    }
    out
}

fn ups_info_labels<'a>(name: &'a str, ups: &'a Ups) -> Vec<(&'a str, &'a str)> {
    let mut labels = vec![("ups", name), ("description", ups.description.as_str())];
    for (label, var) in UPS_INFO_LABELS {
        if let Some(value) = ups.vars.get(var) {
            labels.push((label, value));
        }
    }
    labels
}

/// Converts a NUT value, or `None` when it is not a number.
fn convert(value: Value, raw: &str) -> Option<String> {
    let number = match value {
        Value::Float | Value::Integer => raw.parse::<f64>().ok()?,
        // Rounded to the 15 significant digits of an f64: 5.8 / 100 gives 0.058, not 0.057999999999999996
        Value::Percentage => format!("{:.14e}", raw.parse::<f64>().ok()? / 100.0).parse().ok()?,
        Value::BeeperStatus => match raw {
            "enabled" => 1.0,
            "disabled" => 2.0,
            "muted" => 3.0,
            _ => 0.0,
        },
        // Only the first status counts ("OB LB" is on battery)
        Value::OldUpsStatus => match raw.split(' ').next() {
            Some("OL") => 1.0,
            Some("OB") => 2.0,
            Some("LB") => 3.0,
            _ => 0.0,
        },
    };
    let is_integer = matches!(value, Value::Integer | Value::BeeperStatus | Value::OldUpsStatus);
    Some(format_number(number, is_integer))
}

/// Integers never have a decimal point and floats always have one, in their shortest exact form.
fn format_number(number: f64, is_integer: bool) -> String {
    if is_integer {
        format!("{number:.0}")
    } else if number.is_finite() && number.fract() == 0.0 {
        format!("{number:.1}")
    } else {
        number.to_string()
    }
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => escaped.push_str(r"\n"),
            '"' => escaped.push_str(r#"\""#),
            '\\' => escaped.push_str(r"\\"),
            _ => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrape_with(vars: &[(&str, &str)]) -> Scrape {
        let ups = Ups {
            description: "desc".into(),
            vars: vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        Scrape { nut_version: "2.8.2".into(), upses: [("ups \"1\"".to_owned(), ups)].into() }
    }

    #[test]
    fn numbers() {
        assert_eq!(convert(Value::Float, "237.2").unwrap(), "237.2");
        assert_eq!(convert(Value::Float, "230").unwrap(), "230.0");
        assert_eq!(convert(Value::Integer, "1320").unwrap(), "1320");
        assert_eq!(convert(Value::Percentage, "21").unwrap(), "0.21");
        assert_eq!(convert(Value::Percentage, "100").unwrap(), "1.0");
        assert_eq!(convert(Value::Percentage, "5.8").unwrap(), "0.058");
        assert_eq!(convert(Value::BeeperStatus, "muted").unwrap(), "3");
        assert_eq!(convert(Value::OldUpsStatus, "OB LB").unwrap(), "2");
        assert_eq!(convert(Value::OldUpsStatus, "CHRG").unwrap(), "0");
        assert_eq!(convert(Value::Float, "n/a"), None);
    }

    #[test]
    fn labels_are_escaped() {
        let out = render(&scrape_with(&[("ups.status", "OL"), ("device.model", "a\\b\nc")]), "1.0.0");
        assert!(out.contains(r#"nut_ups_status{ups="ups \"1\"",status="OL"} 1"#), "{out}");
        assert!(out.contains(r#"model="a\\b\nc""#), "{out}");
    }

    #[test]
    fn metrics_without_samples_are_left_out() {
        let out = render(&scrape_with(&[]), "1.0.0");
        assert!(out.contains("# TYPE nut_ups_info info\n"));
        assert!(!out.contains("nut_ups_status"));
        assert!(!out.contains("nut_battery_charge"));
        assert!(out.ends_with("# EOF\n"));
    }
}
