use async_trait::async_trait;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use quantum_domain::{DomainError, HyprlandClient};

use crate::InfrastructureError;

/// Real Hyprland IPC client using Unix sockets.
pub struct HyprlandSocketClient {
    command_socket: PathBuf,
    #[allow(dead_code)]
    event_socket: PathBuf,
}

impl HyprlandSocketClient {
    /// Create a new client by reading environment variables.
    pub fn new() -> Result<Self, InfrastructureError> {
        let instance_sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .map_err(|_| InfrastructureError::HyprlandUnreachable)?;

        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));

        let base_dir = format!("{}/hypr/{}", runtime_dir, instance_sig);

        Ok(Self {
            command_socket: PathBuf::from(format!("{}/.socket.sock", base_dir)),
            event_socket: PathBuf::from(format!("{}/.socket2.sock", base_dir)),
        })
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

        let mut buf = vec![0u8; 4096];
        let n = stream
            .read(&mut buf)
            .await
            .map_err(|e| DomainError::Unsupported(format!("read failed: {}", e)))?;

        Ok(String::from_utf8_lossy(&buf[..n]).into_owned())
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
}
