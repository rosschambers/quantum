//! WiFi management provider using `nmcli` shell commands.
//!
//! Mirrors the audio provider's pactl shell-out: cheap reads via
//! `nmcli -t -f`, writes that check exit status, change-gated streaming,
//! and command-gated scanning so the overlay only scans while open.

use async_trait::async_trait;
use futures::stream::BoxStream;
use quantum_domain::{
    Action, ActionOutcome, DomainError, Ipv4Method, Match, ProviderId, ProviderSource, Query,
    SavedNetwork, WifiBand, WifiConnectionDetails, WifiNetwork, WifiSecurity, WifiState,
};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::{Mutex, Notify};

use crate::error::ProvidersError;

/// nmcli connection TYPE literal for WiFi profiles. Used to filter `connection
/// show` output to wireless rows in both saved-list parsing and active-connection
/// lookup; kept as one constant so the two call sites cannot drift apart.
const WIRELESS_TYPE: &str = "802-11-wireless";

/// Map an nmcli SECURITY field to WifiSecurity.
pub(crate) fn map_security(field: &str) -> WifiSecurity {
    if field.is_empty() {
        WifiSecurity::Open
    } else if field.contains("WPA3") {
        WifiSecurity::Wpa3
    } else if field.contains("WPA2") {
        WifiSecurity::Wpa2
    } else if field.contains("WPA") {
        WifiSecurity::Wpa
    } else {
        WifiSecurity::Other
    }
}

/// Map an nmcli FREQ value (MHz) to a WifiBand.
pub(crate) fn map_band(freq_mhz: u32) -> WifiBand {
    match freq_mhz {
        2400..=2500 => WifiBand::TwoFour,
        5925..=7125 => WifiBand::Six,
        5000..=5900 => WifiBand::Five,
        _ => WifiBand::Unknown,
    }
}

/// Split one nmcli `-t` line into fields, honouring backslash-escaped
/// colons (nmcli escapes colons inside values like BSSID).
fn split_terse_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(&next) = chars.peek() {
                    current.push(next);
                    chars.next();
                }
            }
            ':' => {
                fields.push(std::mem::take(&mut current));
            }
            other => current.push(other),
        }
    }
    fields.push(current);
    fields
}

/// Parse `nmcli -t -f SSID,BSSID,SIGNAL,SECURITY,FREQ,ACTIVE,IN-USE
/// device wifi list` output into deduplicated WifiNetwork rows.
pub(crate) fn parse_scan_list(raw: &str) -> Vec<WifiNetwork> {
    // Keyed by SSID; for hidden networks (empty SSID) key on BSSID so
    // multiple hidden access points are not collapsed together.
    let mut best: HashMap<String, WifiNetwork> = HashMap::new();
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let f = split_terse_line(line);
        if f.len() < 7 {
            continue;
        }
        let ssid = f[0].clone();
        let bssid = f[1].clone();
        let signal_percent: u8 = f[2].parse().unwrap_or(0);
        let security = map_security(&f[3]);
        let freq: u32 = f[4].parse().unwrap_or(0);
        let band = map_band(freq);
        let active = f[5].eq_ignore_ascii_case("yes") || f[6].trim() == "*";

        let net = WifiNetwork {
            ssid: ssid.clone(),
            bssid: bssid.clone(),
            signal_percent,
            security,
            band,
            saved: false,
            active,
        };

        let key = if ssid.is_empty() {
            format!("\0hidden\0{bssid}")
        } else {
            ssid
        };
        best.entry(key)
            .and_modify(|existing| {
                if net.signal_percent > existing.signal_percent {
                    let was_active = existing.active;
                    *existing = net.clone();
                    existing.active = was_active || net.active;
                } else if net.active {
                    existing.active = true;
                }
            })
            .or_insert(net);
    }
    let mut out: Vec<WifiNetwork> = best.into_values().collect();
    out.sort_by(|a, b| b.signal_percent.cmp(&a.signal_percent));
    out
}

