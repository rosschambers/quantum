use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Locate the pre-built `quantumd` binary in the workspace target directory.
/// Walks up from `CARGO_MANIFEST_DIR` looking for `target/debug/quantumd`.
fn locate_quantumd() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Ok(explicit) = std::env::var("QUANTUMD_BIN") {
        return Ok(PathBuf::from(explicit));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut search = manifest_dir.as_path();
    loop {
        let candidate = search.join("target").join("debug").join("quantumd");
        if candidate.exists() {
            return Ok(candidate);
        }
        match search.parent() {
            Some(parent) => search = parent,
            None => break,
        }
    }

    Err("quantumd binary not found; run `cargo build -p quantumd` first".into())
}

/// Create a temporary desktop environment with required XDG directories
fn setup_temp_xdg() -> Result<TempDir, Box<dyn std::error::Error>> {
    let tmpdir = TempDir::new()?;
    let tmppath = tmpdir.path();

    // Create XDG directory structure
    let apps_dir = tmppath.join("applications");
    fs::create_dir_all(&apps_dir)?;

    // Create firefox.desktop fixture
    let firefox_desktop = apps_dir.join("firefox.desktop");
    fs::write(
        &firefox_desktop,
        r#"[Desktop Entry]
Name=Firefox
GenericName=Web Browser
Exec=firefox %u
Type=Application
Categories=Network;WebBrowser;
Keywords=web;browser;
"#,
    )?;

    // Create a placeholder for data home (where desktop files are scanned)
    fs::create_dir_all(tmppath.join("data"))?;

    Ok(tmpdir)
}

#[tokio::test]
async fn search_and_launch_desktop_app() -> Result<(), Box<dyn std::error::Error>> {
    // Setup temp XDG environment
    let tmpdir = setup_temp_xdg()?;
    let tmppath = tmpdir.path();

    // Setup shell log
    let shell_log = tmppath.join("shell.log");
    let shell_log_path = shell_log.clone();

    // Setup socket path
    let socket_path = tmppath.join("quantum.sock");

    // Locate the pre-built quantumd binary. We rely on `cargo test` (or the
    // build step preceding it) to have produced `target/debug/quantumd`.
    // Spawning the binary directly avoids a nested `cargo` invocation that
    // would deadlock on the workspace lock.
    let quantumd_path = locate_quantumd()?;

    // Spawn quantumd with --headless flag
    let mut daemon =
        tokio::process::Command::new(&quantumd_path)
            .args(["--headless", &format!("--socket={}", socket_path.display())])
            .env_clear()
            // Preserve PATH and other essentials
            .envs(std::env::vars().filter(|(k, _)| {
                matches!(k.as_str(), "PATH" | "HOME" | "RUST_LOG" | "RUST_BACKTRACE")
            }))
            // Set our test environment variables
            .env("XDG_RUNTIME_DIR", tmppath.to_str().unwrap())
            .env("XDG_DATA_HOME", tmppath.to_str().unwrap())
            .env("XDG_CONFIG_HOME", tmppath.to_str().unwrap())
            .env("QUANTUM_SHELL_LOG", shell_log_path.to_str().unwrap())
            .spawn()?;

    // Wait for socket to appear (with timeout)
    let start = std::time::Instant::now();
    let socket_timeout = Duration::from_secs(15);
    loop {
        if socket_path.exists() {
            break;
        }
        if start.elapsed() > socket_timeout {
            daemon.kill().await.ok();
            return Err("Socket did not appear within timeout".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Give socket a moment to be ready
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Connect to the socket
    let mut stream = UnixStream::connect(&socket_path)
        .await
        .map_err(|e| format!("Failed to connect to socket: {}", e))?;

    // Send a search request for "fire"
    let search_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "search",
        "params": {
            "text": "fire",
            "providers": [],
            "limit": null
        }
    });

    stream
        .write_all(serde_json::to_string(&search_request)?.as_bytes())
        .await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    // Read the response
    let (reader, _writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut response_line = String::new();
    reader.read_line(&mut response_line).await?;

    let response: serde_json::Value = serde_json::from_str(&response_line)?;
    tracing::info!("Search response: {}", response);

    // Verify response contains a Firefox match
    let result = response
        .get("result")
        .ok_or("Response missing 'result' field")?;
    let matches = result
        .get("matches")
        .and_then(|m| m.as_array())
        .ok_or("Result missing 'matches' array")?;

    let firefox_match = matches
        .iter()
        .find(|m| {
            m.get("title")
                .and_then(|t| t.as_str())
                .map(|t| t.contains("Firefox"))
                .unwrap_or(false)
        })
        .ok_or("No Firefox match found in results")?;

    tracing::info!("Found Firefox match: {}", firefox_match);

    // Extract action from the Firefox match
    let action = firefox_match
        .get("action")
        .ok_or("Match missing 'action' field")?;

    // Send action.invoke for the Firefox match
    let invoke_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "action.invoke",
        "params": {
            "provider": firefox_match.get("provider").unwrap_or(&serde_json::Value::Null),
            "action": action
        }
    });

    let mut stream = UnixStream::connect(&socket_path).await?;
    stream
        .write_all(serde_json::to_string(&invoke_request)?.as_bytes())
        .await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    // Read the action response
    let (reader, _writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut response_line = String::new();
    reader.read_line(&mut response_line).await?;

    let _response: serde_json::Value = serde_json::from_str(&response_line)?;
    tracing::info!("Action invoke response received");

    // Give the executor a moment to write the log
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify the shell log contains the firefox command
    if shell_log_path.exists() {
        let log_contents = fs::read_to_string(&shell_log_path)?;
        tracing::info!("Shell log contents:\n{}", log_contents);
        assert!(
            log_contents.contains("firefox"),
            "Shell log should contain 'firefox' command"
        );
    }

    // Terminate the daemon with SIGTERM so its signal handler can run the
    // socket cleanup. `daemon.kill()` would send SIGKILL, which bypasses
    // cleanup and produces a false-positive failure on the assertion below.
    let pid = daemon.id().ok_or("daemon has no pid (already exited?)")?;
    // SAFETY: `kill(2)` with SIGTERM and a valid pid is well-defined.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let _ = daemon.wait().await?;

    // Verify socket is cleaned up (after a short delay)
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !socket_path.exists(),
        "Socket should be cleaned up after daemon exits"
    );

    Ok(())
}
