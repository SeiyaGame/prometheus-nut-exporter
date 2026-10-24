//! Definitions of all exported metrics, in output order.

/// How the samples of a metric are produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `nut_exporter_info`: version of the exporter
    ExporterInfo,
    /// `nut_server_info` and `nut_info`: version of the NUT server
    ServerInfo,
    /// `nut_ups_info`: metadata of each UPS as labels
    UpsInfo,
    /// `nut_ups_status`: one sample per known status
    UpsStatus,
    /// One sample per UPS that has the NUT variable, converted as given
    Gauge(Value),
}

/// Conversion of a NUT variable to a sample value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Float,
    Integer,
    /// 0-100 to 0-1
    Percentage,
    /// Unknown (0), enabled (1), disabled (2) or muted (3)
    BeeperStatus,
    /// Unknown (0), OL (1), OB (2) or LB (3)
    OldUpsStatus,
}

#[derive(Debug, Clone, Copy)]
pub struct Metric {
    pub name: &'static str,
    pub kind: Kind,
    pub unit: &'static str,
    pub nut_var: &'static str,
    pub help: &'static str,
}

impl Metric {
    pub fn type_name(&self) -> &'static str {
        match self.kind {
            Kind::ExporterInfo | Kind::ServerInfo | Kind::UpsInfo => "info",
            Kind::UpsStatus => "stateset",
            Kind::Gauge(_) => "gauge",
        }
    }
}

const fn special(name: &'static str, kind: Kind, nut_var: &'static str, help: &'static str) -> Metric {
    Metric { name, kind, unit: "", nut_var, help }
}

const fn gauge(
    name: &'static str,
    nut_var: &'static str,
    unit: &'static str,
    value: Value,
    help: &'static str,
) -> Metric {
    Metric { name, kind: Kind::Gauge(value), unit, nut_var, help }
}

/// Statuses of `nut_ups_status`, with their description (`ups.status` is a list of them).
pub const UPS_STATUSES: [(&str, &str); 11] = [
    ("OL", "Online"),
    ("OB", "On battery"),
    ("LB", "Low battery (critical)"),
    ("CHRG", "Charging"),
    ("RB", "Replace battery"),
    ("FSD", "Forced shutdown"),
    ("BYPASS", "Battery bypass"),
    ("SD", "Shutdown"),
    ("CP", "Cable power"),
    ("BOOST", "Boosted voltage"),
    ("OFF", "Off"),
];

/// Labels of `nut_ups_info` taken from NUT variables (the description comes from `LIST UPS`).
pub const UPS_INFO_LABELS: [(&str, &str); 17] = [
    ("description2", "device.description"),
    ("device_type", "device.type"),
    ("location", "device.location"),
    ("manufacturer", "device.mfr"),
    ("manufacturing_date", "device.mfr.date"),
    ("model", "device.model"),
    ("battery_type", "battery.type"),
    ("driver", "driver.name"),
    ("driver_version", "driver.version"),
    ("driver_version_internal", "driver.version.internal"),
    ("driver_version_data", "driver.version.data"),
    ("usb_vendor_id", "ups.vendorid"),
    ("usb_product_id", "ups.productid"),
    ("ups_firmware", "ups.firmware"),
    ("ups_type", "ups.type"),
    // Deprecated
    ("type", "device.type"),
    ("nut_version", "driver.version"),
];