/// Unescape an nmcli `-t` value, turning `\:` into `:` and `\\` into `\`.
/// Used for single values (not whole lines) where the key has already been
/// separated on the first colon.
fn unescape_terse(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                out.push(next);
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse `nmcli radio wifi` output into a boolean. True only for the trimmed,
/// case-insensitive literal `enabled`.
pub(crate) fn parse_radio(raw: &str) -> bool {
    raw.trim().eq_ignore_ascii_case("enabled")
}

/// Parse `nmcli -t -f NAME,UUID,TYPE,AUTOCONNECT connection show` output into
/// saved wireless profiles, keeping only `802-11-wireless` rows. The `id` is
/// the stable NetworkManager UUID (used as an opaque handle for
/// `nmcli connection {up,delete,modify} uuid <id>`), while `ssid` is the
/// connection NAME used for display. The `security` and `in_range` fields
/// default here and are filled in later by the provider from the scan list.
pub(crate) fn parse_saved_list(raw: &str) -> Vec<SavedNetwork> {
    let mut out = Vec::new();
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let fields = split_terse_line(line);
        if fields.len() < 4 {
            continue;
        }
        if fields[2] != WIRELESS_TYPE {
            continue;
        }
        out.push(SavedNetwork {
            id: fields[1].clone(),
            ssid: fields[0].clone(),
            security: WifiSecurity::Other,
            autoconnect: fields[3].eq_ignore_ascii_case("yes"),
            in_range: false,
        });
    }
    out
}

/// Parse `nmcli -t` key:value output for a connection into details. Handles
/// indexed keys like `IP4.ADDRESS[1]`, collects `IP4.DNS[n]` in order, and
/// unescapes the colon-laden `GENERAL.HWADDR` value. `frequency_mhz` is filled
/// later by the provider from the scan row, so it defaults to None.
pub(crate) fn parse_details(raw: &str) -> WifiConnectionDetails {
    let mut ip_address: Option<String> = None;
    let mut gateway: Option<String> = None;
    let mut dns: Vec<String> = Vec::new();
    let mut mac: Option<String> = None;
    let mut ipv4_method = Ipv4Method::Auto;
    let mut metered = false;

    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let Some((key, raw_value)) = line.split_once(':') else {
            continue;
        };
        let value = unescape_terse(raw_value);
        if key.starts_with("IP4.ADDRESS") {
            if ip_address.is_none() {
                ip_address = Some(value);
            }
        } else if key == "IP4.GATEWAY" {
            gateway = Some(value);
        } else if key.starts_with("IP4.DNS") {
            dns.push(value);
        } else if key == "GENERAL.HWADDR" {
            mac = Some(value);
        } else if key == "ipv4.method" {
            ipv4_method = if value == "auto" {
                Ipv4Method::Auto
            } else {
                Ipv4Method::Manual
            };
        } else if key == "connection.metered" {
            metered = value == "yes" || value == "1";
        }
    }

    WifiConnectionDetails {
        ip_address,
        gateway,
        dns,
        mac,
        frequency_mhz: None,
        ipv4_method,
        metered,
    }
}

/// A typed WiFi command parsed from the JSON payload the frontend sends through
/// `action.invoke`. Each variant maps to a snake_case `command` string.
pub(crate) enum WifiAction {
    OpenSession,
    CloseSession,
    Rescan,
    SetRadio(bool),
    Connect {
        ssid: String,
        bssid: Option<String>,
        password: Option<String>,
    },
    Disconnect,
    Forget {
        id: String,
    },
    ConnectHidden {
        ssid: String,
        password: Option<String>,
    },
    SetAutoconnect {
        id: String,
        enabled: bool,
    },
    SetIpv4 {
        id: String,
        method: Ipv4Method,
        address: Option<String>,
        gateway: Option<String>,
        prefix: Option<u8>,
    },
    SetDns {
        id: String,
        servers: Vec<String>,
    },
    SetMetered {
        id: String,
        metered: bool,
    },
    FetchDetails {
        ssid: String,
    },
}

/// Read a required string field, erroring with Unsupported when absent or not a
/// string.
fn required_str(payload: &serde_json::Value, key: &str) -> Result<String, DomainError> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            DomainError::Unsupported(format!("missing or non-string '{key}' in wifi action"))
        })
}

