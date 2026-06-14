//! WiFi management provider using `nmcli` shell commands.
//!
//! Mirrors the audio provider's pactl shell-out: cheap reads via
//! `nmcli -t -f`, writes that check exit status, change-gated streaming,
//! and command-gated scanning so the overlay only scans while open.

use quantum_domain::{
    Ipv4Method, SavedNetwork, WifiBand, WifiConnectionDetails, WifiNetwork, WifiSecurity,
};
use std::collections::HashMap;

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
        best
            .entry(key)
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
        if fields[2] != "802-11-wireless" {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
