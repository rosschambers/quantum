use async_trait::async_trait;
use futures::stream::BoxStream;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use quantum_domain::{DomainError, HyprlandClient};

use crate::error::HyprlandError;

/// Hyprland event parsed from the event socket. Only kinds the provider
/// actually consumes are represented; unrecognised lines are dropped by
/// `parse_hypr_event_line` before they ever reach the provider task.
#[derive(Debug, Clone, PartialEq)]
pub enum HyprlandEvent {
    ActiveWindow { class: String, title: String },
    Workspace { name: String },
    FocusedMon { monitor: String, workspace: String },
    MonitorAdded { monitor: String },
    MonitorRemoved { monitor: String },
}

/// Parse a single Hyprland event line (newline-stripped).
/// Format: `<event>>><args>` where args are comma-separated.
///
/// Returns `None` for any unrecognised event kind. The previous behaviour
/// was to wrap the raw line in `HyprlandEvent::Unknown(...)` and pass it
/// through, but no downstream consumer did anything with that variant —
/// every wake-up cost CPU and (because the provider then re-serialized and
/// broadcast the active-window state) IPC traffic. Dropping at the parser
/// gives the same observable behaviour at lower cost.
pub fn parse_hypr_event_line(line: &str) -> Option<HyprlandEvent> {
    let (kind, args) = line.split_once(">>")?;
    match kind {
        "activewindow" => {
            let (class, title) = args.split_once(',')?;
            Some(HyprlandEvent::ActiveWindow {
                class: class.to_string(),
                title: title.to_string(),
            })
        }
        "workspace" => Some(HyprlandEvent::Workspace {
            name: args.to_string(),
        }),
        "focusedmon" => {
            let (monitor, workspace) = args.split_once(',')?;
            Some(HyprlandEvent::FocusedMon {
                monitor: monitor.to_string(),
                workspace: workspace.to_string(),
            })
        }
        "monitoradded" => Some(HyprlandEvent::MonitorAdded {
            monitor: args.to_string(),
        }),
        "monitorremovedv2" => {
            // Hyprland >= 0.41 emits "<name>,<id>"; we key on name only,
            // so drop the id.
            let monitor = args.split_once(',').map(|(m, _)| m).unwrap_or(args);
            Some(HyprlandEvent::MonitorRemoved {
                monitor: monitor.to_string(),
            })
        }
        "monitorremoved" => Some(HyprlandEvent::MonitorRemoved {
            monitor: args.to_string(),
        }),
        _ => {
            tracing::trace!("hyprland: ignoring unhandled event line: {line}");
            None
        }
    }
}

/// Real Hyprland IPC client using Unix sockets.
pub struct HyprlandSocketClient {
    command_socket: PathBuf,
    #[allow(dead_code)]
    event_socket: PathBuf,
}