/// Read an optional string field, returning None when absent or not a string.
fn optional_str(payload: &serde_json::Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Read a required boolean field, erroring with Unsupported when absent or not a
/// boolean.
fn required_bool(payload: &serde_json::Value, key: &str) -> Result<bool, DomainError> {
    payload.get(key).and_then(|v| v.as_bool()).ok_or_else(|| {
        DomainError::Unsupported(format!("missing or non-bool '{key}' in wifi action"))
    })
}

/// Read a required array-of-strings field, erroring with Unsupported when the
/// field is absent, not an array, or contains a non-string element.
fn required_string_array(
    payload: &serde_json::Value,
    key: &str,
) -> Result<Vec<String>, DomainError> {
    let array = payload.get(key).and_then(|v| v.as_array()).ok_or_else(|| {
        DomainError::Unsupported(format!("missing or non-array '{key}' in wifi action"))
    })?;
    let mut out = Vec::with_capacity(array.len());
    for element in array {
        let value = element.as_str().ok_or_else(|| {
            DomainError::Unsupported(format!("non-string element in '{key}' array"))
        })?;
        out.push(value.to_string());
    }
    Ok(out)
}

/// Parse an optional IPv4 prefix length. Absent -> None. Present must be an
/// integer in 0..=32; anything else is rejected.
fn optional_prefix(payload: &serde_json::Value) -> Result<Option<u8>, DomainError> {
    match payload.get("prefix") {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|n| *n <= 32)
            .map(|n| Some(n as u8))
            .ok_or_else(|| {
                DomainError::Unsupported(
                    "prefix must be an integer in 0..=32 in wifi action".to_string(),
                )
            }),
    }
}

/// Parse the IPv4 method string into an Ipv4Method, erroring with Unsupported on
/// anything other than "auto" or "manual".
fn parse_ipv4_method(payload: &serde_json::Value) -> Result<Ipv4Method, DomainError> {
    match required_str(payload, "method")?.as_str() {
        "auto" => Ok(Ipv4Method::Auto),
        "manual" => Ok(Ipv4Method::Manual),
        other => Err(DomainError::Unsupported(format!(
            "unknown ipv4 method: {other}"
        ))),
    }
}

/// Parse a WiFi action from a JSON payload.
///
/// Reads the snake_case `command` string and builds the matching variant,
/// enforcing required versus optional fields. Missing or unknown commands and
/// missing or wrong-typed required arguments return `DomainError::Unsupported`.
pub(crate) fn parse_wifi_action(payload: &serde_json::Value) -> Result<WifiAction, DomainError> {
    let command = payload
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            DomainError::Unsupported("missing or non-string command in wifi action".to_string())
        })?;

    match command {
        "open_session" => Ok(WifiAction::OpenSession),
        "close_session" => Ok(WifiAction::CloseSession),
        "rescan" => Ok(WifiAction::Rescan),
        "set_radio" => Ok(WifiAction::SetRadio(required_bool(payload, "enabled")?)),
        "connect" => Ok(WifiAction::Connect {
            ssid: required_str(payload, "ssid")?,
            bssid: optional_str(payload, "bssid"),
            password: optional_str(payload, "password"),
        }),
        "disconnect" => Ok(WifiAction::Disconnect),
        "forget" => Ok(WifiAction::Forget {
            id: required_str(payload, "id")?,
        }),
        "connect_hidden" => Ok(WifiAction::ConnectHidden {
            ssid: required_str(payload, "ssid")?,
            password: optional_str(payload, "password"),
        }),
        "set_autoconnect" => Ok(WifiAction::SetAutoconnect {
            id: required_str(payload, "id")?,
            enabled: required_bool(payload, "enabled")?,
        }),
        "set_ipv4" => Ok(WifiAction::SetIpv4 {
            id: required_str(payload, "id")?,
            method: parse_ipv4_method(payload)?,
            address: optional_str(payload, "address"),
            gateway: optional_str(payload, "gateway"),
            prefix: optional_prefix(payload)?,
        }),
        "set_dns" => Ok(WifiAction::SetDns {
            id: required_str(payload, "id")?,
            servers: required_string_array(payload, "servers")?,
        }),
        "set_metered" => Ok(WifiAction::SetMetered {
            id: required_str(payload, "id")?,
            metered: required_bool(payload, "metered")?,
        }),
        "fetch_details" => Ok(WifiAction::FetchDetails {
            ssid: required_str(payload, "ssid")?,
        }),
        other => Err(DomainError::Unsupported(format!(
            "unknown wifi command: {other}"
        ))),
    }
}

