//! Defensive parser for RouterOS REST payloads.
//!
//! RouterOS returns ALL values as strings, so every numeric/boolean field
//! goes through a coercion helper: absent or malformed fields become `None`,
//! never `0`/`false`. Singleton endpoints accept BOTH a bare object and a
//! single-element array; `/system/health` accepts BOTH the property-set shape
//! and the sensor-record array shape. Sensor values are normalized from
//! deci-units (e.g. `cpu-temperature="430"` = 43.0°C) by magnitude rules
//! locked in the fixture tests below.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::mikrotik::error::MikrotikError;

// ---------------------------------------------------------------------------
// Scalar coercion helpers
// ---------------------------------------------------------------------------

/// Parse a RouterOS string into `u64`; malformed input yields `None`.
pub fn parse_u64(raw: &str) -> Option<u64> {
    raw.trim().parse().ok()
}

/// Parse a RouterOS string into `f64`; malformed input yields `None`.
pub fn parse_f64(raw: &str) -> Option<f64> {
    raw.trim().parse().ok()
}

/// Parse a RouterOS boolean string (`"true"` / `"false"`); anything else
/// (including absent fields) yields `None`.
pub fn parse_bool(raw: &str) -> Option<bool> {
    match raw {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Shape-tolerant object extraction
// ---------------------------------------------------------------------------

/// Singleton endpoints answer with either a bare object or a single-element
/// array wrapping that object — accept both.
fn first_object(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        Value::Array(items) => items.first().and_then(Value::as_object),
        _ => None,
    }
}

/// Command endpoints (e.g. `check-for-updates`) may stream progressive JSON
/// sections as an array — the LAST section carries the current state.
fn last_object(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        Value::Array(items) => items.last().and_then(Value::as_object),
        _ => None,
    }
}

/// Endpoint lists answer with an array, but tolerate a bare object or a
/// singleton array wrapping one record.
fn object_list(value: &Value) -> Vec<&Map<String, Value>> {
    match value {
        Value::Array(items) => items.iter().filter_map(Value::as_object).collect(),
        Value::Object(map) => vec![map],
        _ => Vec::new(),
    }
}

fn get_str<'m>(map: &'m Map<String, Value>, key: &str) -> Option<&'m str> {
    map.get(key).and_then(Value::as_str)
}

