use clap::{Parser, Subcommand};
use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "quantum-dev")]
#[command(about = "Development utilities for quantum", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Watch theme files and reload on changes
    Watch,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Watch => watch_themes().await?,
    }

    Ok(())
}

async fn watch_themes() -> Result<(), Box<dyn std::error::Error>> {
    info!("Watching theme files for changes...");

    let themes_dir = PathBuf::from("src/ui/themes");

    if !themes_dir.exists() {
        eprintln!("ERROR: src/ui/themes directory not found");
        eprintln!("Please run from the project root directory");
        std::process::exit(1);
    }

    // Create a synchronous channel for watch events
    let (tx, rx) = std::sync::mpsc::channel();

    // Create a debounced watcher with the channel
    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;

    // Get the watcher and watch the directory
    debouncer
        .watcher()
        .watch(&themes_dir, RecursiveMode::Recursive)?;

    info!("Watching {} for changes", themes_dir.display());
    info!("Press Ctrl+C to stop");

    // Get socket path
    let socket_path = {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("{}/.run", std::env::var("HOME").unwrap_or_default()));
        PathBuf::from(runtime_dir).join("quantum.sock")
    };

    // Watch for file changes (blocking on sync channel)
    for res in rx {
        match res {
            Ok(events) => {
                // The debouncer collapses events to either `Any` (a debounced
                // change settled) or `AnyContinuous` (changes still in flight).
                // We reload on any settled change.
                if events
                    .iter()
                    .any(|e| matches!(e.kind, DebouncedEventKind::Any))
                {
                    info!("Theme file changed, reloading...");

                    match send_reload(&socket_path).await {
                        Ok(_) => info!("Theme reload sent to daemon"),
                        Err(e) => {
                            eprintln!("WARNING: Failed to send reload to daemon: {e}");
                            eprintln!("Make sure quantumd is running");
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Watch error: {e}");
                break;
            }
        }
    }

    Ok(())
}

async fn send_reload(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(socket_path).await?;

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "theme.reload",
        "params": {}
    });

    let request_str = request.to_string() + "\n";
    stream.write_all(request_str.as_bytes()).await?;
    stream.flush().await?;

    Ok(())
}