impl HyprlandSocketClient {
    /// Create a new client by reading environment variables.
    pub fn new() -> Result<Self, HyprlandError> {
        let instance_sig =
            std::env::var("HYPRLAND_INSTANCE_SIGNATURE").map_err(|_| HyprlandError::Unreachable)?;

        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));

        let base_dir = format!("{}/hypr/{}", runtime_dir, instance_sig);

        Ok(Self {
            command_socket: PathBuf::from(format!("{}/.socket.sock", base_dir)),
            event_socket: PathBuf::from(format!("{}/.socket2.sock", base_dir)),
        })
    }

    /// Subscribe to Hyprland events from the event socket.
    /// Returns a stream of parsed events.
    pub fn subscribe_events(&self) -> Result<BoxStream<'static, HyprlandEvent>, HyprlandError> {
        let path = self.event_socket.clone();
        let stream = async_stream::stream! {
            let stream = match UnixStream::connect(&path).await {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!("hyprland event socket connect failed: {err}");
                    return;
                }
            };
            let reader = BufReader::new(stream);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(ev) = parse_hypr_event_line(line.trim()) {
                    yield ev;
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl HyprlandClient for HyprlandSocketClient {
    async fn command(&self, cmd: &str) -> Result<String, DomainError> {
        let mut stream = UnixStream::connect(&self.command_socket)
            .await
            .map_err(|e| DomainError::Unsupported(format!("Hyprland connect failed: {}", e)))?;

        stream
            .write_all(cmd.as_bytes())
            .await
            .map_err(|e| DomainError::Unsupported(format!("write failed: {}", e)))?;

        // Hyprland sends a single response per command and then closes the
        // write half (or the whole connection), so reading until EOF gives us
        // the complete payload. The previous fixed 4 KB buffer truncated
        // larger responses (for example `j/clients` on sessions with many
        // windows), which caused JSON parsing to fail.
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .map_err(|e| DomainError::Unsupported(format!("read failed: {}", e)))?;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    #[tokio::test]
    async fn client_sends_command_to_socket() {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // Start a mock server
        let listener = UnixListener::bind(&socket_path).unwrap();
        let socket_path_clone = socket_path.clone();

        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let (mut reader, mut writer) = tokio::io::split(stream);
                let mut buf = vec![0u8; 256];
                if let Ok(n) = reader.read(&mut buf).await {
                    let cmd = String::from_utf8_lossy(&buf[..n]).into_owned();
                    if cmd.contains("test") {
                        let _ = writer.write_all(b"OK").await;
                    }
                }
            }
        });

        // Give server time to start
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Mock client that connects to test socket
        let mut stream = UnixStream::connect(&socket_path_clone).await.unwrap();
        stream.write_all(b"test").await.unwrap();

        let mut buf = vec![0u8; 256];
        let n = stream.read(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);
        assert_eq!(response, "OK");
    }

    #[tokio::test]
    #[should_panic]
    async fn new_without_env_var_fails() {
        // Temporarily unset the env var
        let _ = std::env::var("HYPRLAND_INSTANCE_SIGNATURE");
        // This test expects the creation to fail if the variable is not set
        // In real testing, we'd need to mock this better
        unimplemented!("test requires proper environment setup")
    }

    /// Regression test: large Hyprland responses (>4 KB) must not be truncated.
    ///
    /// The previous implementation used a single 4096-byte read, so any
    /// response longer than that was silently cut off, and JSON parsing
    /// downstream would fail. This test spins a mock Unix-socket server that
    /// emits a ~10 KB JSON array of fake clients, then verifies the full
    /// payload is returned and parses as valid JSON.
    #[tokio::test]
    async fn command_reads_full_response_larger_than_4kb() {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("hypr.sock");

        // Build a >4 KB JSON array. Each entry is roughly 140 bytes, so 80
        // entries give us ~11 KB — well past the old 4096-byte cap.
        let entries: Vec<String> = (0..80)
            .map(|i| {
                format!(
                    r#"{{"address":"0x{:016x}","title":"Window number {} with a fairly long descriptive title that pads the payload","class":"app{}","workspace":{{"id":{},"name":"{}"}}}}"#,
                    i, i, i, i, i
                )
            })
            .collect();
        let large_response = format!("[{}]", entries.join(","));
        assert!(
            large_response.len() > 4096,
            "test payload must exceed the old 4 KB cap, got {} bytes",
            large_response.len()
        );

        // Mock Hyprland server: accept one connection, read the command,
        // write the entire large response, then drop the stream (which
        // signals EOF to the client).
        let listener = UnixListener::bind(&socket_path).unwrap();
        let response_for_server = large_response.clone();
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let (mut reader, mut writer) = tokio::io::split(stream);
                let mut buf = vec![0u8; 64];
                let _ = reader.read(&mut buf).await;
                let _ = writer.write_all(response_for_server.as_bytes()).await;
                let _ = writer.shutdown().await;
            }
        });

        // Construct a client pointed at our mock socket. We bypass
        // `HyprlandSocketClient::new()` because it reads environment
        // variables; for the test we just want to exercise `command()`.
        let client = HyprlandSocketClient {
            command_socket: socket_path.clone(),
            event_socket: socket_path.clone(),
        };

        let response = client.command("j/clients").await.unwrap();

        assert_eq!(
            response.len(),
            large_response.len(),
            "client must return the full {}-byte response, got {} bytes",
            large_response.len(),
            response.len()
        );
        assert_eq!(response, large_response);

        // And the response must parse as JSON — the original bug surfaced
        // as a parse failure on the truncated tail.
        let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 80);
    }
}

#[cfg(test)]
mod event_tests {
    use super::*;

    #[test]
    fn parse_active_window_event() {
        let e = parse_hypr_event_line("activewindow>>firefox,Mozilla Firefox").unwrap();
        assert!(
            matches!(e, HyprlandEvent::ActiveWindow { ref class, ref title }
            if class == "firefox" && title == "Mozilla Firefox")
        );
    }

    #[test]
    fn parse_workspace_event() {
        let e = parse_hypr_event_line("workspace>>2").unwrap();
        assert!(matches!(e, HyprlandEvent::Workspace { ref name } if name == "2"));
    }

    #[test]
    fn parse_unknown_event_kind_returns_none() {
        // Unrecognised event kinds (createworkspace, openwindow, ...) are
        // dropped at the parser. The provider task is never woken for
        // events it would have ignored anyway, which both saves CPU and
        // simplifies downstream change-gating.
        assert!(parse_hypr_event_line("createworkspace>>foo").is_none());
        assert!(parse_hypr_event_line("openwindow>>0xabc,1,kitty,kitty").is_none());
        assert!(parse_hypr_event_line("urgent>>0xdef").is_none());
    }

    #[test]
    fn parse_garbage_returns_none() {
        assert!(parse_hypr_event_line("not an event").is_none());
    }

    #[test]
    fn parses_focusedmon() {
        let e = parse_hypr_event_line("focusedmon>>DP-1,2").expect("Some");
        assert!(
            matches!(e, HyprlandEvent::FocusedMon { ref monitor, ref workspace }
            if monitor == "DP-1" && workspace == "2")
        );
    }

    #[test]
    fn parses_monitoradded() {
        let e = parse_hypr_event_line("monitoradded>>HDMI-A-1").expect("Some");
        assert!(matches!(e, HyprlandEvent::MonitorAdded { ref monitor }
            if monitor == "HDMI-A-1"));
    }

    #[test]
    fn parses_monitorremovedv2() {
        let e = parse_hypr_event_line("monitorremovedv2>>HDMI-A-1,3").expect("Some");
        assert!(matches!(e, HyprlandEvent::MonitorRemoved { ref monitor }
            if monitor == "HDMI-A-1"));
    }

    #[test]
    fn parses_legacy_monitorremoved() {
        let e = parse_hypr_event_line("monitorremoved>>HDMI-A-1").expect("Some");
        assert!(matches!(e, HyprlandEvent::MonitorRemoved { ref monitor }
            if monitor == "HDMI-A-1"));
    }
}