/// Shared scan/session state for the WiFi provider.
///
/// The overlay drives scanning explicitly: `OpenSession` flips `active` on so
/// the streaming task (Task 7) starts periodic rescans, and `CloseSession`
/// flips it off and drops the cached state. `notify` lets any write command
/// wake the streaming task immediately so the next emitted `WifiState` reflects
/// the change without waiting for the next poll tick. `last` caches the most
/// recently emitted state for change-gating.
struct ScanSession {
    active: AtomicBool,
    notify: Notify,
    last: Mutex<Option<WifiState>>,
}

impl Default for ScanSession {
    fn default() -> Self {
        Self {
            active: AtomicBool::new(false),
            notify: Notify::new(),
            last: Mutex::new(None),
        }
    }
}

/// WiFi management provider using `nmcli` shell commands.
///
/// Reads are served by the streaming `subscribe` (Task 7); this struct owns the
/// write side: every `invoke` either flips session/scan flags or runs an
/// `nmcli` command, then notifies the streaming task so the overlay sees the
/// effect quickly.
pub struct WifiProvider {
    id: ProviderId,
    available: bool,
    scan: Arc<ScanSession>,
}

impl WifiProvider {
    /// Probe for `nmcli` and build the provider.
    ///
    /// Never errors on a missing `nmcli`: it records `available = false` so
    /// `invoke` returns `Unsupported` rather than failing construction, mirroring
    /// the audio provider's behaviour when `pactl` is absent.
    pub async fn connect() -> Result<Self, ProvidersError> {
        let available = which::which("nmcli").is_ok();
        Ok(Self {
            id: ProviderId::from("wifi"),
            available,
            scan: Arc::new(ScanSession::default()),
        })
    }

    /// Execute a parsed WiFi command, running the matching `nmcli` invocation
    /// and notifying the streaming task so the next emitted state reflects it.
    async fn execute(&self, command: WifiAction) -> Result<ActionOutcome, DomainError> {
        match command {
            WifiAction::OpenSession => {
                self.scan.active.store(true, Ordering::SeqCst);
                self.scan.notify.notify_one();
                Ok(ActionOutcome { message: None })
            }
            WifiAction::CloseSession => {
                self.scan.active.store(false, Ordering::SeqCst);
                {
                    let mut last = self.scan.last.lock().await;
                    *last = None;
                }
                self.scan.notify.notify_one();
                Ok(ActionOutcome { message: None })
            }
            WifiAction::Rescan => {
                run_nmcli(&["device", "wifi", "rescan"])
                    .await
                    .map_err(map_nmcli_error)?;
                self.scan.notify.notify_one();
                Ok(ActionOutcome { message: None })
            }
            WifiAction::SetRadio(enabled) => {
                let state = if enabled { "on" } else { "off" };
                run_nmcli(&["radio", "wifi", state])
                    .await
                    .map_err(map_nmcli_error)?;
                self.scan.notify.notify_one();
                Ok(ActionOutcome { message: None })
            }
            WifiAction::Connect {
                ssid,
                bssid: _,
                password,
            } => {
                let result = connect_to_network(&ssid, password.as_deref(), false).await;
                self.scan.notify.notify_one();
                result.map_err(map_connect_error)?;
                Ok(ActionOutcome { message: None })
            }
            WifiAction::ConnectHidden { ssid, password } => {
                let result = connect_to_network(&ssid, password.as_deref(), true).await;
                self.scan.notify.notify_one();
                result.map_err(map_connect_error)?;
                Ok(ActionOutcome { message: None })
            }
            WifiAction::Disconnect => {
                let outcome = disconnect_active_wifi().await.map_err(map_nmcli_error)?;
                self.scan.notify.notify_one();
                Ok(outcome)
            }
            WifiAction::Forget { id } => {
                run_nmcli(&["connection", "delete", "uuid", &id])
                    .await
                    .map_err(map_nmcli_error)?;
                self.scan.notify.notify_one();
                Ok(ActionOutcome { message: None })
            }
            WifiAction::SetAutoconnect { id, enabled } => {
                let value = if enabled { "yes" } else { "no" };
                run_nmcli(&[
                    "connection",
                    "modify",
                    "uuid",
                    &id,
                    "connection.autoconnect",
                    value,
                ])
                .await
                .map_err(map_nmcli_error)?;
                self.scan.notify.notify_one();
                Ok(ActionOutcome { message: None })
            }
            WifiAction::SetIpv4 {
                id,
                method,
                address,
                gateway,
                prefix,
            } => {
                set_ipv4(&id, method, address.as_deref(), gateway.as_deref(), prefix).await?;
                self.scan.notify.notify_one();
                Ok(ActionOutcome { message: None })
            }
            WifiAction::SetDns { id, servers } => {
                let joined = servers.join(",");
                run_nmcli(&["connection", "modify", "uuid", &id, "ipv4.dns", &joined])
                    .await
                    .map_err(map_nmcli_error)?;
                apply_connection(&id).await?;
                self.scan.notify.notify_one();
                Ok(ActionOutcome { message: None })
            }
            WifiAction::SetMetered { id, metered } => {
                let value = if metered { "yes" } else { "no" };
                run_nmcli(&[
                    "connection",
                    "modify",
                    "uuid",
                    &id,
                    "connection.metered",
                    value,
                ])
                .await
                .map_err(map_nmcli_error)?;
                self.scan.notify.notify_one();
                Ok(ActionOutcome { message: None })
            }
            WifiAction::FetchDetails { ssid } => {
                // Parsed for validation / future stream enrichment; the stream
                // is the primary channel, so the outcome carries no payload.
                let _ = fetch_details(&ssid).await.map_err(map_nmcli_error)?;
                self.scan.notify.notify_one();
                Ok(ActionOutcome { message: None })
            }
        }
    }
}

