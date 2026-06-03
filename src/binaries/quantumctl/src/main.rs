use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Parser)]
#[command(name = "quantumctl")]
#[command(about = "Control the quantum daemon", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output raw JSON response
    #[arg(global = true, long)]
    json: bool,

    /// Socket path for daemon communication
    #[arg(global = true, long)]
    socket: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Toggle a view (show if hidden, hide if shown)
    Toggle {
        /// View name
        view: String,
    },
    /// Show a view
    Show {
        /// View name
        view: String,
    },
    /// Hide a view
    Hide {
        /// View name
        view: String,
    },
    /// Search for items
    Search {
        /// Query text
        query: String,
    },
    /// List registered providers
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Get system status
    System {
        #[command(subcommand)]
        command: SystemCommand,
    },
    /// Reload theme
    Theme {
        #[command(subcommand)]
        command: ThemeCommand,
    },
    /// Raw JSON-RPC call
    Call {
        /// Method name
        method: String,
        /// JSON parameters
        params: Option<String>,
    },
}

#[derive(Subcommand)]
enum ProviderCommand {
    /// List all providers
    List,
}

#[derive(Subcommand)]
enum SystemCommand {
    /// Get system status
    Status,
}

#[derive(Subcommand)]
enum ThemeCommand {
    /// Reload the theme
    Reload,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let socket_path = cli.socket.unwrap_or_else(|| {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("{}/.run", std::env::var("HOME").unwrap_or_default()));
        PathBuf::from(runtime_dir).join("quantum.sock")
    });

    match cli.command {
        Commands::Toggle { view } => {
            let result = call_daemon(&socket_path, "view.toggle", json!({ "name": view })).await?;
            print_response(&result, cli.json);
        }
        Commands::Show { view } => {
            let result = call_daemon(&socket_path, "view.show", json!({ "name": view })).await?;
            print_response(&result, cli.json);
        }
        Commands::Hide { view } => {
            let result = call_daemon(&socket_path, "view.hide", json!({ "name": view })).await?;
            print_response(&result, cli.json);
        }
        Commands::Search { query } => {
            let result = call_daemon(&socket_path, "search", json!({ "text": query })).await?;
            print_response(&result, cli.json);
        }
        Commands::Provider { command } => match command {
            ProviderCommand::List => {
                let result = call_daemon(&socket_path, "provider.list", json!({})).await?;
                print_response(&result, cli.json);
            }
        },
        Commands::System { command } => match command {
            SystemCommand::Status => {
                let result = call_daemon(&socket_path, "system.status", json!({})).await?;
                print_response(&result, cli.json);
            }
        },
        Commands::Theme { command } => match command {
            ThemeCommand::Reload => {
                let result = call_daemon(&socket_path, "theme.reload", json!({})).await?;
                print_response(&result, cli.json);
            }
        },
        Commands::Call { method, params } => {
            let params_value = if let Some(p) = params {
                serde_json::from_str(&p)?
            } else {
                json!({})
            };
            let result = call_daemon(&socket_path, &method, params_value).await?;
            print_response(&result, cli.json);
        }
    }

    Ok(())
}

async fn call_daemon(
    socket_path: &std::path::Path,
    method: &str,
    params: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    // Connect to daemon
    let stream = UnixStream::connect(socket_path).await.map_err(|e| {
        format!(
            "Failed to connect to daemon at {}: {}",
            socket_path.display(),
            e
        )
    })?;

    // Build JSON-RPC request
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    });

    // Split for independent read/write halves so we can buffer the reader.
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Send request as a single newline-delimited JSON-RPC line.
    let request_str = request.to_string() + "\n";
    write_half.write_all(request_str.as_bytes()).await?;
    write_half.flush().await?;

    // Read exactly one newline-delimited response line. The daemon's IPC
    // server (see src/infrastructure/ipc/src/server.rs) writes each
    // response as JSON followed by `\n`, so `read_line` consumes the
    // entire response regardless of its size. The previous fixed 4 KB
    // read truncated large responses such as search results containing
    // many desktop entries plus Hyprland windows.
    let mut response_str = String::new();
    reader.read_line(&mut response_str).await?;

    // Parse response
    let response: Value = serde_json::from_str(&response_str)?;

    // Check for JSON-RPC error
    if let Some(error) = response.get("error") {
        if !error.is_null() {
            eprintln!(
                "RPC error: {}",
                error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown")
            );
            std::process::exit(1);
        }
    }

    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

fn print_response(value: &Value, json_mode: bool) {
    if json_mode {
        println!("{}", value);
    } else {
        // Human-readable output
        if value.is_null() {
            println!("(no response)");
        } else if value.is_object() {
            print_object(value, 0);
        } else if value.is_array() {
            println!(
                "{}",
                serde_json::to_string_pretty(value).unwrap_or_default()
            );
        } else {
            println!("{}", value);
        }
    }
}

fn print_object(value: &Value, indent: usize) {
    if let Some(obj) = value.as_object() {
        for (key, val) in obj {
            let prefix = " ".repeat(indent);
            if val.is_object() {
                println!("{}{}:", prefix, key);
                print_object(val, indent + 2);
            } else if val.is_array() {
                println!("{}{}:", prefix, key);
                if let Some(arr) = val.as_array() {
                    for item in arr {
                        if item.is_object() {
                            print_object(item, indent + 2);
                        } else {
                            println!("{}{}", " ".repeat(indent + 2), item);
                        }
                    }
                }
            } else {
                println!("{}{}: {}", prefix, key, val);
            }
        }
    }
}