fn get_u64(map: &Map<String, Value>, key: &str) -> Option<u64> {
    match map.get(key)? {
        Value::String(s) => parse_u64(s),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

fn get_f64(map: &Map<String, Value>, key: &str) -> Option<f64> {
    match map.get(key)? {
        Value::String(s) => parse_f64(s),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

fn get_bool(map: &Map<String, Value>, key: &str) -> Option<bool> {
    get_str(map, key).and_then(parse_bool)
}

fn parse_err(context: &str) -> MikrotikError {
    MikrotikError::Parse(format!("expected {context}"))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDto {
    pub cpu_load: Option<f64>,
    pub mem_used_bytes: Option<u64>,
    pub mem_total_bytes: Option<u64>,
    pub uptime: Option<String>,
    pub board_name: Option<String>,
    pub version: Option<String>,
    pub architecture_name: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SensorKind {
    Temperature,
    Fan,
    Voltage,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SensorDto {
    pub name: String,
    pub value: f64,
    pub unit: Option<String>,
    pub kind: SensorKind,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceDto {
    pub name: String,
    pub iface_type: Option<String>,
    pub running: Option<bool>,
    pub disabled: Option<bool>,
    pub rx_byte: Option<u64>,
    pub tx_byte: Option<u64>,
    pub rx_packet: Option<u64>,
    pub tx_packet: Option<u64>,
    pub tx_queue_drop: Option<u64>,
    pub link_downs: Option<u64>,
    pub rx_error: Option<u64>,
    pub tx_error: Option<u64>,
    pub rx_drop: Option<u64>,
    pub rx_error_events: Option<u64>,
    pub tx_error_events: Option<u64>,
    pub rx_fcs_error: Option<u64>,
    pub rx_align_error: Option<u64>,
    pub tx_collision: Option<u64>,
    pub tx_drop: Option<u64>,
    /// Negotiated link rate from the ethernet monitor call (todo 5 merges).
    pub rate: Option<String>,
    /// Negotiated duplex from the ethernet monitor call (todo 5 merges).
    pub full_duplex: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EthernetStatsDto {
    pub name: String,
    pub default_name: Option<String>,
    pub rx_error_events: Option<u64>,
    pub tx_error_events: Option<u64>,
    pub rx_fcs_error: Option<u64>,
    pub rx_align_error: Option<u64>,
    pub tx_collision: Option<u64>,
    pub tx_drop: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EthernetMonitorDto {
    pub name: String,
    pub rate: Option<String>,
    pub full_duplex: Option<bool>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VlanDto {
    pub name: String,
    pub vlan_id: Option<u64>,
    pub interface: Option<String>,
    pub running: Option<bool>,
    pub disabled: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeVlanDto {
    pub bridge: Option<String>,
    pub vlan_ids: Vec<String>,
    pub tagged: Vec<String>,
    pub untagged: Vec<String>,
    pub current_tagged: Vec<String>,
    pub current_untagged: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusDto {
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub channel: Option<String>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterboardDto {
    pub routerboard: Option<bool>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub current_firmware: Option<String>,
    pub upgrade_firmware: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntryDto {
    pub id: Option<String>,
    pub name: Option<String>,
    pub size: Option<u64>,
}

// ---------------------------------------------------------------------------
// Sensor normalization
// ---------------------------------------------------------------------------

/// Classify a sensor by NAME PATTERNS (not a fixed list), covering the
/// documented RouterOS aliases: any `*temp*` name is a temperature candidate
/// (`temperature`, `cpu-temperature`, `cpu-temp`, `board-temp`,
/// `pcb-temperature`, `switch-temperature`, `sfp-temperature`, `lm87-temp`,
/// `temp1`-`temp3`, `board-temperature1`-`board-temperature2`), any `fan*`
/// name is a fan (`fan-speed`, `fan1`-`fan4`, `fan1-speed`-`fan4-speed`), and
/// any `*voltage*` name or named rail (`3.3v`, `5v`, `12v`, `core`,
/// `psu1-voltage`/`psu2-voltage`) is a voltage.
fn sensor_kind(name: &str) -> SensorKind {
    let n = name.to_lowercase();
    if n.contains("temp") {
        SensorKind::Temperature
    } else if n.starts_with("fan") {
        SensorKind::Fan
    } else if n.contains("voltage") || matches!(n.as_str(), "3.3v" | "5v" | "12v" | "core") {
        SensorKind::Voltage
    } else {
        SensorKind::Other
    }
}

fn sensor_unit(kind: SensorKind) -> Option<&'static str> {
    match kind {
        SensorKind::Temperature => Some("°C"),
        SensorKind::Fan => Some("RPM"),
        SensorKind::Voltage => Some("V"),
        SensorKind::Other => None,
    }
}

/// MikroTik's Health docs warn API-exposed values can be in DECI-UNITS
/// (`cpu-temperature="430"` = 43.0°C, `voltage="240"` = 24.0V). Magnitude
/// rules normalize to displayed units: a temperature above 150 or a voltage
/// above 100 is implausible as a raw reading and is divided by 10. Fans
/// (thousands of RPM) and unknown sensors are never scaled.
fn normalize_sensor_value(kind: SensorKind, raw: f64) -> f64 {
    let deci = match kind {
        SensorKind::Temperature => raw > 150.0,
        SensorKind::Voltage => raw > 100.0,
        SensorKind::Fan | SensorKind::Other => false,
    };
    if deci { raw / 10.0 } else { raw }
}

fn make_sensor(name: &str, raw: &str) -> Option<SensorDto> {
    let value = parse_f64(raw)?;
    let kind = sensor_kind(name);
    Some(SensorDto {
        name: name.to_owned(),
        value: normalize_sensor_value(kind, value),
        unit: sensor_unit(kind).map(str::to_owned),
        kind,
    })
}

// ---------------------------------------------------------------------------
// Endpoint parsers
// ---------------------------------------------------------------------------

/// `GET /rest/system/resource` — accepts a bare object or a singleton array.
/// Memory is DERIVED: `mem_used = total-memory - free-memory`; when either
/// side is absent or malformed, `mem_used_bytes` is `None` (never 0).
pub fn parse_resource(value: &Value) -> Result<ResourceDto, MikrotikError> {
    let map = first_object(value).ok_or_else(|| parse_err("resource object"))?;
    let mem_total = get_u64(map, "total-memory");
    let mem_free = get_u64(map, "free-memory");
    let mem_used = match (mem_total, mem_free) {
        (Some(total), Some(free)) => Some(total.saturating_sub(free)),
        _ => None,
    };
    Ok(ResourceDto {
        cpu_load: get_f64(map, "cpu-load"),
        mem_used_bytes: mem_used,
        mem_total_bytes: mem_total,
        uptime: get_str(map, "uptime").map(str::to_owned),
        board_name: get_str(map, "board-name").map(str::to_owned),
        version: get_str(map, "version").map(str::to_owned),
        architecture_name: get_str(map, "architecture-name").map(str::to_owned),
    })
}

/// `GET /rest/system/health` — accepts BOTH shapes found in the wild: a
/// property-set object (`{"temperature": "42", ...}`) AND an array of
/// `name`/`value`/`type` sensor records (possibly multiple temperatures and
/// fans). Records without a parseable value are skipped, never fatal.
pub fn parse_health(value: &Value) -> Result<Vec<SensorDto>, MikrotikError> {
    match value {
        Value::Array(records) => {
            let mut sensors = Vec::new();
            for record in records {
                let Some(map) = record.as_object() else { continue };
                let Some(name) = get_str(map, "name") else { continue };
                let raw_number;
                let raw: Option<&str> = match map.get("value") {
                    Some(Value::String(s)) => Some(s.as_str()),
                    Some(Value::Number(n)) => {
                        raw_number = n.to_string();
                        Some(raw_number.as_str())
                    }
                    _ => None,
                };
                let Some(raw) = raw else { continue };
                if let Some(sensor) = make_sensor(name, raw) {
                    sensors.push(sensor);
                }
            }
            Ok(sensors)
        }
        Value::Object(map) => Ok(map
            .iter()
            .filter_map(|(name, v)| v.as_str().and_then(|raw| make_sensor(name, raw)))
            .collect()),
        _ => Err(parse_err("health property set or sensor array")),
    }
}

/// Parse one interface record from `POST /rest/interface/print` (stats) or
/// the stats-detail union — the same DTO carries base counters and the
/// driver-level error counters; every counter is `None` when the driver
/// does not provide it (never 0).
fn parse_interface(map: &Map<String, Value>) -> Option<InterfaceDto> {
    let name = get_str(map, "name")?.to_owned();
    Some(InterfaceDto {
        name,
        iface_type: get_str(map, "type").map(str::to_owned),
        running: get_bool(map, "running"),
        disabled: get_bool(map, "disabled"),
        rx_byte: get_u64(map, "rx-byte"),
        tx_byte: get_u64(map, "tx-byte"),
        rx_packet: get_u64(map, "rx-packet"),
        tx_packet: get_u64(map, "tx-packet"),
        tx_queue_drop: get_u64(map, "tx-queue-drop"),
        link_downs: get_u64(map, "link-downs"),
        rx_error: get_u64(map, "rx-error"),
        tx_error: get_u64(map, "tx-error"),
        rx_drop: get_u64(map, "rx-drop"),
        rx_error_events: get_u64(map, "rx-error-events"),
        tx_error_events: get_u64(map, "tx-error-events"),
        rx_fcs_error: get_u64(map, "rx-fcs-error"),
        rx_align_error: get_u64(map, "rx-align-error"),
        tx_collision: get_u64(map, "tx-collision"),
        tx_drop: get_u64(map, "tx-drop"),
        rate: None,
        full_duplex: None,
    })
}

/// `POST /rest/interface/print` with `stats` (the counter-bearing source —
/// a bare GET is metadata-only). Tolerates a bare object wrapper.
pub fn parse_interfaces(value: &Value) -> Result<Vec<InterfaceDto>, MikrotikError> {
    if !matches!(value, Value::Array(_) | Value::Object(_)) {
        return Err(parse_err("interface list"));
    }
    Ok(object_list(value)
        .into_iter()
        .filter_map(parse_interface)
        .collect())
}

/// `POST /rest/interface/ethernet/print` with `stats` — DRIVER-level error
/// counters (rx-error-events, tx-error-events, rx-fcs-error, rx-align-error,
/// tx-collision, tx-drop) keyed by `name`/`default-name` for later merging.
pub fn parse_ethernet_stats(value: &Value) -> Result<Vec<EthernetStatsDto>, MikrotikError> {
    if !matches!(value, Value::Array(_) | Value::Object(_)) {
        return Err(parse_err("ethernet stats list"));
    }
    Ok(object_list(value)
        .into_iter()
        .filter_map(|map| {
            let name = get_str(map, "name")?.to_owned();
            Some(EthernetStatsDto {
                name,
                default_name: get_str(map, "default-name").map(str::to_owned),
                rx_error_events: get_u64(map, "rx-error-events"),
                tx_error_events: get_u64(map, "tx-error-events"),
                rx_fcs_error: get_u64(map, "rx-fcs-error"),
                rx_align_error: get_u64(map, "rx-align-error"),
                tx_collision: get_u64(map, "tx-collision"),
                tx_drop: get_u64(map, "tx-drop"),
            })
        })
        .collect())
}

/// `POST /rest/interface/ethernet/monitor {numbers, once}` — negotiated link
/// rate/duplex only.
pub fn parse_ethernet_monitor(value: &Value) -> Result<Vec<EthernetMonitorDto>, MikrotikError> {
    if !matches!(value, Value::Array(_) | Value::Object(_)) {
        return Err(parse_err("ethernet monitor result"));
    }
    Ok(object_list(value)
        .into_iter()
        .filter_map(|map| {
            let name = get_str(map, "name")?.to_owned();
            Some(EthernetMonitorDto {
                name,
                rate: get_str(map, "rate").map(str::to_owned),
                full_duplex: get_bool(map, "full-duplex"),
                status: get_str(map, "status").map(str::to_owned),
            })
        })
        .collect())
}

/// `GET /rest/interface/vlan`.
pub fn parse_vlans(value: &Value) -> Result<Vec<VlanDto>, MikrotikError> {
    if !matches!(value, Value::Array(_) | Value::Object(_)) {
        return Err(parse_err("vlan list"));
    }
    Ok(object_list(value)
        .into_iter()
        .filter_map(|map| {
            let name = get_str(map, "name")?.to_owned();
            Some(VlanDto {
                name,
                vlan_id: get_u64(map, "vlan-id"),
                interface: get_str(map, "interface").map(str::to_owned),
                running: get_bool(map, "running"),
                disabled: get_bool(map, "disabled"),
            })
        })
        .collect())
}

/// RouterOS list fields arrive either as an array of strings/numbers or as
/// a comma-separated string — accept both.
fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect(),
        Some(Value::String(s)) => s
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// `GET /rest/interface/bridge/vlan`.
pub fn parse_bridge_vlans(value: &Value) -> Result<Vec<BridgeVlanDto>, MikrotikError> {
    if !matches!(value, Value::Array(_) | Value::Object(_)) {
        return Err(parse_err("bridge vlan list"));
    }
    Ok(object_list(value)
        .into_iter()
        .map(|map| BridgeVlanDto {
            bridge: get_str(map, "bridge").map(str::to_owned),
            vlan_ids: string_list(map.get("vlan-ids")),
            tagged: string_list(map.get("tagged")),
            untagged: string_list(map.get("untagged")),
            current_tagged: string_list(map.get("current-tagged")),
            current_untagged: string_list(map.get("current-untagged")),
        })
        .collect())
}

/// `GET /rest/system/package/update` — accepts a bare object, a singleton
/// array, or a progressive section array (LAST section wins, per the
/// check-for-updates streaming behavior). Absent `latest-version` stays
/// `None` (the UI renders "unknown", never "up to date").
pub fn parse_update_status(value: &Value) -> Result<UpdateStatusDto, MikrotikError> {
    let map = last_object(value).ok_or_else(|| parse_err("update status object"))?;
    Ok(UpdateStatusDto {
        installed_version: get_str(map, "installed-version").map(str::to_owned),
        latest_version: get_str(map, "latest-version").map(str::to_owned),
        channel: get_str(map, "channel").map(str::to_owned),
        status: get_str(map, "status").map(str::to_owned),
    })
}

/// `GET /rest/system/routerboard` — accepts a bare object or a singleton
/// array. `routerboard="false"` (CHR/x86/i386) parses to `Some(false)`; an
/// absent field stays `None`.
pub fn parse_routerboard(value: &Value) -> Result<RouterboardDto, MikrotikError> {
    let map = first_object(value).ok_or_else(|| parse_err("routerboard object"))?;
    Ok(RouterboardDto {
        routerboard: get_bool(map, "routerboard"),
        model: get_str(map, "model").map(str::to_owned),
        serial_number: get_str(map, "serial-number").map(str::to_owned),
        current_firmware: get_str(map, "current-firmware").map(str::to_owned),
        upgrade_firmware: get_str(map, "upgrade-firmware").map(str::to_owned),
    })
}

/// `GET /rest/file` — record ids live in the `.id` field (values contain
/// `*`, e.g. `*1`).
pub fn parse_files(value: &Value) -> Result<Vec<FileEntryDto>, MikrotikError> {
    if !matches!(value, Value::Array(_) | Value::Object(_)) {
        return Err(parse_err("file list"));
    }
    Ok(object_list(value)
        .into_iter()
        .map(|map| FileEntryDto {
            id: get_str(map, ".id").map(str::to_owned),
            name: get_str(map, "name").map(str::to_owned),
            size: get_u64(map, "size"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mikrotik_parse_scalar_helpers() {
        assert_eq!(parse_u64("123456789"), Some(123_456_789));
        assert_eq!(parse_u64("junk"), None);
        assert_eq!(parse_u64(""), None);
        assert_eq!(parse_f64("43.5"), Some(43.5));
        assert_eq!(parse_f64("not-a-number"), None);
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool("yes"), None);
        assert_eq!(parse_bool(""), None);
    }

    const RESOURCE_FIXTURE: &str = r#"{
        "architecture-name": "arm64",
        "board-name": "RB4011iGS+",
        "cpu-count": "4",
        "cpu-frequency": "1400",
        "cpu-load": "12",
        "free-memory": "1048576000",
        "total-memory": "2097152000",
        "uptime": "3d 04:12:33",
        "version": "7.18.2"
    }"#;

    #[test]
    fn mikrotik_parse_resource_derives_mem_used() {
        let value: Value = serde_json::from_str(RESOURCE_FIXTURE).unwrap();
        let dto = parse_resource(&value).unwrap();
        assert_eq!(dto.cpu_load, Some(12.0));
        assert_eq!(dto.mem_total_bytes, Some(2_097_152_000));
        // mem_used = total - free, locked by fixture.
        assert_eq!(dto.mem_used_bytes, Some(2_097_152_000 - 1_048_576_000));
        assert_eq!(dto.board_name.as_deref(), Some("RB4011iGS+"));
        assert_eq!(dto.version.as_deref(), Some("7.18.2"));
        assert_eq!(dto.architecture_name.as_deref(), Some("arm64"));
        assert_eq!(dto.uptime.as_deref(), Some("3d 04:12:33"));
    }

    #[test]
    fn mikrotik_parse_resource_accepts_singleton_array() {
        let value = json!([serde_json::from_str::<Value>(RESOURCE_FIXTURE).unwrap()]);
        let dto = parse_resource(&value).unwrap();
        assert_eq!(dto.version.as_deref(), Some("7.18.2"));
    }

    #[test]
    fn mikrotik_parse_resource_mem_none_when_inputs_missing_or_malformed() {
        let value = json!({"total-memory": "junk", "free-memory": "100"});
        let dto = parse_resource(&value).unwrap();
        assert_eq!(dto.mem_used_bytes, None);
        assert_eq!(dto.mem_total_bytes, None);
        let value = json!({"total-memory": "100"});
        let dto = parse_resource(&value).unwrap();
        assert_eq!(dto.mem_used_bytes, None);
        assert_eq!(dto.mem_total_bytes, Some(100));
    }

    #[test]
    fn mikrotik_parse_resource_wrong_shape_is_parse_error() {
        let value = json!("junk");
        assert!(matches!(
            parse_resource(&value),
            Err(MikrotikError::Parse(_))
        ));
    }

    #[test]
    fn mikrotik_parse_health_property_set_shape() {
        let value = json!({"temperature": "42", "voltage": "24.1", "fan1-speed": "3000"});
        let sensors = parse_health(&value).unwrap();
        assert_eq!(sensors.len(), 3);
        let temp = sensors.iter().find(|s| s.name == "temperature").unwrap();
        assert_eq!(temp.kind, SensorKind::Temperature);
        assert_eq!(temp.value, 42.0);
        assert_eq!(temp.unit.as_deref(), Some("°C"));
        let volt = sensors.iter().find(|s| s.name == "voltage").unwrap();
        assert_eq!(volt.kind, SensorKind::Voltage);
        assert_eq!(volt.value, 24.1);
        let fan = sensors.iter().find(|s| s.name == "fan1-speed").unwrap();
        assert_eq!(fan.kind, SensorKind::Fan);
        assert_eq!(fan.value, 3000.0);
    }

    #[test]
    fn mikrotik_parse_health_sensor_record_array_shape_with_aliases() {
        // Recorded RouterOS shape: name/value/type records, multiple temps.
        let value = json!([
            {"name": "cpu-temperature", "value": "48", "type": "C"},
            {"name": "board-temperature1", "value": "41", "type": "C"},
            {"name": "sfp-temperature", "value": "39.5", "type": "C"},
            {"name": "fan1-speed", "value": "2345", "type": "RPM"},
            {"name": "psu1-voltage", "value": "12.1", "type": "V"}
        ]);
        let sensors = parse_health(&value).unwrap();
        assert_eq!(sensors.len(), 5);
        let names: Vec<&str> = sensors.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"cpu-temperature"));
        assert!(names.contains(&"board-temperature1"));
        assert!(names.contains(&"sfp-temperature"));
        assert!(names.contains(&"fan1-speed"));
        assert!(names.contains(&"psu1-voltage"));
        assert!(sensors
            .iter()
            .all(|s| s.kind != SensorKind::Other));
    }

    #[test]
    fn mikrotik_parse_health_temperature_alias_patterns() {
        // Every documented temperature alias family must classify as such.
        for name in [
            "temperature",
            "cpu-temperature",
            "cpu-temp",
            "board-temp",
            "pcb-temperature",
            "switch-temperature",
            "sfp-temperature",
            "lm87-temp",
            "temp1",
            "temp2",
            "temp3",
            "board-temperature1",
            "board-temperature2",
        ] {
            assert_eq!(
                sensor_kind(name),
                SensorKind::Temperature,
                "alias not classified: {name}"
            );
        }
        for name in ["fan-speed", "fan1", "fan2", "fan3", "fan4", "fan1-speed", "fan4-speed"] {
            assert_eq!(sensor_kind(name), SensorKind::Fan, "alias not classified: {name}");
        }
        for name in [
            "voltage",
            "voltage1",
            "voltage10",
            "3.3v",
            "5v",
            "12v",
            "core",
            "psu1-voltage",
            "psu2-voltage",
        ] {
            assert_eq!(
                sensor_kind(name),
                SensorKind::Voltage,
                "alias not classified: {name}"
            );
        }
        assert_eq!(sensor_kind("something-else"), SensorKind::Other);
    }

    #[test]
    fn mikrotik_parse_health_deci_unit_normalization_raw_and_deci() {
        // Raw shapes stay as-is.
        let raw = parse_health(&json!({"cpu-temperature": "48", "voltage": "24"})).unwrap();
        assert_eq!(raw[0].value, 48.0);
        assert_eq!(raw[1].value, 24.0);
        // Deci shapes divide by 10 (430 = 43.0°C, 240 = 24.0V).
        let deci = parse_health(&json!({"cpu-temperature": "430", "voltage": "240"})).unwrap();
        assert_eq!(deci[0].value, 43.0);
        assert_eq!(deci[1].value, 24.0);
        // Deci alias in the array shape, too.
        let deci_arr = parse_health(&json!([
            {"name": "cpu-temperature", "value": "485", "type": "C"}
        ]))
        .unwrap();
        assert_eq!(deci_arr[0].value, 48.5);
        // Fans are never deci-scaled (3000 RPM stays 3000).
        let fan = parse_health(&json!({"fan1-speed": "3000"})).unwrap();
        assert_eq!(fan[0].value, 3000.0);
    }

    #[test]
    fn mikrotik_parse_health_skips_unparseable_records() {
        let value = json!([
            {"name": "temperature", "value": "42"},
            {"name": "broken", "value": "junk"},
            {"value": "42"},
            "not-an-object"
        ]);
        let sensors = parse_health(&value).unwrap();
        assert_eq!(sensors.len(), 1);
        assert_eq!(sensors[0].name, "temperature");
    }

    #[test]
    fn mikrotik_parse_health_wrong_shape_is_parse_error() {
        assert!(matches!(parse_health(&json!(42)), Err(MikrotikError::Parse(_))));
    }

    #[test]
    fn mikrotik_parse_interfaces_every_promised_counter_present() {
        // Recorded RouterOS interface/print stats shape; every promised
        // counter present must map into the DTO.
        let value = json!([
            {
                ".id": "*1",
                "name": "ether1",
                "type": "ether",
                "running": "true",
                "disabled": "false",
                "rx-byte": "1000000",
                "tx-byte": "2000000",
                "rx-packet": "9000",
                "tx-packet": "8000",
                "tx-queue-drop": "3",
                "link-downs": "2",
                "rx-error": "11",
                "tx-error": "12",
                "rx-drop": "13",
                "rx-error-events": "14",
                "tx-error-events": "15",
                "rx-fcs-error": "16",
                "rx-align-error": "17",
                "tx-collision": "18",
                "tx-drop": "19"
            }
        ]);
        let ifaces = parse_interfaces(&value).unwrap();
        assert_eq!(ifaces.len(), 1);
        let iface = &ifaces[0];
        assert_eq!(iface.name, "ether1");
        assert_eq!(iface.iface_type.as_deref(), Some("ether"));
        assert_eq!(iface.running, Some(true));
        assert_eq!(iface.disabled, Some(false));
        assert_eq!(iface.rx_byte, Some(1_000_000));
        assert_eq!(iface.tx_byte, Some(2_000_000));
        assert_eq!(iface.rx_packet, Some(9_000));
        assert_eq!(iface.tx_packet, Some(8_000));
        assert_eq!(iface.tx_queue_drop, Some(3));
        assert_eq!(iface.link_downs, Some(2));
        assert_eq!(iface.rx_error, Some(11));
        assert_eq!(iface.tx_error, Some(12));
        assert_eq!(iface.rx_drop, Some(13));
        assert_eq!(iface.rx_error_events, Some(14));
        assert_eq!(iface.tx_error_events, Some(15));
        assert_eq!(iface.rx_fcs_error, Some(16));
        assert_eq!(iface.rx_align_error, Some(17));
        assert_eq!(iface.tx_collision, Some(18));
        assert_eq!(iface.tx_drop, Some(19));
    }

    #[test]
    fn mikrotik_parse_interfaces_absent_counters_are_none_never_zero() {
        // Minimal bridge/loopback record: no counters provided at all.
        let value = json!([
            {
                ".id": "*7",
                "name": "bridge-lan",
                "type": "bridge",
                "running": "true",
                "disabled": "false"
            }
        ]);
        let ifaces = parse_interfaces(&value).unwrap();
        assert_eq!(ifaces.len(), 1);
        let iface = &ifaces[0];
        assert_eq!(iface.rx_byte, None);
        assert_eq!(iface.tx_byte, None);
        assert_eq!(iface.rx_packet, None);
        assert_eq!(iface.tx_packet, None);
        assert_eq!(iface.tx_queue_drop, None);
        assert_eq!(iface.link_downs, None);
        assert_eq!(iface.rx_error, None);
        assert_eq!(iface.tx_error, None);
        assert_eq!(iface.rx_drop, None);
        assert_eq!(iface.rx_error_events, None);
        assert_eq!(iface.tx_error_events, None);
        assert_eq!(iface.rx_fcs_error, None);
        assert_eq!(iface.tx_collision, None);
        assert_eq!(iface.tx_drop, None);
    }

    #[test]
    fn mikrotik_parse_interfaces_stats_detail_union_and_object_tolerance() {
        // Stats-detail response carries only the detail counter union, and a
        // bare object (not array) must still parse.
        let value = json!({
            "name": "ether2",
            "rx-error-events": "1",
            "tx-error-events": "2",
            "rx-fcs-error": "3",
            "tx-collision": "4",
            "tx-drop": "5"
        });
        let ifaces = parse_interfaces(&value).unwrap();
        assert_eq!(ifaces.len(), 1);
        assert_eq!(ifaces[0].rx_error_events, Some(1));
        assert_eq!(ifaces[0].tx_error_events, Some(2));
        assert_eq!(ifaces[0].rx_fcs_error, Some(3));
        assert_eq!(ifaces[0].tx_collision, Some(4));
        assert_eq!(ifaces[0].tx_drop, Some(5));
        assert_eq!(ifaces[0].rx_byte, None);
    }

    #[test]
    fn mikrotik_parse_interfaces_malformed_counters_become_none() {
        let value = json!([{"name": "ether1", "rx-byte": "junk", "tx-byte": "42"}]);
        let ifaces = parse_interfaces(&value).unwrap();
        assert_eq!(ifaces[0].rx_byte, None);
        assert_eq!(ifaces[0].tx_byte, Some(42));
    }

    #[test]
    fn mikrotik_parse_ethernet_stats_driver_counters() {
        let value = json!([
            {
                "name": "uplink",
                "default-name": "ether1",
                "rx-error-events": "21",
                "tx-error-events": "22",
                "rx-fcs-error": "23",
                "rx-align-error": "24",
                "tx-collision": "25",
                "tx-drop": "26"
            },
            {
                "name": "ether5",
                "default-name": "ether5"
            }
        ]);
        let stats = parse_ethernet_stats(&value).unwrap();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].name, "uplink");
        assert_eq!(stats[0].default_name.as_deref(), Some("ether1"));
        assert_eq!(stats[0].rx_error_events, Some(21));
        assert_eq!(stats[0].tx_error_events, Some(22));
        assert_eq!(stats[0].rx_fcs_error, Some(23));
        assert_eq!(stats[0].rx_align_error, Some(24));
        assert_eq!(stats[0].tx_collision, Some(25));
        assert_eq!(stats[0].tx_drop, Some(26));
        // Driver without error counters yields nulls.
        assert_eq!(stats[1].rx_error_events, None);
        assert_eq!(stats[1].tx_drop, None);
    }

    #[test]
    fn mikrotik_parse_ethernet_monitor_rate_and_duplex() {
        let value = json!([
            {
                "name": "ether1",
                "status": "link-ok",
                "rate": "1Gbps",
                "full-duplex": "true"
            }
        ]);
        let monitors = parse_ethernet_monitor(&value).unwrap();
        assert_eq!(monitors.len(), 1);
        assert_eq!(monitors[0].rate.as_deref(), Some("1Gbps"));
        assert_eq!(monitors[0].full_duplex, Some(true));
        assert_eq!(monitors[0].status.as_deref(), Some("link-ok"));
    }

    #[test]
    fn mikrotik_parse_vlans_fixture() {
        let value = json!([
            {
                ".id": "*12",
                "name": "vlan10-mgmt",
                "vlan-id": "10",
                "interface": "bridge-lan",
                "running": "true",
                "disabled": "false"
            },
            {
                ".id": "*13",
                "name": "vlan20-iot",
                "vlan-id": "20",
                "interface": "ether5",
                "running": "false",
                "disabled": "true"
            }
        ]);
        let vlans = parse_vlans(&value).unwrap();
        assert_eq!(vlans.len(), 2);
        assert_eq!(vlans[0].name, "vlan10-mgmt");
        assert_eq!(vlans[0].vlan_id, Some(10));
        assert_eq!(vlans[0].interface.as_deref(), Some("bridge-lan"));
        assert_eq!(vlans[0].running, Some(true));
        assert_eq!(vlans[1].vlan_id, Some(20));
        assert_eq!(vlans[1].disabled, Some(true));
    }

    #[test]
    fn mikrotik_parse_bridge_vlans_both_list_shapes() {
        // Array form (typical RouterOS REST output).
        let value = json!([
            {
                ".id": "*5",
                "bridge": "bridge-lan",
                "vlan-ids": ["10", "20"],
                "tagged": ["ether1", "sfp1"],
                "untagged": ["ether2", "ether3"],
                "current-tagged": ["ether1"],
                "current-untagged": ["ether2"]
            }
        ]);
        let bvs = parse_bridge_vlans(&value).unwrap();
        assert_eq!(bvs.len(), 1);
        assert_eq!(bvs[0].bridge.as_deref(), Some("bridge-lan"));
        assert_eq!(bvs[0].vlan_ids, vec!["10".to_owned(), "20".to_owned()]);
        assert_eq!(bvs[0].tagged, vec!["ether1".to_owned(), "sfp1".to_owned()]);
        assert_eq!(bvs[0].untagged, vec!["ether2".to_owned(), "ether3".to_owned()]);
        assert_eq!(bvs[0].current_tagged, vec!["ether1".to_owned()]);
        assert_eq!(bvs[0].current_untagged, vec!["ether2".to_owned()]);
        // Comma-separated string form (some versions serialize it so).
        let value = json!([{
            "bridge": "bridge-lan",
            "vlan-ids": "30,40",
            "tagged": "ether1",
            "untagged": "ether2, ether3"
        }]);
        let bvs = parse_bridge_vlans(&value).unwrap();
        assert_eq!(bvs[0].vlan_ids, vec!["30".to_owned(), "40".to_owned()]);
        assert_eq!(bvs[0].untagged, vec!["ether2".to_owned(), "ether3".to_owned()]);
    }

    #[test]
    fn mikrotik_parse_update_status_with_and_without_latest_version() {
        // Update available: latest-version present.
        let value = json!({
            "installed-version": "7.18.2",
            "latest-version": "7.19",
            "channel": "stable",
            "status": "New version is available"
        });
        let dto = parse_update_status(&value).unwrap();
        assert_eq!(dto.installed_version.as_deref(), Some("7.18.2"));
        assert_eq!(dto.latest_version.as_deref(), Some("7.19"));
        assert_eq!(dto.channel.as_deref(), Some("stable"));
        assert_eq!(dto.status.as_deref(), Some("New version is available"));
        // Up to date / still checking: latest-version ABSENT stays None.
        let value = json!({
            "installed-version": "7.19",
            "channel": "stable",
            "status": "System is already up to date"
        });
        let dto = parse_update_status(&value).unwrap();
        assert_eq!(dto.latest_version, None);
        // Progressive check-for-updates array: LAST section wins.
        let value = json!([
            {"status": "finding out latest version..."},
            {
                "installed-version": "7.18.2",
                "status": "System is already up to date"
            }
        ]);
        let dto = parse_update_status(&value).unwrap();
        assert_eq!(dto.status.as_deref(), Some("System is already up to date"));
        assert_eq!(dto.installed_version.as_deref(), Some("7.18.2"));
    }

    #[test]
    fn mikrotik_parse_routerboard_true_and_false() {
        let value = json!({
            "routerboard": "true",
            "model": "RB4011iGS+",
            "serial-number": "A1B2C3D4E5",
            "current-firmware": "7.18.2",
            "upgrade-firmware": "7.19"
        });
        let dto = parse_routerboard(&value).unwrap();
        assert_eq!(dto.routerboard, Some(true));
        assert_eq!(dto.model.as_deref(), Some("RB4011iGS+"));
        assert_eq!(dto.serial_number.as_deref(), Some("A1B2C3D4E5"));
        assert_eq!(dto.current_firmware.as_deref(), Some("7.18.2"));
        assert_eq!(dto.upgrade_firmware.as_deref(), Some("7.19"));
        // CHR/x86 answers routerboard="false" — must parse as Some(false).
        let value = json!({
            "routerboard": "false",
            "model": "CHR"
        });
        let dto = parse_routerboard(&value).unwrap();
        assert_eq!(dto.routerboard, Some(false));
        // Singleton-array tolerance.
        let value = json!([{"routerboard": "true", "model": "hAP ax"}]);
        let dto = parse_routerboard(&value).unwrap();
        assert_eq!(dto.routerboard, Some(true));
        assert_eq!(dto.model.as_deref(), Some("hAP ax"));
    }

    #[test]
    fn mikrotik_parse_files_with_star_ids() {
        let value = json!([
            {
                ".id": "*1",
                "name": "verkkokyyla-20260906-120000.backup",
                "size": "1048576"
            },
            {
                ".id": "*2",
                "name": "verkkokyyla-20260906-120000.rsc",
                "size": "2048"
            }
        ]);
        let files = parse_files(&value).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].id.as_deref(), Some("*1"));
        assert_eq!(
            files[0].name.as_deref(),
            Some("verkkokyyla-20260906-120000.backup")
        );
        assert_eq!(files[0].size, Some(1_048_576));
        assert_eq!(files[1].id.as_deref(), Some("*2"));
    }
}