#[async_trait]
impl ProviderSource for WifiProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, _query: &Query) -> Result<Vec<Match>, DomainError> {
        Ok(vec![])
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        if !self.available {
            return Err(DomainError::Unsupported(
                "wifi provider unavailable".to_string(),
            ));
        }
        match action {
            Action::Custom { kind, payload } if kind == "wifi" => {
                let command = parse_wifi_action(payload)?;
                self.execute(command).await
            }
            _ => Err(DomainError::Unsupported(
                "wifi provider only handles custom actions with kind='wifi'".to_string(),
            )),
        }
    }

    fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>> {
        // Placeholder: Task 7 replaces this with the change-gated,
        // session-driven streaming implementation.
        None
    }
}

/// Run an `nmcli` invocation with no shell, each argument passed as a separate
/// argv item. Returns stdout on a zero exit code. On a non-zero exit it returns
/// a `ProvidersError` carrying the captured stderr — this signals a failed
/// command, not a transport or availability condition; callers remap it with
/// `map_nmcli_error` to `DomainError::ActionFailed`. If the process could not be
/// started at all it returns `ProvidersError::Spawn`, which `map_nmcli_error`
/// turns into `DomainError::Unsupported`.
async fn run_nmcli(args: &[&str]) -> Result<String, ProvidersError> {
    let output = Command::new("nmcli")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| ProvidersError::Spawn(e.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(ProvidersError::ServiceUnavailable(stderr))
    }
}

/// Connect to a scanned or new network keyed on SSID (no profile UUID exists
/// yet). Appends `password <pw>` when a password is supplied and `hidden yes`
/// for hidden networks.
async fn connect_to_network(
    ssid: &str,
    password: Option<&str>,
    hidden: bool,
) -> Result<String, ProvidersError> {
    let mut args: Vec<&str> = vec!["device", "wifi", "connect", ssid];
    if let Some(pw) = password {
        args.push("password");
        args.push(pw);
    }
    if hidden {
        args.push("hidden");
        args.push("yes");
    }
    run_nmcli(&args).await
}

/// Bring down the active wireless connection.
///
/// Finds the active 802-11-wireless connection's UUID via
/// `nmcli -t -f UUID,TYPE,DEVICE connection show --active` and runs
/// `connection down uuid <uuid>`. Returns an informational `ActionOutcome` when
/// there is no active wifi connection rather than erroring.
async fn disconnect_active_wifi() -> Result<ActionOutcome, ProvidersError> {
    let raw = run_nmcli(&[
        "-t",
        "-f",
        "UUID,TYPE,DEVICE",
        "connection",
        "show",
        "--active",
    ])
    .await?;
    let uuid = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(split_terse_line)
        .find(|fields| fields.len() >= 2 && fields[1] == WIRELESS_TYPE)
        .map(|fields| fields[0].clone());
    match uuid {
        Some(uuid) => {
            run_nmcli(&["connection", "down", "uuid", &uuid]).await?;
            Ok(ActionOutcome { message: None })
        }
        None => Ok(ActionOutcome {
            message: Some("no active wifi connection".to_string()),
        }),
    }
}

