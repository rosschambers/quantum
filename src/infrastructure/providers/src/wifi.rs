//! WiFi management provider using `nmcli` shell commands.
//!
//! Mirrors the audio provider's pactl shell-out: cheap reads via
//! `nmcli -t -f`, writes that check exit status, change-gated streaming,
//! and command-gated scanning so the overlay only scans while open.

use quantum_domain::{WifiBand, WifiSecurity};

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
}
