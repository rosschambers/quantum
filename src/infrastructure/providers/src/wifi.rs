//! WiFi management provider using `nmcli` shell commands.
//!
//! Mirrors the audio provider's pactl shell-out: cheap reads via
//! `nmcli -t -f`, writes that check exit status, change-gated streaming,
//! and command-gated scanning so the overlay only scans while open.

use quantum_domain::{WifiBand, WifiNetwork, WifiSecurity};
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
}