/// Modify the IPv4 method (and, for manual, address/gateway) of a saved
/// connection by UUID, then re-apply it with `connection up uuid <id>`.
///
/// The modify step maps failures with the normal `map_nmcli_error` rule. The
/// re-apply step maps via `apply_connection`, which produces a distinguishable
/// "settings saved but failed to apply" error so a caller can tell the profile
/// was changed even though it did not activate.
async fn set_ipv4(
    id: &str,
    method: Ipv4Method,
    address: Option<&str>,
    gateway: Option<&str>,
    prefix: Option<u8>,
) -> Result<(), DomainError> {
    let method_value = match method {
        Ipv4Method::Auto => "auto",
        Ipv4Method::Manual => "manual",
    };
    let mut args: Vec<String> = vec![
        "connection".to_string(),
        "modify".to_string(),
        "uuid".to_string(),
        id.to_string(),
        "ipv4.method".to_string(),
        method_value.to_string(),
    ];
    if matches!(method, Ipv4Method::Manual) {
        if let (Some(address), Some(prefix)) = (address, prefix) {
            args.push("ipv4.addresses".to_string());
            args.push(format!("{address}/{prefix}"));
        }
        if let Some(gateway) = gateway {
            args.push("ipv4.gateway".to_string());
            args.push(gateway.to_string());
        }
    }
    let modify_args: Vec<&str> = args.iter().map(String::as_str).collect();
    run_nmcli(&modify_args).await.map_err(map_nmcli_error)?;
    apply_connection(id).await
}

/// Fetch and parse connection details keyed by SSID/connection name. nmcli
/// accepts the connection name in `connection show`, and the frontend passes
/// the SSID, so keying by SSID here is acceptable.
async fn fetch_details(ssid: &str) -> Result<WifiConnectionDetails, ProvidersError> {
    let raw = run_nmcli(&[
        "-t",
        "-f",
        "IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,GENERAL.HWADDR,ipv4.method,connection.metered",
        "connection",
        "show",
        ssid,
    ])
    .await?;
    Ok(parse_details(&raw))
}

/// Map a generic `nmcli` failure to a `DomainError`. A spawn failure means the
/// binary is effectively unusable, so it surfaces as `Unsupported`; a non-zero
/// exit surfaces as `ActionFailed` carrying the cleaned stderr text.
fn map_nmcli_error(error: ProvidersError) -> DomainError {
    match error {
        ProvidersError::Spawn(message) => DomainError::Unsupported(message),
        other => DomainError::ActionFailed {
            reason: other.to_string(),
        },
    }
}

/// Map a `device wifi connect` failure to a `DomainError`, recognising the
/// wrong-password signatures nmcli emits and collapsing them to a stable
/// `incorrect_password` reason the frontend can branch on.
fn map_connect_error(error: ProvidersError) -> DomainError {
    let message = error.to_string();
    let lowered = message.to_lowercase();
    if lowered.contains("secrets were required") || lowered.contains("no secrets provided") {
        return DomainError::ActionFailed {
            reason: "incorrect_password".to_string(),
        };
    }
    map_nmcli_error(error)
}