#[rustfmt::skip]
pub const METRICS: &[Metric] = &[
    special("nut_exporter_info", Kind::ExporterInfo, "", "Metadata about the exporter."),
    special("nut_server_info", Kind::ServerInfo, "", "Metadata about the NUT server."),
    special("nut_ups_info", Kind::UpsInfo, "", "Metadata about the UPS."),
    special("nut_info", Kind::ServerInfo, "", "Metadata about the NUT server. (Deprecated, use nut_server_info instead.)"),
    special("nut_ups_status", Kind::UpsStatus, "ups.status", "UPS status. Check for a specific status with the \"status\" label."),
    // Status, uptime, load
    gauge("nut_beeper_status", "ups.beeper.status", "", Value::BeeperStatus, "If the beeper is enabled. Unknown (0), enabled (1), disabled (2) or muted (3)."),
    gauge("nut_uptime_seconds", "device.uptime", "seconds", Value::Integer, "Device uptime."),
    gauge("nut_load", "ups.load", "", Value::Percentage, "Load. (0-1)"),
    gauge("nut_temperature_celsius", "ups.temperature", "celsius", Value::Float, "UPS temperature"),
    // Battery
    gauge("nut_battery_charge", "battery.charge", "", Value::Percentage, "Battery level. (0-1)"),
    gauge("nut_battery_charge_low", "battery.charge.low", "", Value::Percentage, "Battery level threshold for low state. (0-1)"),
    gauge("nut_battery_charge_warning", "battery.charge.warning", "", Value::Percentage, "Battery level threshold for warning state. (0-1)"),
    gauge("nut_battery_charge_restart", "battery.charge.restart", "", Value::Percentage, "Battery level threshold for restarting after power-off. (0-1)"),
    gauge("nut_battery_runtime_seconds", "battery.runtime", "seconds", Value::Integer, "Battery runtime."),
    gauge("nut_battery_runtime_low_seconds", "battery.runtime.low", "seconds", Value::Integer, "Battery runtime threshold for state low."),
    gauge("nut_battery_runtime_restart_seconds", "battery.runtime.restart", "seconds", Value::Integer, "Battery runtime threshold for restart after power-off."),
    gauge("nut_delay_shutdown_seconds", "ups.delay.shutdown", "seconds", Value::Integer, "Interval to wait after shutdown with delay command."),
    gauge("nut_delay_start_seconds", "ups.delay.start", "seconds", Value::Integer, "Interval to wait before (re)starting the load."),
    gauge("nut_battery_voltage_volts", "battery.voltage", "volts", Value::Float, "Battery voltage."),
    gauge("nut_battery_voltage_nominal_volts", "battery.voltage.nominal", "volts", Value::Float, "Battery voltage (nominal)."),
    gauge("nut_battery_voltage_high_volts", "battery.voltage.high", "volts", Value::Float, "Battery voltage for full (charge level calculation)."),
    gauge("nut_battery_voltage_low_volts", "battery.voltage.low", "volts", Value::Float, "Battery voltage for empty (charge level calculation)."),
    gauge("nut_battery_temperature_celsius", "battery.temperature", "celsius", Value::Float, "Battery temperature."),
    // Input
    gauge("nut_input_voltage_volts", "input.voltage", "volts", Value::Float, "Input voltage."),
    gauge("nut_input_voltage_nominal_volts", "input.voltage.nominal", "volts", Value::Float, "Input voltage (nominal)."),
    gauge("nut_input_voltage_minimum_volts", "input.voltage.minimum", "volts", Value::Float, "Input voltage (minimum seen)."),
    gauge("nut_input_voltage_maximum_volts", "input.voltage.maximum", "volts", Value::Float, "Input voltage (maximum seen)."),
    gauge("nut_input_transfer_low_volts", "input.transfer.low", "volts", Value::Float, "Input lower transfer threshold."),
    gauge("nut_input_transfer_high_volts", "input.transfer.high", "volts", Value::Float, "Input upper transfer threshold."),
    gauge("nut_input_current_amperes", "input.current", "amperes", Value::Float, "Input current."),
    gauge("nut_input_current_nominal_amperes", "input.current.nominal", "amperes", Value::Float, "Input current (nominal)."),
    gauge("nut_input_frequency_hertz", "input.frequency", "hertz", Value::Float, "Input frequency."),
    gauge("nut_input_frequency_nominal_hertz", "input.frequency.nominal", "hertz", Value::Float, "Input frequency (nominal)."),
    gauge("nut_input_frequency_low_hertz", "input.frequency.low", "hertz", Value::Float, "Input frequency (low)."),
    gauge("nut_input_frequency_high_hertz", "input.frequency.high", "hertz", Value::Float, "Input frequency (high)."),
    // Output
    gauge("nut_output_voltage_volts", "output.voltage", "volts", Value::Float, "Output voltage."),
    gauge("nut_output_voltage_nominal_volts", "output.voltage.nominal", "volts", Value::Float, "Output voltage (nominal)."),
    gauge("nut_output_current_amperes", "output.current", "amperes", Value::Float, "Output current."),
    gauge("nut_output_current_nominal_amperes", "output.current.nominal", "amperes", Value::Float, "Output current (nominal)."),
    gauge("nut_output_frequency_hertz", "output.frequency", "hertz", Value::Float, "Output frequency."),
    gauge("nut_output_frequency_nominal_hertz", "output.frequency.nominal", "hertz", Value::Float, "Output frequency (nominal)."),
    // Power
    gauge("nut_power_watts", "ups.power", "watts", Value::Float, "Apparent power."),
    gauge("nut_power_nominal_watts", "ups.power.nominal", "watts", Value::Float, "Apparent power (nominal)."),
    gauge("nut_real_power_watts", "ups.realpower", "watts", Value::Float, "Real power."),
    gauge("nut_real_power_nominal_watts", "ups.realpower.nominal", "watts", Value::Float, "Real power (nominal)."),
    // Deprecated
    gauge("nut_status", "ups.status", "", Value::OldUpsStatus, "UPS status. Unknown (0), on line (1, \"OL\"), on battery (2, \"OB\"), or low battery (3, \"LB\"). (Deprecated, use nut_ups_status instead.)"),
    gauge("nut_battery_volts", "battery.voltage", "volts", Value::Float, "Battery voltage. (Deprecated, use nut_battery_voltage_volts instead.)"),
    gauge("nut_input_volts", "input.voltage", "volts", Value::Float, "Input voltage. (Deprecated, use nut_input_voltage_volts instead.)"),
    gauge("nut_output_volts", "output.voltage", "volts", Value::Float, "Output voltage. (Deprecated, use nut_output_voltage_volts instead.)"),
];

/// The metrics as a Markdown table, for `metrics.md` (`PRINT_METRICS_AND_EXIT=true`).
pub fn markdown_table() -> String {
    let mut table = String::from("| Metric | NUT Var | Unit | Description |\n| - | - | - | - |\n");
    for metric in METRICS {
        let row = format!("| `{}` | `{}` | `{}` | {} |\n", metric.name, metric.nut_var, metric.unit, metric.help);
        table.push_str(&row.replace("``", ""));
    }
    table
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn names_are_unique() {
        let names: HashSet<_> = METRICS.iter().map(|m| m.name).collect();
        assert_eq!(names.len(), METRICS.len());
    }

    #[test]
    fn gauges_have_a_nut_var() {
        for metric in METRICS.iter().filter(|m| matches!(m.kind, Kind::Gauge(_))) {
            assert!(!metric.nut_var.is_empty(), "{}", metric.name);
        }
    }

    #[test]
    fn metrics_md_is_up_to_date() {
        let doc = include_str!("../metrics.md").replace("\r\n", "\n");
        assert!(
            doc.contains(&markdown_table()),
            "metrics.md: update the table with PRINT_METRICS_AND_EXIT=true cargo run"
        );
    }
}
