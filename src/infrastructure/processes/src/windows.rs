//! Parse Hyprland's `j/clients` JSON into a pid-to-window map for the process
//! monitor to correlate windows with the processes that own them.
//!
//! This is deliberately separate from the launcher's window provider
//! (`hyprland_windows.rs` in the providers crate): only the process monitor
//! needs pids, so this small standalone parser reads the JSON directly into
//! [`quantum_domain::WindowInfo`] rather than coupling to the provider's own,
//! richer window struct.

use std::collections::HashMap;

use quantum_domain::WindowInfo;

/// Parse a Hyprland `j/clients` JSON response into a map from process
/// identifier to the window that process owns.
///
/// Each array entry contributes `pid -> WindowInfo { class, title }` when it
/// carries a numeric `pid` greater than zero. An entry with no `pid` field, a
/// non-numeric `pid`, or a `pid` less than or equal to zero is skipped. Empty
/// `class` or `title` strings are tolerated (a missing field reads as empty).
///
/// When two windows report the same pid (a multi-window application), the LAST
/// entry in the array wins: the map is filled in array order and a later insert
/// overwrites an earlier one. Last-wins is simply what `HashMap::insert` gives
/// us, and Hyprland lists the most recently focused/created windows later, so
/// the surviving label tends to be the more relevant one.
///
/// Malformed JSON, or a top-level value that is not an array, yields an empty
/// map. There is no error path: a bad response simply means no windows.
pub fn window_pid_map(clients_json: &str) -> HashMap<i32, WindowInfo> {
    let mut windows = HashMap::new();

    let value: serde_json::Value = match serde_json::from_str(clients_json) {
        Ok(value) => value,
        Err(_) => return windows,
    };
    let clients = match value.as_array() {
        Some(clients) => clients,
        None => return windows,
    };

    for client in clients {
        let pid = match client["pid"].as_i64() {
            Some(pid) => pid,
            None => continue,
        };
        if pid <= 0 {
            continue;
        }
        let class = client["class"].as_str().unwrap_or("").to_string();
        let title = client["title"].as_str().unwrap_or("").to_string();
        windows.insert(pid as i32, WindowInfo { class, title });
    }

    windows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_valid_windows_and_skips_missing_or_nonpositive_pid() {
        // Two normal windows with distinct pids, one entry with no `pid` field,
        // and one entry with `pid: 0`. Only the two valid entries survive.
        let clients_json = r#"[
            {
                "address": "0x111",
                "pid": 1234,
                "class": "firefox",
                "title": "Mozilla Firefox",
                "workspace": {"id": 1, "name": "1"}
            },
            {
                "address": "0x222",
                "pid": 5678,
                "class": "kitty",
                "title": "kitty",
                "workspace": {"id": 2, "name": "2"}
            },
            {
                "address": "0x333",
                "class": "no-pid-app",
                "title": "No Pid",
                "workspace": {"id": 3, "name": "3"}
            },
            {
                "address": "0x444",
                "pid": 0,
                "class": "zero-pid-app",
                "title": "Zero Pid",
                "workspace": {"id": 4, "name": "4"}
            }
        ]"#;

        let map = window_pid_map(clients_json);

        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get(&1234),
            Some(&WindowInfo {
                class: "firefox".to_string(),
                title: "Mozilla Firefox".to_string(),
            })
        );
        assert_eq!(
            map.get(&5678),
            Some(&WindowInfo {
                class: "kitty".to_string(),
                title: "kitty".to_string(),
            })
        );
    }

    #[test]
    fn malformed_json_yields_empty_map() {
        assert!(window_pid_map("not json at all").is_empty());
        assert!(window_pid_map("").is_empty());
        // Well-formed JSON, but the top-level value is not an array.
        assert!(window_pid_map(r#"{"pid": 1}"#).is_empty());
    }

    #[test]
    fn two_windows_same_pid_keep_last() {
        // Both windows report pid 42. The LAST entry in the array wins.
        let clients_json = r#"[
            {
                "address": "0x555",
                "pid": 42,
                "class": "app",
                "title": "First Window"
            },
            {
                "address": "0x666",
                "pid": 42,
                "class": "app",
                "title": "Second Window"
            }
        ]"#;

        let map = window_pid_map(clients_json);

        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get(&42),
            Some(&WindowInfo {
                class: "app".to_string(),
                title: "Second Window".to_string(),
            })
        );
    }

    #[test]
    fn empty_class_and_title_are_tolerated() {
        let clients_json = r#"[
            {"pid": 99, "class": "", "title": ""},
            {"pid": 100}
        ]"#;

        let map = window_pid_map(clients_json);

        assert_eq!(
            map.get(&99),
            Some(&WindowInfo {
                class: String::new(),
                title: String::new(),
            })
        );
        // Missing class and title both read as empty strings.
        assert_eq!(
            map.get(&100),
            Some(&WindowInfo {
                class: String::new(),
                title: String::new(),
            })
        );
    }
}