/// Re-apply a saved connection by UUID with `connection up uuid <id>`, used
/// after a successful `connection modify`. A failure here means the profile was
/// already mutated on disk but could not be brought up, so the returned error
/// says so explicitly rather than presenting an opaque failure that hides the
/// fact that the saved settings changed. nmcli has no transactional batch, so
/// no rollback is attempted.
async fn apply_connection(id: &str) -> Result<(), DomainError> {
    run_nmcli(&["connection", "up", "uuid", id])
        .await
        .map(|_| ())
        .map_err(|error| DomainError::ActionFailed {
            reason: format!("settings saved but failed to apply: {error}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_connect_with_password() {
        match parse_wifi_action(&json!({"command":"connect","ssid":"Net","password":"pw"})) {
            Ok(WifiAction::Connect { ssid, password, .. }) => {
                assert_eq!(ssid, "Net");
                assert_eq!(password.as_deref(), Some("pw"));
            }
            _ => panic!("expected Connect"),
        }
    }

    #[test]
    fn parses_connect_without_password() {
        match parse_wifi_action(&json!({"command":"connect","ssid":"Net"})) {
            Ok(WifiAction::Connect { password: None, .. }) => {}
            _ => panic!("expected Connect with no password"),
        }
    }

    #[test]
    fn parses_session_and_radio_commands() {
        assert!(matches!(
            parse_wifi_action(&json!({"command":"open_session"})),
            Ok(WifiAction::OpenSession)
        ));
        assert!(matches!(
            parse_wifi_action(&json!({"command":"close_session"})),
            Ok(WifiAction::CloseSession)
        ));
        assert!(matches!(
            parse_wifi_action(&json!({"command":"set_radio","enabled":true})),
            Ok(WifiAction::SetRadio(true))
        ));
    }

    #[test]
    fn parses_set_ipv4_manual() {
        match parse_wifi_action(
            &json!({"command":"set_ipv4","id":"Net","method":"manual","address":"10.0.0.2","gateway":"10.0.0.1","prefix":24}),
        ) {
            Ok(WifiAction::SetIpv4 {
                method: Ipv4Method::Manual,
                prefix: Some(24),
                ..
            }) => {}
            _ => panic!("expected manual SetIpv4"),
        }
    }

    #[test]
    fn rejects_out_of_range_prefix() {
        assert!(parse_wifi_action(
            &json!({"command":"set_ipv4","id":"Net","method":"manual","prefix":300})
        )
        .is_err());
    }

    #[test]
    fn set_ipv4_without_prefix_is_none() {
        match parse_wifi_action(&json!({"command":"set_ipv4","id":"Net","method":"auto"})) {
            Ok(WifiAction::SetIpv4 {
                prefix: None,
                method: Ipv4Method::Auto,
                ..
            }) => {}
            _ => panic!("expected auto SetIpv4 with no prefix"),
        }
    }

    #[test]
    fn parses_set_dns() {
        match parse_wifi_action(
            &json!({"command":"set_dns","id":"Net","servers":["1.1.1.1","9.9.9.9"]}),
        ) {
            Ok(WifiAction::SetDns { servers, .. }) => {
                assert_eq!(servers, vec!["1.1.1.1", "9.9.9.9"])
            }
            _ => panic!("expected SetDns"),
        }
    }

    #[test]
    fn rejects_unknown_and_missing_command() {
        assert!(parse_wifi_action(&json!({"command":"frobnicate"})).is_err());
        assert!(parse_wifi_action(&json!({})).is_err());
        assert!(parse_wifi_action(&json!({"command":"forget"})).is_err()); // missing id
        assert!(
            parse_wifi_action(&json!({"command":"set_ipv4","id":"Net","method":"bogus"})).is_err()
        ); // bad method
    }

    #[test]
    fn map_security_classifies_fields() {
        assert_eq!(map_security(""), WifiSecurity::Open);
        assert_eq!(map_security("WPA3"), WifiSecurity::Wpa3);
        assert_eq!(map_security("WPA2 WPA3"), WifiSecurity::Wpa3);
        assert_eq!(map_security("WPA2"), WifiSecurity::Wpa2);
        assert_eq!(map_security("WPA1"), WifiSecurity::Wpa);
        assert_eq!(map_security("802.1X"), WifiSecurity::Other);
    }

    #[test]
    fn map_band_classifies_frequencies() {
        assert_eq!(map_band(2412), WifiBand::TwoFour);
        assert_eq!(map_band(5180), WifiBand::Five);
        assert_eq!(map_band(5955), WifiBand::Six);
        assert_eq!(map_band(900), WifiBand::Unknown);
    }

    const SAMPLE_SCAN: &str = "\
Skynet_5G:3C\\:22\\:FB\\:1A\\:8E\\:00:92:WPA2 WPA3:5180:yes:*
Skynet_5G:3C\\:22\\:FB\\:1A\\:8E\\:01:70:WPA2 WPA3:2412:no:
CoffeeShopFree:AA\\:BB\\:CC\\:DD\\:EE\\:FF:55::2437:no:
:11\\:22\\:33\\:44\\:55\\:66:30:WPA2:5200:no:";

    #[test]
    fn parse_scan_list_dedups_by_ssid_keeping_strongest() {
        let nets = parse_scan_list(SAMPLE_SCAN);
        // Skynet collapses to one row at 92% on the 5 GHz band.
        let skynet: Vec<_> = nets.iter().filter(|n| n.ssid == "Skynet_5G").collect();
        assert_eq!(skynet.len(), 1);
        assert_eq!(skynet[0].signal_percent, 92);
        assert_eq!(skynet[0].band, WifiBand::Five);
        assert_eq!(skynet[0].security, WifiSecurity::Wpa3);
        assert!(skynet[0].active);
        assert_eq!(skynet[0].bssid, "3C:22:FB:1A:8E:00");
    }

    #[test]
    fn parse_scan_list_handles_open_and_hidden() {
        let nets = parse_scan_list(SAMPLE_SCAN);
        let open = nets.iter().find(|n| n.ssid == "CoffeeShopFree").unwrap();
        assert_eq!(open.security, WifiSecurity::Open);
        // Hidden network retained with empty SSID.
        assert!(nets.iter().any(|n| n.ssid.is_empty()));
    }

    #[test]
    fn parse_radio_reads_enabled() {
        assert!(parse_radio("enabled\n"));
        assert!(!parse_radio("disabled\n"));
        assert!(parse_radio("ENABLED"));
    }

    #[test]
    fn parse_saved_list_filters_wireless() {
        let raw = "\
Skynet_5G:uuid-1:802-11-wireless:yes
Wired connection 1:uuid-2:802-3-ethernet:yes
Office-Floor3:uuid-3:802-11-wireless:no";
        let saved = parse_saved_list(raw);
        assert_eq!(saved.len(), 2);
        let skynet = saved.iter().find(|s| s.ssid == "Skynet_5G").unwrap();
        assert!(skynet.autoconnect);
        assert_eq!(skynet.id, "uuid-1");
        let office = saved.iter().find(|s| s.ssid == "Office-Floor3").unwrap();
        assert!(!office.autoconnect);
        assert_eq!(office.id, "uuid-3");
    }

    #[test]
    fn parse_details_extracts_fields() {
        let raw = "\
IP4.ADDRESS[1]:192.168.1.42/24
IP4.GATEWAY:192.168.1.1
IP4.DNS[1]:1.1.1.1
IP4.DNS[2]:9.9.9.9
GENERAL.HWADDR:3C\\:22\\:FB\\:1A\\:8E\\:00
ipv4.method:auto
connection.metered:no";
        let d = parse_details(raw);
        assert_eq!(d.ip_address.as_deref(), Some("192.168.1.42/24"));
        assert_eq!(d.gateway.as_deref(), Some("192.168.1.1"));
        assert_eq!(d.dns, vec!["1.1.1.1", "9.9.9.9"]);
        assert_eq!(d.mac.as_deref(), Some("3C:22:FB:1A:8E:00"));
        assert_eq!(d.ipv4_method, Ipv4Method::Auto);
        assert!(!d.metered);
    }

    #[test]
    fn parse_scan_list_active_propagates_from_weaker_row() {
        // The strongest row is NOT active; a weaker duplicate is active.
        // The collapsed row must keep the strongest signal AND be active.
        let raw = "\
HomeNet:AA\\:AA\\:AA\\:AA\\:AA\\:01:90:WPA2:5180:no:
HomeNet:AA\\:AA\\:AA\\:AA\\:AA\\:02:40:WPA2:2412:yes:*";
        let nets = parse_scan_list(raw);
        let home: Vec<_> = nets.iter().filter(|n| n.ssid == "HomeNet").collect();
        assert_eq!(home.len(), 1);
        assert_eq!(home[0].signal_percent, 90);
        assert!(home[0].active);
    }

    #[tokio::test]
    async fn invoke_on_unavailable_provider_is_unsupported() {
        let provider = WifiProvider {
            id: quantum_domain::ProviderId::from("wifi"),
            available: false,
            scan: std::sync::Arc::new(ScanSession::default()),
        };
        let action = quantum_domain::Action::Custom {
            kind: "wifi".to_string(),
            payload: serde_json::json!({"command":"open_session"}),
        };
        assert!(provider.invoke(&action).await.is_err());
    }

    #[tokio::test]
    async fn invoke_rejects_foreign_kind() {
        let provider = WifiProvider {
            id: quantum_domain::ProviderId::from("wifi"),
            available: true,
            scan: std::sync::Arc::new(ScanSession::default()),
        };
        let action = quantum_domain::Action::Custom {
            kind: "audio".to_string(),
            payload: serde_json::json!({"command":"open_session"}),
        };
        assert!(provider.invoke(&action).await.is_err());
    }
}
